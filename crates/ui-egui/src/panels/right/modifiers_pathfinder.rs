//! PICABIA: the Design panel's settings for a `pathfinder` row (a child of `modifiers.rs`, so it
//! uses that section's fields, rows and status line).

use egui::Ui;
use figcraft_doc::NodeId;

use super::{Field, RowView, field_rows, generator_status, text};
use crate::FigApp;
use crate::i18n::tr_noop;

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
    generator_status(app, ui, ids);
}
