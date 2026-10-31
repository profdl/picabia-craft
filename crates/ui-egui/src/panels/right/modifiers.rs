//! PICABIA: the Design panel's Modifiers section, built like the Effects section (`effects.rs`)
//! with the same widgets: a header with `+` (a menu of kinds) and Paste on right-click; one 32 pt
//! row per modifier with its settings toggle, kind dropdown, eye and minus; settings inline under
//! the row; row menus; drag to reorder. Rows list the stack top-down in evaluation order: the top
//! row runs first.
//!
//! Every edit is a `modifier.*` command. The fields come from [`FIELDS`], one table row per
//! settings row, so a parameter change in `figcraft-modifiers` is a one-line change here.

use std::sync::Arc;

use egui::{Align2, CornerRadius, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_color::{Color, Paint};
use figcraft_doc::{Document, NodeId, VariableType};
use figcraft_engine::cmd::lists::{moved_index, removed_index};
use figcraft_modifiers::ModifierKind;
use serde_json::{Map, Value, json};

use super::menu::{context_popup, menu_item, menu_popup, row, row_context_menu, row_rect};
use crate::FigApp;
use crate::a11y::Tip;
use crate::colorpicker::PickerTarget;
use crate::i18n::{tr, tr_noop, trf, trn};
use crate::icons::{self, Icon};
use crate::icons_picabia::{self, Glyph};
use crate::list_drag::{ListDrag, move_flags, remove_flags};
use crate::theme::{Tokens, regular};
use crate::widgets::{self, Edit, Empty, FIELD_H, Lead, PAD_RIGHT};

/// Salt of the flags that keep modifier rows' settings open (`Id::unique((MOD_OPEN, index))`).
pub(crate) const MOD_OPEN: &str = "mod_open";

/// The list id rows drag in (tests find the grips by it).
pub(crate) const LIST: &str = "modifiers";

/// The kinds the `+` menu and the kind dropdown offer, in menu order: (kind id, label).
pub(crate) const KIND_MENU: &[(&str, &str)] = &[
    ("linearArray", tr_noop("Linear array")),
    ("circularArray", tr_noop("Circular array")),
    ("gridArray", tr_noop("Grid array")),
    ("mirror", tr_noop("Mirror")),
    ("transformEach", tr_noop("Transform each")),
    ("randomize", tr_noop("Randomize")),
    ("wallpaper", tr_noop("Wallpaper")),
    ("copyToShape", tr_noop("Copy to shape")),
    ("stencil", tr_noop("Stencil")),
];

/// A field's label: plain text not translated ("X"), a catalog key, or an icon with its name.
#[derive(Clone, Copy, Debug)]
pub(crate) enum FieldLead {
    Raw(&'static str),
    Text(&'static str),
    Icon(Icon, &'static str),
}

/// One settings field of a modifier kind, editing parameter `param`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Field {
    /// A number field. `min`/`max`/`step` are in shown units; the parameter is the shown value
    /// divided by `scale` (100 for factors shown as %); `int` parameters are whole numbers.
    Num { param: &'static str, lead: FieldLead, unit: &'static str, min: f64, max: f64, step: f64, scale: f64, int: bool },
    /// An on/off parameter as a checkbox.
    Check { param: &'static str, label: &'static str },
    /// A choice among string values `(value, label)`, as a dropdown; `translate` false shows the
    /// labels as they are (symbols such as wallpaper group names).
    Choice { param: &'static str, options: &'static [(&'static str, &'static str)], translate: bool },
    /// Another layer on the page (its id as text), as a dropdown of the layers every selected
    /// layer can copy onto (`modifier::target_choices`).
    Target { param: &'static str },
    /// A `"#rrggbb"` colour: a swatch that opens FigCraft's colour picker and the hex, drawn like a
    /// fill row's colour, under a small label naming it.
    Colour { param: &'static str, label: &'static str },
    /// A stencil's `cutColours`: the design's colours (`modifier.stencilInfo`), one row each with
    /// its swatch, hex and area and a toggle that cuts it out.
    CutColours { param: &'static str },
}

impl Field {
    pub(crate) fn param(&self) -> &'static str {
        match self {
            Field::Num { param, .. }
            | Field::Check { param, .. }
            | Field::Choice { param, .. }
            | Field::Target { param }
            | Field::Colour { param, .. }
            | Field::CutColours { param } => param,
        }
    }
}

const BIG: f64 = 1.0e6;
const SEED_MAX: f64 = 4_294_967_295.0;

/// A length (points) or other plain number.
const fn num(param: &'static str, lead: FieldLead, min: f64, max: f64) -> Field {
    Field::Num { param, lead, unit: "", min, max, step: 1.0, scale: 1.0, int: false }
}
/// An angle in degrees.
const fn deg(param: &'static str, lead: FieldLead, min: f64, max: f64) -> Field {
    Field::Num { param, lead, unit: "°", min, max, step: 1.0, scale: 1.0, int: false }
}
/// A factor shown as a percentage (1 = 100%).
const fn pct(param: &'static str, lead: FieldLead, min: f64, max: f64) -> Field {
    Field::Num { param, lead, unit: "%", min, max, step: 1.0, scale: 100.0, int: false }
}
/// A whole number (counts, seeds).
const fn int(param: &'static str, lead: FieldLead, min: f64, max: f64) -> Field {
    Field::Num { param, lead, unit: "", min, max, step: 1.0, scale: 1.0, int: true }
}

/// A physical length in millimetres (stencil sizes and limits).
const fn mm(param: &'static str, lead: FieldLead, unit: &'static str, max: f64) -> Field {
    Field::Num { param, lead, unit, min: 0.0, max, step: 0.1, scale: 1.0, int: false }
}

const X: FieldLead = FieldLead::Raw("X");
const Y: FieldLead = FieldLead::Raw("Y");
const fn text(key: &'static str) -> FieldLead {
    FieldLead::Text(key)
}
const ROTATION: FieldLead = FieldLead::Icon(Icon::Angle, tr_noop("Rotation"));

/// Mirror axes: (value written in `axis`, label).
const AXES: &[(&str, &str)] = &[("vertical", tr_noop("Vertical axis")), ("horizontal", tr_noop("Horizontal axis")), ("both", tr_noop("Both axes"))];

/// Wallpaper plane groups: their short names, shown as they are.
const GROUPS: &[(&str, &str)] = &[
    ("p1", "p1"),
    ("p2", "p2"),
    ("pm", "pm"),
    ("pg", "pg"),
    ("cm", "cm"),
    ("pmm", "pmm"),
    ("pmg", "pmg"),
    ("pgg", "pgg"),
    ("cmm", "cmm"),
    ("p4", "p4"),
    ("p4m", "p4m"),
    ("p4g", "p4g"),
    ("p3", "p3"),
    ("p3m1", "p3m1"),
    ("p31m", "p31m"),
    ("p6", "p6"),
    ("p6m", "p6m"),
];

/// Copy-to-shape placements: (value written in `mode`, label).
const COPY_MODES: &[(&str, &str)] =
    &[("points", tr_noop("On each point")), ("alongPath", tr_noop("Along the outline")), ("populate", tr_noop("Scattered inside"))];

/// Stencil colour modes: (value written in `colourMode`, label). `separate` (P12) isn't offered.
const STENCIL_MODES: &[(&str, &str)] = &[("threshold", tr_noop("Cut the lighter colours")), ("pick", tr_noop("Cut the colours I pick"))];

/// Stencil previews: (value written in `preview`, label).
const STENCIL_PREVIEWS: &[(&str, &str)] = &[("paper", tr_noop("Paper preview")), ("cloth", tr_noop("Dyed cloth preview"))];

/// Small grey labels that group a kind's settings (`widgets::sublabels`, as the Design panel labels
/// "Position" and "Rotation"): (kind, the first parameter of the row they go over, label).
const HEADINGS: &[(&str, &str, &str)] = &[
    ("stencil", "colourMode", tr_noop("Colours")),
    ("stencil", "widthMm", tr_noop("Size")),
    ("stencil", "minCutAreaMm2", tr_noop("Clean-up")),
    ("stencil", "preview", tr_noop("Preview")),
];

/// The heading over the settings row of `kind` that starts with parameter `param`.
fn heading(kind: &str, param: &str) -> Option<&'static str> {
    HEADINGS.iter().find(|(k, p, _)| *k == kind && *p == param).map(|(_, _, l)| *l)
}

/// Each kind's settings, one inner slice per 32 pt row (numbers two to a row, like the Effects
/// grid). Parameters per `docs/picabia/modifiers.md`.
pub(crate) const FIELDS: &[(&str, &[&[Field]])] = &[
    (
        "linearArray",
        &[
            &[int("count", text(tr_noop("Count")), 1.0, 1000.0)],
            &[num("offsetX", X, -BIG, BIG), num("offsetY", Y, -BIG, BIG)],
            &[deg("rotateEach", ROTATION, -BIG, BIG), pct("scaleEach", text(tr_noop("Scale")), 0.0, 10_000.0)],
        ],
    ),
    (
        "circularArray",
        &[
            &[int("count", text(tr_noop("Count")), 1.0, 1000.0), num("radius", text(tr_noop("Radius")), 0.0, BIG)],
            &[deg("startAngle", text(tr_noop("Start")), -BIG, BIG), deg("sweep", text(tr_noop("Sweep")), -360.0, 360.0)],
            &[num("centerX", X, -BIG, BIG), num("centerY", Y, -BIG, BIG)],
            &[Field::Check { param: "align", label: tr_noop("Rotate copies along the ring") }],
        ],
    ),
    (
        "gridArray",
        &[
            &[int("rows", text(tr_noop("Rows")), 1.0, 1000.0), int("cols", text(tr_noop("Columns")), 1.0, 1000.0)],
            &[
                num("gapX", FieldLead::Icon(Icon::Gap, tr_noop("Column gap")), -BIG, BIG),
                num("gapY", FieldLead::Icon(Icon::DistributeV, tr_noop("Row gap")), -BIG, BIG),
            ],
        ],
    ),
    (
        "mirror",
        &[
            &[Field::Choice { param: "axis", options: AXES, translate: true }],
            &[num("offset", text(tr_noop("Offset")), -BIG, BIG)],
            &[Field::Check { param: "keepOriginal", label: tr_noop("Keep original") }],
        ],
    ),
    (
        "transformEach",
        &[
            &[num("offsetX", X, -BIG, BIG), num("offsetY", Y, -BIG, BIG)],
            &[deg("rotate", ROTATION, -BIG, BIG)],
            &[pct("scaleX", text(tr_noop("Scale X")), 0.0, 10_000.0), pct("scaleY", text(tr_noop("Scale Y")), 0.0, 10_000.0)],
            &[pct("jitter", text(tr_noop("Jitter")), 0.0, 1000.0), int("jitterSeed", text(tr_noop("Seed")), 0.0, SEED_MAX)],
            &[Field::Check { param: "progressive", label: tr_noop("Progressive (first copy to last)") }],
        ],
    ),
    (
        "randomize",
        &[
            &[num("position", text(tr_noop("Move")), 0.0, BIG), deg("rotation", ROTATION, 0.0, BIG)],
            &[pct("scale", text(tr_noop("Scale")), 0.0, 10_000.0), int("seed", text(tr_noop("Seed")), 0.0, SEED_MAX)],
        ],
    ),
    (
        "wallpaper",
        &[
            &[Field::Choice { param: "group", options: GROUPS, translate: false }],
            &[int("cols", text(tr_noop("Columns")), 1.0, 100.0), int("rows", text(tr_noop("Rows")), 1.0, 100.0)],
            &[num("cellWidth", FieldLead::Raw("W"), 0.001, BIG), num("cellHeight", FieldLead::Raw("H"), 0.001, BIG)],
            &[deg("angle", text(tr_noop("Angle")), 1.0, 179.0)],
            &[num("originX", X, -BIG, BIG), num("originY", Y, -BIG, BIG)],
        ],
    ),
    (
        "copyToShape",
        &[
            &[Field::Target { param: "target" }],
            &[Field::Choice { param: "mode", options: COPY_MODES, translate: true }],
            &[int("count", text(tr_noop("Count")), 1.0, 1000.0), pct("scale", text(tr_noop("Scale")), 0.0, 10_000.0)],
            &[int("seed", text(tr_noop("Seed")), 0.0, SEED_MAX)],
            &[Field::Check { param: "align", label: tr_noop("Follow the outline's direction") }],
        ],
    ),
    (
        "stencil",
        &[
            &[Field::Choice { param: "colourMode", options: STENCIL_MODES, translate: true }],
            &[pct("threshold", text(tr_noop("Threshold")), 0.0, 100.0)],
            &[Field::CutColours { param: "cutColours" }],
            &[Field::Check { param: "invert", label: tr_noop("Invert (cut the other colours)") }],
            &[mm("widthMm", FieldLead::Raw("W"), "mm", 10_000.0), mm("marginMm", text(tr_noop("Margin")), "mm", 1000.0)],
            // One to a row: their names don't fit half a row.
            &[mm("minCutAreaMm2", text(tr_noop("Smallest cut")), "mm²", 10_000.0)],
            &[mm("minCutWidthMm", text(tr_noop("Thinnest cut")), "mm", 100.0)],
            &[mm("minBridgeMm", text(tr_noop("Thinnest paper")), "mm", 100.0)],
            &[mm("toleranceMm", text(tr_noop("Smoothing")), "mm", 10.0)],
            &[Field::Choice { param: "preview", options: STENCIL_PREVIEWS, translate: true }],
            &[Field::Colour { param: "paperColour", label: tr_noop("Paper") }],
            &[Field::Colour { param: "pasteColour", label: tr_noop("Paste") }, Field::Colour { param: "clothColour", label: tr_noop("Cloth") }],
        ],
    ),
];

/// The settings rows of kind id `kind` (none for kinds this build doesn't know).
pub(crate) fn fields_of(kind: &str) -> &'static [&'static [Field]] {
    FIELDS.iter().find(|(k, _)| *k == kind).map_or(&[], |(_, f)| f)
}

/// The label of kind id `kind`, translated (the id itself for kinds this build doesn't know).
fn kind_label(kind: &str) -> &str {
    KIND_MENU.iter().find(|(k, _)| *k == kind).map_or(kind, |(_, l)| tr(l))
}

/// One row as the selection has it: the same kind at this index on every selected layer.
struct RowView {
    kind: String,
    /// Whether it is enabled on every layer.
    enabled: bool,
    /// Its parameters on each layer, with the layer.
    params: Vec<(NodeId, Map<String, Value>)>,
    /// Its id on the first layer (warnings name it).
    id: String,
    /// Its id when it is the same on every layer: its fields can then be bound to variables
    /// (binding keys name the modifier id).
    common_id: Option<String>,
    /// Its kind and params on the first layer.
    first: Option<ModifierKind>,
    /// Why its copy-to-shape target can't be used, on the first layer where it can't
    /// (`modifier::check_target`); `None` when every set target is valid (or it has none).
    target_problem: Option<String>,
    /// A stencil row's result on the first layer (`modifier.stencilInfo`); `None` for other kinds
    /// and before the engine has drawn it.
    stencil: Option<Arc<StencilInfo>>,
}

/// What the section shows for the selection.
enum View {
    /// The stacks have the same kinds in the same order: one row per index.
    Rows(Vec<RowView>),
    /// The stacks differ ("Mixed"): no rows, like a mixed effect list.
    Mixed,
    /// A layer's modifier data is damaged: the reason.
    Damaged(String),
}

fn view(doc: &Document, sel: &[NodeId]) -> View {
    let mut stacks = Vec::new();
    for id in sel {
        let Some(n) = doc.node(*id) else { continue };
        match figcraft_modifiers::node_stack(n) {
            None => stacks.push((*id, Vec::new())),
            Some(Ok(s)) => stacks.push((*id, s.modifiers.clone())),
            Some(Err(e)) => return View::Damaged(e.to_string()),
        }
    }
    let Some((_, first)) = stacks.first() else { return View::Rows(Vec::new()) };
    let kinds = |m: &[figcraft_modifiers::Modifier]| m.iter().map(|m| m.kind.id().to_string()).collect::<Vec<_>>();
    let want = kinds(first);
    if stacks.iter().any(|(_, s)| kinds(s) != want) {
        return View::Mixed;
    }
    let rows = want
        .iter()
        .enumerate()
        .map(|(i, kind)| {
            let at = |s: &[figcraft_modifiers::Modifier]| s.get(i).cloned();
            let mods: Vec<(NodeId, figcraft_modifiers::Modifier)> = stacks.iter().filter_map(|(id, s)| Some((*id, at(s)?))).collect();
            RowView {
                target_problem: target_problem(doc, &mods),
                stencil: None,
                kind: kind.clone(),
                enabled: mods.iter().all(|(_, m)| m.enabled),
                id: mods.first().map(|(_, m)| m.id.clone()).unwrap_or_default(),
                common_id: mods.first().map(|(_, m)| m.id.clone()).filter(|id| mods.iter().all(|(_, m)| &m.id == id)),
                first: mods.first().map(|(_, m)| m.kind.clone()),
                params: mods.into_iter().map(|(id, m)| (id, m.kind.params())).collect(),
            }
        })
        .collect();
    View::Rows(rows)
}

/// Why a copy-to-shape row's target can't be used, on the first layer of `mods` where it can't.
fn target_problem(doc: &Document, mods: &[(NodeId, figcraft_modifiers::Modifier)]) -> Option<String> {
    mods.iter().find_map(|(id, m)| match &m.kind {
        ModifierKind::CopyToShape(c) if !c.target.is_empty() => {
            figcraft_modifiers::parse_target(&c.target).and_then(|t| figcraft_engine::cmd::modifier::check_target(doc, *id, t)).err()
        }
        _ => None,
    })
}

/// The value every layer has for `param` (None when they differ: "Mixed").
fn common_num(params: &[(NodeId, Map<String, Value>)], param: &str) -> Option<f64> {
    let mut vals = params.iter().map(|(_, p)| p.get(param).and_then(Value::as_f64));
    let first = vals.next()??;
    vals.all(|v| v.is_some_and(|v| (v - first).abs() < 1e-9)).then_some(first)
}

/// The parameter value for a shown value `v` of field `f`: clamped, unscaled, whole if `int`.
fn to_param(f: &Field, v: f64) -> Option<Value> {
    let Field::Num { min, max, scale, int, .. } = *f else { return None };
    if !v.is_finite() {
        return None;
    }
    let p = v.clamp(min, max) / scale;
    Some(if int { json!(p.round() as i64) } else { json!(p) })
}

/// Draw the modifier list: add/paste, one row per modifier with settings, kind, eye and remove,
/// row menus and drag to reorder; keep open-settings flags with their rows.
pub(super) fn modifiers(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let ids = sel.to_vec();
    let v = view(doc, sel);
    let empty = !matches!(&v, View::Rows(r) if !r.is_empty());
    let (clicked, header) = widgets::section_header_resp(ui, tr("Modifiers"), &[(Icon::Plus, tr("Add modifier"))], empty);
    let header = header.tip(tr("Modifiers run from the top row down: each one repeats or changes what the rows above it made"));
    // The `+` opens the kinds (as does an empty header): the menu hangs from the button.
    let plus = Rect::from_min_size(pos2(header.rect.max.x - PAD_RIGHT - 24.0, header.rect.center().y - 12.0), vec2(24.0, 24.0));
    let anchor = ui.interact(plus, Id::unique("mod_add_menu"), Sense::hover());
    menu_popup(&anchor).align(egui::RectAlign::BOTTOM_END).open_memory(clicked.is_some().then_some(egui::SetOpenCommand::Bool(true))).show(|ui| {
        for (kind, label) in KIND_MENU {
            if menu_item(ui, tr(label), "") {
                app.run("modifier.add", json!({"ids": ids, "kind": kind}));
                ui.close();
            }
        }
    });
    let any_data = sel.iter().any(|i| doc.node(*i).is_some_and(figcraft_modifiers::has_modifiers));
    context_popup(&header).show(|ui| {
        if !app.session.clipboard.picabia_modifiers.is_empty() && menu_item(ui, tr("Paste modifiers"), "") {
            app.run("modifier.paste", json!({"ids": ids}));
            ui.close();
        }
        if any_data && menu_item(ui, tr("Remove all modifiers"), "") {
            app.run("modifier.remove_all", json!({"ids": ids}));
            ui.close();
        }
    });
    let t = Tokens::get(ui.ctx());
    let mut rows = match v {
        View::Rows(r) => r,
        View::Mixed => return,
        View::Damaged(e) => {
            widgets::note(ui, &trf("The modifier data can't be read: {error}", &[("error", &e)]), t.danger);
            return;
        }
    };
    // Warnings of the first layer's evaluation (copy-to-shape targets resolved in the document):
    // under their rows, or under the list.
    let warnings = sel
        .first()
        .and_then(|i| doc.node(*i))
        .and_then(|n| figcraft_modifiers::evaluate_node_in_doc(doc, n))
        .map(|o| o.warnings.clone())
        .unwrap_or_default();
    if let Some(first) = sel.first() {
        for r in rows.iter_mut().filter(|r| r.kind == "stencil") {
            r.stencil = stencil_info(app, ui, *first);
        }
    }
    let mut drag = ListDrag::in_order(ui, LIST);
    let mut removed = None;
    for (i, r) in rows.iter().enumerate() {
        let mut open = modifier_open(ui.ctx(), i);
        let header = row_rect(ui, 32.0, |ui| {
            if icons_picabia::button(ui, Glyph::of_kind(&r.kind), 24.0, open, tr("Modifier settings")).clicked() {
                open = !open;
                ui.data_mut(|d| d.insert_temp(Id::unique((MOD_OPEN, i)), open));
            }
            let w = ui.available_width() - 60.0;
            let resp = widgets::dropdown(ui, kind_label(&r.kind), w, None);
            menu_popup(&resp).show(|ui| {
                for (kind, label) in KIND_MENU {
                    if menu_item(ui, tr(label), "") {
                        switch_kind(app, &ids, i, r, kind);
                        ui.close();
                    }
                }
            });
            let (toggle, remove) = widgets::tight_end(ui, |ui| {
                (
                    icons::button(ui, if r.enabled { Icon::Eye } else { Icon::EyeOff }, 24.0, false, tr("Toggle")).clicked(),
                    icons::button(ui, Icon::Minus, 24.0, false, tr("Remove")).clicked(),
                )
            });
            if toggle {
                app.run("modifier.toggle", json!({"ids": ids, "index": i, "enabled": !r.enabled}));
            }
            if remove && app.run("modifier.remove", json!({"ids": ids, "index": i})).is_some() {
                removed = Some(i);
            }
        });
        row_menu(app, ui, header, &ids, i, rows.len());
        if open {
            settings(app, ui, &ids, i, r);
        }
        // An open row already shows its target's problem under the target field.
        let shown = if open { r.target_problem.as_deref() } else { None };
        let own: Vec<&str> = warnings.iter().filter(|w| w.modifier.as_deref() == Some(r.id.as_str())).map(|w| w.message.as_str()).collect();
        for w in own.iter().filter(|w| Some(**w) != shown) {
            widgets::note(ui, w, t.warning);
        }
        // What the stencil's own computation warns of (images skipped, gradients averaged...).
        for w in r.stencil.iter().flat_map(|s| s.warnings.iter()).filter(|w| !own.contains(&w.as_str())) {
            widgets::note(ui, w, t.warning);
        }
        drag.row(ui, i, header, header.with_max_y(ui.cursor().min.y.max(header.max.y)), || kind_label(&r.kind).to_string());
    }
    for w in warnings.iter().filter(|w| w.modifier.as_deref().is_none_or(|m| !rows.iter().any(|r| r.id == m))) {
        widgets::note(ui, &w.message, t.warning);
    }
    if let Some((from, to)) = drag.finish(ui) {
        app.run("modifier.move", json!({"ids": ids, "from": from, "to": to}));
        move_flags(ui, MOD_OPEN, rows.len(), from, to);
        if let Some(PickerTarget::PicabiaModifier { index, .. }) = &mut app.ui.picker {
            *index = moved_index(*index, from, to);
        }
    }
    // Open settings and an open colour picker stay with their rows, or go with theirs.
    if let Some(i) = removed {
        remove_flags(ui, MOD_OPEN, rows.len(), i);
        if let Some(PickerTarget::PicabiaModifier { index, .. }) = &mut app.ui.picker {
            match removed_index(*index, i) {
                Some(k) => *index = k,
                None => app.ui.picker = None,
            }
        }
    }
}

/// Whether modifier `i`'s settings are open in the Design panel.
pub(crate) fn modifier_open(ctx: &egui::Context, i: usize) -> bool {
    ctx.data(|d| d.get_temp::<bool>(Id::unique((MOD_OPEN, i)))).unwrap_or(false)
}

/// Run `steps` (command, params) as one undo step labelled `label`.
fn one_step(app: &mut FigApp, label: &str, steps: Vec<(&str, Value)>) {
    if app.session.begin_interaction(label).is_err() {
        return;
    }
    for (cmd, p) in steps {
        if let Err(e) = app.session.execute(cmd, &p) {
            app.failed(cmd, &e);
            let _ = app.session.cancel_interaction();
            return;
        }
    }
    let _ = app.session.commit_interaction();
}

/// Make row `i` a `kind` modifier with that kind's defaults, keeping its place and its eye.
fn switch_kind(app: &mut FigApp, ids: &[NodeId], i: usize, r: &RowView, kind: &str) {
    if r.kind == kind {
        return;
    }
    one_step(
        app,
        "Set modifier",
        vec![
            ("modifier.remove", json!({"ids": ids, "index": i})),
            ("modifier.add", json!({"ids": ids, "kind": kind, "index": i, "props": {"enabled": r.enabled}})),
        ],
    );
}

/// Row `i`'s right-click menu: copy, paste below, duplicate, and apply up to here once
/// `modifier.apply` exists.
fn row_menu(app: &mut FigApp, ui: &mut Ui, header: Rect, ids: &[NodeId], i: usize, len: usize) {
    let can_paste = !app.session.clipboard.picabia_modifiers.is_empty();
    let can_apply = figcraft_engine::find_command("modifier.apply").is_some();
    row_context_menu(ui, header, Id::unique(("modifier_menu", i))).show(|ui| {
        if menu_item(ui, tr("Copy modifier"), "") {
            app.run("modifier.copy", json!({"ids": ids, "index": i}));
            ui.close();
        }
        if can_paste && menu_item(ui, tr("Paste modifier"), "") {
            // Pasted rows land below this one.
            let n = app.session.clipboard.picabia_modifiers.len();
            let mut steps = vec![("modifier.paste", json!({"ids": ids}))];
            steps.extend((0..n).map(|k| ("modifier.move", json!({"ids": ids, "from": len + k, "to": i + 1 + k}))));
            one_step(app, "Paste modifiers", steps);
            ui.close();
        }
        if menu_item(ui, tr("Duplicate"), "") {
            // The copy keeps its eye and carries the row's variable bindings.
            app.run("modifier.duplicate", json!({"ids": ids, "index": i}));
            ui.close();
        }
        if can_apply && menu_item(ui, tr("Apply up to here"), "") {
            app.run("modifier.apply", json!({"ids": ids, "upTo": i}));
            ui.close();
        }
    });
}

/// Row `i`'s settings from [`FIELDS`]: numbers two to a 32 pt row, choices and checkboxes on
/// rows of their own. Scrubs are live and one undo step; typed relative edits on "Mixed" apply to
/// each layer from its own value.
fn settings(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, r: &RowView) {
    let rows = fields_of(&r.kind);
    let t = Tokens::get(ui.ctx());
    if rows.is_empty() {
        widgets::note(ui, tr("This version doesn't know this kind of modifier: it is kept in the file but not drawn"), t.text_secondary);
        return;
    }
    let half = widgets::col_width(ui.available_width());
    for fields in rows {
        // Fields the row's current choice makes meaningless are left out (wallpaper: the height
        // and angle of lattices the group fixes).
        let shown: Vec<Field> = fields.iter().copied().filter(|f| applies(r, f)).collect();
        if shown.is_empty() {
            continue;
        }
        if let Some(h) = fields.first().and_then(|f| heading(&r.kind, f.param())) {
            widgets::sublabels(ui, tr(h), None);
        }
        let fields = &shown[..];
        // Colours are named by small labels over their columns, like the Layout grid's fields.
        if let [Field::Colour { label: a, .. }, rest @ ..] = fields {
            let b = rest.iter().find_map(|f| match f {
                Field::Colour { label, .. } => Some(tr(label)),
                _ => None,
            });
            widgets::sublabels(ui, tr(a), b);
        }
        // The swatch list takes rows of its own.
        if let [Field::CutColours { param }] = fields {
            cut_colours(app, ui, i, r, param);
            continue;
        }
        // A number alone on its row showing a chip (a bound variable or an expression) takes both
        // columns, so a text lead ("Count") leaves the chip room for its name.
        // So does one whose text lead wouldn't fit a column ("Threshold" with its "%").
        let long_lead = |f: &Field| match f {
            Field::Num { lead: FieldLead::Text(k), .. } => {
                ui.painter().layout_no_wrap(tr(k).to_string(), regular(11.0), egui::Color32::PLACEHOLDER).size().x + 12.0 > half - 40.0
            }
            _ => false,
        };
        let width = match fields {
            [f @ Field::Num { .. }] if shows_chip(app, ids, r, f.param()) || long_lead(f) => half * 2.0 + widgets::GUTTER,
            _ => half,
        };
        row(ui, 32.0, |ui| {
            for f in fields {
                field(app, ui, ids, i, r, f, width);
            }
        });
        // The stencil's sheet, read-only under its size.
        if let (Some(sheet), true) = (r.stencil.as_ref().and_then(|s| s.sheet), fields.iter().any(|f| f.param() == "widthMm")) {
            let (w, h) = (mm_text(sheet.0), mm_text(sheet.1));
            widgets::note(ui, &trf("Sheet {w} × {h} mm, with the margin", &[("w", &w), ("h", &h)]), t.text_secondary);
        }
        // Why the target this row picks can't be used.
        if let (Some(reason), true) = (&r.target_problem, fields.iter().any(|f| matches!(f, Field::Target { .. }))) {
            widgets::note(ui, &trf("Target: {reason}", &[("reason", reason)]), t.warning);
        }
        // Why an expression typed into this row doesn't read.
        for f in fields {
            if let Some(e) = ui.data(|d| d.get_temp::<String>(expr_field_id(i, f.param()).with("error"))) {
                widgets::note(ui, &trf("Expression: {error}", &[("error", &e)]), t.danger);
            }
        }
    }
    // What the stencil came to: its pieces, the specks clean-up dropped and the length to cut.
    if let Some(stats) = r.stencil.as_ref().and_then(|s| s.stats) {
        let line = trn(
            "{n} pieces, {specks} specks dropped, {length} mm cut",
            stats.cut_pieces,
            &[("specks", &stats.dropped_specks.to_string()), ("length", &mm_text(stats.cut_length_mm))],
        );
        widgets::note(ui, &line, t.text_secondary);
    }
}

/// Whether field `f` of row `r` does anything for the row's current choices (wallpaper and
/// stencil have fields that depend on another).
fn applies(r: &RowView, f: &Field) -> bool {
    match r.kind.as_str() {
        "wallpaper" => wallpaper_applies(r, f),
        "stencil" => stencil_applies(r, f),
        _ => true,
    }
}

/// The text every layer has for `param` (None when they differ or it isn't text).
fn common_str<'a>(params: &'a [(NodeId, Map<String, Value>)], param: &str) -> Option<&'a str> {
    let mut vals = params.iter().map(|(_, p)| p.get(param).and_then(Value::as_str));
    let first = vals.next()??;
    vals.all(|v| v == Some(first)).then_some(first)
}

