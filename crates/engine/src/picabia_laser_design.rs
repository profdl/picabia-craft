//! PICABIA: laser-cut any design, not just stencils: `laser.gcode` (and the design jobs of
//! `laser.start`/`laser.frame`). The layers' visible outlines are gathered in page space (shapes,
//! vectors, booleans, text as glyph outlines, modifier copies, generated results; groups, frames,
//! components and instances by their contents), converted to mm and made one
//! [`LaserSheet`]: the page is the layers' bounds, origin top-left. Closed outlines are cut
//! (cleaned, inner contours first, nearest neighbour, the largest outermost last:
//! [`figcraft_stencil::order_contours`]); open paths are cut as open polylines with the score
//! settings. No kerf compensation and no sheet outline. A single stencil layer keeps the stencil
//! path (`modifier.stencilGcode`).

use figcraft_doc::{Document, Node, NodeId, NodeKind};
use figcraft_geom::kurbo::{Affine, BezPath, PathEl, Point, Rect};
use figcraft_render::picabia::stencil as store;
use figcraft_stencil::{Contour, LaserSheet, Polyline};
use serde_json::{Value, json};

use crate::params::{get, num, targets};
use crate::picabia_laser::Out;
use crate::{EngineError, Result, Session, bad};

pub const LASER_GCODE: &str = "laser.gcode";

/// Millimetres per FigCraft unit by default: a CSS pixel (1/96 in), as SVG readers (LightBurn
/// among them) take FigCraft's SVG.
pub const DEFAULT_MM_PER_UNIT: f64 = 25.4 / 96.0;
/// Curves are flattened to within this (mm).
const FLATTEN_MM: f64 = 0.01;
/// Layers visited at most.
const MAX_VISITS: usize = 100_000;
/// Nesting followed at most.
const MAX_DEPTH: usize = 64;
/// Points kept at most (the G-code writer has its own line cap).
const MAX_POINTS: usize = 2_000_000;

/// A design ready to cut.
pub(crate) struct DesignSheet {
    pub sheet: LaserSheet,
    pub warnings: Vec<String>,
    pub mm_per_unit: f64,
    /// A name for files and jobs.
    pub base: String,
}

/// The single stencil layer `p` names, if that is what it names.
pub(crate) fn single_stencil(s: &Session, p: &Value) -> Option<NodeId> {
    let ids = targets(s, p);
    let [id] = ids.as_slice() else { return None };
    let n = s.doc().ok()?.get(*id).ok()?;
    store::is_stencil(n).then_some(*id)
}

/// Millimetres per unit: `widthMm` (the layers' bounds scaled to that width) wins over
/// `mmPerUnit`; default [`DEFAULT_MM_PER_UNIT`].
fn scale(cmd: &str, p: &Value, bounds: Rect) -> Result<f64> {
    if get(p, "widthMm").is_some() {
        let w = num(p, "widthMm").filter(|w| *w > 0.0 && *w <= 5000.0).ok_or_else(|| bad(cmd, "`widthMm` must be a width in mm (0-5000)"))?;
        return figcraft_stencil::mm_per_unit(w, bounds).ok_or_else(|| EngineError::Disabled(cmd.into(), "the layers have no width to scale".into()));
    }
    match get(p, "mmPerUnit") {
        None => Ok(DEFAULT_MM_PER_UNIT),
        Some(_) => num(p, "mmPerUnit").filter(|m| *m >= 0.001 && *m <= 100.0).ok_or_else(|| bad(cmd, "`mmPerUnit` must be 0.001-100")),
    }
}

