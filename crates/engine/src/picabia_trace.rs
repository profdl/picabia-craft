//! PICABIA: the `imageTrace` generator: VectorCraft's Image Trace over the layer as it draws.
//!
//! The framework ([`crate::picabia_generate`]) calls [`compute`] for a layer whose generator row
//! is an `imageTrace`, and [`apply_result`] when Apply (or a `.fig`/Penpot bake) turns the result
//! into layers.
//!
//! [`compute`]:
//! 1. The layer's visible image fills set the **density** (image pixels per layer point, the
//!    densest fill's, clamped to [`MIN_DENSITY`]..[`MAX_DENSITY`]): see [`density`].
//! 2. The layer is rendered **alone** in its own space at that density, on a transparent
//!    background ([`raster`]): its own stack stripped, its fills, strokes, shape and children as
//!    they draw. Its own effects (shadows, blurs, grain) are left out: they draw nowhere once the
//!    trace replaces the layer, and a soft shadow traced turns into banded blobs that take a
//!    limited palette's colours. Layers inside it draw with theirs. Opacity and blend are left
//!    out too: the result draws with the layer's own. The raster is capped at
//!    [`figcraft_vcbridge::trace::MAX_TRACE_PIXELS`] (and 16384 px a side) *before* it is
//!    allocated: the density is scaled down, with a warning ([`raster_size`]).
//! 3. VectorCraft traces it ([`figcraft_vcbridge::trace::run`], settings from [`settings`]).
//! 4. The paths are mapped back to the layer's space and placed at each input copy, bottom first.

use figcraft_color::{ImageScaleMode, PaintKind};
use figcraft_doc::{Document, Node, NodeId, StrokeCap, StrokeJoin};
use figcraft_geom::kurbo::BezPath;
use figcraft_geom::{Affine, Rect};
use figcraft_modifiers::{ImageTrace, TraceMethod, TraceMode, TracePalette};
use figcraft_render::picabia::generated::{self as store, GenFrame, GenItem, GenPaint, GenShape};
use figcraft_vcbridge::BridgeError;
use figcraft_vcbridge::trace::{self as vt, Rgba, TraceSettings, Traced};
use serde_json::{Value, json};

use crate::Result;
use crate::picabia_generate::{GenOut, Info};

/// The fewest image pixels per layer point traced.
pub const MIN_DENSITY: f64 = 0.05;
/// The most image pixels per layer point traced.
pub const MAX_DENSITY: f64 = 16.0;
/// The widest or tallest raster the renderer makes.
pub const MAX_SIDE: u32 = 16_384;
/// Images more than this many pixels on a side trace more slowly (VectorCraft says so too).
pub const LARGE_TRACE_SIDE: u32 = 1500;

/// The warning for a layer without a visible image fill.
pub const NO_IMAGE: &str = "imageTrace needs a layer with an image fill";
/// The warning for an empty trace.
pub const NOTHING: &str = "nothing to trace: the image is empty, transparent or ignored";
/// The warning for a trace past the anchor budget.
pub const TOO_DETAILED: &str = "the trace is too detailed";

/// The bridge settings for the modifier's (clamped) params: every field maps one to one.
pub fn settings(p: &ImageTrace) -> TraceSettings {
    TraceSettings {
        mode: match p.mode {
            TraceMode::BlackAndWhite => vt::Mode::BlackAndWhite,
            TraceMode::Grayscale => vt::Mode::Grayscale,
            TraceMode::Color => vt::Mode::Color,
            TraceMode::Logo => vt::Mode::Logo,
        },
        palette: match p.palette {
            TracePalette::Limited | TracePalette::DocumentLibrary => vt::Palette::Limited,
            TracePalette::FullTone => vt::Palette::FullTone,
            TracePalette::Automatic => vt::Palette::Automatic,
        },
        colors: p.colors,
        color_detail: p.color_detail,
        threshold: u8::try_from(p.threshold.min(255)).unwrap_or(u8::MAX),
        paths: p.paths,
        corners: p.corners,
        noise: p.noise,
        method: match p.method {
            TraceMethod::Abutting => vt::Method::Abutting,
            TraceMethod::Overlapping => vt::Method::Overlapping,
        },
        fills: p.fills,
        strokes: p.strokes,
        stroke_width: p.stroke_width,
        snap_curves_to_lines: p.snap_curves_to_lines,
        ignore_white: p.ignore_white,
        ignore_color: p.ignore_color.as_deref().and_then(figcraft_modifiers::parse_hex_colour),
    }
}

