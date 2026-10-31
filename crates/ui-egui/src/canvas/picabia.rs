//! Picabia: canvas outlines on a layer's modifier copies (the hover outline shows on every copy;
//! a selected layer's copies get the faint outline boolean operands get).

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
