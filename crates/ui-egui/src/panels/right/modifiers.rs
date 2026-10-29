//! PICABIA: the Design panel's Modifiers section, built like the Effects section (`effects.rs`)
//! with the same widgets: a header with `+` (a menu of kinds) and Paste on right-click; one 32 pt
//! row per modifier with its settings toggle, kind dropdown, eye and minus; settings inline under
//! the row; row menus; drag to reorder. Rows list the stack top-down in evaluation order: the top
//! row runs first.
//!
//! Every edit is a `modifier.*` command. The fields come from [`FIELDS`], one table row per
//! settings row, so a parameter change in `figcraft-modifiers` is a one-line change here.

use egui::{Id, Rect, Sense, Ui, pos2, vec2};
use figcraft_doc::{Document, NodeId};
use serde_json::{Map, Value, json};

use super::menu::{context_popup, menu_item, menu_popup, row, row_context_menu, row_rect};
use crate::FigApp;
use crate::a11y::Tip;
use crate::i18n::{tr, tr_noop, trf};
use crate::icons::{self, Icon};
use crate::icons_picabia::{self, Glyph};
use crate::list_drag::{ListDrag, move_flags, remove_flags};
use crate::theme::Tokens;
use crate::widgets::{self, Edit, Empty, Lead, PAD_RIGHT};

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
}

impl Field {
    pub(crate) fn param(&self) -> &'static str {
        match self {
            Field::Num { param, .. } | Field::Check { param, .. } | Field::Choice { param, .. } => param,
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
                kind: kind.clone(),
                enabled: mods.iter().all(|(_, m)| m.enabled),
                id: mods.first().map(|(_, m)| m.id.clone()).unwrap_or_default(),
                params: mods.into_iter().map(|(id, m)| (id, m.kind.params())).collect(),
            }
        })
        .collect();
    View::Rows(rows)
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
    // Warnings of the first layer's evaluation: under their rows, or under the list.
    let warnings = sel.first().and_then(|i| doc.node(*i)).and_then(figcraft_modifiers::evaluate_node).map(|o| o.warnings.clone()).unwrap_or_default();
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
        row_menu(app, ui, header, &ids, i, r, rows.len());
        if open {
            settings(app, ui, &ids, i, r);
        }
        for w in warnings.iter().filter(|w| w.modifier.as_deref() == Some(r.id.as_str())) {
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
fn row_menu(app: &mut FigApp, ui: &mut Ui, header: Rect, ids: &[NodeId], i: usize, r: &RowView, len: usize) {
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
            let steps = r
                .params
                .iter()
                .map(|(id, p)| {
                    let mut props = p.clone();
                    props.insert("enabled".into(), json!(r.enabled));
                    ("modifier.add", json!({"ids": [id], "kind": r.kind, "index": i + 1, "props": props}))
                })
                .collect();
            one_step(app, "Add modifier", steps);
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
    if rows.is_empty() {
        let t = Tokens::get(ui.ctx());
        widgets::note(ui, tr("This version doesn't know this kind of modifier: it is kept in the file but not drawn"), t.text_secondary);
        return;
    }
    let half = widgets::col_width(ui.available_width());
    for fields in rows {
        row(ui, 32.0, |ui| {
            for f in *fields {
                field(app, ui, ids, i, r, f, half);
            }
        });
    }
}

fn field(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, r: &RowView, f: &Field, half: f32) {
    let param = f.param();
    let set = |app: &mut FigApp, v: Value| {
        app.run("modifier.set", json!({"ids": ids, "index": i, "props": {param: v}}));
    };
    match f {
        Field::Num { lead, unit, step, scale, .. } => {
            let lead = match *lead {
                FieldLead::Raw(s) => Lead::Text(s),
                FieldLead::Text(k) => Lead::Text(tr(k)),
                FieldLead::Icon(icon, k) => Lead::Icon(icon, tr(k)),
            };
            let shown = common_num(&r.params, param).map(|v| v * scale);
            let id = Id::unique(("mod", param, i));
            let (edit, _) = widgets::number_field_bindable(ui, id, lead, shown, unit, half, *step, None, Empty::Text(tr("Mixed")));
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
        }
        Field::Check { label, .. } => {
            let on = r.params.iter().all(|(_, p)| p.get(param).and_then(Value::as_bool).unwrap_or(false));
            if widgets::checkbox(ui, on, tr(label)) {
                set(app, json!(!on));
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
            let resp = widgets::dropdown(ui, label, ui.available_width(), None);
            menu_popup(&resp).show(|ui| {
                for (value, l) in *options {
                    if menu_item(ui, tl(l), "") {
                        set(app, json!(value));
                        ui.close();
                    }
                }
            });
        }
    }
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
