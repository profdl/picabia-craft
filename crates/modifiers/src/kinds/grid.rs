//! `gridArray`.
//!
//! `rows × cols` copies in row-major order (copy `k` = row `k / cols`, column `k % cols`; copy 0 is
//! the original at the top left). `gapX`/`gapY` are the space *between neighbouring copies'
//! bounds*, so the step is `bounds width + gapX` by `bounds height + gapY`. A negative gap
//! overlaps copies.

use figcraft_geom::{Affine, Point, Vec2};

use super::{Clamp, Ctx, Stage, StageResult};
use crate::MAX_INSTANCES_PER_NODE;
use crate::eval::Guide;
use crate::model::GridArray;

pub(crate) fn stage(p: &GridArray, ctx: &Ctx) -> StageResult {
    let mut warnings = Vec::new();
    let mut c = Clamp { warnings: &mut warnings };
    let rows = c.count("rows", p.rows);
    let cols = c.count("cols", p.cols);
    let gap = Vec2::new(c.length("gapX", p.gap_x), c.length("gapY", p.gap_y));
    let total = rows * cols;
    // Checked before allocating anything per copy.
    if total.saturating_mul(ctx.n_prev) > MAX_INSTANCES_PER_NODE {
        return StageResult { stage: Stage::TooMany(total), guides: Vec::new(), warnings };
    }
    let step = Vec2::new(ctx.bounds.width() + gap.x, ctx.bounds.height() + gap.y);
    let copies = (0..total).map(|k| Affine::translate(Vec2::new(step.x * (k % cols) as f64, step.y * (k / cols) as f64))).collect();
    // The lattice extents: the outline of the whole grid.
    let (x0, y0) = (ctx.bounds.x0, ctx.bounds.y0);
    let x1 = ctx.bounds.x1 + step.x * (cols - 1) as f64;
    let y1 = ctx.bounds.y1 + step.y * (rows - 1) as f64;
    let (a, b, d, e) = (Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1));
    let guides = vec![Guide::Line { from: a, to: b }, Guide::Line { from: b, to: d }, Guide::Line { from: d, to: e }, Guide::Line { from: e, to: a }];
    StageResult { stage: Stage::Array(copies), guides, warnings }
}
