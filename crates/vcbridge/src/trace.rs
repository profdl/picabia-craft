//! Image Trace over an RGBA8 raster, by VectorCraft's tracer.
//!
//! Budgets (so a huge photo cannot freeze or exhaust memory): an input over [`MAX_TRACE_PIXELS`]
//! (16 megapixels natively, 1 in the browser, where the trace runs on the UI thread) is first
//! box-filtered down, aspect kept, and the result says so ([`Traced::downscaled`], paths are in the *traced* size's pixel space, see
//! [`Traced::size`] and [`Traced::source_size`]); a trace with more than [`MAX_ANCHORS`] anchors
//! (8 million natively, 2 million in the browser) is refused as [`BridgeError::TooComplex`].

use figcraft_geom::kurbo::BezPath;
use vectorcraft_trace::{Method as VcMethod, Mode as VcMode, Palette as VcPalette, Raster, TraceError, TraceParams, preset, trace_watched};

use crate::BridgeError;
use crate::path::from_vc;

/// Most pixels traced at once; a larger raster is box-filtered down first.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_TRACE_PIXELS: u64 = 16 << 20;
/// Most pixels traced at once; a larger raster is box-filtered down first.
/// In the browser the trace runs on the UI thread: 1 MP keeps a trace near a second (a web worker
/// would lift this).
#[cfg(target_arch = "wasm32")]
pub const MAX_TRACE_PIXELS: u64 = 1 << 20;

/// Most anchors a trace may make before it is refused.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_ANCHORS: usize = 8_000_000;
/// Most anchors a trace may make before it is refused.
#[cfg(target_arch = "wasm32")]
pub const MAX_ANCHORS: usize = 2_000_000;

/// A straight-alpha RGBA8 image, row-major, top row first. `pixels.len()` must be
/// `width * height * 4` (checked when it is traced).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    /// An image from its bytes, checking their length.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, BridgeError> {
        let img = Self { width, height, pixels };
        img.check()?;
        Ok(img)
    }

    fn check(&self) -> Result<(), BridgeError> {
        let want = (self.width as usize).checked_mul(self.height as usize).and_then(|n| n.checked_mul(4));
        if want == Some(self.pixels.len()) {
            Ok(())
        } else {
            Err(BridgeError::BadImage(format!("{} bytes for {} x {}", self.pixels.len(), self.width, self.height)))
        }
    }

    /// VectorCraft's raster (a copy), checking the length.
    pub fn to_raster(&self) -> Result<Raster, BridgeError> {
        self.check()?;
        Raster::new(self.width, self.height, self.pixels.clone()).map_err(|e| BridgeError::BadImage(e.to_string()))
    }
}

/// Image Trace mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    BlackAndWhite,
    Grayscale,
    Color,
    /// Flat Logo.
    Logo,
}

/// Where Color mode's colours come from (Document Library is not offered).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Palette {
    #[default]
    Limited,
    FullTone,
    Automatic,
}

/// How colour layers relate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Method {
    #[default]
    Abutting,
    Overlapping,
}

/// Every field of the `imageTrace` modifier table (`docs/picabia/modifiers.md`) except the
/// `preset` name, which only fills the others ([`preset_settings`]). Out-of-range values are
/// clamped by [`params`]; the defaults are VectorCraft's `[Default]` preset.
#[derive(Clone, Debug, PartialEq)]
pub struct TraceSettings {
    pub mode: Mode,
    pub palette: Palette,
    /// Colours (limited, logo: 2 to 10) or grays (grayscale), 2 to 256.
    pub colors: u32,
    /// Full tone and automatic palettes, 0 to 100.
    pub color_detail: f64,
    /// Black and white: pixels darker than this are black, 0 to 255.
    pub threshold: u8,
    /// How tightly paths follow pixels, 0 to 100.
    pub paths: f64,
    /// More corners when higher, 0 to 100.
    pub corners: f64,
    /// Areas smaller than this many pixels merge away, 1 to 100.
    pub noise: u32,
    pub method: Method,
    pub fills: bool,
    pub strokes: bool,
    /// The widest part traced as a stroked line, in image pixels, 1 to 100.
    pub stroke_width: f64,
    pub snap_curves_to_lines: bool,
    pub ignore_white: bool,
    /// With `ignore_white`: the colour left out instead of white.
    pub ignore_color: Option<[u8; 3]>,
}

