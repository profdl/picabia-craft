//! PICABIA: Picabia's modifier icons, drawn in code from simple primitives on the same 16×16 grid,
//! stroke weight and scaling as FigCraft's own set (`icons.rs`). Original artwork for Picabia
//! (MIT OR Apache-2.0): designed from scratch, no third-party icon is used, traced or redrawn.
//!
//! They live apart from [`crate::icons::Icon`] so upstream's icon list stays untouched; [`button`]
//! is [`crate::icons::button`]'s look for these glyphs.

use std::f32::consts::{FRAC_PI_6, TAU};

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

/// A Picabia glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    /// The Modifiers section: a square with two copies stacked behind it.
    Modifiers,
    /// Linear array: three squares in a row, the original filled.
    LinearArray,
    /// Circular array: six rings round a centre point.
    CircularArray,
    /// Grid array: a 3×3 block of squares, the original filled.
    GridArray,
    /// Mirror: a filled triangle and its outlined reflection across a dashed axis.
    Mirror,
    /// Transform each: three squares, each larger and turned further than the one before.
    TransformEach,
    /// Randomize: four squares scattered at different angles.
    Randomize,
    /// Wallpaper: a 2×2 tile, each cell holding a corner triangle turned a quarter more.
    Wallpaper,
}

impl Glyph {
    /// Every glyph (tests, the gallery).
    pub const ALL: &[Glyph] = &[
        Glyph::Modifiers,
        Glyph::LinearArray,
        Glyph::CircularArray,
        Glyph::GridArray,
        Glyph::Mirror,
        Glyph::TransformEach,
        Glyph::Randomize,
        Glyph::Wallpaper,
    ];

    /// The glyph of modifier kind id `kind` (the section's for kinds this build doesn't know).
    pub fn of_kind(kind: &str) -> Glyph {
        match kind {
            "linearArray" => Glyph::LinearArray,
            "circularArray" => Glyph::CircularArray,
            "gridArray" => Glyph::GridArray,
            "mirror" => Glyph::Mirror,
            "transformEach" => Glyph::TransformEach,
            "randomize" => Glyph::Randomize,
            "wallpaper" => Glyph::Wallpaper,
            _ => Glyph::Modifiers,
        }
    }
}

/// Grid-unit painter: the same scale, origin snapping and stroke width rule as `icons::paint`.
struct G<'a> {
    painter: &'a Painter,
    o: Pos2,
    s: f32,
    px: f32,
    stroke: Stroke,
    color: Color32,
}

impl G<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.o.x + x * self.s, self.o.y + y * self.s)
    }
    /// A stroke coordinate on the pixel grid (pixel centres for odd widths).
    fn snap(&self, v: f32) -> f32 {
        let odd = (self.stroke.width / self.px).round() as i32 % 2 == 1;
        let off = if odd { 0.5 } else { 0.0 };
        ((v / self.px - off).round() + off) * self.px
    }
    fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let (a, b) = (self.at(x0, y0), self.at(x1, y1));
        let (a, b) = if y0 == y1 {
            (pos2(a.x, self.snap(a.y)), pos2(b.x, self.snap(b.y)))
        } else if x0 == x1 {
            (pos2(self.snap(a.x), a.y), pos2(self.snap(b.x), b.y))
        } else {
            (a, b)
        };
        self.painter.line_segment([a, b], self.stroke);
    }
    /// An axis-aligned square outline, hinted onto the pixel grid.
    fn square(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let (a, b) = (self.at(x0, y0), self.at(x1, y1));
        let r = Rect::from_min_max(pos2(self.snap(a.x), self.snap(a.y)), pos2(self.snap(b.x), self.snap(b.y)));
        self.painter.rect_stroke(r, egui::CornerRadius::ZERO, self.stroke, egui::StrokeKind::Middle);
    }
    /// A filled axis-aligned box, its edges on the pixel grid.
    fn block(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let e = |v: f32| (v / self.px).round() * self.px;
        let (a, b) = (self.at(x0, y0), self.at(x1, y1));
        self.painter.rect_filled(Rect::from_min_max(pos2(e(a.x), e(a.y)), pos2(e(b.x), e(b.y))), egui::CornerRadius::ZERO, self.color);
    }
    /// The corners of a square `size` wide centred on (`cx`, `cy`), turned `angle` radians.
    fn turned(&self, cx: f32, cy: f32, size: f32, angle: f32) -> Vec<Pos2> {
        let h = size / 2.0;
        let (sin, cos) = angle.sin_cos();
        [(-h, -h), (h, -h), (h, h), (-h, h)].iter().map(|&(x, y)| self.at(cx + x * cos - y * sin, cy + x * sin + y * cos)).collect()
    }
    fn outline(&self, pts: Vec<Pos2>) {
        self.painter.add(Shape::closed_line(pts, self.stroke));
    }
    fn fill(&self, pts: Vec<Pos2>) {
        self.painter.add(Shape::convex_polygon(pts, self.color, Stroke::NONE));
    }
    fn ring(&self, x: f32, y: f32, r: f32) {
        self.painter.circle_stroke(self.at(x, y), r * self.s, self.stroke);
    }
    fn dot(&self, x: f32, y: f32, r: f32) {
        self.painter.circle_filled(self.at(x, y), r * self.s, self.color);
    }
}

