//! PICABIA: the Design panel's settings for an `imageTrace` row (a child of `modifiers.rs`, so it
//! uses that section's fields, rows and status line).
//!
//! Laid out like VectorCraft's Image Trace panel: the preset and mode, the one slider the mode
//! uses (Threshold, Grays, Colours or Colours %), the palette in Color mode, then an "Advanced"
//! group (paths, corners, noise, method, what to create, snapping, ignoring white), then what the
//! trace came to.

use egui::{Id, Ui};
use figcraft_doc::NodeId;
use serde_json::{Value, json};

use super::{Field, RowView, colour_field, common_str, field_rows, row, text};
use crate::FigApp;
use crate::i18n::{tr, tr_noop, trf};
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets;

/// The presets: (value written in `preset`, label). VectorCraft's names.
const PRESETS: &[(&str, &str)] = &[
    ("default", tr_noop("Default")),
    ("autoColor", tr_noop("Auto Color")),
    ("highFidelityPhoto", tr_noop("High Fidelity Photo")),
    ("lowFidelityPhoto", tr_noop("Low Fidelity Photo")),
    ("threeColors", tr_noop("3 Colors")),
    ("sixColors", tr_noop("6 Colors")),
    ("sixteenColors", tr_noop("16 Colors")),
    ("shadesOfGray", tr_noop("Shades of Gray")),
    ("blackAndWhiteLogo", tr_noop("Black and White Logo")),
    ("sketchedArt", tr_noop("Sketched Art")),
    ("silhouettes", tr_noop("Silhouettes")),
    ("lineArt", tr_noop("Line Art")),
    ("technicalDrawing", tr_noop("Technical Drawing")),
    ("outline", tr_noop("Outline")),
    ("flatLogo", tr_noop("Flat Logo")),
    ("custom", tr_noop("Custom")),
];

/// The modes: (value written in `mode`, label).
const MODES: &[(&str, &str)] = &[
    ("blackAndWhite", tr_noop("Black and white")),
    ("grayscale", tr_noop("Grayscale")),
    ("color", tr_noop("Colour")),
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

/// A whole number from `min` to `max`.
const fn count(param: &'static str, key: &'static str, min: f64, max: f64) -> Field {
    Field::Num { param, lead: text(key), unit: "", min, max, step: 1.0, scale: 1.0, int: true }
}

const PRESET: Field = Field::Choice { param: "preset", options: PRESETS, translate: true };
const MODE: Field = Field::Choice { param: "mode", options: MODES, translate: true };
const PALETTE: Field = Field::Choice { param: "palette", options: PALETTES, translate: true };
const THRESHOLD: Field = count("threshold", tr_noop("Threshold"), 0.0, 255.0);
const GRAYS: Field = count("colors", tr_noop("Grays"), 2.0, 256.0);
const COLOURS: Field = count("colors", tr_noop("Colours"), 2.0, 256.0);
const LOGO_COLOURS: Field = count("colors", tr_noop("Colours"), 2.0, 10.0);
const COLOUR_DETAIL: Field = percent("colorDetail", tr_noop("Colours"));
const PATHS: Field = percent("paths", tr_noop("Paths"));
const CORNERS: Field = percent("corners", tr_noop("Corners"));
const NOISE: Field = pixels("noise", tr_noop("Noise"), true);
const METHOD: Field = Field::Choice { param: "method", options: METHODS, translate: true };
const FILLS: Field = Field::Check { param: "fills", label: tr_noop("Fills") };
const STROKES: Field = Field::Check { param: "strokes", label: tr_noop("Strokes") };
const STROKE_WIDTH: Field = pixels("strokeWidth", tr_noop("Stroke"), false);
const SNAP: Field = Field::Check { param: "snapCurvesToLines", label: tr_noop("Snap curves to lines") };
const IGNORE_WHITE: Field = Field::Check { param: "ignoreWhite", label: tr_noop("Ignore white") };
const IGNORE_COLOUR: Field = Field::Colour { param: "ignoreColor", label: tr_noop("Colour left out") };

/// Every settings row (`modifiers.rs` [`super::FIELDS`] lists them for this kind); [`body`] shows
/// the ones the row's mode uses.
pub(super) const FIELDS: &[&[Field]] = &[
    &[PRESET],
    &[MODE],
    &[PALETTE],
    &[COLOURS, COLOUR_DETAIL],
    &[THRESHOLD],
    &[PATHS, CORNERS],
    &[NOISE],
    &[METHOD],
    &[FILLS],
    &[STROKES],
    &[STROKE_WIDTH],
    &[SNAP],
    &[IGNORE_WHITE],
    &[IGNORE_COLOUR],
];

/// Whether boolean `param` is on for every layer of `r`.
fn all_on(r: &RowView, param: &str) -> bool {
    r.params.iter().all(|(_, p)| p.get(param).and_then(Value::as_bool).unwrap_or(false))
}

/// Whether boolean `param` is on for some layer of `r`.
fn any_on(r: &RowView, param: &str) -> bool {
    r.params.iter().any(|(_, p)| p.get(param).and_then(Value::as_bool).unwrap_or(false))
}

/// The main slider rows for the row's mode (every one while the layers' modes differ).
fn main_rows(r: &RowView) -> Vec<&'static [Field]> {
    let mode = common_str(&r.params, "mode");
    match mode {
        Some("blackAndWhite") => vec![&[THRESHOLD]],
        Some("grayscale") => vec![&[GRAYS]],
        Some("logo") => vec![&[LOGO_COLOURS]],
        Some("color") => {
            let mut rows: Vec<&'static [Field]> = vec![&[PALETTE]];
            match common_str(&r.params, "palette") {
                Some("limited" | "documentLibrary") => rows.push(&[COLOURS]),
                Some("fullTone" | "automatic") => rows.push(&[COLOUR_DETAIL]),
                _ => rows.push(&[COLOURS, COLOUR_DETAIL]),
            }
            rows
        }
        _ => vec![&[THRESHOLD], &[PALETTE], &[COLOURS, COLOUR_DETAIL]],
    }
}