/// A stencil's conditional fields: the threshold only when it cuts by lightness, the swatch list
/// only when it cuts the colours picked, the paper colour for the paper preview and the paste and
/// cloth colours for the cloth one. Layers that differ show both.
fn stencil_applies(r: &RowView, f: &Field) -> bool {
    let mode = common_str(&r.params, "colourMode");
    let preview = common_str(&r.params, "preview");
    match f.param() {
        "threshold" => mode != Some("pick"),
        "cutColours" => mode.is_none_or(|m| m == "pick"),
        "paperColour" => preview != Some("cloth"),
        "pasteColour" | "clothColour" => preview != Some("paper"),
        _ => true,
    }
}

/// Wallpaper's conditional fields: `cellHeight`, `angle` and the origin are hidden when the group
/// (the same on every selected layer; mixed groups show everything) has a lattice or symmetry they
/// don't shape.
fn wallpaper_applies(r: &RowView, f: &Field) -> bool {
    let group = |p: &Map<String, Value>| {
        p.get("group").and_then(Value::as_str).and_then(|g| serde_json::from_value::<figcraft_modifiers::WallpaperGroup>(json!(g)).ok())
    };
    let mut groups = r.params.iter().map(|(_, p)| group(p));
    let Some(Some(first)) = groups.next() else { return true };
    if !groups.all(|g| g == Some(first)) {
        return true;
    }
    match f.param() {
        "cellHeight" => first.uses_cell_height(),
        "angle" => first.uses_angle(),
        "originX" | "originY" => first.uses_origin(),
        _ => true,
    }
}

