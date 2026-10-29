//! `randomize`.
//!
//! Per instance `i`, four numbers `r` in -1..=1 come from the stream fixed by `(seed, i)`
//! (SplitMix64, see `rng`): shift `(position · r0, position · r1)` points, turn `rotation · r2`
//! degrees, scale `1 + scale · r3`, applied around the instance's own pivot in its own frame.
//! An instance's numbers depend only on `seed` and its index.

use figcraft_geom::{Affine, Vec2};

use super::rng::Stream;
use super::{Clamp, Ctx, Stage, StageResult};
use crate::model::Randomize;

const MIN_SCALE: f64 = 1.0e-6;

pub(crate) fn stage(p: &Randomize, _ctx: &Ctx) -> StageResult {
    let mut warnings = Vec::new();
    let mut c = Clamp { warnings: &mut warnings };
    let position = c.size("position", p.position);
    let rotation = c.within("rotation", p.rotation, 0.0, crate::MAX_ANGLE, 0.0);
    let scale = c.within("scale", p.scale, 0.0, crate::MAX_SCALE, 0.0);
    let seed = p.seed;
    let each = move |_: &crate::eval::Instance, i: usize, _n: usize| {
        let mut rng = Stream::new(seed, i);
        let (r0, r1, r2, r3) = (rng.signed(), rng.signed(), rng.signed(), rng.signed());
        let s = (1.0 + scale * r3).clamp(MIN_SCALE, crate::MAX_SCALE);
        Affine::translate(Vec2::new(position * r0, position * r1)) * Affine::rotate((rotation * r2).to_radians()) * Affine::scale(s)
    };
    StageResult { stage: Stage::Each(Box::new(each)), guides: Vec::new(), warnings }
}
