//! PICABIA: the Design panel's settings for a `pathfinder` row (a child of `modifiers.rs`, so it
//! uses that section's fields, rows and status line).

use egui::Ui;
use figcraft_doc::NodeId;

use super::{Field, RowView, field_rows, text};
use crate::FigApp;
use crate::i18n::{tr, tr_noop, trf};
use crate::theme::Tokens;
use crate::widgets;

/// The operation dropdown's tooltip.
pub(super) const OP_TIP: &str = tr_noop("Boolean groups already do Unite, Minus front, Intersect and Exclude.");

/// The first of the operations a boolean group also does: a divider goes before it.
pub(super) const FIRST_BOOLEAN: &str = "unite";

/// The operations: (value written in `op`, label).
const OPS: &[(&str, &str)] = &[
    ("divide", tr_noop("Divide")),
    ("trim", tr_noop("Trim")),
    ("merge", tr_noop("Merge")),
    ("crop", tr_noop("Crop")),
    ("outline", tr_noop("Outline")),
    ("minusBack", tr_noop("Minus back")),
    ("unite", tr_noop("Unite")),
    ("minusFront", tr_noop("Minus front")),
    ("intersect", tr_noop("Intersect")),
    ("exclude", tr_noop("Exclude")),
];

/// The settings rows (`modifiers.rs` [`super::FIELDS`] lists them for this kind).
pub(super) const FIELDS: &[&[Field]] = &[
    &[Field::Choice { param: "op", options: OPS, translate: true }],
    &[Field::Num { param: "precision", lead: text(tr_noop("Precision")), unit: "", min: 0.001, max: 10.0, step: 0.01, scale: 1.0, int: false }],
    &[Field::Num { param: "outlineWidth", lead: text(tr_noop("Outline width")), unit: "", min: 0.0, max: 100.0, step: 0.1, scale: 1.0, int: false }],
];

/// Row `i`'s settings (a `pathfinder` row): its fields, then what it came to.
pub(super) fn body(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, r: &RowView) {
    field_rows(app, ui, ids, i, r, FIELDS);
    status(app, ui, ids);
}

/// "Computing…", or "Shapes 5 → Faces 9" (the layer draws its design instead: the warning says
/// why), then the warnings.
fn status(app: &FigApp, ui: &mut Ui, ids: &[NodeId]) {
    let t = Tokens::get(ui.ctx());
    let Ok(ds) = app.session.ds() else { return };
    let Some(n) = ids.first().and_then(|i| ds.doc.node(*i)) else { return };
    match figcraft_engine::picabia_generate::info(&ds.doc, n) {
        None => widgets::note(ui, tr("Computing…"), t.text_secondary),
        Some((info, fresh)) => {
            if !info.keep_design {
                let count = |k: &str| info.stats.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0).to_string();
                let line = trf("Shapes {shapes} → Faces {faces}", &[("shapes", &count("shapes")), ("faces", &count("faces"))]);
                let line = if fresh { line } else { format!("{line} {}", tr("(updating…)")) };
                widgets::note(ui, &line, t.text_secondary);
            }
            for w in &info.warnings {
                widgets::note(ui, w, t.warning);
            }
        }
    }
}
