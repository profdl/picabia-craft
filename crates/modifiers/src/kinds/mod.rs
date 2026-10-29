//! One module per modifier kind. Each turns its params into a [`Stage`]; [`crate::evaluate`] does
//! the composition, caps and indexing. Adding a kind = a module here plus one arm in [`stage`].
//!
//! Kinds without a module yet ([`ModifierKind::CircularArray`], `GridArray`, `Mirror`,
//! `TransformEach`, `Randomize`) evaluate as [`Stage::Unchanged`]: they pass the previous set
//! through untouched.

mod linear;

use figcraft_geom::{Affine, Point, Rect};

use crate::eval::{Guide, Instance};
use crate::model::ModifierKind;
use crate::{MAX_ANGLE, MAX_COUNT, MAX_LENGTH, MAX_SCALE};

/// What a stage sees: the pivot (the layer's bounds centre, parent space), the bounds, and how
/// many instances the stages before it produced.
pub(crate) struct Ctx {
    pub pivot: Point,
    #[allow(dead_code)] // Read by the kinds that size copies from the layer (grid, mirror).
    pub bounds: Rect,
    #[allow(dead_code)] // Read by per-instance kinds that ramp over the set.
    pub n_prev: usize,
}

/// A local transform per instance: `(instance, index, count) -> L_i`, applied around the pivot.
pub(crate) type EachFn = Box<dyn Fn(&Instance, usize, usize) -> Affine>;

/// What one modifier does to the set of instances.
pub(crate) enum Stage {
    /// Pass the set through.
    Unchanged,
    /// Repeat the whole set once per copy transform (`T_0` is usually the identity).
    Array(Vec<Affine>),
    /// Transform each instance around the pivot: `S_i * (pivot · L_i · pivot⁻¹)`.
    #[allow(dead_code)] // Produced by transformEach and randomize.
    Each(EachFn),
}

pub(crate) struct StageResult {
    pub stage: Stage,
    pub guides: Vec<Guide>,
    pub warnings: Vec<String>,
}

impl StageResult {
    pub fn unchanged() -> Self {
        StageResult { stage: Stage::Unchanged, guides: Vec::new(), warnings: Vec::new() }
    }
}

/// The stage modifier `kind` makes in `ctx`.
pub(crate) fn stage(kind: &ModifierKind, ctx: &Ctx) -> StageResult {
    match kind {
        ModifierKind::LinearArray(p) => linear::stage(p, ctx),
        ModifierKind::CircularArray(_)
        | ModifierKind::GridArray(_)
        | ModifierKind::Mirror(_)
        | ModifierKind::TransformEach(_)
        | ModifierKind::Randomize(_)
        | ModifierKind::Other { .. } => StageResult::unchanged(),
    }
}

/// Clamp helpers: every param reaches a transform through one of these.
pub(crate) struct Clamp<'a> {
    pub warnings: &'a mut Vec<String>,
}

impl Clamp<'_> {
    fn bounded(&mut self, name: &str, v: f64, lo: f64, hi: f64, default: f64) -> f64 {
        if !v.is_finite() {
            self.warnings.push(format!("`{name}` isn't a finite number; using {default}"));
            return default;
        }
        if v < lo || v > hi {
            self.warnings.push(format!("`{name}` is limited to {lo}..{hi}"));
        }
        v.clamp(lo, hi)
    }

    /// A length in points, within ±[`MAX_LENGTH`].
    pub fn length(&mut self, name: &str, v: f64) -> f64 {
        self.bounded(name, v, -MAX_LENGTH, MAX_LENGTH, 0.0)
    }

    /// An angle in degrees, within ±[`MAX_ANGLE`].
    pub fn angle(&mut self, name: &str, v: f64) -> f64 {
        self.bounded(name, v, -MAX_ANGLE, MAX_ANGLE, 0.0)
    }

    /// A scale factor within `0..=MAX_SCALE`.
    pub fn scale(&mut self, name: &str, v: f64) -> f64 {
        self.bounded(name, v, 0.0, MAX_SCALE, 1.0)
    }

    /// A copy count within `1..=MAX_COUNT` (0 copies would hide the layer; it reads as 1).
    pub fn count(&mut self, name: &str, v: u32) -> usize {
        if v > MAX_COUNT {
            self.warnings.push(format!("`{name}` is limited to {MAX_COUNT}"));
        }
        usize::try_from(v.clamp(1, MAX_COUNT)).unwrap_or(1)
    }
}