/// Whether paint `p` is a visible image fill.
fn image_fill(p: &figcraft_color::Paint) -> Option<&figcraft_color::ImagePaint> {
    match &p.kind {
        PaintKind::Image(ip) if p.visible && p.opacity > 0.0 => Some(ip),
        _ => None,
    }
}

/// Whether layer `n` has a visible image fill.
pub fn has_image(n: &Node) -> bool {
    n.fills.iter().any(|f| image_fill(f).is_some())
}

/// Image pixels per layer point of `n`'s densest visible image fill, as each fill maps its image
/// onto the box: Fill covers the box (the image's tighter axis), Fit fits inside it (its looser
/// axis), Crop maps the box through the crop transform, Tile repeats it at `scale` points per
/// pixel; a 90° rotation swaps the image's sides. Clamped to [`MIN_DENSITY`]..[`MAX_DENSITY`];
/// 1 when no fill's image is in the document. `None`: no visible image fill.
pub fn density(doc: &Document, n: &Node) -> Option<f64> {
    let (w, h) = (n.width.abs().max(1e-6), n.height.abs().max(1e-6));
    let mut found = false;
    let mut best: Option<f64> = None;
    for ip in n.fills.iter().filter_map(image_fill) {
        found = true;
        let Some(img) = doc.images.get(&ip.image) else { continue };
        let (iw, ih) = (f64::from(img.width.max(1)), f64::from(img.height.max(1)));
        let (rw, rh) = ip.rotated_size(iw, ih);
        let d = match ip.scale_mode {
            ImageScaleMode::Fill => (rw / w).min(rh / h),
            ImageScaleMode::Fit => (rw / w).max(rh / h),
            ImageScaleMode::Crop => {
                let c = ip.effective_crop(iw, ih, w, h);
                (rw * c[0].hypot(c[1]) / w).max(rh * c[2].hypot(c[3]) / h)
            }
            ImageScaleMode::Tile => 1.0 / if ip.scale.is_finite() && ip.scale > 0.0 { ip.scale } else { 1.0 },
        };
        if d.is_finite() && d > 0.0 {
            best = Some(best.map_or(d, |b: f64| b.max(d)));
        }
    }
    found.then(|| best.unwrap_or(1.0).clamp(MIN_DENSITY, MAX_DENSITY))
}

/// The raster for area `b` (layer points) at `density`: its density, width and height in
/// pixels, and whether the density was scaled down to keep it within `max_pixels` and
/// [`MAX_SIDE`] a side. Worked out before anything is allocated. `None`: an empty or non-finite
/// area.
pub fn raster_size(b: Rect, density: f64, max_pixels: u64) -> Option<(f64, u32, u32, bool)> {
    if !(b.is_finite() && b.width() > 0.0 && b.height() > 0.0 && density.is_finite() && density > 0.0) {
        return None;
    }
    let side = |len: f64, d: f64| (len * d - 1e-6).ceil().max(1.0);
    let (mut w, mut h) = (side(b.width(), density), side(b.height(), density));
    let max_pixels = max_pixels.max(1) as f64;
    let mut d = density;
    let mut scaled = false;
    for _ in 0..8 {
        let k = (max_pixels / (w * h)).sqrt().min(f64::from(MAX_SIDE) / w.max(h));
        if k >= 1.0 {
            break;
        }
        scaled = true;
        // A little under, so the rounded-up sides fit.
        d *= k * 0.999;
        w = side(b.width(), d);
        h = side(b.height(), d);
    }
    if w * h > max_pixels || w.max(h) > f64::from(MAX_SIDE) {
        // A sliver: shrink its long side alone.
        w = w.min(f64::from(MAX_SIDE)).min((max_pixels / h).floor().max(1.0));
        h = h.min(f64::from(MAX_SIDE)).min((max_pixels / w).floor().max(1.0));
        scaled = true;
    }
    Some((d, w as u32, h as u32, scaled))
}

