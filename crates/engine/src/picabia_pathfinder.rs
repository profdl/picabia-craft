//! PICABIA: the `pathfinder` generator (Illustrator-style Pathfinder through VectorCraft's `pathops`).
//!
//! The framework ([`crate::picabia_generate`]) calls [`compute`] for a layer whose generator row
//! is a `pathfinder`, and [`apply_result`] when Apply (or a `.fig`/Penpot bake) turns the result
//! into layers. [`compute`] gathers the layer's design, runs the operation and returns the faces.

use figcraft_doc::{Document, Node, NodeId, StrokeCap, StrokeJoin};
use figcraft_geom::kurbo::{BezPath, PathEl, Shape as _};
use figcraft_modifiers::{Pathfinder, PathfinderOp};
use figcraft_render::picabia::generated::{GenFrame, GenItem, GenPaint, GenShape};
use figcraft_vcbridge::BridgeError;
use figcraft_vcbridge::pathfinder::{InShape, MAX_SHAPES, Op};
use serde_json::json;

use crate::Result;
use crate::picabia_generate::GenOut;

/// The bridge's operation for the modifier's.
fn op_of(op: PathfinderOp) -> Op {
    match op {
        PathfinderOp::Divide => Op::Divide,
        PathfinderOp::Trim => Op::Trim,
        PathfinderOp::Merge => Op::Merge,
        PathfinderOp::Crop => Op::Crop,
        PathfinderOp::Outline => Op::Outline,
        PathfinderOp::MinusBack => Op::MinusBack,
        PathfinderOp::Unite => Op::Unite,
        PathfinderOp::MinusFront => Op::MinusFront,
        PathfinderOp::Intersect => Op::Intersect,
        PathfinderOp::Exclude => Op::Exclude,
    }
}

/// `path` with every subpath closed: filled outlines can come open (a circle's last curve ends on
/// its start without a close), and VectorCraft's Divide takes an open path for a cutting edge.
fn closed(path: &BezPath) -> BezPath {
    let mut out = BezPath::new();
    let mut open = false;
    for el in path.elements() {
        match el {
            PathEl::MoveTo(_) => {
                if open {
                    out.close_path();
                }
                open = true;
            }
            PathEl::ClosePath => open = false,
            _ => {}
        }
        out.push(*el);
    }
    if open {
        out.close_path();
    }
    out
}

/// The most pairs of shapes whose bounding boxes overlap one run takes. VectorCraft refuses by
/// segment pairs, but only after minutes of work on a dense pile of big curved shapes; this is the
/// cheap check ahead of it (a 30 x 30 grid of overlapping circles has about 3500 pairs).
const MAX_OVERLAPS: usize = 25_000;

/// Whether more than [`MAX_OVERLAPS`] pairs of `boxes` overlap (a sweep along x; stops at the cap).
fn too_many_overlaps(boxes: &[figcraft_geom::Rect]) -> bool {
    let mut order: Vec<&figcraft_geom::Rect> = boxes.iter().collect();
    order.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let mut pairs = 0usize;
    for (i, a) in order.iter().enumerate() {
        for b in order.iter().skip(i + 1) {
            if b.x0 > a.x1 {
                break;
            }
            if b.y0 <= a.y1 && a.y0 <= b.y1 {
                pairs += 1;
                if pairs > MAX_OVERLAPS {
                    return true;
                }
            }
        }
    }
    false
}

