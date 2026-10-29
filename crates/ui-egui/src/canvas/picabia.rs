//! Picabia: canvas outlines on a layer's modifier copies (the hover outline shows on every copy;
//! a selected layer's copies get the faint outline boolean operands get).

use egui::{Pos2, Shape, Stroke};
use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_geom::{Affine, Point};

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