/// A copy of `doc` in which layer `id` draws by itself in its own space (page space = its space),
/// as it draws but for its stack, effects, opacity, blend and mask; its ancestors' stacks are
/// stripped so their copies don't count. `None` when `id` is gone or its parent's transform
/// can't be inverted.
fn alone(doc: &Document, id: NodeId) -> Option<Document> {
    let mut tmp = doc.clone();
    let parent = doc.node(id)?.parent;
    let pw = parent.map(|p| doc.world_transform(p)).unwrap_or(Affine::IDENTITY);
    if !(pw.is_finite() && pw.determinant().abs() > 1e-12) {
        return None;
    }
    for a in doc.ancestors(id) {
        if let Some(an) = tmp.node_mut(a) {
            figcraft_render::picabia::strip_stack(an);
        }
    }
    let n = tmp.node_mut(id)?;
    figcraft_render::picabia::strip_stack(n);
    n.is_mask = false;
    n.visible = true;
    n.effects.clear();
    n.opacity = 1.0;
    n.blend = figcraft_color::BlendMode::Normal;
    n.transform = pw.inverse();
    Some(tmp)
}

/// A rendered raster: straight RGBA, the layer-space area it shows and its density.
pub struct Raster {
    pub rgba: Rgba,
    /// The layer-space area (its top-left is the raster's).
    pub area: Rect,
    /// Pixels per layer point, after any scaling down.
    pub density: f64,
    /// The density was scaled down to the pixel budget.
    pub scaled: bool,
}

/// Layer `id` rendered alone in its own space at `density`, within `max_pixels`. `None`: nothing
/// to render (an empty layer or a render failure).
pub fn raster(doc: &Document, id: NodeId, density: f64, max_pixels: u64) -> Option<Raster> {
    let tmp = alone(doc, id)?;
    let area = figcraft_render::paint_bounds(&tmp, id)?;
    let (d, w, h, scaled) = raster_size(area, density, max_pixels)?;
    // The pixmap `render_area` makes is `ceil(area · d)`: the size worked out above.
    let pix = figcraft_render::render_area(&tmp, &[id], area, d)?;
    let (pw, ph) = (pix.width(), pix.height());
    if (pw, ph) != (w, h) {
        log::warn!("imageTrace: rendered {pw} x {ph}, expected {w} x {h}");
    }
    let rgba = Rgba::new(pw, ph, figcraft_render::to_rgba8(&pix)).ok()?;
    Some(Raster { rgba, area, density: d, scaled })
}

/// `traced`'s paths as result items in layer space (raster `r`), placed at each of `inputs`,
/// bottom first, made by layer `source`.
fn items(traced: &Traced, r: &Raster, inputs: &[Affine], source: NodeId) -> Vec<GenItem> {
    let (sw, sh) = traced.source_size;
    let (tw, th) = traced.size;
    let kx = if tw > 0 { f64::from(sw) / f64::from(tw) } else { 1.0 };
    let ky = if th > 0 { f64::from(sh) / f64::from(th) } else { 1.0 };
    let px = Affine::translate((r.area.x0, r.area.y0)) * Affine::scale_non_uniform(kx / r.density, ky / r.density);
    let px_width = (kx * ky).sqrt() / r.density;
    let mut out = Vec::with_capacity(traced.paths.len() * inputs.len());
    for c in inputs {
        let m = *c * px;
        let grow = c.determinant().abs().sqrt();
        for p in &traced.paths {
            let mut path: BezPath = p.path.clone();
            path.apply_affine(m);
            let [r8, g8, b8] = p.rgb;
            let paint = match p.stroke {
                Some(w) => GenPaint::Stroke {
                    width: (w * px_width * grow).max(0.0),
                    cap: if p.square_caps { StrokeCap::Square } else { StrokeCap::Round },
                    join: StrokeJoin::Round,
                },
                None => GenPaint::Fill { even_odd: false },
            };
            out.push(GenItem {
                path,
                paint,
                rgba: [f32::from(r8) / 255.0, f32::from(g8) / 255.0, f32::from(b8) / 255.0, 1.0],
                source: Some(source),
                name: figcraft_modifiers::hex_colour(p.rgb),
            });
        }
    }
    out
}