impl Default for TraceSettings {
    fn default() -> Self {
        Self::from_vc(&TraceParams::default())
    }
}

fn unit(v: f64, default: f64, lo: f64, hi: f64) -> f64 {
    if v.is_finite() { v.clamp(lo, hi) } else { default }
}

impl TraceSettings {
    /// The settings that make VectorCraft's `p` (its Shapes, Auto Grouping, Gradients, Document
    /// Library and logo colour list, which are not offered, are dropped).
    pub fn from_vc(p: &TraceParams) -> Self {
        let rgb = p.ignore_color.as_deref().and_then(parse_hex);
        Self {
            mode: match p.mode {
                VcMode::BlackAndWhite => Mode::BlackAndWhite,
                VcMode::Grayscale => Mode::Grayscale,
                VcMode::Color => Mode::Color,
                VcMode::Logo => Mode::Logo,
            },
            palette: match p.palette {
                VcPalette::Limited | VcPalette::DocumentLibrary => Palette::Limited,
                VcPalette::FullTone => Palette::FullTone,
                VcPalette::Automatic => Palette::Automatic,
            },
            colors: p.colors,
            color_detail: p.color_detail,
            threshold: p.threshold,
            paths: p.paths,
            corners: p.corners,
            noise: p.noise,
            method: match p.method {
                VcMethod::Abutting => Method::Abutting,
                VcMethod::Overlapping => Method::Overlapping,
            },
            fills: p.fills,
            strokes: p.strokes,
            stroke_width: p.stroke_width,
            snap_curves_to_lines: p.snap_curves_to_lines,
            ignore_white: p.ignore_white,
            ignore_color: rgb,
        }
    }
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some([v(0)?, v(2)?, v(4)?])
}

/// The modifier's preset ids (camelCase) and VectorCraft's names, in panel order. `custom` is not
/// a preset.
pub const PRESETS: [(&str, &str); 15] = [
    ("default", "Default"),
    ("autoColor", "Auto Color"),
    ("highFidelityPhoto", "High Fidelity Photo"),
    ("lowFidelityPhoto", "Low Fidelity Photo"),
    ("threeColors", "3 Colors"),
    ("sixColors", "6 Colors"),
    ("sixteenColors", "16 Colors"),
    ("shadesOfGray", "Shades of Gray"),
    ("blackAndWhiteLogo", "Black and White Logo"),
    ("sketchedArt", "Sketched Art"),
    ("silhouettes", "Silhouettes"),
    ("lineArt", "Line Art"),
    ("technicalDrawing", "Technical Drawing"),
    ("outline", "Outline"),
    ("flatLogo", "Flat Logo"),
];

/// The settings of preset `id` (a camelCase id from [`PRESETS`]); `None` for `custom` or unknown.
pub fn preset_settings(id: &str) -> Option<TraceSettings> {
    let (_, name) = PRESETS.iter().find(|(i, _)| *i == id)?;
    Some(TraceSettings::from_vc(&preset(name)?))
}

/// VectorCraft's parameters for `s`, every field clamped into its range (a non-finite number takes
/// the default; Fills and Strokes both off turns Fills on).
pub fn params(s: &TraceSettings) -> TraceParams {
    let d = TraceParams::default();
    let mode = match s.mode {
        Mode::BlackAndWhite => VcMode::BlackAndWhite,
        Mode::Grayscale => VcMode::Grayscale,
        Mode::Color => VcMode::Color,
        Mode::Logo => VcMode::Logo,
    };
    let colors = if s.mode == Mode::Logo { s.colors.clamp(2, 10) } else { s.colors.clamp(2, 256) };
    let strokes = s.strokes;
    TraceParams {
        mode,
        threshold: s.threshold,
        palette: match s.palette {
            Palette::Limited => VcPalette::Limited,
            Palette::FullTone => VcPalette::FullTone,
            Palette::Automatic => VcPalette::Automatic,
        },
        colors,
        color_detail: unit(s.color_detail, d.color_detail, 0.0, 100.0),
        paths: unit(s.paths, d.paths, 0.0, 100.0),
        corners: unit(s.corners, d.corners, 0.0, 100.0),
        noise: s.noise.clamp(1, 100),
        method: match s.method {
            Method::Abutting => VcMethod::Abutting,
            Method::Overlapping => VcMethod::Overlapping,
        },
        ignore_white: s.ignore_white,
        ignore_color: s.ignore_color.filter(|_| s.ignore_white).map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}")),
        snap_curves_to_lines: s.snap_curves_to_lines,
        fills: s.fills || !strokes,
        strokes,
        stroke_width: unit(s.stroke_width, d.stroke_width, 1.0, 100.0),
        ..d
    }
}

