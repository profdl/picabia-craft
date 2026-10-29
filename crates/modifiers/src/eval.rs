//! Evaluating a stack into instances.

use figcraft_geom::{Affine, Point, Rect};

use crate::kinds::{self, Ctx, Stage};
use crate::model::{ModifierKind, Stack};
use crate::{MAX_INSTANCES_PER_NODE, MAX_LENGTH};

/// One drawn copy of a layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    /// Applied in the parent's space before the node's own transform:
    /// `world = parent_world * transform * node.transform`. Identity = the original.
    pub transform: Affine,
    /// Flat index in [`Output::instances`] (`k * n_prev + j` after each array stage).
    pub index: u32,
    /// The copy number this instance has in each array stage, in stack order.
    pub stage_indices: Vec<u32>,
    /// A deterministic per-instance seed ([`splitmix64`] of the index); identical in wasm.
    pub seed: u64,
}

impl Instance {
    /// The original layer: identity transform, index 0.
    pub fn identity() -> Instance {
        Instance { transform: Affine::IDENTITY, index: 0, stage_indices: Vec::new(), seed: splitmix64(0) }
    }
}

/// A canvas guide a stage shows while its layer is selected, in the parent's space.
#[derive(Clone, Debug, PartialEq)]
pub enum Guide {
    Line { from: Point, to: Point },
    Circle { center: Point, radius: f64 },
    Point(Point),
}

/// Something worth telling the user about an evaluation (shown in the panel).
#[derive(Clone, Debug, PartialEq)]
pub struct Warning {
    /// The modifier it concerns, when it's about one.
    pub modifier: Option<String>,
    pub message: String,
}

/// What a stack evaluates to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    /// The copies to draw. Empty = no active modifiers: draw the layer normally.
    pub instances: Vec<Instance>,
    pub guides: Vec<Guide>,
    pub warnings: Vec<Warning>,
}

impl Output {
    /// The instance transforms, in draw order.
    pub fn transforms(&self) -> impl Iterator<Item = Affine> + '_ {
        self.instances.iter().map(|i| i.transform)
    }

    fn warn(&mut self, modifier: Option<&str>, message: impl Into<String>) {
        self.warnings.push(Warning { modifier: modifier.map(str::to_string), message: message.into() });
    }
}

/// SplitMix64: the crate's deterministic mixer for seeds (same result on every platform).
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn finite_rect(r: Rect) -> Option<Rect> {
    let ok = [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite() && v.abs() <= MAX_LENGTH * 1e3);
    ok.then_some(r)
}

/// Evaluate `stack` for a layer whose bounding box in its parent's space is `bounds` (the pivot is
/// its centre).
///
/// Disabled modifiers are skipped; unknown kinds are skipped with a warning. Params are clamped
/// (see the crate's caps); a stage that would take the instance count past
/// [`MAX_INSTANCES_PER_NODE`] is not run, nor are the stages after it (with a warning). Never
/// panics; every returned transform is finite.
pub fn evaluate(stack: &Stack, bounds: Rect) -> Output {
    let mut out = Output::default();
    if !stack.is_active() {
        for m in stack.modifiers.iter().filter(|m| m.enabled) {
            if let ModifierKind::Other { kind, error } = &m.kind {
                out.warn(Some(&m.id), other_message(kind, error.as_deref()));
            }
        }
        return out;
    }
    let bounds = finite_rect(bounds).unwrap_or_else(|| {
        out.warn(None, "the layer's bounds aren't finite; copies are placed around the origin");
        Rect::ZERO
    });
    let pivot = bounds.center();
    let mut set = vec![Instance::identity()];
    for m in stack.modifiers.iter().filter(|m| m.enabled) {
        if let ModifierKind::Other { kind, error } = &m.kind {
            out.warn(Some(&m.id), other_message(kind, error.as_deref()));
            continue;
        }
        let ctx = Ctx { pivot, bounds, n_prev: set.len() };
        let r = kinds::stage(&m.kind, &ctx);
        out.guides.extend(r.guides);
        for w in r.warnings {
            out.warn(Some(&m.id), w);
        }
        match r.stage {
            Stage::Unchanged => {}
            Stage::Array(copies) => {
                let n = set.len().saturating_mul(copies.len());
                if n > MAX_INSTANCES_PER_NODE {
                    out.warn(
                        Some(&m.id),
                        format!("{n} copies is more than {MAX_INSTANCES_PER_NODE}; this modifier and the ones after it are skipped"),
                    );
                    break;
                }
                set = repeat_block(&set, &copies);
            }
            Stage::Each(local) => {
                let n = set.len();
                for (i, inst) in set.iter_mut().enumerate() {
                    let l = local(inst, i, n);
                    inst.transform *= around(pivot, l);
                }
            }
        }
        let before = set.len();
        set.retain(|i| i.transform.is_finite());
        if set.len() != before {
            out.warn(Some(&m.id), format!("{} copies with invalid transforms were dropped", before - set.len()));
        }
        reindex(&mut set);
    }
    out.instances = set;
    out
}

fn other_message(kind: &str, error: Option<&str>) -> String {
    match error {
        Some(e) => format!("`{kind}` params don't read ({e}); ignored"),
        None if kind.is_empty() => "a modifier without a kind is ignored".into(),
        None => format!("unknown modifier kind `{kind}` is ignored"),
    }
}

/// `pivot · l · pivot⁻¹`.
pub(crate) fn around(pivot: Point, l: Affine) -> Affine {
    Affine::translate(pivot.to_vec2()) * l * Affine::translate(-pivot.to_vec2())
}

/// Every copy `T_k` of the whole previous block: `T_k * S_j` at index `k * n_prev + j`.
fn repeat_block(set: &[Instance], copies: &[Affine]) -> Vec<Instance> {
    let mut next = Vec::with_capacity(set.len() * copies.len());
    for (k, t) in copies.iter().enumerate() {
        let k = u32::try_from(k).unwrap_or(u32::MAX);
        for s in set {
            let mut stage_indices = Vec::with_capacity(s.stage_indices.len() + 1);
            stage_indices.extend_from_slice(&s.stage_indices);
            stage_indices.push(k);
            next.push(Instance { transform: *t * s.transform, index: 0, stage_indices, seed: 0 });
        }
    }
    next
}

fn reindex(set: &mut [Instance]) {
    for (i, inst) in set.iter_mut().enumerate() {
        let i = u32::try_from(i).unwrap_or(u32::MAX);
        inst.index = i;
        inst.seed = splitmix64(u64::from(i));
    }
}
