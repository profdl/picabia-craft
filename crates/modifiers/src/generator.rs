//! The generator kinds' params: [`ImageTrace`] and [`Pathfinder`].
//!
//! A generator row replaces what the layer draws with geometry computed from it (see
//! [`crate::evaluate`]: the copies the rows above it make are its input, the rows below act on
//! the result). Here: the param structs (lenient reading, defaults, [`ImageTrace::clamped`] /
//! [`Pathfinder::clamped`]) and the image trace presets ([`preset_fields`]). The geometry is the
//! engine's (VectorCraft's tracer and Pathfinder); this crate doesn't depend on them.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::kinds::Clamp;
use crate::model::{hex_colour, parse_hex_colour};

/// VectorCraft's Image Trace presets ([`preset_fields`]), or `custom` (the fields as set).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TracePreset {
    #[default]
    Default,
    AutoColor,
    HighFidelityPhoto,
    LowFidelityPhoto,
    ThreeColors,
    SixColors,
    SixteenColors,
    ShadesOfGray,
    BlackAndWhiteLogo,
    SketchedArt,
    Silhouettes,
    LineArt,
    TechnicalDrawing,
    Outline,
    FlatLogo,
    /// No preset: the fields as set.
    Custom,
}

impl TracePreset {
    /// Every preset in VectorCraft's panel order, then `custom`.
    pub const ALL: [TracePreset; 16] = [
        TracePreset::Default,
        TracePreset::AutoColor,
        TracePreset::HighFidelityPhoto,
        TracePreset::LowFidelityPhoto,
        TracePreset::ThreeColors,
        TracePreset::SixColors,
        TracePreset::SixteenColors,
        TracePreset::ShadesOfGray,
        TracePreset::BlackAndWhiteLogo,
        TracePreset::SketchedArt,
        TracePreset::Silhouettes,
        TracePreset::LineArt,
        TracePreset::TechnicalDrawing,
        TracePreset::Outline,
        TracePreset::FlatLogo,
        TracePreset::Custom,
    ];

    /// The id written in the file (`"autoColor"`).
    pub fn id(self) -> &'static str {
        match self {
            TracePreset::Default => "default",
            TracePreset::AutoColor => "autoColor",
            TracePreset::HighFidelityPhoto => "highFidelityPhoto",
            TracePreset::LowFidelityPhoto => "lowFidelityPhoto",
            TracePreset::ThreeColors => "threeColors",
            TracePreset::SixColors => "sixColors",
            TracePreset::SixteenColors => "sixteenColors",
            TracePreset::ShadesOfGray => "shadesOfGray",
            TracePreset::BlackAndWhiteLogo => "blackAndWhiteLogo",
            TracePreset::SketchedArt => "sketchedArt",
            TracePreset::Silhouettes => "silhouettes",
            TracePreset::LineArt => "lineArt",
            TracePreset::TechnicalDrawing => "technicalDrawing",
            TracePreset::Outline => "outline",
            TracePreset::FlatLogo => "flatLogo",
            TracePreset::Custom => "custom",
        }
    }

    /// The preset with id `id`.
    pub fn from_id(id: &str) -> Option<TracePreset> {
        TracePreset::ALL.into_iter().find(|p| p.id() == id)
    }

    /// VectorCraft's name for it (`vectorcraft_trace::preset` takes it); `None` for `custom`.
    pub fn vectorcraft_name(self) -> Option<&'static str> {
        Some(match self {
            TracePreset::Default => "Default",
            TracePreset::AutoColor => "Auto Color",
            TracePreset::HighFidelityPhoto => "High Fidelity Photo",
            TracePreset::LowFidelityPhoto => "Low Fidelity Photo",
            TracePreset::ThreeColors => "3 Colors",
            TracePreset::SixColors => "6 Colors",
            TracePreset::SixteenColors => "16 Colors",
            TracePreset::ShadesOfGray => "Shades of Gray",
            TracePreset::BlackAndWhiteLogo => "Black and White Logo",
            TracePreset::SketchedArt => "Sketched Art",
            TracePreset::Silhouettes => "Silhouettes",
            TracePreset::LineArt => "Line Art",
            TracePreset::TechnicalDrawing => "Technical Drawing",
            TracePreset::Outline => "Outline",
            TracePreset::FlatLogo => "Flat Logo",
            TracePreset::Custom => return None,
        })
    }
}

