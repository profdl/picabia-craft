//! Picabia: canvas outlines on a layer's modifier copies (the hover outline shows on every copy;
//! a selected layer's copies get the faint outline boolean operands get); a selected stencil
//! layer's tie bars and the islands no tie holds.

use egui::{Pos2, Shape, Stroke};
use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_geom::{Affine, Point};

use super::outline::stroke;
use crate::FigApp;

/// The outline [`super::outline::draw_outline`] draws for layer `id`, once on each of its copies
/// (the original left out). Nothing for a layer without a stack.
pub(super) fn copy_outlines(app: &FigApp, painter: &egui::Painter, doc: &Document, id: NodeId, s: Stroke) {
    let copies = figcraft_render::picabia::page_copies(doc, id);
    let Some(n) = doc.node(id).filter(|_| !copies.is_empty()) else { return };
    let w = doc.world_transform(id);
    let screen = |a: Affine, p: Point| {
        let q = a * p;
        app.to_screen(q.x, q.y)
    };
    let shaped = matches!(
        n.kind,
        NodeKind::Ellipse { .. } | NodeKind::Vector { .. } | NodeKind::Polygon { .. } | NodeKind::Star { .. } | NodeKind::BooleanOp { .. }
    );
    let path = if shaped { figcraft_render::node_outline(doc, n).map(|(p, _)| p) } else { None };
    let zoom = app.session.ds().map(|d| d.view.zoom).unwrap_or(1.0).max(1e-6);
    // Many copies: only as many outlines as stay cheap to draw.
    for c in copies.into_iter().take(2000) {
        let a = c * w;
        match &path {
            Some(p) => {
                let mut cur: Vec<Pos2> = Vec::new();
                let mut lines = Vec::new();
                figcraft_geom::kurbo::flatten(p.iter(), 0.5 / zoom, |el| {
                    use figcraft_geom::kurbo::PathEl::*;
                    match el {
                        MoveTo(q) => {
                            if cur.len() > 1 {
                                lines.push(std::mem::take(&mut cur));
                            }
                            cur.clear();
                            cur.push(screen(a, q));
                        }
                        LineTo(q) => cur.push(screen(a, q)),
                        ClosePath => {
                            if let Some(f) = cur.first().copied() {
                                cur.push(f);
                            }
                        }
                        _ => {}
                    }
                });
                if cur.len() > 1 {
                    lines.push(cur);
                }
                for l in lines {
                    painter.add(Shape::line(l, s));
                }
            }
            None => {
                let corners = [Point::new(0.0, 0.0), Point::new(n.width, 0.0), Point::new(n.width, n.height), Point::new(0.0, n.height)];
                painter.add(Shape::closed_line(corners.map(|p| screen(a, p)).to_vec(), s));
            }
        }
    }
}