/// Whether number field `param` of row `r` shows a chip: an expression, or a variable bound on the
/// first selected layer.
fn shows_chip(app: &FigApp, ids: &[NodeId], r: &RowView, param: &str) -> bool {
    let key = figcraft_modifiers::expr_key(param);
    if r.params.iter().any(|(_, p)| p.get(&key).is_some_and(Value::is_string)) {
        return true;
    }
    let (Some(mid), Some(first), Ok(doc)) = (r.common_id.as_deref(), ids.first(), app.session.doc()) else { return false };
    doc.node(*first).is_some_and(|n| n.bound.contains_key(&figcraft_modifiers::binding_key(mid, param)))
}

/// Field `f` of row `i`: a number field (bindable to a number variable when the row has one id
/// on every layer; with a per-copy expression where the kind takes one), a checkbox or a dropdown
/// (bindable likewise).
fn field(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, r: &RowView, f: &Field, half: f32) {
    let param = f.param();
    let set = |app: &mut FigApp, v: Value| {
        app.run("modifier.set", json!({"ids": ids, "index": i, "props": {param: v}}));
    };
    // The binding key, when the field can be bound here.
    let bind = r
        .common_id
        .as_deref()
        .zip(r.first.as_ref())
        .and_then(|(mid, kind)| Some((figcraft_modifiers::binding_key(mid, param), figcraft_modifiers::field_type(kind, param)?)));
    match f {
        Field::Num { lead, unit, step, scale, .. } => {
            let lead = match *lead {
                FieldLead::Raw(s) => Lead::Text(s),
                FieldLead::Text(k) => Lead::Text(tr(k)),
                FieldLead::Icon(icon, k) => Lead::Icon(icon, tr(k)),
            };
            let area = Rect::from_min_size(ui.cursor().min, vec2(half, FIELD_H));
            let id = Id::unique(("mod", param, i));
            if figcraft_modifiers::expr_fields(&r.kind).contains(&param) && expression(app, ui, ids, i, r, param, lead, area) {
                return;
            }
            let shown = common_num(&r.params, param).map(|v| v * scale);
            let edit = match &bind {
                Some((key, _)) => crate::variable_ui::number_field_mixed(app, ui, ids, key, id, lead, shown, unit, half),
                None => widgets::number_field_bindable(ui, id, lead, shown, unit, half, *step, None, Empty::Text(tr("Mixed"))).0,
            };
            match edit {
                Some(Edit::All(fe)) => {
                    app.field_run(Some(fe), "modifier.set", |v| match to_param(f, v) {
                        Some(pv) => json!({"ids": ids, "index": i, "props": {param: pv}}),
                        None => json!({}),
                    });
                }
                Some(Edit::Each(rel)) => {
                    let steps = r
                        .params
                        .iter()
                        .filter_map(|(id, p)| {
                            let cur = p.get(param).and_then(Value::as_f64)? * scale;
                            let pv = to_param(f, rel.apply(cur)?)?;
                            Some(("modifier.set", json!({"ids": [id], "index": i, "props": {param: pv}})))
                        })
                        .collect();
                    one_step(app, "Edit", steps);
                }
                None => {}
            }
            if figcraft_modifiers::expr_fields(&r.kind).contains(&param) {
                expression_menu(app, ui, ids, i, param, area, None);
            }
        }
        Field::Colour { label, .. } => colour_field(app, ui, ids, i, r, param, tr(label), half),
        Field::CutColours { .. } => cut_colours(app, ui, i, r, param),
        // Dropdowns span both field columns, ending where the number fields do.
        Field::Target { .. } => target_field(app, ui, ids, i, r, param, half * 2.0 + widgets::GUTTER),
        Field::Check { label, .. } => {
            let on = r.params.iter().all(|(_, p)| p.get(param).and_then(Value::as_bool).unwrap_or(false));
            let control = |app: &mut FigApp, ui: &mut Ui| {
                if widgets::checkbox(ui, on, tr(label)) {
                    set(app, json!(!on));
                }
            };
            match &bind {
                Some((key, VariableType::Boolean)) => {
                    crate::variable_ui::bindable(app, ui, ids, key, VariableType::Boolean, ui.available_width().min(200.0), control);
                }
                _ => control(app, ui),
            }
        }
        Field::Choice { options, translate, .. } => {
            let tl = |l: &'static str| if *translate { tr(l) } else { l };
            let first = r.params.first().and_then(|(_, p)| p.get(param).and_then(Value::as_str).map(str::to_string));
            let same = r.params.iter().all(|(_, p)| p.get(param).and_then(Value::as_str) == first.as_deref());
            let label = match (&first, same) {
                (Some(v), true) => options.iter().find(|(o, _)| o == v).map_or(v.as_str(), |(_, l)| tl(l)),
                _ => tr("Mixed"),
            };
            let control = |app: &mut FigApp, ui: &mut Ui| {
                let resp = widgets::dropdown(ui, label, ui.available_width().min(half * 2.0 + widgets::GUTTER), None);
                menu_popup(&resp).show(|ui| {
                    for (value, l) in *options {
                        if menu_item(ui, tl(l), "") {
                            set(app, json!(value));
                            ui.close();
                        }
                    }
                });
            };
            match &bind {
                Some((key, VariableType::String)) => crate::variable_ui::bindable(
                    app,
                    ui,
                    ids,
                    key,
                    VariableType::String,
                    ui.available_width().min(half * 2.0 + widgets::GUTTER),
                    control,
                ),
                _ => control(app, ui),
            }
        }
    }
}