/// Paint `glyph` centred in `rect` (a 16-unit glyph scaled to fit) in `color`.
pub fn paint(painter: &Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let ppp = painter.ctx().pixels_per_point();
    let px = 1.0 / ppp;
    // Whole physical-pixel steps per grid unit from 16 px up (quarter steps on fractional
    // scales), the exact fit below: as `icons::paint`.
    let k = rect.width().min(rect.height()) * ppp / 16.0;
    let k = if k >= 1.0 { (k * 4.0).floor() / 4.0 } else { k };
    if !k.is_finite() || k <= 0.0 {
        return;
    }
    let s = k * px;
    let snap = |v: f32| (v * ppp).round() * px;
    let o = rect.center() - vec2(8.0 * s, 8.0 * s);
    let o = pos2(snap(o.x), snap(o.y));
    let w = (k * 0.9).round().max(1.0) * px;
    let g = G { painter, o, s, px, stroke: Stroke::new(w, color), color };
    draw(&g, glyph);
}

fn draw(g: &G, glyph: Glyph) {
    match glyph {
        Glyph::Modifiers => {
            g.square(2.5, 6.5, 9.5, 13.5);
            g.line(4.5, 4.5, 11.5, 4.5);
            g.line(11.5, 4.5, 11.5, 11.5);
            g.line(6.5, 2.5, 13.5, 2.5);
            g.line(13.5, 2.5, 13.5, 9.5);
        }
        Glyph::LinearArray => {
            g.block(1.0, 6.0, 5.0, 10.0);
            g.square(6.5, 6.5, 9.5, 9.5);
            g.square(11.5, 6.5, 14.5, 9.5);
        }
        Glyph::CircularArray => {
            for i in 0..6 {
                let a = -FRAC_PI_6 * 3.0 + i as f32 * TAU / 6.0;
                g.ring(8.0 + 5.2 * a.cos(), 8.0 + 5.2 * a.sin(), 1.5);
            }
            g.dot(8.0, 8.0, 1.0);
        }
        Glyph::GridArray => {
            for row in 0..3 {
                for col in 0..3 {
                    let (x, y) = (2.0 + col as f32 * 4.5, 2.0 + row as f32 * 4.5);
                    if row == 0 && col == 0 {
                        g.block(x - 0.5, y - 0.5, x + 3.5, y + 3.5);
                    } else {
                        g.square(x + 0.5, y + 0.5, x + 2.5, y + 2.5);
                    }
                }
            }
        }
        Glyph::Mirror => {
            for y in [1.5, 4.5, 7.5, 10.5, 13.5] {
                g.line(8.0, y, 8.0, y + 1.5);
            }
            g.fill(vec![g.at(6.0, 3.5), g.at(6.0, 12.5), g.at(1.5, 12.5)]);
            g.outline(vec![g.at(10.0, 3.5), g.at(14.5, 12.5), g.at(10.0, 12.5)]);
        }
        Glyph::TransformEach => {
            g.fill(g.turned(2.75, 9.5, 3.0, 0.0));
            g.outline(g.turned(7.25, 8.75, 4.0, 0.35));
            g.outline(g.turned(12.25, 7.75, 5.0, 0.7));
        }
        Glyph::Randomize => {
            g.fill(g.turned(4.0, 4.5, 3.5, 0.3));
            g.outline(g.turned(11.5, 3.75, 3.0, -0.45));
            g.outline(g.turned(5.0, 11.75, 3.0, 0.75));
            g.outline(g.turned(11.75, 11.0, 3.5, 0.15));
        }
        Glyph::Wallpaper => {
            g.square(1.5, 1.5, 14.5, 14.5);
            g.line(8.0, 1.5, 8.0, 14.5);
            g.line(1.5, 8.0, 14.5, 8.0);
            // Each cell's triangle sits in the corner nearest the tile's centre: four turns.
            g.fill(vec![g.at(7.0, 7.0), g.at(3.5, 7.0), g.at(7.0, 3.5)]);
            g.fill(vec![g.at(9.0, 7.0), g.at(9.0, 3.5), g.at(12.5, 7.0)]);
            g.fill(vec![g.at(9.0, 9.0), g.at(12.5, 9.0), g.at(9.0, 12.5)]);
            g.fill(vec![g.at(7.0, 9.0), g.at(7.0, 12.5), g.at(3.5, 9.0)]);
        }
    }
}