/// The selected layer's modifier guides (dashed, thin, in the selection colour `col`, like the
/// other canvas guides) and the handles of its modifier, drawn exactly as the arc handles are
/// (`overlays::arc_overlay`: white dots ringed in `col`, larger when hovered or dragged; the ring
/// centre with a dot inside, like the arc's Start), with the drag readout beside the dragged one.
pub(super) fn modifier_overlay(app: &FigApp, painter: &egui::Painter, col: egui::Color32) {
    use figcraft_engine::modifier_handles::{ModHandle, PageGuide};
    let s = stroke(col, 1.0);
    let sp = |p: [f64; 2]| app.to_screen(p[0], p[1]);
    for g in app.session.modifier_guides() {
        match g {
            PageGuide::Line { from, to } => {
                painter.add(Shape::dashed_line(&[sp(from), sp(to)], s, 4.0, 3.0));
            }
            PageGuide::Ellipse { center, a, b } => {
                // Enough segments for a smooth ring at this zoom, capped.
                let (c, ea, eb) = (sp(center), sp([center[0] + a[0], center[1] + a[1]]), sp([center[0] + b[0], center[1] + b[1]]));
                let r = (ea - c).length().max((eb - c).length());
                if !r.is_finite() || r < 0.5 {
                    continue;
                }
                let n = ((r * 0.5) as usize).clamp(32, 720);
                let pts: Vec<Pos2> = (0..=n)
                    .map(|i| {
                        let t = i as f64 / n as f64 * std::f64::consts::TAU;
                        sp([center[0] + a[0] * t.cos() + b[0] * t.sin(), center[1] + a[1] * t.cos() + b[1] * t.sin()])
                    })
                    .collect();
                painter.add(Shape::dashed_line(&pts, s, 4.0, 3.0));
            }
            PageGuide::Point(p) => {
                painter.circle_filled(sp(p), 1.5, col);
            }
        }
    }
    stencil_markers(app, painter, col);
    let ov = &app.session.tool.overlay;
    if let Some(hs) = app.session.modifier_handles() {
        for (h, at) in &hs.handles {
            let c = sp(*at);
            let r = if ov.modifier_hover == Some(*h) { 5.0 } else { 4.0 };
            painter.circle_filled(c, r, egui::Color32::WHITE);
            painter.circle_stroke(c, r, s);
            if *h == ModHandle::RingCenter {
                painter.circle_filled(c, 1.5, col);
            }
        }
    }
    if let Some(hint) = &ov.modifier {
        super::overlays::pill(painter, sp(hint.at) + egui::vec2(12.0, 12.0), egui::Align2::LEFT_TOP, hint.label.clone(), col);
    }
}

/// Every canvas frame: recompute stale stencil sheets of the shown document (off the UI thread
/// natively; on it once a layer stops changing, in the browser) and keep repainting until the
/// results land (the stale sheet shows meanwhile).
pub(super) fn stencil_tick(app: &FigApp, ctx: &egui::Context) {
    let Ok(ds) = app.session.ds() else { return };
    figcraft_engine::picabia_stencil::refresh(&ds.doc);
    if figcraft_engine::picabia_stencil::busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(40));
    }
}