/// A length or area in millimetres as the panel writes it: two decimals at most, one from 100 up.
fn mm_text(v: f64) -> String {
    widgets::fmt_num(if v.abs() >= 100.0 { (v * 10.0).round() / 10.0 } else { v })
}

/// `c` as a stencil colour parameter: `"#rrggbb"`, lowercase.
fn hex_param(c: Color) -> String {
    let [r, g, b, _] = c.to_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The colour of `param` on every layer of `r` (None when they differ or one isn't a colour).
fn common_colour(r: &RowView, param: &str) -> Option<Color> {
    common_str(&r.params, param).and_then(Color::from_hex).map(|c| c.with_alpha(1.0))
}

/// Colour field `param` of row `i`, `width` wide: its swatch (opens the colour picker on it) and
/// its hex, like an effect's colour.
#[allow(clippy::too_many_arguments)]
fn colour_field(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, r: &RowView, param: &str, label: &str, width: f32) {
    let colour = common_colour(r, param);
    let target = PickerTarget::PicabiaModifier { ids: ids.to_vec(), index: i, param: param.to_string() };
    let shown = colour.map_or_else(|| Paint::solid(Color::WHITE), Paint::solid);
    let sw = widgets::swatch_named(ui, &shown, 16.0, label);
    // While the picker edits this colour, its swatch is held out of the popover's closing, so a
    // press on it again reaches the toggle.
    if app.ui.picker.as_ref() == Some(&target) {
        app.ui.picker_button = Some(sw.rect);
    }
    if sw.clicked() {
        app.toggle_picker(target, Some(sw.rect.min.y));
    }
    let hex = colour.map_or_else(|| tr("Mixed").to_string(), Color::to_hex);
    let w = (width - 16.0 - widgets::GUTTER).max(40.0);
    if let Some(h) = widgets::text_field_named(ui, Id::unique(("mod_hex", param, i)), &hex, w, label) {
        match Color::from_hex_or_name(&h) {
            Some(c) => {
                app.run("modifier.set", json!({"ids": ids, "index": i, "props": {param: hex_param(c)}}));
            }
            None => app.toast(tr("Invalid color")),
        }
    }
}

/// The paint the colour picker shows for colour `param` of modifier `index` on the first of
/// `ids` (`PickerTarget::PicabiaModifier`).
pub(crate) fn picker_paint(doc: &Document, ids: &[NodeId], index: usize, param: &str) -> Option<Paint> {
    let stack = figcraft_modifiers::node_stack(doc.node(*ids.first()?)?)?.ok()?;
    let params = stack.modifiers.get(index)?.kind.params();
    let c = Color::from_hex(params.get(param)?.as_str()?)?;
    Some(Paint::solid(c.with_alpha(1.0)))
}

/// The command the colour picker runs to set colour `param` of modifier `index` to `paint`'s colour.
pub(crate) fn picker_command(ids: &[NodeId], index: usize, param: &str, paint: &Paint) -> (&'static str, Value) {
    let c = paint.solid_color().unwrap_or(Color::BLACK);
    ("modifier.set", json!({"ids": ids, "index": index, "props": {param: hex_param(c)}}))
}

/// The stencil's swatch list for field `param` (`cutColours`) of row `i`: one 32 pt row per colour
/// of the design, largest first, with its swatch, hex and area and a toggle (on: listed in
/// `cutColours` on every layer) that adds or removes it on each layer, in one undo step.
fn cut_colours(app: &mut FigApp, ui: &mut Ui, i: usize, r: &RowView, param: &str) {
    let t = Tokens::get(ui.ctx());
    let Some(info) = r.stencil.clone() else {
        widgets::note(ui, tr("The design's colours show here once the stencil is drawn"), t.text_secondary);
        return;
    };
    if info.colours.is_empty() {
        widgets::note(ui, tr("The layer has no colours the stencil can cut"), t.text_secondary);
        return;
    }
    let listed = |p: &Map<String, Value>, hex: &str| {
        p.get(param).and_then(Value::as_array).is_some_and(|l| l.iter().filter_map(Value::as_str).any(|h| h.eq_ignore_ascii_case(hex)))
    };
    let mut toggled = None;
    // Bounded like the Selection colors list: a design with many colours scrolls.
    egui::ScrollArea::vertical().id_salt(("mod_stencil_colours", i)).max_height(240.0).show_rows(ui, 32.0, info.colours.len(), |ui, range| {
        for c in info.colours.get(range).unwrap_or_default() {
            let on = r.params.iter().all(|(_, p)| listed(p, &c.hex));
            ui.push_id((&c.hex, "stencil_colour"), |ui| {
                row(ui, 32.0, |ui| {
                    let colour = Color::from_hex(&c.hex).unwrap_or(Color::BLACK).with_alpha(1.0);
                    widgets::swatch_named(ui, &Paint::solid(colour), 24.0, tr("Design colour"));
                    let w = (ui.available_width() - 24.0 - widgets::GUTTER).max(40.0);
                    let (text, _) = ui.allocate_exact_size(vec2(w, FIELD_H), Sense::hover());
                    ui.painter().text(text.left_center(), Align2::LEFT_CENTER, colour.to_hex(), regular(11.0), t.text);
                    let area = trf("{area} mm²", &[("area", &mm_text(c.area))]);
                    ui.painter().text(text.right_center(), Align2::RIGHT_CENTER, area, regular(11.0), t.text_secondary);
                    let tip = trf("Cut out {colour}", &[("colour", &colour.to_css_hex())]);
                    if icons::button(ui, Icon::Cut, 24.0, on, &tip).clicked() {
                        toggled = Some((c.hex.clone(), on));
                    }
                });
            });
        }
    });
    let Some((hex, on)) = toggled else { return };
    let steps = r
        .params
        .iter()
        .filter_map(|(id, p)| {
            let mut list: Vec<String> = p
                .get(param)
                .and_then(Value::as_array)
                .map(|l| l.iter().filter_map(Value::as_str).map(str::to_lowercase).collect())
                .unwrap_or_default();
            if on {
                list.retain(|h| !h.eq_ignore_ascii_case(&hex));
            } else if !list.iter().any(|h| h.eq_ignore_ascii_case(&hex)) {
                list.push(hex.to_lowercase());
            } else {
                return None;
            }
            Some(("modifier.set", json!({"ids": [id], "index": i, "props": {param: list}})))
        })
        .collect();
    one_step(app, "Set modifier", steps);
}

/// A stencil's computed result as the panel shows it (what `modifier.stencilInfo` reports).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StencilInfo {
    /// The design's colours, largest visible area first.
    pub(crate) colours: Vec<StencilColour>,
    pub(crate) warnings: Vec<String>,
    pub(crate) stats: Option<StencilStats>,
    /// The sheet (design plus margin), mm: (width, height).
    pub(crate) sheet: Option<(f64, f64)>,
}

/// One colour of a stencil's design.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StencilColour {
    /// `"#rrggbb"`, lowercase.
    pub(crate) hex: String,
    /// Whether the stencil cuts it out.
    pub(crate) cut: bool,
    /// Its visible area at physical size, mm².
    pub(crate) area: f64,
}