/// The layers `p` names (`ids`, default the selection) as one sheet.
pub(crate) fn design_sheet(cmd: &str, s: &Session, p: &Value) -> Result<DesignSheet> {
    let doc = s.doc()?;
    let ids = targets(s, p);
    if ids.is_empty() {
        return Err(EngineError::Disabled(cmd.into(), "select the layers to cut".into()));
    }
    let mut g = Gather { doc, closed: Vec::new(), open: Vec::new(), warnings: Vec::new(), visits: 0, points: 0, tol: 0.1 };
    let mut bounds: Option<Rect> = None;
    let mut roots = Vec::new();
    for id in &ids {
        let n = doc.get(*id)?;
        if matches!(n.kind, NodeKind::Page { .. }) {
            return Err(bad(cmd, format!("layer {id} is a page: select the layers to cut")));
        }
        if let Some(b) = doc.world_bounds(*id).filter(|b| [b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite())) {
            bounds = Some(bounds.map_or(b, |a| a.union(b)));
        }
        roots.push(n);
    }
    let bounds = bounds.ok_or_else(|| EngineError::Disabled(cmd.into(), "the layers have no size".into()))?;
    let mpu = scale(cmd, p, bounds)?;
    g.tol = (FLATTEN_MM / mpu).max(1e-6);
    for n in roots {
        let world = doc.world_transform(n.id);
        let parent = world * n.transform.inverse();
        // A selected frame is the page the design sits on: its own rectangle isn't cut.
        g.node(n, if parent.is_finite() { parent } else { world }, 0, true);
    }
    // Page space → mm, origin at the bounds' top-left.
    let to_mm = |pts: Vec<Point>| -> Vec<Point> { pts.into_iter().map(|q| Point::new((q.x - bounds.x0) * mpu, (q.y - bounds.y0) * mpu)).collect() };
    let cut: Vec<Contour> = figcraft_stencil::order_contours(g.closed.drain(..).map(to_mm).collect());
    let score: Vec<Polyline> = order_open(g.open.drain(..).map(to_mm).filter(|l| l.len() >= 2).collect());
    if cut.is_empty() && score.is_empty() {
        let why = g.warnings.first().map(|w| format!(" ({w})")).unwrap_or_default();
        return Err(EngineError::Disabled(cmd.into(), format!("the layers have no outlines to cut{why}")));
    }
    let first = ids.first().and_then(|id| doc.get(*id).ok()).map(|n| n.name.clone()).unwrap_or_default();
    let base = crate::picabia_laser::file_safe(if ids.len() == 1 { &first } else { "Design" });
    let sheet = LaserSheet {
        w_mm: bounds.width() * mpu,
        h_mm: bounds.height() * mpu,
        cut,
        score,
        engrave: Vec::new(),
        name: "design".into(),
        loose_pieces: 0,
    };
    Ok(DesignSheet { sheet, warnings: g.warnings, mm_per_unit: mpu, base })
}

/// Open polylines in a short-travel order (nearest end first; a line may run backwards).
fn order_open(mut lines: Vec<Polyline>) -> Vec<Polyline> {
    let mut out = Vec::with_capacity(lines.len());
    let mut head = Point::ZERO;
    while !lines.is_empty() {
        let mut best = (0, false, f64::INFINITY);
        for (i, l) in lines.iter().enumerate() {
            if let (Some(&a), Some(&b)) = (l.first(), l.last()) {
                let (da, db) = ((a - head).hypot2(), (b - head).hypot2());
                if da < best.2 {
                    best = (i, false, da);
                }
                if db < best.2 {
                    best = (i, true, db);
                }
            }
            // Past a few thousand lines, take them as they come.
            if out.len() > 5000 {
                break;
            }
        }
        let mut l = lines.swap_remove(best.0);
        if best.1 {
            l.reverse();
        }
        if let Some(&e) = l.last() {
            head = e;
        }
        out.push(l);
    }
    out
}

struct Gather<'a> {
    doc: &'a Document,
    closed: Vec<Vec<Point>>,
    open: Vec<Vec<Point>>,
    warnings: Vec<String>,
    visits: usize,
    points: usize,
    /// Flattening tolerance, page units.
    tol: f64,
}