/// Whether the closed polygon `pts` turns one way only (collinear runs allowed).
fn convex(pts: &[Pos2]) -> bool {
    let n = pts.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let (Some(a), Some(b), Some(c)) = (pts.get(i), pts.get((i + 1) % n), pts.get((i + 2) % n)) else { return false };
        let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        if !cross.is_finite() {
            return false;
        }
        if cross.abs() < 1e-3 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// The most pieces [`separate_convex`] compares pairwise; more are outlined.
const MAX_TINTED: usize = 2000;

/// Whether `polys` are separate convex pieces that can each be filled: all convex, wound the same
/// way, and none's box inside another's (a hole).
fn separate_convex(polys: &[Vec<Pos2>]) -> bool {
    if polys.len() > MAX_TINTED {
        return false;
    }
    let area = |p: &[Pos2]| p.iter().zip(p.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum::<f32>();
    let signs: Vec<f32> = polys.iter().map(|p| area(p).signum()).collect();
    if !polys.iter().all(|p| convex(p)) || signs.windows(2).any(|w| w.first() != w.get(1)) {
        return false;
    }
    let boxes: Vec<egui::Rect> = polys.iter().map(|p| egui::Rect::from_points(p)).collect();
    !boxes.iter().enumerate().any(|(i, a)| boxes.iter().enumerate().any(|(k, b)| i != k && a.contains_rect(*b) && a != b))
}

/// The most outline segments the stencil markers draw (a fine lattice over a big sheet has many).
const MAX_MARKER_SEGMENTS: usize = 50_000;

/// Over each selected stencil layer with a result: its tie bars tinted with the selection colour
/// `col` (a lattice outlined faintly in it), and every island no tie holds outlined (its bounds,
/// dashed) in the warning colour, as the panel warns of them. Islands are expected to be loose
/// with gauze, so nothing is flagged then.
fn stencil_markers(app: &FigApp, painter: &egui::Painter, col: egui::Color32) {
    use figcraft_render::picabia::stencil as store;
    let Ok(ds) = app.session.ds() else { return };
    let doc = &ds.doc;
    let zoom = ds.view.zoom.max(1e-6);
    let warn = crate::theme::Tokens::get(painter.ctx()).warning;
    let mut budget = MAX_MARKER_SEGMENTS;
    for id in &ds.selection {
        let Some(n) = doc.node(*id).filter(|n| n.visible && store::is_stencil(n)) else { continue };
        let Some((info, _)) = figcraft_engine::picabia_stencil::info(doc, n) else { continue };
        let w = doc.world_transform(*id);
        if !w.is_finite() {
            continue;
        }
        let screen = |p: Point| {
            let q = w * p;
            app.to_screen(q.x, q.y)
        };
        // The bars as screen polygons, within the budget.
        let mut polys: Vec<Vec<Pos2>> = Vec::new();
        let mut cur: Vec<Pos2> = Vec::new();
        let mut keep = |cur: &mut Vec<Pos2>| {
            if cur.len() > 2 && budget >= cur.len() {
                budget -= cur.len();
                polys.push(std::mem::take(cur));
            }
            cur.clear();
        };
        figcraft_geom::kurbo::flatten(info.ties.iter(), 0.5 / zoom, |el| {
            use figcraft_geom::kurbo::PathEl::*;
            match el {
                MoveTo(q) => {
                    keep(&mut cur);
                    cur.push(screen(q));
                }
                LineTo(q) => cur.push(screen(q)),
                ClosePath => keep(&mut cur),
                _ => {}
            }
        });
        keep(&mut cur);
        // Separate bars (convex pieces, none inside another) are tinted: an outline would cover a
        // thin bar whole. Merged lattice bars (contours with corners in and out, holes) can't be
        // filled by egui, so they're outlined faintly instead.
        if separate_convex(&polys) {
            for p in polys {
                painter.add(Shape::convex_polygon(p, col.gamma_multiply(0.4), Stroke::NONE));
            }
        } else {
            for p in polys {
                painter.add(Shape::closed_line(p, stroke(col.gamma_multiply(0.5), 1.0)));
            }
        }
        let gauze = store::frame(doc, n).is_some_and(|f| f.params.tie_mode == figcraft_modifiers::StencilTieMode::Gauze);
        if gauze {
            continue;
        }
        let loose = stroke(warn, 1.5);
        for island in info.islands.iter().filter(|i| i.tied == 0 && !i.pattern_held) {
            let b = island.bounds;
            if budget < 4 || ![b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite()) {
                continue;
            }
            budget -= 4;
            let c = [Point::new(b.x0, b.y0), Point::new(b.x1, b.y0), Point::new(b.x1, b.y1), Point::new(b.x0, b.y1), Point::new(b.x0, b.y0)];
            painter.add(Shape::dashed_line(&c.map(screen), loose, 4.0, 3.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::pos2;

    #[test]
    fn only_convex_tie_pieces_are_tinted() {
        let square = [pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0), pos2(0.0, 10.0)];
        assert!(super::convex(&square));
        let ell = [pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 5.0), pos2(5.0, 5.0), pos2(5.0, 10.0), pos2(0.0, 10.0)];
        assert!(!super::convex(&ell));
        assert!(!super::convex(&[pos2(0.0, 0.0), pos2(f32::NAN, 0.0), pos2(1.0, 1.0)]));
        // Two bars are tinted; a frame with a hole (a lattice cell) isn't.
        let shifted: Vec<_> = square.iter().map(|p| *p + egui::vec2(20.0, 0.0)).collect();
        assert!(super::separate_convex(&[square.to_vec(), shifted]));
        let hole: Vec<_> = square.iter().rev().map(|p| *p * 0.5 + egui::vec2(2.0, 2.0)).collect();
        assert!(!super::separate_convex(&[square.to_vec(), hole.clone()]));
        let same_way: Vec<_> = hole.into_iter().rev().collect();
        assert!(!super::separate_convex(&[square.to_vec(), same_way]), "nested the same way: an even-odd hole");
        assert!(!super::separate_convex(&[ell.to_vec()]));
    }
}