/// What a stencil came to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct StencilStats {
    pub(crate) cut_pieces: usize,
    pub(crate) dropped_specks: usize,
    pub(crate) cut_length_mm: f64,
}

impl StencilInfo {
    /// The panel's view of the engine's result (`picabia_stencil::Info`).
    fn of(i: &figcraft_engine::picabia_stencil::Info) -> StencilInfo {
        let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
        StencilInfo {
            colours: i
                .colours
                .iter()
                .filter(|c| Color::from_hex(&c.hex).is_some())
                .map(|c| StencilColour { hex: c.hex.to_lowercase(), cut: c.cut, area: finite(c.area) })
                .collect(),
            warnings: i.warnings.clone(),
            stats: Some(StencilStats {
                cut_pieces: i.stats.cut_pieces,
                dropped_specks: i.stats.dropped_specks,
                cut_length_mm: finite(i.stats.cut_length_mm),
            }),
            sheet: Some((finite(i.sheet_mm.0), finite(i.sheet_mm.1))),
        }
    }
}

/// The stencil result the panel last showed for a layer.
#[derive(Clone)]
struct InfoCache {
    document: u64,
    revision: u64,
    node: NodeId,
    /// Computed from the document at `revision` (nothing newer is coming until it changes).
    fresh: bool,
    info: Option<Arc<StencilInfo>>,
}