/// Image trace mode (VectorCraft's Mode).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraceMode {
    #[default]
    BlackAndWhite,
    Grayscale,
    Color,
    /// Flat Logo.
    Logo,
}

/// Where `color` mode's colours come from (VectorCraft's Palette).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TracePalette {
    #[default]
    Limited,
    FullTone,
    Automatic,
    /// VectorCraft's Document Library: reads and writes, traces as `limited` with a warning.
    DocumentLibrary,
}

/// How the colour layers relate (VectorCraft's Method).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraceMethod {
    #[default]
    Abutting,
    Overlapping,
}

/// `imageTrace`: trace the layer's image, as it draws, into vector shapes (a **generator**: the
/// rows above make its input copies, the rows below act on the traced result). VectorCraft's
/// Image Trace fields and defaults; `paths`, `noise` and `strokeWidth` count image pixels.
/// Ranges are applied by [`ImageTrace::clamped`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ImageTrace {
    /// Setting a preset fills the fields below ([`crate::Modifier::set_props`]); a field changed off
    /// the preset makes it `custom`. Unknown values read as `custom`.
    #[serde(deserialize_with = "preset")]
    pub preset: TracePreset,
    /// Unknown values read as the default.
    #[serde(deserialize_with = "crate::lenient::choice")]
    pub mode: TraceMode,
    /// Unknown values read as the default.
    #[serde(deserialize_with = "crate::lenient::choice")]
    pub palette: TracePalette,
    /// Grays (grayscale), colours (`limited`, logo): 2–256 (logo 2–10).
    #[serde(deserialize_with = "crate::lenient::seed")]
    pub colors: u32,
    /// Colours (`fullTone`, `automatic`), 0–100 %.
    pub color_detail: f64,
    /// Black and white: lighter pixels are white, 0–255.
    #[serde(deserialize_with = "crate::lenient::seed")]
    pub threshold: u32,
    /// How tightly the paths follow the pixels, 0–100 %.
    pub paths: f64,
    /// More corners when higher, 0–100 %.
    pub corners: f64,
    /// Areas smaller than this many image pixels merge into a neighbour, 1–100.
    #[serde(deserialize_with = "crate::lenient::seed")]
    pub noise: u32,
    /// Unknown values read as the default.
    #[serde(deserialize_with = "crate::lenient::choice")]
    pub method: TraceMethod,
    /// Create: Fills.
    pub fills: bool,
    /// Create: Strokes (centre lines no wider than `strokeWidth`).
    pub strokes: bool,
    /// The widest part traced as a line, 1–100 image pixels.
    pub stroke_width: f64,
    pub snap_curves_to_lines: bool,
    /// Leave white (or `ignoreColor`) out.
    pub ignore_white: bool,
    /// With `ignoreWhite`: the colour left out instead of white (`"#rrggbb"`); `None` = white.
    #[serde(deserialize_with = "optional_colour")]
    pub ignore_color: Option<String>,
}

impl Default for ImageTrace {
    fn default() -> Self {
        ImageTrace {
            preset: TracePreset::Default,
            mode: TraceMode::BlackAndWhite,
            palette: TracePalette::Limited,
            colors: 6,
            color_detail: 50.0,
            threshold: 128,
            paths: 50.0,
            corners: 75.0,
            noise: 25,
            method: TraceMethod::Abutting,
            fills: true,
            strokes: false,
            stroke_width: 10.0,
            snap_curves_to_lines: false,
            ignore_white: false,
            ignore_color: None,
        }
    }
}

