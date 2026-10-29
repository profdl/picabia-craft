//! Copy-to-shape targets: another layer's outline, read in the source layer's parent space.
//!
//! [`crate::evaluate`] alone has no document, so a `copyToShape` modifier evaluated through it
//! places no copies (with a warning). [`crate::evaluate_with`] takes a [`TargetResolver`];
//! [`DocTargets`] is the one built from a [`Document`] that the renderer, exports, hit testing and
//! commands use. A target is refused (no copies, a warning) when it is missing, a page, on another
//! page, the source itself, an ancestor or descendant of the source, or when its own stack points
//! back at the source, directly or through other targets ([`check_target`]). The engine refuses
//! such targets when they are set; evaluation ignores them in files that have them anyway.
//!
//! The target's outline is its own geometry ([`figcraft_doc::geometry::shape_path`]; a group or
//! boolean is the outlines of its layers), not its modifier copies.

use std::collections::BTreeSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use figcraft_doc::{Document, Node, NodeId};
use figcraft_geom::kurbo::PathEl;
use figcraft_geom::{Affine, BezPath};

use crate::{PLUGIN_KEY, cache};

/// The most path elements a target outline may have (bigger outlines are refused).
pub const MAX_TARGET_ELEMENTS: usize = 100_000;
/// How deep a group target's layers are read.
const MAX_GROUP_DEPTH: usize = 16;
/// The most stacks a cycle check visits.
const MAX_CYCLE_VISITS: usize = 1024;
/// The most layers [`valid_targets`] lists.
pub const MAX_LISTED_TARGETS: usize = 10_000;

/// A resolved target: its outline in the source's parent space and a hash of it (part of the
/// evaluation cache key, so moving or editing the target re-evaluates its sources).
#[derive(Clone, Debug, PartialEq)]
pub struct TargetShape {
    pub outline: BezPath,
    pub hash: u64,
}

impl TargetShape {
    /// A target shape for `outline`, hashed.
    pub fn new(outline: BezPath) -> TargetShape {
        let hash = hash_path(&outline);
        TargetShape { outline, hash }
    }
}

/// Finds a `copyToShape` modifier's target. `Err` is the warning to show (no copies are placed).
pub trait TargetResolver {
    fn resolve(&self, target: &str) -> Result<Arc<TargetShape>, String>;
}

/// No document: every target is unresolved.
pub struct NoTargets;

impl TargetResolver for NoTargets {
    fn resolve(&self, _target: &str) -> Result<Arc<TargetShape>, String> {
        Err("copy to shape needs the document to find its target; no copies".into())
    }
}

/// Targets read from `doc` for the layer `source`.
pub struct DocTargets<'a> {
    pub doc: &'a Document,
    pub source: NodeId,
}

impl<'a> DocTargets<'a> {
    pub fn new(doc: &'a Document, source: NodeId) -> Self {
        DocTargets { doc, source }
    }
}

impl TargetResolver for DocTargets<'_> {
    fn resolve(&self, target: &str) -> Result<Arc<TargetShape>, String> {
        let t = parse_target(target)?;
        check_target(self.doc, self.source, t)?;
        target_outline(self.doc, self.source, t).map(|o| Arc::new(TargetShape::new(o)))
    }
}

/// The layer id a `target` text names (`"12"`, or Figma-style `"0:12"`).
pub fn parse_target(target: &str) -> Result<NodeId, String> {
    target.parse::<NodeId>().map_err(|_| format!("`{}` isn't a layer id", crate::model::short(&serde_json::Value::String(target.into()))))
}

/// Whether layer `target` can be layer `source`'s copy-to-shape target; `Err` says why not.
pub fn check_target(doc: &Document, source: NodeId, target: NodeId) -> Result<(), String> {
    let Some(t) = doc.node(target) else { return Err("the target layer is gone (deleted, or not in this file); no copies".into()) };
    if t.is_page() {
        return Err("a page can't be a copy-to-shape target".into());
    }
    if target == source {
        return Err("a layer can't copy onto itself".into());
    }
    if doc.is_ancestor(target, source) {
        return Err(format!("\"{}\" contains this layer, so it can't be its target", t.name));
    }
    if doc.is_ancestor(source, target) {
        return Err(format!("\"{}\" is inside this layer, so it can't be its target", t.name));
    }
    if doc.page_of(source) != doc.page_of(target) {
        return Err(format!("\"{}\" is on another page", t.name));
    }
    if reaches(doc, target, source) {
        return Err(format!("\"{}\" copies onto this layer itself (directly or through other layers), so it can't be its target", t.name));
    }
    Ok(())
}

