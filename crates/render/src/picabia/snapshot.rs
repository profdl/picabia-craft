//! Picabia: what a computed result (a stencil sheet, a generator's geometry) was computed from,
//! and the process-wide stores that keep the results.
//!
//! The engine computes; render, exports, bounds and hit testing read the results back. Each
//! result is kept with an [`Inputs`] snapshot: the layer's subtree (the layer itself with what
//! doesn't change the result reset, by the store's `neutral` function), its own copies (the
//! store's `own` function) and every copy transform below it, and the font generation. A lookup
//! is **fresh** when the document still matches the snapshot, otherwise the layer's latest result
//! is returned **stale** (drawn while the engine recomputes). Compared, not hashed: a match is
//! exact.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use figcraft_doc::{Document, Node, NodeId};
use figcraft_geom::Affine;
use figcraft_modifiers::Repeat;

use super::{has_stack, instance_transforms};

/// The most results one store keeps (one per layer and document state; oldest dropped first).
const MAX_ENTRIES: usize = 64;
/// The most layers an [`Inputs`] snapshot records; a bigger subtree compares its first ones.
const MAX_INPUT_NODES: usize = 100_000;

/// The layer with what doesn't change its result reset.
pub(crate) type Neutral = fn(&Node) -> Node;
/// The layer's own copies (in its own space) that its result depends on, and its repeat lattice
/// (in its own space; `None` for results that don't repeat).
pub(crate) type OwnCopies = fn(&Document, &Node) -> (Vec<Affine>, Option<Repeat>);

/// What a result was computed from (see the module docs).
#[derive(Clone, Debug)]
pub struct Inputs {
    root: Node,
    nodes: Vec<Node>,
    /// Per layer with a stack (pre-order): its copy transforms (the layer's own: in its space).
    copies: Vec<(NodeId, Vec<Affine>)>,
    /// The layer's repeat lattice, in its own space (it follows from the stack and the copies,
    /// compared anyway: the result depends on it).
    repeat: Option<Repeat>,
    fonts: u64,
    /// Some value doesn't equal itself (a NaN): any snapshot of the layer then counts as a match,
    /// or it would be recomputed forever.
    unstable: bool,
}

impl PartialEq for Inputs {
    fn eq(&self, o: &Self) -> bool {
        self.fonts == o.fonts
            && self.root == o.root
            && same_repeat(self.repeat, o.repeat)
            && self.nodes == o.nodes
            && self.copies.len() == o.copies.len()
            && self.copies.iter().zip(&o.copies).all(|((a, ta), (b, tb))| a == b && same_affines(ta, tb))
    }
}

fn same_affines(a: &[Affine], b: &[Affine]) -> bool {
    // Copies of a moved layer come out of `N⁻¹ · T · N` with last-bit differences.
    let close = |x: f64, y: f64| (x - y).abs() <= 1e-9 * (1.0 + x.abs().max(y.abs()));
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.as_coeffs().iter().zip(q.as_coeffs()).all(|(x, y)| close(*x, y)))
}

fn same_repeat(a: Option<Repeat>, b: Option<Repeat>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let t = |r: Repeat| Affine::new([r.a.x, r.a.y, r.b.x, r.b.y, r.origin.x, r.origin.y]);
            same_affines(&[t(a)], &[t(b)])
        }
        _ => false,
    }
}

/// The subtree below `root` (pre-order, capped), `root` left out.
pub(crate) fn descendants<'a>(doc: &'a Document, root: &Node, mut f: impl FnMut(&'a Node) -> bool) {
    let mut stack: Vec<NodeId> = root.children.iter().rev().copied().collect();
    let mut seen = 0usize;
    while let Some(id) = stack.pop() {
        seen += 1;
        if seen > MAX_INPUT_NODES {
            return;
        }
        let Some(n) = doc.node(id) else { continue };
        if !f(n) {
            return;
        }
        stack.extend(n.children.iter().rev().copied());
    }
}

impl Inputs {
    /// What layer `id`'s result would be computed from now.
    pub(crate) fn take(doc: &Document, id: NodeId, neutral: Neutral, own: OwnCopies) -> Option<Inputs> {
        let n = doc.node(id)?;
        let mut nodes = Vec::new();
        let (mine, repeat) = own(doc, n);
        let mut copies = vec![(id, mine)];
        descendants(doc, n, |c| {
            nodes.push(c.clone());
            if has_stack(c)
                && let Some(ts) = instance_transforms(doc, c)
            {
                copies.push((c.id, ts));
            }
            true
        });
        let mut i = Inputs { root: neutral(n), nodes, copies, repeat, fonts: figcraft_text::layout_generation(), unstable: false };
        let copy = i.clone();
        i.unstable = copy != i;
        Some(i)
    }