/// Layer `node`'s stencil result, as the engine's store has it (computed off the UI thread as the
/// canvas draws; the latest one stands while a newer one is coming). Looked up again only while
/// the one shown isn't fresh or once the document has changed.
fn stencil_info(app: &FigApp, ui: &Ui, node: NodeId) -> Option<Arc<StencilInfo>> {
    let ds = app.session.ds().ok()?;
    let key = Id::unique(("mod_stencil_info", node));
    let cached = ui.data(|d| d.get_temp::<InfoCache>(key)).filter(|c| c.document == ds.uid && c.node == node);
    if let Some(c) = &cached
        && c.fresh
        && c.revision == ds.revision
    {
        return c.info.clone();
    }
    let found = ds.doc.node(node).and_then(|n| figcraft_engine::picabia_stencil::info(&ds.doc, n));
    let fresh = found.as_ref().is_some_and(|(_, f)| *f);
    let info = match (found, cached) {
        (Some((i, _)), Some(c)) if c.info.as_deref() == Some(&StencilInfo::of(&i)) => c.info,
        (Some((i, _)), _) => Some(Arc::new(StencilInfo::of(&i))),
        (None, _) => None,
    };
    ui.data_mut(|d| d.insert_temp(key, InfoCache { document: ds.uid, revision: ds.revision, node, fresh, info: info.clone() }));
    info
}