impl Gather<'_> {
    fn warn(&mut self, w: &str) {
        if !self.warnings.iter().any(|x| x == w) {
            self.warnings.push(w.to_string());
        }
    }

    /// `n` placed by `parent` (its parent's page transform), with its modifier copies.
    fn node(&mut self, n: &Node, parent: Affine, depth: usize, root: bool) {
        if !n.visible {
            return;
        }
        match figcraft_render::picabia::instance_transforms(self.doc, n) {
            Some(ts) => {
                for t in ts {
                    self.once(n, parent * t * n.transform, depth, root);
                }
            }
            None => self.once(n, parent * n.transform, depth, root),
        }
    }

    fn once(&mut self, n: &Node, xf: Affine, depth: usize, root: bool) {
        self.visits += 1;
        if depth > MAX_DEPTH || self.visits > MAX_VISITS || self.points > MAX_POINTS {
            self.warn("the design is too large or nested too deep; part of it is left out");
            return;
        }
        if !xf.is_finite() {
            return;
        }
        if let Some((shape, _)) = figcraft_render::picabia::generated::lookup(self.doc, n)
            && !shape.keep_design
        {
            for it in &shape.items {
                self.path(&it.path, xf);
            }
            return;
        }
        if n.fills.iter().any(|f| f.visible && matches!(f.kind, figcraft_color::PaintKind::Image(_) | figcraft_color::PaintKind::Pattern(_))) {
            self.warn("images are cut as their outline only");
        }
        match &n.kind {
            NodeKind::Group => self.children(n, xf, depth),
            NodeKind::Text(_) => match figcraft_render::node_outline(self.doc, n) {
                Some((p, _)) => self.path(&p, xf),
                None => self.warn("a text layer couldn't be laid out; it is left out"),
            },
            NodeKind::BooleanOp { .. } => {
                if let Some((p, _)) = figcraft_render::node_outline(self.doc, n) {
                    self.path(&p, xf);
                }
            }
            _ => {
                let container = n.kind.is_frame_like() || !n.children.is_empty();
                // A frame's own rectangle is cut when it shows (a fill or a stroke), unless it is a
                // selected frame: that is the page.
                let shows = n.fills.iter().any(|f| f.visible) || n.has_visible_stroke();
                if !(container && (root || !shows))
                    && let Some((p, _)) = figcraft_render::node_outline(self.doc, n)
                {
                    self.path(&p, xf);
                }
                if !n.children.is_empty() && !n.hides_contents() {
                    if n.clip && n.kind.is_frame_like() {
                        self.warn("clipping isn't applied: clipped layers are cut whole");
                    }
                    self.children(n, xf, depth);
                }
            }
        }
    }

    fn children(&mut self, n: &Node, xf: Affine, depth: usize) {
        let doc = self.doc;
        for c in n.children.iter().filter_map(|c| doc.node(*c)) {
            self.node(c, xf, depth + 1, false);
        }
    }

    /// `p` (local space) placed by `xf`, flattened: closed subpaths and open ones.
    fn path(&mut self, p: &BezPath, xf: Affine) {
        let mut world = p.clone();
        world.apply_affine(xf);
        let mut cur: Vec<Point> = Vec::new();
        let done = |cur: &mut Vec<Point>, closed: bool, g: &mut Self| {
            let pts = std::mem::take(cur);
            let closes = closed || (pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| (*a - *b).hypot() < 1e-9));
            g.points += pts.len();
            if pts.len() >= 2 && pts.iter().all(|q| q.x.is_finite() && q.y.is_finite()) {
                if closes { g.closed.push(pts) } else { g.open.push(pts) }
            }
        };
        let mut els: Vec<PathEl> = Vec::new();
        figcraft_geom::kurbo::flatten(&world, self.tol, |el| els.push(el));
        for el in els {
            match el {
                PathEl::MoveTo(q) => {
                    done(&mut cur, false, self);
                    cur.push(q);
                }
                PathEl::LineTo(q) => cur.push(q),
                PathEl::ClosePath => done(&mut cur, true, self),
                _ => {}
            }
        }
        done(&mut cur, false, self);
    }
}

/// `laser.gcode`: any layers as one G-code file (a single stencil layer: as
/// `modifier.stencilGcode`).
pub fn gcode(s: &mut Session, p: &Value) -> Result<Value> {
    if single_stencil(s, p).is_some() {
        return crate::picabia_laser_gcode::stencil_gcode(s, p);
    }
    for k in ["kerfMm", "overlapMm", "label", "bedWidthMm", "bedHeightMm"] {
        if get(p, k).is_some() {
            return Err(bad(LASER_GCODE, format!("`{k}` is for stencil layers only")));
        }
    }
    let out = crate::picabia_laser::out_of(LASER_GCODE, p)?;
    let material = crate::picabia_laser_gcode::material_param(LASER_GCODE, p)?.unwrap_or(figcraft_modifiers::StencilMaterial::Paper);
    let gp = crate::picabia_laser_gcode::job_params(s, material, p);
    if matches!(out, Out::Dialog) {
        let ids: Vec<Value> = targets(s, p).into_iter().map(|id| json!(id)).collect();
        s.request_ui("dialog.laserGcode", json!({"ids": ids, "material": material.id(), "gcode": gp.to_json()}));
        return Ok(json!({"dialog": true}));
    }
    let d = design_sheet(LASER_GCODE, s, p)?;
    let (w, h) = crate::picabia_laser_gcode::bed_mm(s);
    let mut warnings = d.warnings.clone();
    if d.sheet.w_mm > w + 0.01 || d.sheet.h_mm > h + 0.01 {
        warnings.push(format!("the design is {:.1} × {:.1} mm, bigger than the bed ({w} × {h} mm)", d.sheet.w_mm, d.sheet.h_mm));
    }
    let files = crate::picabia_laser_gcode::emit_one(
        LASER_GCODE,
        &d.sheet,
        &gp,
        &out,
        &format!("{} laser", d.base),
        crate::picabia_laser_gcode::MachineProfile::of(s).rapid_mm_min,
    )?;
    if out != Out::Summary {
        let mut prof = crate::picabia_laser_gcode::MachineProfile::of(s);
        prof.remember(material.id(), &gp);
        s.prefs.picabia_machine = prof.to_json();
    }
    Ok(json!({
        "files": files,
        "sheets": 1,
        "warnings": warnings,
        "material": material.id(),
        "mmPerUnit": d.mm_per_unit,
        "gcode": gp.to_json(),
    }))
}