/// The result kept when a new trace can't be made: the previous one (its items and stats), if
/// the layer has one drawn, else the design. Either way with `warning`.
fn previous_or_design(doc: &Document, n: &Node, warning: String) -> GenOut {
    if let Some((shape, _)) = store::lookup(doc, n)
        && !shape.keep_design
    {
        let stats = shape.extra.clone().and_then(|e| e.downcast::<Info>().ok()).map_or_else(|| json!({}), |i| i.stats.clone());
        let mut out = GenOut::new(shape.items.clone(), stats);
        out.warnings.push(warning);
        return out;
    }
    GenOut::keep_design(warning)
}

/// Layer `n`'s trace for `frame` (its input copies in `frame.inputs`, maps of its own space),
/// with params `p` (clamped). The result is in the layer's own space, bottom first.
pub fn compute(doc: &Document, n: &Node, frame: &GenFrame, p: &ImageTrace) -> GenOut {
    compute_within(doc, n, frame, p, vt::MAX_TRACE_PIXELS)
}

/// [`compute`] with the raster capped at `max_pixels` (tests lower it).
pub fn compute_within(doc: &Document, n: &Node, frame: &GenFrame, p: &ImageTrace, max_pixels: u64) -> GenOut {
    let Some(want) = density(doc, n) else { return GenOut::keep_design(NO_IMAGE) };
    let max_pixels = max_pixels.min(vt::MAX_TRACE_PIXELS);
    let Some(r) = raster(doc, n.id, want, max_pixels) else {
        return GenOut::new(Vec::new(), stats(0, 0, 0, (0, 0))).warn(NOTHING);
    };
    let s = settings(p);
    let traced = match vt::run(&r.rgba, &s, vt::MAX_ANCHORS, &|| false) {
        Ok(t) => t,
        Err(BridgeError::TooComplex) => return previous_or_design(doc, n, TOO_DETAILED.into()),
        Err(e) => return previous_or_design(doc, n, format!("the image couldn't be traced: {e}")),
    };
    // Every input copy repeats the paths: the drawn result keeps to the anchor budget too.
    if traced.anchors.saturating_mul(frame.inputs.len().max(1)) > vt::MAX_ANCHORS {
        return previous_or_design(doc, n, TOO_DETAILED.into());
    }
    let mut warnings = Vec::new();
    let (tw, th) = traced.size;
    if r.scaled || traced.downscaled {
        warnings.push(format!("the image is traced at {tw} × {th} px, scaled down to stay within {} megapixels", vt::MAX_TRACE_PIXELS >> 20));
    }
    if tw.max(th) > LARGE_TRACE_SIDE && p.mode != TraceMode::Logo {
        warnings.push(format!(
            "The image is {tw} × {th} pixels, more than {LARGE_TRACE_SIDE} on a side: it is traced, but more slowly. A lower-fidelity preset or a smaller copy of the image traces faster."
        ));
    }
    if traced.paths.is_empty() {
        warnings.push(NOTHING.into());
    }
    let st = stats(traced.paths.len(), traced.anchors, traced.palette.len(), traced.size);
    let mut out = GenOut::new(items(&traced, &r, &frame.inputs, n.id), st);
    out.warnings = warnings;
    out
}

fn stats(paths: usize, anchors: usize, colours: usize, (w, h): (u32, u32)) -> Value {
    json!({"paths": paths, "anchors": anchors, "colours": colours, "tracedPx": {"w": w, "h": h}})
}

trait Warn {
    fn warn(self, w: &str) -> Self;
}

impl Warn for GenOut {
    fn warn(mut self, w: &str) -> Self {
        self.warnings.push(w.into());
        self
    }
}

/// Replace layer `id` by the group Apply makes of its trace `shape`: "<layer> (traced)", one
/// vector layer per traced path, bottom first, each named by its colour.
pub fn apply_result(doc: &mut Document, id: NodeId, shape: &GenShape) -> Result<NodeId> {
    let name = format!("{} (traced)", doc.get(id)?.name);
    crate::picabia_generate::replace_with_items(doc, id, shape, &name)
}