/// Put `info` in the panel's cache for layer `node` at the session's current revision (tests: a
/// swatch list without computing a stencil).
#[cfg(test)]
pub(crate) fn cache_stencil_info(ctx: &egui::Context, session: &figcraft_engine::Session, node: NodeId, info: StencilInfo) {
    let Ok(ds) = session.ds() else { return };
    let c = InfoCache { document: ds.uid, revision: ds.revision, node, fresh: true, info: Some(Arc::new(info)) };
    ctx.data_mut(|d| d.insert_temp(Id::unique(("mod_stencil_info", node)), c));
}

/// The most layers the target dropdown lists.
const MAX_TARGETS: usize = 300;

/// The layers every layer of `sel` can copy onto (`modifier::target_choices`, intersected), in
/// the first layer's order, as (id, name); at most [`MAX_TARGETS`].
pub(crate) fn shared_targets(doc: &Document, sel: &[NodeId]) -> Vec<(NodeId, String)> {
    let Some((first, rest)) = sel.split_first() else { return Vec::new() };
    let others: Vec<std::collections::BTreeSet<NodeId>> =
        rest.iter().map(|s| figcraft_engine::cmd::modifier::target_choices(doc, *s).into_iter().map(|(t, _)| t).collect()).collect();
    figcraft_engine::cmd::modifier::target_choices(doc, *first)
        .into_iter()
        .filter(|(t, _)| others.iter().all(|o| o.contains(t)))
        .take(MAX_TARGETS)
        .collect()
}

/// The target dropdown of field `param` of row `i`: the target layer's name (or a prompt; a
/// warning sign and the reason as its tooltip when it can't be used), and a menu of the layers
/// every selected layer can copy onto.
fn target_field(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, r: &RowView, param: &str, width: f32) {
    let first = r.params.first().and_then(|(_, p)| p.get(param).and_then(Value::as_str).map(str::to_string)).unwrap_or_default();
    let same = r.params.iter().all(|(_, p)| p.get(param).and_then(Value::as_str).unwrap_or_default() == first);
    let name = first.parse::<u64>().ok().and_then(|n| app.session.doc().ok()?.node(NodeId(n)).map(|n| n.name.clone()));
    let label = match (same, first.is_empty(), name) {
        (false, ..) => tr("Mixed").to_string(),
        (true, true, _) => tr("Choose a layer").to_string(),
        (true, false, Some(n)) => n,
        (true, false, None) => tr("Missing layer").to_string(),
    };
    let lead = if r.target_problem.is_some() { Icon::GamutWarning } else { Icon::Vector };
    let mut resp = widgets::dropdown(ui, &label, ui.available_width().min(width), Some(lead));
    if let Some(reason) = &r.target_problem {
        resp = resp.on_hover_text(reason);
    }
    let mut picked = None;
    menu_popup(&resp).show(|ui| {
        let choices = app.session.doc().map(|d| shared_targets(d, ids)).unwrap_or_default();
        if choices.is_empty() {
            ui.label(egui::RichText::new(tr("No layer to copy onto")).font(regular(11.0)));
        }
        for (id, n) in choices {
            if menu_item(ui, &n, "") {
                picked = Some(id);
                ui.close();
            }
        }
    });
    if let Some(id) = picked {
        app.run("modifier.set", json!({"ids": ids, "index": i, "props": {param: id.0.to_string()}}));
    }
}

/// The flag of field `param` of row `i` whose expression is being typed.
fn expr_editing_id(i: usize, param: &str) -> Id {
    Id::unique(("mod_expr_edit", param, i))
}

