//! PICABIA: the Design panel's Modifiers section, built like the Effects section (`effects.rs`)
//! with the same widgets: a header with `+` (a menu of kinds) and Paste on right-click; one 32 pt
//! row per modifier with its settings toggle, kind dropdown, eye and minus; settings inline under
//! the row; row menus; drag to reorder. Rows list the stack top-down in evaluation order: the top
//! row runs first.
//!
//! Every edit is a `modifier.*` command. The fields come from [`FIELDS`], one table row per
//! settings row, so a parameter change in `figcraft-modifiers` is a one-line change here.

use egui::{CornerRadius, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_doc::{Document, NodeId, VariableType};
use figcraft_modifiers::ModifierKind;
use serde_json::{Map, Value, json};

use super::menu::{context_popup, menu_item, menu_popup, row, row_context_menu, row_rect};
use crate::FigApp;
use crate::a11y::Tip;
use crate::i18n::{tr, tr_noop, trf};
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
}

impl Field {
    pub(crate) fn param(&self) -> &'static str {
        match self {
            Field::Num { param, .. } | Field::Check { param, .. } | Field::Choice { param, .. } | Field::Target { param } => param,
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
            &[num("cellWidth", FieldLead::Raw("W"), 0.0, BIG), num("cellHeight", FieldLead::Raw("H"), 0.0, BIG)],
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
    let rows = match v {
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
        for w in warnings.iter().filter(|w| w.modifier.as_deref() == Some(r.id.as_str()) && Some(w.message.as_str()) != shown) {
            widgets::note(ui, &w.message, t.warning);
        }
        drag.row(ui, i, header, header.with_max_y(ui.cursor().min.y.max(header.max.y)), || kind_label(&r.kind).to_string());
    }
    for w in warnings.iter().filter(|w| w.modifier.as_deref().is_none_or(|m| !rows.iter().any(|r| r.id == m))) {
        widgets::note(ui, &w.message, t.warning);
    }
    if let Some((from, to)) = drag.finish(ui) {
        app.run("modifier.move", json!({"ids": ids, "from": from, "to": to}));
        move_flags(ui, MOD_OPEN, rows.len(), from, to);
    }
    if let Some(i) = removed {
        remove_flags(ui, MOD_OPEN, rows.len(), i);
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
        // A number alone on its row showing a chip (a bound variable or an expression) takes both
        // columns, so a text lead ("Count") leaves the chip room for its name.
        let width = match fields {
            [f @ Field::Num { .. }] if shows_chip(app, ids, r, f.param()) => half * 2.0 + widgets::GUTTER,
            _ => half,
        };
        row(ui, 32.0, |ui| {
            for f in *fields {
                field(app, ui, ids, i, r, f, width);
            }
        });
        // Why the target this row picks can't be used.
        if let (Some(reason), true) = (&r.target_problem, fields.iter().any(|f| matches!(f, Field::Target { .. }))) {
            widgets::note(ui, &trf("Target: {reason}", &[("reason", reason)]), t.warning);
        }
        // Why an expression typed into this row doesn't read.
        for f in *fields {
            if let Some(e) = ui.data(|d| d.get_temp::<String>(expr_field_id(i, f.param()).with("error"))) {
                widgets::note(ui, &trf("Expression: {error}", &[("error", &e)]), t.danger);
            }
        }
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