/// The egui memory key for row `i`'s Advanced group being open.
fn advanced_id(i: usize) -> Id {
    Id::unique(("picabia_trace_advanced", i))
}

/// Row `i`'s settings (an `imageTrace` row): its fields, then what the trace came to.
pub(super) fn body(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, r: &RowView) {
    field_rows(app, ui, ids, i, r, &[&[PRESET], &[MODE]]);
    field_rows(app, ui, ids, i, r, &main_rows(r));
    let id = advanced_id(i);
    let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if widgets::disclosure(ui, tr("Advanced"), open) {
        ui.data_mut(|d| d.insert_temp(id, !open));
    }
    if open {
        advanced(app, ui, ids, i, r);
    }
    status(app, ui, ids);
}

/// The Advanced group.
fn advanced(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, r: &RowView) {
    field_rows(app, ui, ids, i, r, &[&[PATHS, CORNERS], &[NOISE], &[METHOD]]);
    widgets::sublabels(ui, tr("Create"), None);
    // At least one of Fills and Strokes stays on: the one on alone can't be turned off.
    let (fills, strokes) = (any_on(r, "fills"), any_on(r, "strokes"));
    ui.add_enabled_ui(!all_on(r, "fills") || strokes, |ui| field_rows(app, ui, ids, i, r, &[&[FILLS]]));
    ui.add_enabled_ui(!all_on(r, "strokes") || fills, |ui| field_rows(app, ui, ids, i, r, &[&[STROKES]]));
    ui.add_enabled_ui(strokes, |ui| field_rows(app, ui, ids, i, r, &[&[STROKE_WIDTH]]));
    widgets::sublabels(ui, tr("Options"), None);
    field_rows(app, ui, ids, i, r, &[&[SNAP], &[IGNORE_WHITE]]);
    if any_on(r, "ignoreWhite") {
        ignore_colour(app, ui, ids, i, r);
    }
}

/// With Ignore white: the colour left out, white unless one is picked. Unset on every layer: a
/// row offering to pick one; set: its swatch and hex, and a minus that goes back to white.
fn ignore_colour(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, r: &RowView) {
    let unset = r.params.iter().all(|(_, p)| p.get("ignoreColor").is_none_or(Value::is_null));
    let half = widgets::col_width(ui.available_width());
    if unset {
        row(ui, 32.0, |ui| {
            if widgets::button(ui, tr("Leave out another colour"), false, true).clicked() {
                app.run("modifier.set", json!({"ids": ids, "index": i, "props": {"ignoreColor": "#ffffff"}}));
            }
        });
        return;
    }
    let Field::Colour { label, .. } = IGNORE_COLOUR else { return };
    widgets::sublabels(ui, tr(label), None);
    row(ui, 32.0, |ui| {
        colour_field(app, ui, ids, i, r, "ignoreColor", tr(label), half * 2.0 + widgets::GUTTER - 24.0 - widgets::GUTTER);
        if icons::button(ui, Icon::Minus, 24.0, false, tr("Leave out white")).clicked() {
            app.run("modifier.set", json!({"ids": ids, "index": i, "props": {"ignoreColor": Value::Null}}));
        }
    });
}

/// What the first selected layer's trace came to: "Tracing…" until the engine has a fresh one,
/// then "Paths N · Anchors N · Colours N", then its warnings.
fn status(app: &FigApp, ui: &mut Ui, ids: &[NodeId]) {
    let t = Tokens::get(ui.ctx());
    let Ok(ds) = app.session.ds() else { return };
    let Some(n) = ids.first().and_then(|i| ds.doc.node(*i)) else { return };
    let got = figcraft_engine::picabia_generate::info(&ds.doc, n);
    if got.as_ref().is_none_or(|(_, fresh)| !fresh) {
        widgets::note(ui, tr("Tracing…"), t.text_secondary);
    }
    let Some((info, _)) = got else { return };
    if !info.keep_design {
        let num = |k: &str| info.stats.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
        let (paths, anchors, colours) = (num("paths"), num("anchors"), num("colours"));
        let line = trf("Paths {paths} · Anchors {anchors} · Colours {colours}", &[("paths", &paths), ("anchors", &anchors), ("colours", &colours)]);
        widgets::note(ui, &line, t.text_secondary);
    }
    for w in &info.warnings {
        widgets::note(ui, w, t.warning);
    }
}