/// Whether following copy-to-shape targets from `from`'s stack reaches `to`.
fn reaches(doc: &Document, from: NodeId, to: NodeId) -> bool {
    let mut seen = BTreeSet::new();
    let mut todo = vec![from];
    while let Some(id) = todo.pop() {
        if !seen.insert(id) || seen.len() > MAX_CYCLE_VISITS {
            continue;
        }
        for t in stack_targets(doc.node(id)) {
            if t == to {
                return true;
            }
            todo.push(t);
        }
    }
    false
}

/// The layer ids `n`'s stack copies onto (enabled modifiers; unreadable ids left out).
pub fn stack_targets(n: Option<&Node>) -> Vec<NodeId> {
    let Some(src) = n.and_then(|n| n.plugin_data.get(PLUGIN_KEY)) else { return Vec::new() };
    let Ok(stack) = cache::parsed(src) else { return Vec::new() };
    stack.targets().filter_map(|(_, t)| t.parse::<NodeId>().ok()).collect()
}

/// The layers `source` can copy onto: the layers on its page passing [`check_target`] that have an
/// outline, in layer order (at most [`MAX_LISTED_TARGETS`]).
pub fn valid_targets(doc: &Document, source: NodeId) -> Vec<NodeId> {
    let Some(page) = doc.page_of(source) else { return Vec::new() };
    doc.subtree(page)
        .into_iter()
        .filter(|id| *id != page && check_target(doc, source, *id).is_ok())
        .filter(|id| doc.node(*id).and_then(|n| local_outline(doc, n, 0)).is_some_and(|p| !p.elements().is_empty()))
        .take(MAX_LISTED_TARGETS)
        .collect()
}

/// `target`'s outline in `source`'s parent space.
fn target_outline(doc: &Document, source: NodeId, target: NodeId) -> Result<BezPath, String> {
    let t = doc.node(target).ok_or("the target layer is gone; no copies")?;
    let local = local_outline(doc, t, 0).ok_or_else(|| format!("\"{}\" has no outline to copy onto", t.name))?;
    if local.elements().len() > MAX_TARGET_ELEMENTS {
        return Err(format!("\"{}\" has more than {MAX_TARGET_ELEMENTS} path points", t.name));
    }
    let parent = doc.node(source).and_then(|n| n.parent).map(|p| doc.world_transform(p)).unwrap_or(Affine::IDENTITY);
    let world = doc.world_transform(target);
    if !parent.is_finite() || !world.is_finite() || parent.determinant().abs() < 1e-12 {
        return Err("the target or this layer has an invalid transform".into());
    }
    let m = parent.inverse() * world;
    let mut out = local;
    out.apply_affine(m);
    let finite = out.elements().iter().all(|e| el_points(e).iter().all(|p| p.x.is_finite() && p.y.is_finite()));
    if !finite || out.elements().is_empty() {
        return Err(format!("\"{}\" has no outline to copy onto", t.name));
    }
    Ok(out)
}

/// `n`'s outline in its own space: its shape, or for a group or boolean its layers' outlines.
fn local_outline(doc: &Document, n: &Node, depth: usize) -> Option<BezPath> {
    if let Some(p) = figcraft_doc::geometry::shape_path(n) {
        return Some(p);
    }
    if depth >= MAX_GROUP_DEPTH || n.is_page() {
        return None;
    }
    let mut out = BezPath::new();
    for c in n.children.iter().filter_map(|c| doc.node(*c)) {
        if !c.visible {
            continue;
        }
        let Some(mut p) = local_outline(doc, c, depth + 1) else { continue };
        if !c.transform.is_finite() {
            continue;
        }
        p.apply_affine(c.transform);
        if out.elements().len() + p.elements().len() > MAX_TARGET_ELEMENTS {
            break;
        }
        out.extend(p.elements().iter().copied());
    }
    (!out.elements().is_empty()).then_some(out)
}

fn el_points(e: &PathEl) -> Vec<figcraft_geom::Point> {
    match *e {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => vec![p],
        PathEl::QuadTo(a, b) => vec![a, b],
        PathEl::CurveTo(a, b, c) => vec![a, b, c],
        PathEl::ClosePath => Vec::new(),
    }
}

fn hash_path(p: &BezPath) -> u64 {
    let mut h = DefaultHasher::new();
    for e in p.elements() {
        std::mem::discriminant(e).hash(&mut h);
        for q in el_points(e) {
            q.x.to_bits().hash(&mut h);
            q.y.to_bits().hash(&mut h);
        }
    }
    h.finish()
}