/// The fields of image trace preset `name` (its id, `"autoColor"`), VectorCraft's values, with
/// `preset` set to it; `None` for `custom` and names that aren't presets.
pub fn preset_fields(name: &str) -> Option<ImageTrace> {
    let preset = TracePreset::from_id(name)?;
    let d = ImageTrace { preset, ..ImageTrace::default() };
    let color = |colors: u32, paths: f64, corners: f64, noise: u32, method: TraceMethod| ImageTrace {
        mode: TraceMode::Color,
        colors,
        paths,
        corners,
        noise,
        method,
        ..d.clone()
    };
    let bw = |threshold: u32, paths: f64, corners: f64, noise: u32, snap: bool| ImageTrace {
        mode: TraceMode::BlackAndWhite,
        threshold,
        paths,
        corners,
        noise,
        ignore_white: true,
        snap_curves_to_lines: snap,
        ..d.clone()
    };
    Some(match preset {
        TracePreset::Default => d,
        TracePreset::AutoColor => {
            ImageTrace { palette: TracePalette::Automatic, color_detail: 85.0, ..color(6, 65.0, 60.0, 16, TraceMethod::Abutting) }
        }
        TracePreset::HighFidelityPhoto => color(64, 90.0, 25.0, 4, TraceMethod::Overlapping),
        TracePreset::LowFidelityPhoto => color(20, 60.0, 50.0, 12, TraceMethod::Overlapping),
        TracePreset::ThreeColors => color(3, 60.0, 60.0, 20, TraceMethod::Abutting),
        TracePreset::SixColors => color(6, 65.0, 60.0, 16, TraceMethod::Abutting),
        TracePreset::SixteenColors => color(16, 70.0, 55.0, 10, TraceMethod::Abutting),
        TracePreset::ShadesOfGray => {
            ImageTrace { mode: TraceMode::Grayscale, colors: 8, paths: 60.0, corners: 50.0, noise: 10, method: TraceMethod::Overlapping, ..d }
        }
        TracePreset::BlackAndWhiteLogo => bw(128, 95.0, 80.0, 8, true),
        TracePreset::SketchedArt => bw(150, 50.0, 50.0, 100, false),
        TracePreset::Silhouettes => bw(200, 40.0, 60.0, 30, false),
        TracePreset::LineArt => bw(128, 80.0, 70.0, 10, false),
        TracePreset::TechnicalDrawing => bw(128, 95.0, 90.0, 4, true),
        TracePreset::Outline => ImageTrace { fills: false, strokes: true, ..bw(128, 80.0, 70.0, 10, false) },
        TracePreset::FlatLogo => {
            ImageTrace { mode: TraceMode::Logo, colors: 8, paths: 60.0, corners: 50.0, noise: 6, snap_curves_to_lines: true, ..d }
        }
        TracePreset::Custom => return None,
    })
}

impl ImageTrace {
    /// These params with every number in its range, `documentLibrary` traced as `limited`, both
    /// Create options off as fills on, and `ignoreColor` checked; with a warning for each change.
    /// What [`crate::evaluate`] puts in [`crate::Output::generator`].
    pub fn clamped(&self) -> (ImageTrace, Vec<String>) {
        let d = ImageTrace::default();
        let mut warnings = Vec::new();
        let mut c = Clamp { warnings: &mut warnings };
        let max_colors = if self.mode == TraceMode::Logo { 10.0 } else { 256.0 };
        let whole = |v: f64| v.round().clamp(0.0, f64::from(u32::MAX)) as u32;
        let colors = whole(c.within("colors", f64::from(self.colors), 2.0, max_colors, f64::from(d.colors)));
        let color_detail = c.within("colorDetail", self.color_detail, 0.0, 100.0, d.color_detail);
        let threshold = whole(c.within("threshold", f64::from(self.threshold), 0.0, 255.0, f64::from(d.threshold)));
        let paths = c.within("paths", self.paths, 0.0, 100.0, d.paths);
        let corners = c.within("corners", self.corners, 0.0, 100.0, d.corners);
        let noise = whole(c.within("noise", f64::from(self.noise), 1.0, 100.0, f64::from(d.noise)));
        let stroke_width = c.within("strokeWidth", self.stroke_width, 1.0, 100.0, d.stroke_width);
        let palette = match self.palette {
            TracePalette::DocumentLibrary => {
                warnings.push("the Document Library palette isn't available; tracing with a limited palette".into());
                TracePalette::Limited
            }
            p => p,
        };
        let mut fills = self.fills;
        if !self.fills && !self.strokes {
            warnings.push("neither fills nor strokes is on; tracing fills".into());
            fills = true;
        }
        let ignore_color = match &self.ignore_color {
            Some(s) => match parse_hex_colour(s) {
                Some(rgb) => Some(hex_colour(rgb)),
                None => {
                    warnings.push("`ignoreColor` isn't a colour; leaving white out".into());
                    None
                }
            },
            None => None,
        };
        let out = ImageTrace {
            preset: self.preset,
            mode: self.mode,
            palette,
            colors,
            color_detail,
            threshold,
            paths,
            corners,
            noise,
            method: self.method,
            fills,
            strokes: self.strokes,
            stroke_width,
            snap_curves_to_lines: self.snap_curves_to_lines,
            ignore_white: self.ignore_white,
            ignore_color,
        };
        (out, warnings)
    }
}