/// A square button showing `glyph`: [`crate::icons::button`]'s size, fills, colours, tooltip and
/// screen-reader toggle state, for a Picabia glyph.
pub fn button(ui: &mut egui::Ui, glyph: Glyph, size: f32, active: bool, tooltip: &str) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), egui::Sense::click());
    let bg = if active {
        t.selected_row
    } else if resp.is_pointer_button_down_on() {
        t.pressed
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(5), bg);
    let col = if active { t.accent_text } else { t.icon };
    let glyph_size = if size >= 22.0 { 16.0 } else { 12.0 };
    paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(glyph_size)), glyph, col);
    if tooltip.is_empty() {
        return resp;
    }
    let resp = crate::a11y::Tip::tip(resp, tooltip);
    if active {
        crate::a11y::toggle(&resp, tooltip, true);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every glyph paints at every size the UI uses (and odd positions) without panicking, and
    /// draws something.
    #[test]
    fn every_glyph_paints_at_every_size() {
        let ctx = egui::Context::default();
        for &glyph in Glyph::ALL {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                for size in [0.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0] {
                    paint(ui.painter(), Rect::from_min_size(pos2(3.3, 7.7), Vec2::splat(size)), glyph, Color32::WHITE);
                }
            });
            out.textures_delta.clear();
            assert!(out.shapes.len() >= 6, "{glyph:?} draws at each size");
        }
    }

    /// No two glyphs draw alike, and none is FigCraft's own logo or layout grid icon.
    #[test]
    fn glyphs_differ() {
        let draw = |f: &dyn Fn(&Painter, Rect)| {
            let ctx = egui::Context::default();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| f(ui.painter(), Rect::from_min_size(Pos2::ZERO, Vec2::splat(32.0))));
            out.textures_delta.clear();
            format!("{:?}", out.shapes.into_iter().map(|c| c.shape).collect::<Vec<_>>())
        };
        let mut seen: Vec<String> = [crate::icons::Icon::Logo, crate::icons::Icon::Grid]
            .iter()
            .map(|&i| draw(&|p, r| crate::icons::paint(p, r, i, Color32::WHITE)))
            .collect();
        for &glyph in Glyph::ALL {
            let d = draw(&|p, r| paint(p, r, glyph, Color32::WHITE));
            assert!(!seen.contains(&d), "{glyph:?} draws like another icon");
            seen.push(d);
        }
    }

    #[test]
    fn every_kind_has_its_own_glyph() {
        for (kind, _) in figcraft_modifiers::KINDS {
            assert_ne!(Glyph::of_kind(kind), Glyph::Modifiers, "{kind}");
        }
        assert_eq!(Glyph::of_kind("noSuchKind"), Glyph::Modifiers);
    }
}
