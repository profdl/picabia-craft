//! Stage 3: clean-up of the cut region at physical size.
//!
//! The limits are **widths**, as a cutter's spec sheet gives them: a cut sliver narrower than
//! `minCutWidthMm` is removed by an opening of radius `minCutWidthMm / 2`, a paper bridge narrower
//! than `minBridgeMm` (paper between two cuts) is cut through by a closing of the cut of radius
//! `minBridgeMm / 2`. Areas filter whole contours: a cut piece (outer contour) under
//! `minCutAreaMm2` is dropped (a speck stays paper), a paper hole in the cut (hole contour) under
//! the same area is filled (it would fall out anyway). Anything nested in a dropped contour is
//! smaller still, so it goes too and the even-odd nesting of the rest holds.

use figcraft_geom::kurbo::{BezPath, Shape};
use figcraft_geom::split_subpaths;

use crate::rings;

/// The cut region after clean-up, with what was removed.
#[derive(Debug, Default)]
pub(crate) struct Cleaned {
    pub cut: BezPath,
    pub dropped_specks: usize,
    pub filled_holes: usize,
    pub warnings: Vec<String>,
}

/// Clean-up limits in layer units (radii already halved from the widths).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub min_area: f64,
    pub open_radius: f64,
    pub close_radius: f64,
    pub tolerance: f64,
}

/// `path` (plain contours: outers positive, holes negative) without the contours enclosing less
/// than `min_area`; returns (path, outers dropped, holes filled).
pub(crate) fn drop_small(path: &BezPath, min_area: f64) -> (BezPath, usize, usize) {
    if min_area <= 0.0 {
        return (path.clone(), 0, 0);
    }
    let mut out = BezPath::new();
    let (mut specks, mut holes) = (0, 0);
    for sub in split_subpaths(path) {
        let a = sub.area();
        if a.abs() >= min_area {
            out.extend(sub.iter());
        } else if a > 0.0 {
            specks += 1;
        } else if a < 0.0 {
            holes += 1;
        }
    }
    (out, specks, holes)
}

/// Every contour of the polygonal region `path` simplified to `tolerance` and the result
/// normalised again (simplifying can make a contour cross itself). `None` if that sweep fails.
fn simplify_to(path: &BezPath, tolerance: f64) -> Option<BezPath> {
    let rs = rings::rings(path, tolerance * 0.1, usize::MAX)?;
    let rs: Vec<_> = rs.iter().map(|r| rings::simplify_ring(r, tolerance)).filter(|r| r.len() >= 3).collect();
    figcraft_pathops::simplify(&rings::to_path(&rs)).ok()
}

/// `figcraft_pathops::open` or `close`.
type Morph = fn(&BezPath, f64) -> Result<BezPath, figcraft_pathops::PathOpsError>;

/// Run the clean-up on `cut` (plain contours). A morphology step that fails is skipped with a
/// warning; the rest still runs.
pub(crate) fn clean(cut: BezPath, l: Limits) -> Cleaned {
    let mut c = Cleaned::default();
    // Area first: specks dropped before the offsets cost nothing to offset.
    let (mut cut, s, h) = drop_small(&cut, l.min_area);
    c.dropped_specks += s;
    c.filled_holes += h;
    let steps: [(f64, Morph, &str); 2] =
        [(l.open_radius, figcraft_pathops::open, "minimum cut width"), (l.close_radius, figcraft_pathops::close, "minimum bridge")];
    for (r, op, name) in steps {
        if r <= 0.0 || cut.elements().is_empty() {
            continue;
        }
        match op(&cut, r) {
            Ok(next) => cut = next,
            Err(_) => c.warnings.push(format!("couldn't apply the {name}; the cut is left as it was before that step")),
        }
    }
    let (next, s, h) = drop_small(&cut, l.min_area);
    cut = next;
    c.dropped_specks += s;
    c.filled_holes += h;
    if l.tolerance > 0.0 && !cut.elements().is_empty() {
        match simplify_to(&cut, l.tolerance) {
            Some(s) => cut = s,
            None => c.warnings.push("couldn't simplify the cut; it is left unsimplified".into()),
        }
    }
    c.cut = cut;
    c
}