/// [`crate::Modifier::set_props`] on an image trace, `old` → `new` from `props`: setting `preset`
/// (not `custom`) fills every field that `props` doesn't set itself; setting a field (without
/// `preset`) to a value off the current preset makes the preset `custom`.
pub(crate) fn apply_preset(old: &ImageTrace, new: &mut ImageTrace, props: &Map<String, Value>) {
    let fields = |p: &ImageTrace| match serde_json::to_value(p) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    if props.contains_key("preset") {
        let Some(preset) = preset_fields(new.preset.id()) else { return };
        let mut merged = fields(&preset);
        let set = fields(new);
        for (k, v) in set {
            if props.contains_key(&k) {
                merged.insert(k, v);
            }
        }
        if let Ok(p) = serde_json::from_value::<ImageTrace>(Value::Object(merged)) {
            *new = p;
        }
        return;
    }
    let Some(preset) = preset_fields(old.preset.id()) else { return };
    let (want, got) = (fields(&preset), fields(new));
    if props.keys().filter(|k| k.as_str() != "enabled").any(|k| want.get(k) != got.get(k)) {
        new.preset = TracePreset::Custom;
    }
}

/// A preset id: anything that doesn't name one reads as `custom`.
fn preset<'de, D: Deserializer<'de>>(d: D) -> Result<TracePreset, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or(TracePreset::Custom))
}

/// A colour text (normalised to lowercase `#rrggbb`) or none (`null`, blank, or not a colour).
fn optional_colour<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(v.as_str().and_then(parse_hex_colour).map(hex_colour))
}

/// A [`Pathfinder`] operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PathfinderOp {
    /// Every face, each with the front-most covering shape's fill.
    #[default]
    Divide,
    Trim,
    Merge,
    Crop,
    Outline,
    MinusBack,
    Unite,
    MinusFront,
    Intersect,
    Exclude,
}

impl PathfinderOp {
    /// Every operation, in the panel's order.
    pub const ALL: [PathfinderOp; 10] = [
        PathfinderOp::Divide,
        PathfinderOp::Trim,
        PathfinderOp::Merge,
        PathfinderOp::Crop,
        PathfinderOp::Outline,
        PathfinderOp::MinusBack,
        PathfinderOp::Unite,
        PathfinderOp::MinusFront,
        PathfinderOp::Intersect,
        PathfinderOp::Exclude,
    ];

    /// The id written in the file (`"minusBack"`).
    pub fn id(self) -> &'static str {
        match self {
            PathfinderOp::Divide => "divide",
            PathfinderOp::Trim => "trim",
            PathfinderOp::Merge => "merge",
            PathfinderOp::Crop => "crop",
            PathfinderOp::Outline => "outline",
            PathfinderOp::MinusBack => "minusBack",
            PathfinderOp::Unite => "unite",
            PathfinderOp::MinusFront => "minusFront",
            PathfinderOp::Intersect => "intersect",
            PathfinderOp::Exclude => "exclude",
        }
    }
}

/// `pathfinder`: Illustrator-style Pathfinder on a group's contents (a **generator**: the rows
/// above make its input copies, the rows below act on the result). Ranges are applied by
/// [`Pathfinder::clamped`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Pathfinder {
    /// Unknown values read as the default.
    #[serde(deserialize_with = "crate::lenient::choice")]
    pub op: PathfinderOp,
    /// How closely results follow the curves, 0.001–10 points; thinner pieces are dropped.
    pub precision: f64,
    /// `outline`: the edges' stroke width, 0–100 points.
    pub outline_width: f64,
}

impl Default for Pathfinder {
    fn default() -> Self {
        Pathfinder { op: PathfinderOp::Divide, precision: 0.01, outline_width: 1.0 }
    }
}

impl Pathfinder {
    /// These params with every number in its range, with a warning for each change.
    pub fn clamped(&self) -> (Pathfinder, Vec<String>) {
        let d = Pathfinder::default();
        let mut warnings = Vec::new();
        let mut c = Clamp { warnings: &mut warnings };
        let precision = c.within("precision", self.precision, 0.001, 10.0, d.precision);
        let outline_width = c.within("outlineWidth", self.outline_width, 0.0, 100.0, d.outline_width);
        (Pathfinder { op: self.op, precision, outline_width }, warnings)
    }
}

#[cfg(test)]
#[path = "generator_tests.rs"]
mod tests;