/// The text field an expression is typed into.
fn expr_field_id(i: usize, param: &str) -> Id {
    Id::unique(("mod_expr", param, i))
}

/// Start typing an expression for field `param` of row `i`, from `text`.
fn edit_expression(ctx: &egui::Context, i: usize, param: &str, text: &str) {
    let fid = expr_field_id(i, param);
    ctx.data_mut(|d| {
        d.insert_temp(expr_editing_id(i, param), true);
        d.insert_temp(fid.with("editing"), text.to_string());
        d.remove::<String>(fid.with("error"));
    });
    ctx.memory_mut(|m| m.request_focus(fid.with("te")));
}

/// Set (or with an empty text, clear) the expression of field `param` of row `i`.
fn set_expression(app: &mut FigApp, ids: &[NodeId], i: usize, param: &str, text: &str) {
    let v = if text.trim().is_empty() { Value::Null } else { json!(text.trim()) };
    app.run("modifier.set", json!({"ids": ids, "index": i, "props": {figcraft_modifiers::expr_key(param): v}}));
}

/// An expression field in place of the number field, while one is being typed or the field has
/// one (a chip like a bound variable's: click to edit, the button removes it). True when it took
/// the field's place.
#[allow(clippy::too_many_arguments)]
fn expression(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, r: &RowView, param: &str, lead: Lead, area: Rect) -> bool {
    let key = figcraft_modifiers::expr_key(param);
    let texts: Vec<Option<&str>> = r.params.iter().map(|(_, p)| p.get(&key).and_then(Value::as_str)).collect();
    let current = texts.first().copied().flatten();
    let same = texts.iter().all(|t| *t == current);
    let fid = expr_field_id(i, param);
    if ui.data(|d| d.get_temp::<bool>(expr_editing_id(i, param))).unwrap_or(false) {
        let committed = widgets::text_field(ui, fid, if same { current.unwrap_or("") } else { "" }, area.width(), "i * 15");
        let still = ui.data(|d| d.get_temp::<String>(fid.with("editing"))).is_some();
        match committed {
            Some(text) => match figcraft_modifiers::expr::check(&text) {
                Err(e) if !text.trim().is_empty() => {
                    // A typo keeps the text open, with why under the row.
                    ui.data_mut(|d| {
                        d.insert_temp(fid.with("editing"), text.clone());
                        d.insert_temp(fid.with("error"), e.to_string());
                    });
                    ui.memory_mut(|m| m.request_focus(fid.with("te")));
                }
                _ => {
                    ui.data_mut(|d| {
                        d.remove::<bool>(expr_editing_id(i, param));
                        d.remove::<String>(fid.with("error"));
                    });
                    set_expression(app, ids, i, param, &text);
                }
            },
            None if !still => ui.data_mut(|d| {
                d.remove::<bool>(expr_editing_id(i, param));
                d.remove::<String>(fid.with("error"));
            }),
            None => {}
        }
        return true;
    }
    let Some(text) = (if same { current.map(str::to_string) } else { Some(tr("Mixed").to_string()) }) else { return false };
    let (rect, _) = ui.allocate_exact_size(area.size(), Sense::hover());
    match expr_chip(ui, fid, lead, rect, &text) {
        Some(ChipEvent::Edit) => edit_expression(ui.ctx(), i, param, if same { &text } else { "" }),
        Some(ChipEvent::Remove) => set_expression(app, ids, i, param, ""),
        None => {}
    }
    expression_menu(app, ui, ids, i, param, rect, Some(if same { text.as_str() } else { "" }));
    true
}

/// Field `param`'s right-click menu: add an expression, or edit or remove the one it has.
fn expression_menu(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, param: &str, rect: Rect, current: Option<&str>) {
    row_context_menu(ui, rect, Id::unique(("mod_expr_menu", param, i))).show(|ui| match current {
        None => {
            if menu_item(ui, tr("Add expression"), "") {
                edit_expression(ui.ctx(), i, param, "");
                ui.close();
            }
        }
        Some(text) => {
            if menu_item(ui, tr("Edit expression"), "") {
                edit_expression(ui.ctx(), i, param, text);
                ui.close();
            }
            if menu_item(ui, tr("Remove expression"), "") {
                set_expression(app, ids, i, param, "");
                ui.close();
            }
        }
    });
}

/// What an expression chip did.
enum ChipEvent {
    Edit,
    Remove,
}

/// A field showing expression `text` as a chip in `rect`, drawn like a bound variable's
/// (`widgets::bound_field`): click to edit, the button at its end removes it.
fn expr_chip(ui: &mut Ui, id: Id, lead: Lead, rect: Rect, text: &str) -> Option<ChipEvent> {
    let t = Tokens::get(ui.ctx());
    let resp = ui.interact(rect, id.with("chip"), Sense::click());
    crate::a11y::combo(&resp, lead.name(), text, false);
    ui.painter().rect_filled(rect, CornerRadius::same(5), t.field);
    if resp.hovered() {
        ui.painter().rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.0, t.field_hover_border), StrokeKind::Inside);
    }
    let lead_w = match lead {
        Lead::None | Lead::Named(_) => 4.0,
        Lead::Text(s) => {
            let w = widgets::text_lead_width(ui, s, rect.width());
            crate::bidi::paint_name(ui.painter(), pos2(rect.min.x + 8.0, rect.center().y), s, regular(11.0), t.text_secondary, w - 12.0);
            w
        }
        Lead::Icon(i, _) => {
            icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.min.x + 13.0, rect.center().y), vec2(14.0, 14.0)), i, t.icon_secondary);
            24.0
        }
    };
    let dr = Rect::from_center_size(pos2(rect.max.x - 11.0, rect.center().y), vec2(18.0, 18.0));
    let chip = Rect::from_min_max(pos2(rect.min.x + lead_w - 2.0, rect.min.y + 3.0), pos2(dr.min.x - 2.0, rect.max.y - 3.0));
    ui.painter().rect_filled(chip, CornerRadius::same(4), t.hover);
    ui.painter().rect_stroke(chip, CornerRadius::same(4), Stroke::new(1.0, t.border), StrokeKind::Inside);
    let galley = ui.painter().layout_no_wrap(format!("= {text}"), regular(11.0), t.text);
    ui.painter().with_clip_rect(chip.shrink2(vec2(5.0, 0.0))).galley(pos2(chip.min.x + 5.0, chip.center().y - galley.size().y / 2.0), galley, t.text);
    let dresp = ui.interact(dr, id.with("remove"), Sense::click()).tip(tr("Remove expression"));
    if resp.hovered() || dresp.hovered() || dresp.has_focus() {
        if dresp.hovered() {
            ui.painter().rect_filled(dr, CornerRadius::same(4), t.hover);
        }
        icons::paint(ui.painter(), dr.shrink(3.0), Icon::Detach, t.icon_secondary);
    }
    if dresp.clicked() {
        return Some(ChipEvent::Remove);
    }
    let resp = resp.on_hover_text(text);
    resp.clicked().then_some(ChipEvent::Edit)
}

/// The section's tests (`src/picabia_modifiers_tests.rs`, beside FigCraft's panel suites).
#[cfg(test)]
#[path = "../../picabia_modifiers_tests.rs"]
mod tests;

/// Whether `kind` is a kind the panel's tables know (tests).
#[cfg(test)]
pub(crate) fn known(kind: &str) -> bool {
    figcraft_modifiers::ModifierKind::default_for(kind).is_some() && !fields_of(kind).is_empty()
}
