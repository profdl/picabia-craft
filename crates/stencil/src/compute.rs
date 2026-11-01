//! The whole pipeline: [`compute`].

use figcraft_doc::BoolOp;
use figcraft_geom::kurbo::{BezPath, Rect, Shape};
use figcraft_geom::split_subpaths;
use figcraft_modifiers::{Stencil, StencilLimitMode};

use crate::clean::{Limits, clean};
use crate::problems::{CheckLimits, MAX_ZONES, Problem, ProblemCounts, Zones, check};
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
    /// Paper islands found (paper pieces not joined to the outer sheet before tying), nested ones
    /// included.
    pub islands: usize,
    /// Tie bars added.
    pub ties: usize,
    /// Islands held by the lattice (`pattern` mode), not counted in `ties`' islands.
    pub pattern_held: usize,
    /// Islands left without a tie (gauze mode, or no bar could be placed).
    pub untied: usize,
    /// What still breaks a limit, per kind (all of them; [`StencilOut::problems`] lists at most
    /// [`MAX_PROBLEMS`](crate::MAX_PROBLEMS)).
    pub problems_by_kind: ProblemCounts,
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
    /// The tie bars and, in `pattern` mode, the lattice bars that ended up in the paper (non-zero;
    /// rectangles), already merged into `paper` and removed from `cut`; for the preview overlay.
    /// Empty in gauze mode.
    pub ties: BezPath,
    /// Every paper island found in the cut sheet, outermost first.
    pub islands: Vec<Island>,
    /// What still breaks a cutting limit in [`StencilOut::cut`], at most [`MAX_PROBLEMS`](crate::MAX_PROBLEMS) (taken in
    /// turn from each kind, then grouped by kind); [`Stats::problems_by_kind`] has the counts.
    pub problems: Vec<Problem>,
}

/// A paper island: a piece of paper the cut separates from the outer sheet (it would fall out
/// without ties).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Island {
    /// Its outer contour's bounds, layer units.
    pub bounds: Rect,
    /// Tie bars holding it (0 in gauze mode, when the lattice holds it, or when no bar could be
    /// placed).
    pub tied: u32,
    /// The `pattern` lattice holds it.
    pub pattern_held: bool,
    /// How deep it sits: 0 = in a cut-out of the sheet, 1 = in a cut-out of a depth-0 island, ...
    pub nested_depth: u32,
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
/// area filter again, simplify to `toleranceMm / 2`; with `limitMode: flag` only the
/// simplification), the cut clipped to the sheet, the islands found and held (ties, the pattern
/// lattice, or none in gauze mode), `paper = sheet − cut`, and the checks ([`Problem`]s).
///
/// More than [`MAX_PAINTS`] paints: an uncut sheet and a warning.
pub fn compute(paints: &[Paint], params: &Stencil, layer_bounds: Rect) -> Result<StencilOut, StencilError> {
    let (p, mut warnings) = params.clamped();
    let mpu = mm_per_unit(p.width_mm, layer_bounds).ok_or(StencilError::BadBounds)?;
    let margin = p.margin_mm / mpu;
    let sheet = layer_bounds.abs().inflate(margin, margin);
    if !sheet.is_finite() {
        return Err(StencilError::BadBounds);
    }
    if paints.len() > MAX_PAINTS {
        warnings.push(format!("the design has {} shapes; a stencil takes at most {MAX_PAINTS}", paints.len()));
        return Ok(StencilOut { paper: sheet.to_path(0.1), sheet, mm_per_unit: mpu, warnings, ..StencilOut::default() });
    }
    let pre = prepare(paints, &p, mpu, sheet);
    warnings.extend(pre.warnings);
    let held = crate::hold::hold(pre.cut, sheet, &p, mpu);
    warnings.extend(held.warnings);
    let cut = held.cut;
    let limits = check_limits(&p, mpu);
    let zones = zones(&p, &limits, sheet, &[&held.ties]);
    let checked = check(&cut, sheet, &held.untied_bounds, &limits, &zones);
    warnings.extend(checked.warnings);
    let mut stats = measure(&cut, mpu);
    stats.dropped_specks = pre.dropped_specks;
    stats.filled_holes = pre.filled_holes;
    stats.islands = held.islands.len();
    stats.ties = held.ties_placed;
    stats.pattern_held = held.pattern_held;
    stats.untied = held.untied;
    stats.problems_by_kind = checked.counts;
    Ok(StencilOut {
        paper: paper(sheet, &cut),
        cut,
        sheet,
        mm_per_unit: mpu,
        warnings,
        stats,
        colours: pre.colours,
        ties: held.ties,
        islands: held.islands,
        problems: checked.problems,
    })
}

