//! Pathfinder over a back-to-front stack of filled shapes, by VectorCraft's `pathops`.
//!
//! Budgets: at most [`MAX_SHAPES`] shapes are taken (the rest are left out and reported by
//! [`run_report`]); the vendored crate refuses an input whose segments' bounds overlap too often
//! (`MAX_SEGMENT_PAIRS`, counted before its sweep) and that refusal comes back as
//! [`BridgeError::TooComplex`]. Non-finite shapes are skipped.

use std::panic::{AssertUnwindSafe, catch_unwind};

use figcraft_geom::kurbo::BezPath;
use vectorcraft_geom::{FillRule, PathData};
use vectorcraft_pathops::{PathOpsError, PathfinderOp, Shape, pathfinder_with, try_unite_all};

use crate::BridgeError;
use crate::path::{from_vc, rule_from_even_odd, to_vc};

/// The most input shapes one run takes; later (front-most) ones are dropped and reported.
pub const MAX_SHAPES: usize = 5000;

/// Precision used when the requested one is not a finite number.
pub const DEFAULT_PRECISION: f64 = 0.01;

/// A Pathfinder operation (the bridge's own mirror of VectorCraft's, without the CMYK-only mixes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    /// Faces: every non-overlapping piece, keeping the front-most covering shape's key.
    Divide,
    /// Remove hidden parts; one result per shape that remains visible.
    Trim,
    /// Trim, then merge touching visible parts with the same key.
    Merge,
    /// The lower shapes' visible parts inside the front-most shape (which is removed).
    Crop,
    /// Every edge split at intersections, as open paths keyed by their shape.
    Outline,
    /// Front-most minus everything behind it.
    MinusBack,
    /// Union of everything (front-most key).
    Unite,
    /// Back-most minus everything in front (back-most key).
    MinusFront,
    /// The area all shapes cover (front-most key).
    Intersect,
    /// The area an odd number of shapes cover (front-most key).
    Exclude,
}

impl Op {
    /// All operations, in the Pathfinder panel's order.
    pub const ALL: [Op; 10] =
        [Op::Divide, Op::Trim, Op::Merge, Op::Crop, Op::Outline, Op::MinusBack, Op::Unite, Op::MinusFront, Op::Intersect, Op::Exclude];

    /// VectorCraft's operation.
    pub fn to_vc(self) -> PathfinderOp {
        match self {
            Op::Divide => PathfinderOp::Divide,
            Op::Trim => PathfinderOp::Trim,
            Op::Merge => PathfinderOp::Merge,
            Op::Crop => PathfinderOp::Crop,
            Op::Outline => PathfinderOp::Outline,
            Op::MinusBack => PathfinderOp::MinusBack,
            Op::Unite => PathfinderOp::Unite,
            Op::MinusFront => PathfinderOp::MinusFront,
            Op::Intersect => PathfinderOp::Intersect,
            Op::Exclude => PathfinderOp::Exclude,
        }
    }

    /// The bridge's operation for VectorCraft's; `None` for the CMYK-mix effects (`HardMix`,
    /// `SoftMix`).
    pub fn from_vc(op: PathfinderOp) -> Option<Op> {
        Some(match op {
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
            PathfinderOp::HardMix | PathfinderOp::SoftMix => return None,
        })
    }
}

/// One input shape. `key` identifies its paint; results keep the key of the shape whose paint they
/// carry.
#[derive(Clone, Debug, PartialEq)]
pub struct InShape {
    pub path: BezPath,
    pub even_odd: bool,
    pub key: u64,
}

/// One result shape.
#[derive(Clone, Debug, PartialEq)]
pub struct OutShape {
    pub path: BezPath,
    pub key: u64,
    /// An open outline edge (Outline) rather than a closed filled region.
    pub open: bool,
}

/// A run's shapes and what the budgets left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub shapes: Vec<OutShape>,
    /// How many input shapes past [`MAX_SHAPES`] were left out.
    pub dropped: usize,
    /// How many inputs were skipped as empty or non-finite.
    pub skipped: usize,
}

/// Precision clamped to 0.001..=10; not finite gives [`DEFAULT_PRECISION`].
pub fn clamp_precision(p: f64) -> f64 {
    if p.is_finite() { p.clamp(0.001, 10.0) } else { DEFAULT_PRECISION }
}

/// [`run_report`] without the report: the result shapes only.
pub fn run(op: Op, shapes: &[InShape], precision: f64) -> Result<Vec<OutShape>, BridgeError> {
    run_report(op, shapes, precision).map(|r| r.shapes)
}