/// One traced path, in the pixel space of [`Traced::size`] (x right, y down).
#[derive(Clone, Debug, PartialEq)]
pub struct TracedOut {
    pub path: BezPath,
    pub rgb: [u8; 3],
    /// `Some(width)`: stroke the path this wide (image px) instead of filling it.
    pub stroke: Option<f64>,
    /// A stroke's ends are square, not round.
    pub square_caps: bool,
}

/// A finished trace, bottom path first.
#[derive(Clone, Debug, PartialEq)]
pub struct Traced {
    pub paths: Vec<TracedOut>,
    /// The colours used.
    pub palette: Vec<[u8; 3]>,
    /// Anchor points over all paths.
    pub anchors: usize,
    /// The size (px) the image was traced at: the paths' coordinate space.
    pub size: (u32, u32),
    /// The image's own size; differs from `size` when it was scaled down to the budget.
    pub source_size: (u32, u32),
    /// Was the image scaled down to fit [`MAX_TRACE_PIXELS`]?
    pub downscaled: bool,
}

/// The largest size with the aspect of `w x h` and at most `max` pixels (each side at least 1; a
/// size already within `max` is unchanged).
pub fn fit_size(w: u32, h: u32, max: u64) -> (u32, u32) {
    let pixels = u64::from(w) * u64::from(h);
    if pixels <= max || pixels == 0 || max == 0 {
        return (w, h);
    }
    let k = (max as f64 / pixels as f64).sqrt();
    let side = |v: u32| ((f64::from(v) * k).floor() as u32).clamp(1, v);
    let (mut nw, mut nh) = (side(w), side(h));
    if u64::from(nw) * u64::from(nh) > max {
        if nw >= nh {
            nw = u32::try_from(max / u64::from(nh)).unwrap_or(nw).clamp(1, nw);
        } else {
            nh = u32::try_from(max / u64::from(nw)).unwrap_or(nh).clamp(1, nh);
        }
    }
    (nw, nh)
}

fn map_err(e: TraceError) -> BridgeError {
    match e {
        TraceError::TooComplex { .. } => BridgeError::TooComplex,
        TraceError::Cancelled => BridgeError::Cancelled,
        TraceError::Decode(m) => BridgeError::BadImage(m),
        TraceError::Empty => BridgeError::BadImage("image is empty".into()),
        other => BridgeError::Failed(other.to_string()),
    }
}

/// Trace `img` with `s`. `max_anchors` is capped at [`MAX_ANCHORS`]; `cancel` is polled between
/// stages and colour layers (from any thread) and a `true` stops the trace as
/// [`BridgeError::Cancelled`]. An empty, fully transparent or fully ignored image traces to no
/// paths. Never panics.
pub fn run(img: &Rgba, s: &TraceSettings, max_anchors: usize, cancel: &(dyn Fn() -> bool + Sync)) -> Result<Traced, BridgeError> {
    img.check()?;
    let source_size = (img.width, img.height);
    if img.width == 0 || img.height == 0 {
        return Ok(Traced { paths: Vec::new(), palette: Vec::new(), anchors: 0, size: source_size, source_size, downscaled: false });
    }
    let size = fit_size(img.width, img.height, MAX_TRACE_PIXELS);
    let raster = if size == source_size {
        img.to_raster()?
    } else {
        let view = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(img.width, img.height, img.pixels.as_slice())
            .ok_or_else(|| BridgeError::BadImage("size does not match bytes".into()))?;
        let small = image::imageops::thumbnail(&view, size.0, size.1);
        Raster::new(size.0, size.1, small.into_raw()).map_err(map_err)?
    };
    let r = trace_watched(&raster, &params(s), max_anchors.min(MAX_ANCHORS), &|_, _| !cancel()).map_err(map_err)?;
    let anchors = r.anchor_count();
    let paths = r
        .paths
        .iter()
        .map(|p| TracedOut { path: from_vc(&p.path), rgb: p.color, stroke: p.stroke, square_caps: p.square_caps })
        .filter(|p| !p.path.elements().is_empty())
        .collect();
    Ok(Traced { paths, palette: r.palette, anchors, size, source_size, downscaled: size != source_size })
}

#[cfg(test)]
mod tests;