/// The checks' limits for the clamped params `p` at `mpu` mm per unit.
pub(crate) fn check_limits(p: &Stencil, mpu: f64) -> CheckLimits {
    CheckLimits {
        min_bridge: p.min_bridge_mm / mpu,
        min_cut_width: p.min_cut_width_mm / mpu,
        min_area: p.min_cut_area_mm2 / (mpu * mpu),
        min_corner_angle: p.min_corner_angle,
        slack: p.tolerance_mm / mpu / 2.0,
    }
}

/// Where the narrow checks look: everywhere in flag mode; in fix mode (the clean-up removed every
/// narrow part) only near what changed the cut after it: the bars in `changed` (one zone per
/// subpath) and a band along the sheet's edge (the clip, a thin margin).
pub(crate) fn zones(p: &Stencil, l: &CheckLimits, sheet: Rect, changed: &[&BezPath]) -> Zones {
    if p.limit_mode == StencilLimitMode::Flag {
        return Zones::Everywhere;
    }
    let mut rects: Vec<Rect> = Zones::frame(sheet, l.min_bridge.max(l.min_cut_width) + l.slack).to_vec();
    for path in changed {
        if path.elements().len() > 8 * MAX_ZONES {
            return Zones::Everywhere;
        }
        rects.extend(split_subpaths(path).iter().map(|s| s.bounding_box()));
    }
    Zones::near(rects)
}

/// The cleaned cut of a design, before islands are held.
#[derive(Debug, Default)]
pub(crate) struct Prepared {
    pub cut: BezPath,
    pub colours: Vec<ColourInfo>,
    pub dropped_specks: usize,
    pub filled_holes: usize,
    pub warnings: Vec<String>,
}

/// Visible regions, colour selection, clean-up and the clip to `sheet` for the clamped params
/// `p` at `mpu` mm per unit (stages 1–4). Nothing cut is an empty `cut` with a warning.
pub(crate) fn prepare(paints: &[Paint], p: &Stencil, mpu: f64, sheet: Rect) -> Prepared {
    let units = |mm: f64| mm / mpu;
    let mut out = Prepared::default();
    let tolerance = units(p.tolerance_mm);
    let flatten = units(if p.tolerance_mm > 0.0 { p.tolerance_mm / 4.0 } else { FINEST_MM });
    let visible = visible_regions(paints, flatten, tolerance / 2.0);
    out.warnings.extend(visible.warnings);
    let selector = Selector::new(p);
    out.colours = colour_infos(&visible.regions, &selector, mpu);
    let mut chosen = BezPath::new();
    for r in visible.regions.iter().filter(|r| selector.cuts(r.colour)) {
        chosen.extend(r.path.iter());
    }
    if chosen.elements().is_empty() {
        if !visible.regions.is_empty() {
            out.warnings.push("nothing is cut: no visible colour is chosen for cutting".into());
        }
        return out;
    }
    // The visible pieces are disjoint but touch: one sweep merges them into plain contours.
    let Ok(merged) = figcraft_pathops::simplify(&chosen) else {
        out.warnings.push("couldn't merge the cut colours; nothing is cut".into());
        return out;
    };
    let limits = Limits {
        min_area: p.min_cut_area_mm2 / (mpu * mpu),
        open_radius: units(p.min_cut_width_mm) / 2.0,
        close_radius: units(p.min_bridge_mm) / 2.0,
        tolerance: tolerance / 2.0,
        fix: p.limit_mode == StencilLimitMode::Fix,
    };
    let cleaned = clean(merged, limits);
    out.warnings.extend(cleaned.warnings);
    out.dropped_specks = cleaned.dropped_specks;
    out.filled_holes = cleaned.filled_holes;
    let mut cut = cleaned.cut;
    if !cut.elements().is_empty() && !contains(sheet, cut.bounding_box()) {
        match figcraft_pathops::boolean_step(&cut, &sheet.to_path(0.1), BoolOp::Intersect) {
            Ok(c) => cut = c,
            Err(_) => out.warnings.push("couldn't clip the cut to the sheet".into()),
        }
    }
    if cut.elements().is_empty() {
        out.warnings.push("nothing is left to cut after clean-up".into());
    }
    out.cut = cut;
    out
}

fn contains(outer: Rect, inner: Rect) -> bool {
    inner.x0 >= outer.x0 && inner.y0 >= outer.y0 && inner.x1 <= outer.x1 && inner.y1 <= outer.y1
}

/// The sheet with the cut's contours reversed inside it: sheet − cut, with no sweep (the cut is
/// plain contours within the sheet), winding 1 on paper and 0 in the cut-outs.
pub(crate) fn paper(sheet: Rect, cut: &BezPath) -> BezPath {
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
