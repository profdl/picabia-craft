//! The whole pipeline: [`compute`].

use figcraft_doc::BoolOp;
use figcraft_geom::kurbo::{BezPath, Rect, Shape};
use figcraft_geom::split_subpaths;
use figcraft_modifiers::Stencil;

use crate::clean::{Limits, clean};
use crate::{ColourInfo, Paint, Selector, colour_infos, mm_per_unit, visible_regions};

/// The most paints [`compute`] takes; past it it makes an empty sheet with a warning.
pub const MAX_PAINTS: usize = 20_000;

/// The flattening tolerance used when `toleranceMm` is 0, mm.
const FINEST_MM: f64 = 0.005;

/// What the stencil cuts and leaves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Separate cut-outs (outer contours of the cut, islands in paper holes included).
    pub cut_pieces: usize,
    /// Cut pieces smaller than `minCutAreaMm2` left as paper.
    pub dropped_specks: usize,
    /// Paper holes in the cut smaller than `minCutAreaMm2` cut away.
    pub filled_holes: usize,
    /// Length of every cut contour, mm (what the laser travels while cutting).
    pub cut_length_mm: f64,
    /// Area cut out, mm².
    pub area_cut_mm2: f64,
}

/// The computed stencil, in the layer's units and space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StencilOut {
    /// The cut-outs: plain polygonal contours (even-odd; non-zero reads the same).
    pub cut: BezPath,
    /// The sheet minus the cut (even-odd; non-zero reads the same).
    pub paper: BezPath,
    /// The sheet: the layer bounds grown by `marginMm`.
    pub sheet: Rect,
    /// Millimetres per layer unit (`widthMm / layer width`): the sheet is
    /// `sheet.width() · mm_per_unit` mm wide.
    pub mm_per_unit: f64,
    pub warnings: Vec<String>,
    pub stats: Stats,
    /// Every colour of the design, largest visible area first.
    pub colours: Vec<ColourInfo>,
}

/// Why [`compute`] made nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StencilError {
    /// The layer bounds aren't finite or have no width, so there is no physical scale.
    BadBounds,
}

impl std::fmt::Display for StencilError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StencilError::BadBounds => write!(f, "the layer has no usable size for a stencil"),
        }
    }
}

impl std::error::Error for StencilError {}

/// Turn `paints` (in the layer's space) into a stencil sheet for `params` (clamped here; its
/// warnings are passed on). `layer_bounds` (layer units) sets the physical scale
/// ([`mm_per_unit`]) and, grown by the margin, the sheet.
///
/// Steps: [`visible_regions`] (curves flattened at `toleranceMm / 4`, every ring simplified to
/// `toleranceMm / 2`), the cut colours ([`Selector`]) merged into one region, clean-up (area
/// filter, opening by `minCutWidthMm / 2` then closing by `minBridgeMm / 2` as three offsets,
/// area filter again, simplify to `toleranceMm / 2`), the cut clipped to the sheet, `paper = sheet − cut`.
///
/// More than [`MAX_PAINTS`] paints: an uncut sheet and a warning.
pub fn compute(paints: &[Paint], params: &Stencil, layer_bounds: Rect) -> Result<StencilOut, StencilError> {
    let (p, mut warnings) = params.clamped();
    let mpu = mm_per_unit(p.width_mm, layer_bounds).ok_or(StencilError::BadBounds)?;
    let units = |mm: f64| mm / mpu;
    let margin = units(p.margin_mm);
    let sheet = layer_bounds.abs().inflate(margin, margin);
    if !sheet.is_finite() {
        return Err(StencilError::BadBounds);
    }
    let blank = |warnings: Vec<String>| StencilOut { paper: sheet.to_path(0.1), sheet, mm_per_unit: mpu, warnings, ..StencilOut::default() };
    if paints.len() > MAX_PAINTS {
        warnings.push(format!("the design has {} shapes; a stencil takes at most {MAX_PAINTS}", paints.len()));
        return Ok(blank(warnings));
    }
    let tolerance = units(p.tolerance_mm);
    let flatten = units(if p.tolerance_mm > 0.0 { p.tolerance_mm / 4.0 } else { FINEST_MM });
    let visible = visible_regions(paints, flatten, tolerance / 2.0);
    warnings.extend(visible.warnings);
    let selector = Selector::new(&p);
    let colours = colour_infos(&visible.regions, &selector, mpu);
    let mut chosen = BezPath::new();
    for r in visible.regions.iter().filter(|r| selector.cuts(r.colour)) {
        chosen.extend(r.path.iter());
    }
    if chosen.elements().is_empty() {
        if !visible.regions.is_empty() {
            warnings.push("nothing is cut: no visible colour is chosen for cutting".into());
        }
        return Ok(StencilOut { colours, ..blank(warnings) });
    }
    // The visible pieces are disjoint but touch: one sweep merges them into plain contours.
    let Ok(merged) = figcraft_pathops::simplify(&chosen) else {
        warnings.push("couldn't merge the cut colours; nothing is cut".into());
        return Ok(StencilOut { colours, ..blank(warnings) });
    };
    let limits = Limits {
        min_area: p.min_cut_area_mm2 / (mpu * mpu),
        open_radius: units(p.min_cut_width_mm) / 2.0,
        close_radius: units(p.min_bridge_mm) / 2.0,
        tolerance: tolerance / 2.0,
    };
    let cleaned = clean(merged, limits);
    warnings.extend(cleaned.warnings);
    let mut cut = cleaned.cut;
    if !cut.elements().is_empty() && !contains(sheet, cut.bounding_box()) {
        match figcraft_pathops::boolean_step(&cut, &sheet.to_path(0.1), BoolOp::Intersect) {
            Ok(c) => cut = c,
            Err(_) => warnings.push("couldn't clip the cut to the sheet".into()),
        }
    }
    if cut.elements().is_empty() {
        warnings.push("nothing is left to cut after clean-up".into());
    }
    let mut stats = measure(&cut, mpu);
    stats.dropped_specks = cleaned.dropped_specks;
    stats.filled_holes = cleaned.filled_holes;
    Ok(StencilOut { paper: paper(sheet, &cut), cut, sheet, mm_per_unit: mpu, warnings, stats, colours })
}

fn contains(outer: Rect, inner: Rect) -> bool {
    inner.x0 >= outer.x0 && inner.y0 >= outer.y0 && inner.x1 <= outer.x1 && inner.y1 <= outer.y1
}

/// The sheet with the cut's contours reversed inside it: sheet − cut, with no sweep (the cut is
/// plain contours within the sheet), winding 1 on paper and 0 in the cut-outs.
fn paper(sheet: Rect, cut: &BezPath) -> BezPath {
    let mut out = sheet.to_path(0.1);
    if out.area() < 0.0 {
        out = out.reverse_subpaths();
    }
    out.extend(cut.reverse_subpaths().iter());
    out
}

/// Pieces, length and area of the (plain-contour) cut at `mpu` mm per unit.
fn measure(cut: &BezPath, mpu: f64) -> Stats {
    let subs = split_subpaths(cut);
    Stats {
        cut_pieces: subs.iter().filter(|s| s.area() > 0.0).count(),
        cut_length_mm: subs.iter().map(|s| s.perimeter(1e-6)).sum::<f64>() * mpu,
        area_cut_mm2: cut.area().max(0.0) * mpu * mpu,
        ..Stats::default()
    }
}
