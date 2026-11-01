//! PICABIA: the Design panel's settings for an `imageTrace` row (a child of `modifiers.rs`, so it
//! uses that section's fields, rows and status line).

use egui::Ui;
use figcraft_doc::NodeId;

use super::{Field, RowView, field_rows, generator_status, int, text};
use crate::FigApp;
use crate::i18n::tr_noop;

/// The presets: (value written in `preset`, label).
const PRESETS: &[(&str, &str)] = &[
    ("default", tr_noop("Default")),
    ("autoColor", tr_noop("Auto color")),
    ("highFidelityPhoto", tr_noop("High fidelity photo")),
    ("lowFidelityPhoto", tr_noop("Low fidelity photo")),
    ("threeColors", tr_noop("3 colors")),
    ("sixColors", tr_noop("6 colors")),
    ("sixteenColors", tr_noop("16 colors")),
    ("shadesOfGray", tr_noop("Shades of gray")),
    ("blackAndWhiteLogo", tr_noop("Black and white logo")),
    ("sketchedArt", tr_noop("Sketched art")),
    ("silhouettes", tr_noop("Silhouettes")),
    ("lineArt", tr_noop("Line art")),
    ("technicalDrawing", tr_noop("Technical drawing")),
    ("outline", tr_noop("Outline")),
    ("flatLogo", tr_noop("Flat logo")),
    ("custom", tr_noop("Custom")),
];

/// The modes: (value written in `mode`, label).
const MODES: &[(&str, &str)] = &[
    ("blackAndWhite", tr_noop("Black and white")),
    ("grayscale", tr_noop("Grayscale")),
    ("color", tr_noop("Color")),
    ("logo", tr_noop("Flat logo")),
];

/// The palettes: (value written in `palette`, label).
const PALETTES: &[(&str, &str)] = &[("limited", tr_noop("Limited")), ("fullTone", tr_noop("Full tone")), ("automatic", tr_noop("Automatic"))];

/// The methods: (value written in `method`, label).
const METHODS: &[(&str, &str)] = &[("abutting", tr_noop("Abutting")), ("overlapping", tr_noop("Overlapping"))];

/// A percentage stored as 0–100.
const fn percent(param: &'static str, key: &'static str) -> Field {
    Field::Num { param, lead: text(key), unit: "%", min: 0.0, max: 100.0, step: 1.0, scale: 1.0, int: false }
}

/// An image-pixel count stored as is.
const fn pixels(param: &'static str, key: &'static str, int: bool) -> Field {
    Field::Num { param, lead: text(key), unit: "px", min: 1.0, max: 100.0, step: 1.0, scale: 1.0, int }
}

/// The settings rows (`modifiers.rs` [`super::FIELDS`] lists them for this kind).
pub(super) const FIELDS: &[&[Field]] = &[
    &[Field::Choice { param: "preset", options: PRESETS, translate: true }],
    &[Field::Choice { param: "mode", options: MODES, translate: true }],
    &[Field::Choice { param: "palette", options: PALETTES, translate: true }],
    &[int("colors", text(tr_noop("Colors")), 2.0, 256.0), percent("colorDetail", tr_noop("Color detail"))],
    &[int("threshold", text(tr_noop("Threshold")), 0.0, 255.0)],
    &[percent("paths", tr_noop("Paths")), percent("corners", tr_noop("Corners"))],
    &[pixels("noise", tr_noop("Noise"), true)],
    &[Field::Choice { param: "method", options: METHODS, translate: true }],
    &[Field::Check { param: "fills", label: tr_noop("Fills") }],
    &[Field::Check { param: "strokes", label: tr_noop("Strokes") }],
    &[pixels("strokeWidth", tr_noop("Stroke width"), false)],
    &[Field::Check { param: "snapCurvesToLines", label: tr_noop("Snap curves to lines") }],
    &[Field::Check { param: "ignoreWhite", label: tr_noop("Ignore white") }],
    &[Field::Colour { param: "ignoreColor", label: tr_noop("Colour left out") }],
];

/// Row `i`'s settings (an `imageTrace` row): its fields, then what the trace came to.
pub(super) fn body(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, r: &RowView) {
    field_rows(app, ui, ids, i, r, FIELDS);
    generator_status(app, ui, ids);
}