    /// Whether `doc`'s layer `n` still matches this snapshot (without copying the subtree).
    pub(crate) fn matches(&self, doc: &Document, n: &Node, neutral: Neutral, own: OwnCopies) -> bool {
        if self.unstable {
            return true;
        }
        if self.fonts != figcraft_text::layout_generation() || self.root != neutral(n) {
            return false;
        }
        let Some((_, mine)) = self.copies.first() else { return false };
        let (now, repeat) = own(doc, n);
        if !same_affines(mine, &now) || !same_repeat(self.repeat, repeat) {
            return false;
        }
        let (mut i, mut k, mut same) = (0usize, 1usize, true);
        descendants(doc, n, |c| {
            same = self.nodes.get(i) == Some(c);
            i += 1;
            if same
                && has_stack(c)
                && let Some(ts) = instance_transforms(doc, c)
            {
                same = self.copies.get(k).is_some_and(|(cid, stored)| *cid == c.id && same_affines(stored, &ts));
                k += 1;
            }
            same
        });
        same && i == self.nodes.len() && k == self.copies.len()
    }
}

struct Entry<S> {
    id: NodeId,
    inputs: Arc<Inputs>,
    shape: Arc<S>,
    stamp: u64,
}

/// A process-wide store of results of type `S`, each kept with the [`Inputs`] it was computed
/// from (snapshots taken with this store's `neutral` and `own` functions).
pub(crate) struct Store<S> {
    entries: Mutex<Vec<Entry<S>>>,
    generation: AtomicU64,
    neutral: Neutral,
    own: OwnCopies,
}

impl<S> Store<S> {
    pub(crate) const fn new(neutral: Neutral, own: OwnCopies) -> Store<S> {
        Store { entries: Mutex::new(Vec::new()), generation: AtomicU64::new(0), neutral, own }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Entry<S>>> {
        self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Bumped by every [`Store::store`] (and [`Store::clear`]).
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// What layer `id`'s result would be computed from now.
    pub(crate) fn inputs(&self, doc: &Document, id: NodeId) -> Option<Inputs> {
        Inputs::take(doc, id, self.neutral, self.own)
    }

    /// Keep `shape`, layer `id`'s result computed from `inputs`. Replaces what was kept for the
    /// same inputs; the oldest results go past [`MAX_ENTRIES`].
    pub(crate) fn store(&self, id: NodeId, inputs: Inputs, shape: S) {
        let stamp = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let mut s = self.lock();
        s.retain(|e| !(e.id == id && *e.inputs == inputs));
        s.push(Entry { id, inputs: Arc::new(inputs), shape: Arc::new(shape), stamp });
        if s.len() > MAX_ENTRIES {
            s.sort_by_key(|e| std::cmp::Reverse(e.stamp));
            s.truncate(MAX_ENTRIES);
        }
    }

    /// Forget every result (tests).
    pub(crate) fn clear(&self) {
        self.lock().clear();
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Whether some result was ever stored for layer `id` (cheap: no snapshot comparison).
    pub(crate) fn has(&self, id: NodeId) -> bool {
        self.lock().iter().any(|e| e.id == id)
    }

    /// Layer `n`'s result computed from what `doc` holds now (`true`: fresh) or its latest one
    /// (`false`: stale). `None` when none was ever computed.
    pub(crate) fn lookup(&self, doc: &Document, n: &Node) -> Option<(Arc<S>, bool)> {
        let mut found: Vec<(Arc<Inputs>, Arc<S>, u64)> =
            self.lock().iter().filter(|e| e.id == n.id).map(|e| (e.inputs.clone(), e.shape.clone(), e.stamp)).collect();
        found.sort_by_key(|(_, _, stamp)| std::cmp::Reverse(*stamp));
        // Compared outside the lock: render threads share the store.
        for (inputs, shape, _) in &found {
            if inputs.matches(doc, n, self.neutral, self.own) {
                return Some((shape.clone(), true));
            }
        }
        found.into_iter().next().map(|(_, s, _)| (s, false))
    }
}