/// Run `op` over `shapes` (back to front) at `precision`.
///
/// An empty result is `Ok` when the input is sound (an Intersect of disjoint shapes is empty), and
/// an error when VectorCraft refused the input: [`BridgeError::TooComplex`] past its segment-pair
/// budget, [`BridgeError::Failed`] for degenerate geometry. It never panics.
pub fn run_report(op: Op, shapes: &[InShape], precision: f64) -> Result<Run, BridgeError> {
    let precision = clamp_precision(precision);
    let taken = shapes.get(..MAX_SHAPES).unwrap_or(shapes);
    let dropped = shapes.len() - taken.len();
    let input: Vec<Shape> =
        taken.iter().map(|s| Shape::new(to_vc(&s.path), rule_from_even_odd(s.even_odd), s.key)).filter(|s| !s.path.is_empty()).collect();
    let skipped = taken.len() - input.len();
    if input.is_empty() {
        return Ok(Run { shapes: Vec::new(), dropped, skipped });
    }
    let out = catch_unwind(AssertUnwindSafe(|| pathfinder_with(op.to_vc(), &input, precision)))
        .map_err(|_| BridgeError::Failed("internal error".into()))?;
    if out.is_empty() {
        // The vendored crate returns nothing both for a refused input and for an empty result:
        // ask its union (which builds the same planar map, budget first) which it was.
        let refs: Vec<(&PathData, FillRule)> = input.iter().map(|s| (&s.path, s.rule)).collect();
        let probe = catch_unwind(AssertUnwindSafe(|| try_unite_all(&refs))).map_err(|_| BridgeError::Failed("internal error".into()))?;
        match probe {
            Err(PathOpsError::TooComplex) => return Err(BridgeError::TooComplex),
            Err(e) => return Err(BridgeError::Failed(e.to_string())),
            Ok(_) => {}
        }
    }
    let open = op == Op::Outline;
    let shapes = out.iter().map(|s| OutShape { path: from_vc(&s.path), key: s.key, open }).filter(|s| !s.path.elements().is_empty()).collect();
    Ok(Run { shapes, dropped, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use figcraft_geom::kurbo::{Rect, Shape as _};

    fn sq(x: f64, y: f64, s: f64, key: u64) -> InShape {
        InShape { path: Rect::new(x, y, x + s, y + s).to_path(0.1), even_odd: false, key }
    }

    fn area(v: &[OutShape]) -> f64 {
        v.iter().map(|s| s.path.area().abs()).sum()
    }

    /// Two 10x10 squares overlapping in a 5x5 corner: A at 0,0 (back, key 1), B at 5,5 (front, key 2).
    fn two() -> Vec<InShape> {
        vec![sq(0.0, 0.0, 10.0, 1), sq(5.0, 5.0, 10.0, 2)]
    }

    #[test]
    fn divide_makes_three_faces_front_key_on_overlap() {
        let r = run(Op::Divide, &two(), 0.01).unwrap();
        assert_eq!(r.len(), 3);
        assert!((area(&r) - 175.0).abs() < 0.5);
        let overlap = r.iter().find(|s| (s.path.area().abs() - 25.0).abs() < 0.5).unwrap();
        assert_eq!(overlap.key, 2);
        assert!(r.iter().all(|s| !s.open));
        let mut keys: Vec<u64> = r.iter().map(|s| s.key).collect();
        keys.sort();
        assert_eq!(keys, vec![1, 2, 2]);
    }

    #[test]
    fn trim_keeps_each_visible_shape() {
        let r = run(Op::Trim, &two(), 0.01).unwrap();
        assert_eq!(r.len(), 2);
        assert!((area(&r) - 175.0).abs() < 0.5);
        let a = r.iter().find(|s| s.key == 1).unwrap();
        assert!((a.path.area().abs() - 75.0).abs() < 0.5);
    }

    #[test]
    fn merge_joins_same_key() {
        let same = vec![sq(0.0, 0.0, 10.0, 7), sq(5.0, 5.0, 10.0, 7)];
        let r = run(Op::Merge, &same, 0.01).unwrap();
        assert_eq!(r.len(), 1);
        assert!((area(&r) - 175.0).abs() < 0.5);
        // different keys stay apart
        assert_eq!(run(Op::Merge, &two(), 0.01).unwrap().len(), 2);
    }

    #[test]
    fn merge_of_a_touching_3x3_grid_is_one_shape() {
        let mut g = Vec::new();
        for i in 0..3 {
            for j in 0..3 {
                g.push(sq(f64::from(i) * 10.0, f64::from(j) * 10.0, 10.0, 5));
            }
        }
        let r = run(Op::Merge, &g, 0.01).unwrap();
        assert_eq!(r.len(), 1);
        assert!((area(&r) - 900.0).abs() < 0.5);
    }

    #[test]
    fn crop_keeps_lower_parts_inside_the_front_shape() {
        let r = run(Op::Crop, &two(), 0.01).unwrap();
        assert_eq!(r.len(), 1);
        assert!((area(&r) - 25.0).abs() < 0.5);
        assert_eq!(r[0].key, 1);
    }

    #[test]
    fn outline_gives_open_edges() {
        let r = run(Op::Outline, &two(), 0.01).unwrap();
        assert!(!r.is_empty());
        assert!(r.iter().all(|s| s.open));
        assert!(r.iter().any(|s| s.key == 1) && r.iter().any(|s| s.key == 2));
    }

    #[test]
    fn minus_back_and_front() {
        let r = run(Op::MinusBack, &two(), 0.01).unwrap();
        assert_eq!(r.len(), 1);
        assert!((area(&r) - 75.0).abs() < 0.5);
        assert_eq!(r[0].key, 2);
        let f = run(Op::MinusFront, &two(), 0.01).unwrap();
        assert_eq!(f.len(), 1);
        assert!((area(&f) - 75.0).abs() < 0.5);
        assert_eq!(f[0].key, 1);
    }

    #[test]
    fn unite_intersect_exclude() {
        let u = run(Op::Unite, &two(), 0.01).unwrap();
        assert_eq!(u.len(), 1);
        assert!((area(&u) - 175.0).abs() < 0.5);
        assert_eq!(u[0].key, 2);
        let i = run(Op::Intersect, &two(), 0.01).unwrap();
        assert_eq!(i.len(), 1);
        assert!((area(&i) - 25.0).abs() < 0.5);
        let x = run(Op::Exclude, &two(), 0.01).unwrap();
        assert!((area(&x) - 150.0).abs() < 0.5);
    }

    #[test]
    fn every_op_runs_on_every_input_shape_count() {
        for op in Op::ALL {
            assert!(run(op, &[], 0.01).unwrap().is_empty());
            assert!(run(op, &[sq(0.0, 0.0, 10.0, 1)], 0.01).is_ok());
            assert!(!run(op, &two(), 0.01).unwrap().is_empty() || op == Op::Intersect);
            assert_eq!(Op::from_vc(op.to_vc()), Some(op));
        }
        assert_eq!(Op::from_vc(PathfinderOp::HardMix), None);
    }

    #[test]
    fn disjoint_intersect_is_ok_and_empty() {
        let d = vec![sq(0.0, 0.0, 10.0, 1), sq(50.0, 50.0, 10.0, 2)];
        assert!(run(Op::Intersect, &d, 0.01).unwrap().is_empty());
    }

    #[test]
    fn even_odd_input_is_respected() {
        let mut ring = Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1);
        ring.extend(Rect::new(3.0, 3.0, 7.0, 7.0).to_path(0.1));
        let r = run(Op::Unite, &[InShape { path: ring, even_odd: true, key: 1 }], 0.01).unwrap();
        assert!((area(&r) - 84.0).abs() < 0.5);
    }

    #[test]
    fn bad_input_never_panics() {
        let mut nan = BezPath::new();
        nan.move_to((f64::NAN, 0.0));
        nan.line_to((1.0, 1.0));
        let mut zero = BezPath::new();
        zero.move_to((1.0, 1.0));
        zero.line_to((1.0, 1.0));
        zero.close_path();
        let mut huge = BezPath::new();
        huge.move_to((-1e300, -1e300));
        huge.line_to((1e300, -1e300));
        huge.line_to((1e300, 1e300));
        huge.close_path();
        let mut bow = BezPath::new();
        for p in [(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)] {
            if bow.elements().is_empty() { bow.move_to(p) } else { bow.line_to(p) }
        }
        bow.close_path();
        let all = vec![
            InShape { path: nan, even_odd: false, key: 1 },
            InShape { path: zero, even_odd: false, key: 2 },
            InShape { path: huge, even_odd: true, key: 3 },
            InShape { path: bow, even_odd: false, key: 4 },
            sq(2.0, 2.0, 3.0, 5),
        ];
        for op in Op::ALL {
            for p in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1e9] {
                let _ = run_report(op, &all, p);
            }
            for s in all.iter() {
                let _ = run(op, std::slice::from_ref(s), 0.01);
            }
        }
        let r = run_report(Op::Unite, &all[..1], 0.01).unwrap();
        assert_eq!((r.skipped, r.shapes.len()), (1, 0));
    }

    #[test]
    fn precision_is_clamped() {
        assert_eq!(clamp_precision(f64::NAN), DEFAULT_PRECISION);
        assert_eq!(clamp_precision(0.0), 0.001);
        assert_eq!(clamp_precision(1e9), 10.0);
        assert_eq!(clamp_precision(0.5), 0.5);
    }

    #[test]
    fn too_many_shapes_are_cut_and_reported() {
        let many: Vec<InShape> = (0..MAX_SHAPES + 3).map(|i| sq((i % 70) as f64 * 3.0, (i / 70) as f64 * 3.0, 2.0, 1)).collect();
        let r = run_report(Op::Trim, &many[..], 0.1).unwrap();
        assert_eq!(r.dropped, 3);
    }

    #[test]
    fn too_complex_is_refused_not_frozen() {
        // 2200 long thin bars crossing 2200 others: 4.8M crossing pairs of bounds, past the 4.2M budget.
        let n = 2200;
        let mut v = Vec::new();
        for i in 0..n {
            let f = f64::from(i) * 2.0;
            v.push(InShape { path: Rect::new(f, 0.0, f + 1.0, 5000.0).to_path(0.1), even_odd: false, key: 1 });
            v.push(InShape { path: Rect::new(0.0, f, 5000.0, f + 1.0).to_path(0.1), even_odd: false, key: 2 });
        }
        assert_eq!(run(Op::Divide, &v, 0.01), Err(BridgeError::TooComplex));
    }
}
