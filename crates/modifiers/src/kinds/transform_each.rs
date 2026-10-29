//! `transformEach`.
//!
//! Every instance gets `L = translate(offset) · rotate(rotate) · scale(sx, sy)` applied around its
//! own pivot, in its own frame (`S_i · pivot · L · pivot⁻¹`). With `progressive` the amounts are
//! scaled by `t = i / (n - 1)` (0 when n = 1): offset and rotation by `t`, scale from 1 to the
//! full factor (`1 + (s - 1) · t`), so the first instance is untouched and the last gets the full
//! amount. `jitter` multiplies each amount by `1 + jitter · r` with `r` in -1..=1 drawn per
//! instance and channel from `jitterSeed` and the instance index.

use figcraft_geom::{Affine, Vec2};

use super::rng::Stream;
use super::{Clamp, Ctx, Stage, StageResult};
use crate::model::TransformEach;

const MIN_SCALE: f64 = 1.0e-6;

pub(crate) fn stage(p: &TransformEach, _ctx: &Ctx) -> StageResult {
    let mut warnings = Vec::new();
    let mut c = Clamp { warnings: &mut warnings };
    let offset = Vec2::new(c.length("offsetX", p.offset_x), c.length("offsetY", p.offset_y));
    let rotate = c.angle("rotate", p.rotate);
    let (sx, sy) = (c.scale("scaleX", p.scale_x), c.scale("scaleY", p.scale_y));
    let jitter = c.within("jitter", p.jitter, 0.0, 10.0, 0.0);
    let (progressive, seed) = (p.progressive, p.jitter_seed);
    let each = move |_: &crate::eval::Instance, i: usize, n: usize| {
        let t = if progressive && n > 1 {
            i as f64 / (n - 1) as f64
        } else if progressive {
            0.0
        } else {
            1.0
        };
        let mut j = [1.0; 5];
        if jitter > 0.0 {
            let mut rng = Stream::new(seed, i);
            for v in &mut j {
                *v = 1.0 + jitter * rng.signed();
            }
        }
        let off = Vec2::new(offset.x * t * j[0], offset.y * t * j[1]);
        let turn = (rotate * t * j[2]).rem_euclid(360.0).to_radians();
        let fx = (1.0 + (sx - 1.0) * t * j[3]).clamp(MIN_SCALE, crate::MAX_SCALE);
        let fy = (1.0 + (sy - 1.0) * t * j[4]).clamp(MIN_SCALE, crate::MAX_SCALE);
        Affine::translate(off) * Affine::rotate(turn) * Affine::scale_non_uniform(fx, fy)
    };
    StageResult { stage: Stage::Each(Box::new(each)), guides: Vec::new(), warnings }
}
