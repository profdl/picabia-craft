//! `BezPath` ↔ VectorCraft `PathData`.
//!
//! VectorCraft re-exports the same `kurbo` (0.13) FigCraft uses, so the types unify and the
//! conversions go straight through `PathData::from_bezpath` / `to_bezpath` (quadratics are
//! elevated to cubics). Non-finite coordinates are never passed on: such a path converts to an
//! empty one.

use figcraft_doc::WindingRule;
use figcraft_geom::kurbo::{Affine, BezPath, PathEl, Point};
use vectorcraft_geom::{FillRule, PathData};

fn point_ok(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Are all points of `bp` finite?
pub fn is_finite(bp: &BezPath) -> bool {
    bp.elements().iter().all(|el| match *el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => point_ok(p),
        PathEl::QuadTo(a, b) => point_ok(a) && point_ok(b),
        PathEl::CurveTo(a, b, c) => point_ok(a) && point_ok(b) && point_ok(c),
        PathEl::ClosePath => true,
    })
}

/// FigCraft path to VectorCraft's. A path with a non-finite point gives an empty `PathData`.
pub fn to_vc(bp: &BezPath) -> PathData {
    if !is_finite(bp) {
        return PathData::default();
    }
    PathData::from_bezpath(bp)
}

/// [`to_vc`] after applying `xf` (an affine with a non-finite entry, or one that overflows a
/// point, gives an empty path).
pub fn to_vc_transformed(bp: &BezPath, xf: Affine) -> PathData {
    if !xf.as_coeffs().iter().all(|c| c.is_finite()) {
        return PathData::default();
    }
    to_vc(&(xf * bp.clone()))
}

/// VectorCraft path to FigCraft's. A result with a non-finite point is an empty path.
pub fn from_vc(pd: &PathData) -> BezPath {
    let bp = pd.to_bezpath();
    if is_finite(&bp) { bp } else { BezPath::new() }
}

/// [`from_vc`] then `xf`.
pub fn from_vc_transformed(pd: &PathData, xf: Affine) -> BezPath {
    if !xf.as_coeffs().iter().all(|c| c.is_finite()) {
        return BezPath::new();
    }
    let bp = xf * from_vc(pd);
    if is_finite(&bp) { bp } else { BezPath::new() }
}

/// Fill rule from "is it even-odd" (the renderer's own rule type lives above this layer).
pub fn rule_from_even_odd(even_odd: bool) -> FillRule {
    if even_odd { FillRule::EvenOdd } else { FillRule::NonZero }
}

/// Is `rule` even-odd?
pub fn is_even_odd(rule: FillRule) -> bool {
    rule == FillRule::EvenOdd
}

/// Fill rule from the document's winding rule.
pub fn rule_from_winding(w: WindingRule) -> FillRule {
    match w {
        WindingRule::NonZero => FillRule::NonZero,
        WindingRule::EvenOdd => FillRule::EvenOdd,
    }
}

/// The document's winding rule for a VectorCraft fill rule.
pub fn winding_from_rule(rule: FillRule) -> WindingRule {
    match rule {
        FillRule::NonZero => WindingRule::NonZero,
        FillRule::EvenOdd => WindingRule::EvenOdd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use figcraft_geom::kurbo::{Rect, Shape};

    fn square(x: f64, y: f64, s: f64) -> BezPath {
        Rect::new(x, y, x + s, y + s).to_path(0.1)
    }

    #[test]
    fn lines_round_trip() {
        let p = square(0.0, 0.0, 10.0);
        let back = from_vc(&to_vc(&p));
        assert_eq!(back.bounding_box(), p.bounding_box());
        assert!((back.area() - p.area()).abs() < 1e-9);
    }

    #[test]
    fn cubics_round_trip() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.curve_to((10.0, 0.0), (20.0, 10.0), (20.0, 20.0));
        p.curve_to((10.0, 30.0), (0.0, 20.0), (0.0, 0.0));
        p.close_path();
        let back = from_vc(&to_vc(&p));
        assert!((back.area() - p.area()).abs() < 1e-6);
        assert!(back.elements().iter().any(|e| matches!(e, PathEl::CurveTo(..))));
    }

    #[test]
    fn quads_are_elevated_without_moving() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.quad_to((10.0, 20.0), (20.0, 0.0));
        p.close_path();
        let back = from_vc(&to_vc(&p));
        assert!(back.elements().iter().all(|e| !matches!(e, PathEl::QuadTo(..))));
        assert!((back.area() - p.area()).abs() < 1e-9);
    }

    #[test]
    fn holes_keep_their_subpaths() {
        let mut p = square(0.0, 0.0, 10.0);
        p.extend(square(3.0, 3.0, 4.0).reverse_subpaths());
        let vc = to_vc(&p);
        assert_eq!(vc.subpaths.len(), 2);
        assert!((from_vc(&vc).area().abs() - 84.0).abs() < 1e-9);
    }

    #[test]
    fn rules_map_both_ways() {
        assert_eq!(rule_from_even_odd(true), FillRule::EvenOdd);
        assert_eq!(rule_from_even_odd(false), FillRule::NonZero);
        assert!(is_even_odd(FillRule::EvenOdd) && !is_even_odd(FillRule::NonZero));
        for w in [WindingRule::NonZero, WindingRule::EvenOdd] {
            assert_eq!(winding_from_rule(rule_from_winding(w)), w);
        }
    }

    #[test]
    fn empty_and_degenerate() {
        assert!(to_vc(&BezPath::new()).is_empty());
        assert!(from_vc(&PathData::default()).elements().is_empty());
        let mut one = BezPath::new();
        one.move_to((3.0, 4.0));
        let vc = to_vc(&one);
        assert_eq!(vc.anchor_count(), 1);
        let _ = from_vc(&vc);
    }

    #[test]
    fn non_finite_is_dropped() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut p = square(0.0, 0.0, 10.0);
            p.line_to((bad, 1.0));
            assert!(to_vc(&p).is_empty());
            let mut q = BezPath::new();
            q.move_to((0.0, 0.0));
            q.curve_to((1.0, 1.0), (bad, 2.0), (3.0, 3.0));
            assert!(to_vc(&q).is_empty());
        }
        let mut pd = to_vc(&square(0.0, 0.0, 10.0));
        if let Some(a) = pd.anchor_mut(0, 0) {
            a.p.x = f64::NAN;
        }
        assert!(from_vc(&pd).elements().is_empty());
    }

    #[test]
    fn transforms() {
        let p = square(0.0, 0.0, 10.0);
        let vc = to_vc_transformed(&p, Affine::translate((5.0, 0.0)));
        assert_eq!(vc.bounds().map(|b| b.x0), Some(5.0));
        let back = from_vc_transformed(&vc, Affine::translate((-5.0, 0.0)));
        assert!((back.bounding_box().x0).abs() < 1e-9);
        assert!(to_vc_transformed(&p, Affine::scale(f64::NAN)).is_empty());
        assert!(to_vc_transformed(&p, Affine::scale(f64::MAX) * Affine::scale(f64::MAX)).is_empty());
        assert!(from_vc_transformed(&vc, Affine::scale(f64::INFINITY)).elements().is_empty());
    }
}