/// A colour as 8-bit channels, the key shapes of one paint share.
fn quantised(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Layer `n`'s Pathfinder result for `frame` (its input copies in `frame.inputs`), with params `p`
/// (clamped). The result is in the layer's own space, back first.
///
/// Shapes are keyed by their colour (8-bit RGBA): Merge joins touching shapes of one colour, as
/// Illustrator's does, and a key's source (its name, for Apply) is the front-most shape with that
/// colour. Trim keeps same-coloured shapes apart (VectorCraft keys it by shape).
pub fn compute(doc: &Document, n: &Node, frame: &GenFrame, p: &Pathfinder) -> GenOut {
    if !(matches!(n.kind, figcraft_doc::NodeKind::Group | figcraft_doc::NodeKind::BooleanOp { .. }) || n.kind.is_frame_like()) {
        return GenOut::keep_design("pathfinder works on a group, frame or boolean group");
    }
    let g = crate::picabia_stencil::gather_design(doc, n, &frame.inputs);
    let shapes = g.paints.len();
    let mut warnings = g.warnings;
    // The palette: one key per colour, its front-most shape's colour and source.
    let mut palette: Vec<([u8; 4], usize)> = Vec::new();
    let mut input = Vec::with_capacity(shapes);
    for (i, paint) in g.paints.iter().enumerate() {
        let q = quantised(paint.colour);
        let key = match palette.iter().position(|(c, _)| *c == q) {
            Some(k) => {
                if let Some(e) = palette.get_mut(k) {
                    e.1 = i;
                }
                k
            }
            None => {
                palette.push((q, i));
                palette.len() - 1
            }
        };
        input.push(InShape { path: closed(&paint.path), even_odd: false, key: key as u64 });
    }
    let colours = palette.len();
    let boxes: Vec<figcraft_geom::Rect> = input.iter().take(MAX_SHAPES).map(|s| s.path.bounding_box()).filter(|b| b.is_finite()).collect();
    if too_many_overlaps(&boxes) {
        return GenOut::keep_design("these shapes overlap too much to compute (the contents are drawn as they are)");
    }
    let run = match figcraft_vcbridge::pathfinder::run_report(op_of(p.op), &input, p.precision) {
        Ok(r) => r,
        Err(BridgeError::TooComplex) => {
            return GenOut::keep_design("these shapes overlap too much to compute (the contents are drawn as they are)");
        }
        Err(e) => return GenOut::keep_design(format!("pathfinder couldn't run: {e}")),
    };
    if run.dropped > 0 {
        warnings.push(format!("{} shapes past the {MAX_SHAPES} limit were left out", run.dropped));
    }
    if run.skipped > 0 {
        warnings.push(format!("{} empty shapes were skipped", run.skipped));
    }
    if run.shapes.is_empty() && shapes > 0 && matches!(p.op, PathfinderOp::Divide | PathfinderOp::Trim | PathfinderOp::Merge | PathfinderOp::Unite) {
        return GenOut::keep_design("pathfinder made no shapes from these (the contents are drawn as they are)");
    }
    let outline = p.op == PathfinderOp::Outline;
    if outline && p.outline_width <= 0.0 {
        warnings.push("the outline width is 0, so the edges draw as nothing".into());
    }
    let items: Vec<GenItem> = run
        .shapes
        .into_iter()
        .filter_map(|o| {
            let (_, src) = *palette.get(usize::try_from(o.key).ok()?)?;
            let paint = g.paints.get(src)?;
            let source = g.sources.get(src)?;
            let paint_kind = if o.open {
                GenPaint::Stroke { width: p.outline_width, cap: StrokeCap::None, join: StrokeJoin::Miter }
            } else {
                GenPaint::Fill { even_odd: false }
            };
            Some(GenItem { path: o.path, paint: paint_kind, rgba: paint.colour, source: Some(source.node), name: source.name.clone() })
        })
        .collect();
    let faces = items.len();
    let mut out = GenOut::new(items, json!({"shapes": shapes, "faces": faces, "colours": colours}));
    out.warnings = warnings;
    out
}

/// Replace layer `id` by the group Apply makes of its result `shape`: a group of the same name,
/// one vector layer per face, back first, each named after its source layer.
pub fn apply_result(doc: &mut Document, id: NodeId, shape: &GenShape) -> Result<NodeId> {
    let name = doc.get(id)?.name.clone();
    crate::picabia_generate::replace_with_items(doc, id, shape, &name)
}
