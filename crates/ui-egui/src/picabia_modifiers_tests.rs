//! PICABIA: the Design panel's Modifiers section, driven through egui headless like FigCraft's own
//! panel suites (`panel_reorder_tests`, `panel_remove_tests`, `property_menu_tests`,
//! `field_tests`): adding from the `+` menu, switching kind, reordering by drag, removing and
//! toggling rows, scrubbing a field as one undo step, "Mixed" across layers, the row and header
//! menus, and the field table covering every kind's parameters; the copy-to-shape target picker
//! (the layers every selected layer can copy onto, a refused target flagged with its reason) and
//! Duplicate carrying variable bindings; the stencil's grouped settings (colour mode, swatch list
//! with cut toggles, size, clean-up, ties, preview colours with FigCraft's swatch and picker): the
//! Ties group's modes (ties, a tie pattern with fallback ties, gauze), its optional direction, and
//! the stats line and the problem rows (one per kind, each click showing its next marker); the sheet material (a preset fills the limits), output
//! (a tile only with a repeat above), repeat, limit mode, sharpest corner and registration holes.

use egui::accesskit::Role;
use egui::{Event, Id, Modifiers, PointerButton, Pos2, pos2};
use figcraft_doc::NodeId;
use figcraft_engine::Session;
use figcraft_engine::picabia_stencil::ProblemKind;
use figcraft_geom::Rect;
use serde_json::{Value, json};

use super::{FIELDS, KIND_MENU, LIST, MOD_OPEN, StencilColour, StencilInfo, cache_stencil_info, fields_of, known, shared_targets};
use crate::layers_drag_tests::Ui;
use crate::list_drag::grip_id;
use crate::live_tests::{flat, text_center};

fn id(v: &Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

fn rect(s: &mut Session, x: f64) -> NodeId {
    id(&s.execute("shape.rectangle", &json!({"x": x, "y": 0, "width": 40, "height": 40})).unwrap())
}

/// A session with one rectangle, selected, carrying modifiers of `kinds` (top row first).
fn setup(kinds: &[&str]) -> (Ui, NodeId) {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    for k in kinds {
        s.execute("modifier.add", &json!({"ids": [r], "kind": k})).unwrap();
    }
    s.execute("select.set", &json!({"ids": [r]})).unwrap();
    let mut u = Ui::new(s);
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    (u, r)
}

/// The stack of `n` as `modifier.list` reports it.
fn stack(u: &mut Ui, n: NodeId) -> Vec<Value> {
    u.app.session.execute("modifier.list", &json!({"id": n})).unwrap()["modifiers"].as_array().cloned().unwrap_or_default()
}

fn kinds(u: &mut Ui, n: NodeId) -> Vec<String> {
    stack(u, n).iter().map(|m| m["kind"].as_str().unwrap().to_string()).collect()
}

fn undo_steps(u: &Ui) -> usize {
    u.app.session.ds().unwrap().history.undo.len()
}

fn wheel(u: &mut Ui, dy: f32) {
    let wheel = Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        source: egui::MouseWheelSource::Unknown,
        delta: egui::vec2(0.0, dy),
        phase: egui::TouchPhase::Move,
        modifiers: Default::default(),
    };
    u.frame(vec![Event::PointerMoved(Pos2::new(1300.0, 500.0)), wheel]);
    for _ in 0..30 {
        u.frame(vec![]);
    }
}

/// Scroll the Design panel until the widget `id` lies in view; its rect.
fn into_view(u: &mut Ui, id: Id) -> egui::Rect {
    for _ in 0..5 {
        u.frame(vec![]);
    }
    for _ in 0..30 {
        let r = u.ctx.read_response(id).unwrap_or_else(|| panic!("{id:?} is shown")).rect;
        if r.max.y < 860.0 && r.min.y > 60.0 {
            return r;
        }
        wheel(u, if r.max.y >= 860.0 { -100.0 } else { 100.0 });
    }
    panic!("{id:?} never came into view");
}

/// The bounds' centre of the AccessKit nodes labelled `label`.
fn labelled(u: &mut Ui, label: &str) -> Vec<Pos2> {
    let out = u.frame_output(vec![]);
    out.platform_output
        .accesskit_update
        .unwrap()
        .nodes
        .into_iter()
        .filter(|(_, n)| n.label() == Some(label))
        .filter_map(|(_, n)| n.bounds())
        .map(|b| Pos2::new(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))
        .collect()
}

/// The button labelled `label` in modifier row `index`.
fn row_button(u: &mut Ui, index: usize, label: &str) -> Pos2 {
    let row = into_view(u, grip_id(LIST, index));
    labelled(u, label)
        .into_iter()
        .find(|p| p.y > row.min.y && p.y < row.min.y + 32.0)
        .unwrap_or_else(|| panic!("no {label:?} button in modifier row {index}"))
}

/// A click with `button` at `p`, and the frames that follow; what the last frame drew.
fn click_with(u: &mut Ui, p: Pos2, button: PointerButton) -> Vec<egui::Shape> {
    for _ in 0..3 {
        u.frame(vec![Event::PointerMoved(p)]);
    }
    let mut shapes = Vec::new();
    for pressed in [Some(true), Some(false)].into_iter().chain(std::iter::repeat_n(None, 10)) {
        let events = pressed.map(|pressed| Event::PointerButton { pos: p, button, pressed, modifiers: Modifiers::NONE });
        shapes = flat(&u.frame_output(events.into_iter().collect()).shapes);
    }
    shapes
}

fn click(u: &mut Ui, p: Pos2) -> Vec<egui::Shape> {
    click_with(u, p, PointerButton::Primary)
}

fn texts(shapes: &[egui::Shape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// The middle of modifier row `index`'s kind dropdown (where its label is drawn).
fn dropdown(u: &mut Ui, index: usize) -> Pos2 {
    let row = into_view(u, grip_id(LIST, index));
    pos2(row.min.x + 80.0, row.center().y)
}

fn open_settings(u: &mut Ui, index: usize) {
    u.ctx.data_mut(|d| d.insert_temp(Id::unique((MOD_OPEN, index)), true));
    u.frame(vec![]);
}

/// The middle of the label (what scrubs) of modifier row `index`'s `param` field.
fn lead(u: &mut Ui, param: &'static str, index: usize) -> Pos2 {
    into_view(u, Id::unique(("mod", param, index)).with("lead")).center()
}

/// What modifier row `index`'s `param` field shows (its AccessKit value).
fn shown(u: &mut Ui, param: &'static str, index: usize) -> String {
    let at = lead(u, param, index);
    let out = u.frame_output(vec![]);
    let nodes = out.platform_output.accesskit_update.unwrap().nodes;
    let n = nodes
        .iter()
        .map(|(_, n)| n)
        .filter(|n| n.role() == Role::SpinButton)
        .find(|n| n.bounds().is_some_and(|b| (b.y0 as f32) <= at.y && at.y <= b.y1 as f32 && (b.x0 as f32) <= at.x + 30.0 && at.x <= b.x1 as f32))
        .unwrap_or_else(|| panic!("no field for {param}"));
    n.value().unwrap_or_default().to_string()
}

#[test]
fn the_field_table_covers_every_kind_and_parameter() {
    let menu: Vec<&str> = KIND_MENU.iter().map(|(k, _)| *k).collect();
    let model: Vec<&str> = figcraft_modifiers::KINDS.iter().map(|(k, _)| *k).collect();
    assert_eq!(menu, model, "the + menu offers every kind, in the model's order");
    for ((k, l), (mk, ml)) in KIND_MENU.iter().zip(figcraft_modifiers::KINDS) {
        assert_eq!((k, l), (mk, ml), "labels match the model's");
    }
    for (kind, _) in figcraft_modifiers::KINDS {
        assert!(known(kind), "{kind} has settings");
        let defaults = figcraft_modifiers::ModifierKind::default_for(kind).unwrap().params();
        let fields: Vec<&str> = fields_of(kind).iter().flat_map(|r| r.iter().map(|f| f.param())).collect();
        for p in &fields {
            assert!(defaults.contains_key(*p), "{kind} has no param {p}");
        }
        for p in defaults.keys() {
            assert!(fields.contains(&p.as_str()), "{kind}'s param {p} has no field");
        }
    }
    assert_eq!(FIELDS.len(), KIND_MENU.len());
}

#[test]
fn the_plus_menu_adds_a_modifier_in_one_step() {
    let (mut u, r) = setup(&[]);
    let steps = undo_steps(&u);
    let plus = labelled(&mut u, "Add modifier");
    let plus = *plus.first().expect("the section's + button");
    let menu = click(&mut u, plus);
    for (_, label) in KIND_MENU {
        assert!(texts(&menu).iter().any(|t| t == label), "the menu offers {label}");
    }
    click(&mut u, text_center(&menu, "Circular array"));
    assert_eq!(kinds(&mut u, r), ["circularArray"]);
    assert_eq!(undo_steps(&u), steps + 1);
    assert!(u.app.ui.last_error.is_none(), "{:?}", u.app.ui.last_error);
}

#[test]
fn the_kind_dropdown_switches_kind_in_place_in_one_step() {
    let (mut u, r) = setup(&["linearArray", "gridArray"]);
    u.app.run("modifier.toggle", json!({"ids": [r], "index": 0, "enabled": false}));
    let steps = undo_steps(&u);
    let at = dropdown(&mut u, 0);
    let menu = click(&mut u, at);
    click(&mut u, text_center(&menu, "Mirror"));
    let s = stack(&mut u, r);
    assert_eq!(kinds(&mut u, r), ["mirror", "gridArray"], "the row keeps its place");
    assert_eq!(s[0]["enabled"], json!(false), "and its eye");
    assert_eq!(undo_steps(&u), steps + 1);
    u.app.run("edit.undo", json!({}));
    assert_eq!(kinds(&mut u, r), ["linearArray", "gridArray"]);
}

#[test]
fn dragging_a_row_reorders_the_stack_in_one_step() {
    let (mut u, r) = setup(&["linearArray", "mirror", "gridArray"]);
    open_settings(&mut u, 0);
    let bottom = into_view(&mut u, grip_id(LIST, 2));
    let top = u.ctx.read_response(grip_id(LIST, 0)).unwrap().rect;
    assert!(top.min.y > 60.0, "both rows in view: {top:?}");
    assert!(top.center().y < bottom.center().y, "the top row (first to run) is listed first");
    let steps = undo_steps(&u);
    let (from, to) = (top.center(), bottom.center() + egui::vec2(0.0, 12.0));
    u.button(from, true);
    for k in 1..=10 {
        u.frame(vec![Event::PointerMoved(from + (to - from) * (k as f32 / 10.0))]);
    }
    assert_eq!(kinds(&mut u, r), ["linearArray", "mirror", "gridArray"], "nothing moves before the drop");
    u.button(to, false);
    assert_eq!(kinds(&mut u, r), ["mirror", "gridArray", "linearArray"]);
    assert_eq!(undo_steps(&u), steps + 1);
    let open = |u: &Ui, i: usize| u.ctx.data(|d| d.get_temp::<bool>(Id::unique((MOD_OPEN, i)))).unwrap_or(false);
    assert!(open(&u, 2) && !open(&u, 0), "the open settings went with their row");
}

#[test]
fn remove_and_toggle_act_on_their_row_and_keep_open_settings_with_theirs() {
    let (mut u, r) = setup(&["linearArray", "mirror", "gridArray"]);
    open_settings(&mut u, 2);
    let rm = row_button(&mut u, 0, "Remove");
    click(&mut u, rm);
    assert_eq!(kinds(&mut u, r), ["mirror", "gridArray"]);
    let open = |u: &Ui, i: usize| u.ctx.data(|d| d.get_temp::<bool>(Id::unique((MOD_OPEN, i)))).unwrap_or(false);
    assert!(open(&u, 1) && !open(&u, 2));
    let eye = row_button(&mut u, 1, "Toggle");
    click(&mut u, eye);
    let s = stack(&mut u, r);
    assert_eq!((s[0]["enabled"].clone(), s[1]["enabled"].clone()), (json!(true), json!(false)));
    let eye = row_button(&mut u, 1, "Toggle");
    click(&mut u, eye);
    assert_eq!(stack(&mut u, r)[1]["enabled"], json!(true));
}

#[test]
fn scrubbing_a_field_is_live_and_one_undo_step() {
    let (mut u, r) = setup(&["linearArray"]);
    open_settings(&mut u, 0);
    let steps = undo_steps(&u);
    let from = lead(&mut u, "offsetX", 0);
    u.frame(vec![
        Event::PointerMoved(from),
        Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ]);
    for k in 1..=10 {
        u.frame(vec![Event::PointerMoved(pos2(from.x + 2.0 * k as f32, from.y))]);
    }
    assert_eq!(stack(&mut u, r)[0]["offsetX"], json!(120.0), "live while dragging");
    let to = pos2(from.x + 20.0, from.y);
    u.frame(vec![
        Event::PointerMoved(to),
        Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    u.frame(vec![]);
    assert_eq!(stack(&mut u, r)[0]["offsetX"], json!(120.0));
    assert_eq!(undo_steps(&u), steps + 1, "one undo step for the whole scrub");
    // A count scrubbed with Shift moves ten a point, whole numbers.
    let from = lead(&mut u, "count", 0);
    let shift = Modifiers { shift: true, ..Modifiers::NONE };
    u.frame_with(
        vec![Event::PointerMoved(from), Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: shift }],
        shift,
    );
    for k in 1..=10 {
        u.frame_with(vec![Event::PointerMoved(pos2(from.x + k as f32, from.y))], shift);
    }
    let to = pos2(from.x + 10.0, from.y);
    u.frame_with(
        vec![Event::PointerMoved(to), Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: shift }],
        shift,
    );
    u.frame(vec![]);
    assert_eq!(stack(&mut u, r)[0]["count"], json!(103));
    assert_eq!(undo_steps(&u), steps + 2);
    u.app.run("edit.undo", json!({}));
    assert_eq!(stack(&mut u, r)[0]["count"], json!(3));
}

#[test]
fn fields_show_mixed_across_layers_and_take_relative_edits_per_layer() {
    let mut s = Session::new();
    let a = rect(&mut s, 0.0);
    let b = rect(&mut s, 100.0);
    s.execute("modifier.add", &json!({"ids": [a], "kind": "linearArray", "props": {"count": 3}})).unwrap();
    s.execute("modifier.add", &json!({"ids": [b], "kind": "linearArray", "props": {"count": 5}})).unwrap();
    s.execute("select.set", &json!({"ids": [a, b]})).unwrap();
    let mut u = Ui::new(s);
    u.ctx.enable_accesskit();
    open_settings(&mut u, 0);
    assert_eq!(shown(&mut u, "count", 0), "Mixed");
    assert_eq!(shown(&mut u, "offsetX", 0), "100", "the same offset on both shows");
    // `+2` typed into the mixed count adds two to each layer's own, in one step.
    let steps = undo_steps(&u);
    let at = lead(&mut u, "count", 0) + egui::vec2(40.0, 0.0);
    click(&mut u, at);
    u.frame(vec![Event::Text("+2".into())]);
    let enter = |pressed| Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    u.frame(vec![enter(true), enter(false)]);
    u.frame(vec![]);
    assert_eq!((stack(&mut u, a)[0]["count"].clone(), stack(&mut u, b)[0]["count"].clone()), (json!(5), json!(7)));
    assert_eq!(undo_steps(&u), steps + 1);
    // Stacks of different kinds have no rows ("Mixed", like effects).
    u.app.run("modifier.add", json!({"ids": [b], "kind": "mirror"}));
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert!(labelled(&mut u, "Modifier settings").is_empty(), "no rows for stacks that differ");
}

#[test]
fn the_row_menu_copies_pastes_and_duplicates() {
    let (mut u, r) = setup(&["linearArray", "mirror"]);
    u.app.run("modifier.set", json!({"ids": [r], "index": 0, "props": {"count": 7}}));
    let at = dropdown(&mut u, 0);
    let menu = click_with(&mut u, at, PointerButton::Secondary);
    assert!(texts(&menu).iter().any(|t| t == "Duplicate"));
    let apply = texts(&menu).iter().any(|t| t == "Apply up to here");
    assert_eq!(apply, figcraft_engine::find_command("modifier.apply").is_some(), "Apply up to here only with modifier.apply");
    assert!(!texts(&menu).iter().any(|t| t == "Paste modifier"), "nothing to paste yet");
    click(&mut u, text_center(&menu, "Copy modifier"));
    assert_eq!(u.app.session.clipboard.picabia_modifiers.len(), 1);
    // Paste from row 0's menu: the copy lands right below row 0, in one step.
    let steps = undo_steps(&u);
    let at = dropdown(&mut u, 0);
    let menu = click_with(&mut u, at, PointerButton::Secondary);
    click(&mut u, text_center(&menu, "Paste modifier"));
    assert_eq!(kinds(&mut u, r), ["linearArray", "linearArray", "mirror"], "pasted right below the row");
    assert_eq!(stack(&mut u, r)[1]["count"], json!(7));
    assert_eq!(undo_steps(&u), steps + 1);
    // Duplicate the mirror: the copy sits right under it.
    u.app.run("modifier.set", json!({"ids": [r], "index": 2, "props": {"axis": "both"}}));
    let at = dropdown(&mut u, 2);
    let menu = click_with(&mut u, at, PointerButton::Secondary);
    click(&mut u, text_center(&menu, "Duplicate"));
    let s = stack(&mut u, r);
    assert_eq!(kinds(&mut u, r), ["linearArray", "linearArray", "mirror", "mirror"]);
    assert_eq!(s[3]["axis"], json!("both"));
    assert_ne!(s[3]["id"], s[2]["id"]);
}

#[test]
fn the_header_menu_pastes_modifiers_onto_another_layer() {
    let mut s = Session::new();
    let a = rect(&mut s, 0.0);
    let b = rect(&mut s, 100.0);
    s.execute("modifier.add", &json!({"ids": [a], "kind": "gridArray"})).unwrap();
    s.execute("modifier.add", &json!({"ids": [a], "kind": "randomize"})).unwrap();
    s.execute("modifier.copy", &json!({"id": a})).unwrap();
    s.execute("select.set", &json!({"ids": [b]})).unwrap();
    let mut u = Ui::new(s);
    for _ in 0..3 {
        u.frame(vec![]);
    }
    let mut shapes = flat(&u.frame_output(vec![]).shapes);
    for _ in 0..10 {
        if texts(&shapes).iter().any(|t| t == "Modifiers") && text_center(&shapes, "Modifiers").y < 860.0 {
            break;
        }
        wheel(&mut u, -100.0);
        shapes = flat(&u.frame_output(vec![]).shapes);
    }
    let menu = click_with(&mut u, text_center(&shapes, "Modifiers"), PointerButton::Secondary);
    click(&mut u, text_center(&menu, "Paste modifiers"));
    assert_eq!(kinds(&mut u, b), ["gridArray", "randomize"]);
}

#[test]
fn a_checkbox_and_the_mirror_axis_dropdown_set_their_params() {
    let (mut u, r) = setup(&["mirror"]);
    open_settings(&mut u, 0);
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let menu = click(&mut u, text_center(&shapes, "Vertical axis"));
    click(&mut u, text_center(&menu, "Both axes"));
    assert_eq!(stack(&mut u, r)[0]["axis"], json!("both"));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    click(&mut u, text_center(&shapes, "Keep original"));
    assert_eq!(stack(&mut u, r)[0]["keepOriginal"], json!(false));
}

/// A field bound to a variable shows the variable's chip (FigCraft's own bound field), and its
/// Detach button unbinds it.
#[test]
fn a_bound_field_shows_its_variable_and_detaches() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    s.execute("modifier.add", &json!({"ids": [r], "kind": "linearArray"})).unwrap();
    let c = s.execute("variable.createCollection", &json!({"name": "Theme", "modes": ["Light"]})).unwrap();
    let v = s.execute("variable.create", &json!({"collection": c["id"], "name": "copies", "type": "number", "value": 4})).unwrap();
    let key = figcraft_modifiers::binding_key("m1", "count");
    s.execute("variable.bind", &json!({"ids": [r], "field": key, "variable": v["id"]})).unwrap();
    s.execute("select.set", &json!({"ids": [r]})).unwrap();
    let mut u = Ui::new(s);
    u.ctx.enable_accesskit();
    open_settings(&mut u, 0);
    into_view(&mut u, grip_id(LIST, 0));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    assert!(texts(&shapes).iter().any(|t| t == "copies"), "the count field shows the variable");
    // Alone on its row, the bound field takes both columns so the chip has room for the name.
    let nodes = u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes;
    let width =
        |label: &str| nodes.iter().filter(|(_, n)| n.label() == Some(label)).filter_map(|(_, n)| n.bounds()).map(|b| b.x1 - b.x0).fold(0.0, f64::max);
    // The offset X field below it is one column wide.
    assert!(width("X") > 40.0, "the X field is found");
    assert!(width("Count") > width("X") * 1.5, "the bound count field spans the row: {} vs {}", width("Count"), width("X"));
    let detach = labelled(&mut u, "Detach variable");
    let detach = *detach.first().expect("a Detach button");
    // The button shows while the field is hovered.
    u.frame(vec![Event::PointerMoved(detach)]);
    click(&mut u, detach);
    let doc = u.app.session.doc().unwrap();
    assert!(!doc.node(r).unwrap().bound.contains_key(&key), "detached");
}

/// A per-copy expression: added from the field's right-click menu, typed and committed; it then
/// shows as a chip; a typo is refused with a note; the menu removes it.
#[test]
fn an_expression_is_added_typed_shown_and_removed() {
    let (mut u, r) = setup(&["transformEach"]);
    open_settings(&mut u, 0);
    let at = lead(&mut u, "rotate", 0) + egui::vec2(30.0, 0.0);
    let menu = click_with(&mut u, at, PointerButton::Secondary);
    click(&mut u, text_center(&menu, "Add expression"));
    let enter = |pressed| Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    // A typo stays open with why.
    u.frame(vec![Event::Text("i *".into())]);
    u.frame(vec![enter(true), enter(false)]);
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert!(stack(&mut u, r)[0].get("rotateExpr").is_none(), "nothing set");
    let shapes = flat(&u.frame_output(vec![]).shapes);
    assert!(texts(&shapes).iter().any(|t| t.starts_with("Expression: ")), "the error shows: {:?}", texts(&shapes));
    // Fixed, it's set in one step.
    let steps = undo_steps(&u);
    u.frame(vec![Event::Text(" 15".into())]);
    u.frame(vec![enter(true), enter(false)]);
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert_eq!(stack(&mut u, r)[0]["rotateExpr"], json!("i * 15"));
    assert_eq!(undo_steps(&u), steps + 1);
    let shapes = flat(&u.frame_output(vec![]).shapes);
    assert!(texts(&shapes).iter().any(|t| t == "= i * 15"), "the chip shows the expression");
    let menu = click_with(&mut u, text_center(&shapes, "= i * 15"), PointerButton::Secondary);
    click(&mut u, text_center(&menu, "Remove expression"));
    assert!(stack(&mut u, r)[0].get("rotateExpr").is_none_or(Value::is_null), "removed");
}

/// Row Duplicate is `modifier.duplicate`: the copy carries the row's variable bindings, under its
/// own id, in one undo step.
#[test]
fn duplicate_carries_the_rows_variable_bindings() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    s.execute("modifier.add", &json!({"ids": [r], "kind": "linearArray"})).unwrap();
    let c = s.execute("variable.createCollection", &json!({"name": "Theme", "modes": ["Light"]})).unwrap();
    let v = s.execute("variable.create", &json!({"collection": c["id"], "name": "copies", "type": "number", "value": 4})).unwrap();
    s.execute("variable.bind", &json!({"ids": [r], "field": figcraft_modifiers::binding_key("m1", "count"), "variable": v["id"]})).unwrap();
    s.execute("select.set", &json!({"ids": [r]})).unwrap();
    let mut u = Ui::new(s);
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let steps = undo_steps(&u);
    let at = dropdown(&mut u, 0);
    let menu = click_with(&mut u, at, PointerButton::Secondary);
    click(&mut u, text_center(&menu, "Duplicate"));
    let st = stack(&mut u, r);
    assert_eq!(st.len(), 2);
    assert_eq!(undo_steps(&u), steps + 1);
    let copy = st[1]["id"].as_str().unwrap().to_string();
    assert_ne!(copy, "m1");
    let doc = u.app.session.doc().unwrap();
    let bound = &doc.node(r).unwrap().bound;
    assert_eq!(bound.get(&figcraft_modifiers::binding_key(&copy, "count")), bound.get(&figcraft_modifiers::binding_key("m1", "count")));
    assert!(bound.contains_key(&figcraft_modifiers::binding_key(&copy, "count")), "the copy is bound too");
}

/// The picker offers what `modifier::target_choices` allows every selected layer: not the
/// selected layers themselves, not a group around one of them.
#[test]
fn the_target_picker_offers_the_layers_every_selected_layer_can_copy_onto() {
    let mut s = Session::new();
    let a = rect(&mut s, 0.0);
    let b = rect(&mut s, 100.0);
    let e = id(&s.execute("shape.ellipse", &json!({"x": 200, "y": 0, "width": 80, "height": 40})).unwrap());
    let doc = s.doc().unwrap();
    let both: Vec<NodeId> = shared_targets(doc, &[a, b]).into_iter().map(|(t, _)| t).collect();
    assert!(both.contains(&e));
    assert!(!both.contains(&a) && !both.contains(&b), "neither selected layer: each can't copy onto itself");
    let only_a: Vec<NodeId> = shared_targets(doc, &[a]).into_iter().map(|(t, _)| t).collect();
    assert!(only_a.contains(&b) && only_a.contains(&e) && !only_a.contains(&a));
    // In the panel: the dropdown lists the ellipse and picking it sets the target.
    s.execute("modifier.add", &json!({"ids": [a, b], "kind": "copyToShape"})).unwrap();
    s.execute("select.set", &json!({"ids": [a, b]})).unwrap();
    let mut u = Ui::new(s);
    u.ctx.enable_accesskit();
    open_settings(&mut u, 0);
    into_view(&mut u, grip_id(LIST, 0));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let menu = click(&mut u, text_center(&shapes, "Choose a layer"));
    let name = u.app.session.doc().unwrap().node(e).unwrap().name.clone();
    let names = texts(&menu);
    assert!(names.contains(&name), "{names:?}");
    // The menu's entry, not the Layers panel's row of the same name (the panel is on the left).
    let entry = menu
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.visual_bounding_rect().center()),
            _ => None,
        })
        .max_by(|p, q| p.x.total_cmp(&q.x))
        .unwrap();
    click(&mut u, entry);
    for n in [a, b] {
        assert_eq!(stack(&mut u, n)[0]["target"], json!(e.0.to_string()));
    }
}

/// A target that can't be used (here: deleted) is flagged under the target field with its reason,
/// once (the evaluation's warning saying the same isn't repeated).
#[test]
fn a_refused_target_shows_its_reason_once() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    let e = id(&s.execute("shape.ellipse", &json!({"x": 200, "y": 0, "width": 80, "height": 40})).unwrap());
    s.execute("modifier.add", &json!({"ids": [r], "kind": "copyToShape", "props": {"target": e.0.to_string(), "mode": "alongPath"}})).unwrap();
    s.execute("edit.delete", &json!({"ids": [e]})).unwrap();
    s.execute("select.set", &json!({"ids": [r]})).unwrap();
    let mut u = Ui::new(s);
    u.ctx.enable_accesskit();
    // Closed: the evaluation's warning shows under the row.
    into_view(&mut u, grip_id(LIST, 0));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let gone = |t: &String| t.contains("the target layer is gone");
    assert_eq!(texts(&shapes).iter().filter(|t| gone(t)).count(), 1, "{:?}", texts(&shapes));
    // Open: the target field says why, and the same warning isn't repeated.
    open_settings(&mut u, 0);
    into_view(&mut u, grip_id(LIST, 0));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let t = texts(&shapes);
    assert!(t.iter().any(|t| t.starts_with("Target: ") && gone(t)), "{t:?}");
    assert_eq!(t.iter().filter(|t| gone(t)).count(), 1, "{t:?}");
    assert!(t.iter().any(|t| t == "Missing layer"));
}

/// Whether modifier row `index`'s `param` field is drawn.
fn has_field(u: &mut Ui, param: &'static str, index: usize) -> bool {
    for _ in 0..3 {
        u.frame(vec![]);
    }
    u.ctx.read_response(Id::unique(("mod", param, index)).with("lead")).is_some()
}

#[test]
fn wallpaper_shows_only_the_inputs_its_group_uses() {
    use figcraft_modifiers::WallpaperGroup;
    let (mut u, r) = setup(&["wallpaper"]);
    open_settings(&mut u, 0);
    for g in WallpaperGroup::ALL {
        let name = serde_json::to_value(g).unwrap();
        u.app.run("modifier.set", json!({"ids": [r], "index": 0, "props": {"group": name}}));
        for p in ["cols", "rows", "cellWidth"] {
            assert!(has_field(&mut u, p, 0), "{g:?} shows {p}");
        }
        assert_eq!(has_field(&mut u, "cellHeight", 0), g.uses_cell_height(), "{g:?} cellHeight");
        assert_eq!(has_field(&mut u, "angle", 0), g.uses_angle(), "{g:?} angle");
        assert_eq!(has_field(&mut u, "originX", 0), g.uses_origin(), "{g:?} originX");
        assert_eq!(has_field(&mut u, "originY", 0), g.uses_origin(), "{g:?} originY");
    }
}

#[test]
fn wallpaper_numbers_have_the_evaluators_limits() {
    let rows = fields_of("wallpaper");
    let limits = |param: &str| {
        rows.iter().flat_map(|r| r.iter()).find_map(|f| match f {
            super::Field::Num { param: p, min, max, .. } if *p == param => Some((*min, *max)),
            _ => None,
        })
    };
    assert_eq!(limits("cols"), Some((1.0, 100.0)));
    assert_eq!(limits("rows"), Some((1.0, 100.0)));
    assert_eq!(limits("cellWidth"), Some((0.001, figcraft_modifiers::MAX_LENGTH)));
    assert_eq!(limits("cellHeight"), Some((0.001, figcraft_modifiers::MAX_LENGTH)));
    assert_eq!(limits("angle"), Some((1.0, 179.0)));
    assert_eq!(limits("originX"), Some((-figcraft_modifiers::MAX_LENGTH, figcraft_modifiers::MAX_LENGTH)));
}

fn enter(u: &mut Ui) {
    let key = |pressed| Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    u.frame(vec![key(true), key(false)]);
    u.frame(vec![]);
}

/// Scroll the Design panel until the widget `id` lies in its upper part (clear of the panel's
/// foot); its rect.
fn centred(u: &mut Ui, id: Id) -> egui::Rect {
    let mut r = into_view(u, id);
    for _ in 0..40 {
        if r.min.y > 150.0 && r.max.y < 600.0 {
            break;
        }
        wheel(u, if r.max.y >= 600.0 { -100.0 } else { 100.0 });
        let moved = into_view(u, id);
        if moved == r {
            break;
        }
        r = moved;
    }
    r
}

/// Scroll the Design panel to its end.
fn to_bottom(u: &mut Ui) {
    for _ in 0..5 {
        wheel(u, -1000.0);
    }
}

/// Type `text` into modifier row 0's `param` number field and press Enter.
fn type_into(u: &mut Ui, param: &'static str, text: &str) {
    let id = Id::unique(("mod", param, 0)).with("lead");
    centred(u, id);
    for _ in 0..3 {
        u.frame(vec![]);
    }
    let lead = u.ctx.read_response(id).unwrap().rect;
    click(u, pos2(lead.max.x + 6.0, lead.center().y));
    u.frame(vec![Event::Text(text.into())]);
    enter(u);
}

#[test]
fn stencil_numbers_set_their_params() {
    let (mut u, r) = setup(&["stencil"]);
    open_settings(&mut u, 0);
    for (param, typed, want) in [
        ("threshold", "30", json!(0.3)),
        ("widthMm", "420", json!(420.0)),
        ("marginMm", "20", json!(20.0)),
        ("minCutAreaMm2", "2", json!(2.0)),
        ("minCutWidthMm", "0.4", json!(0.4)),
        ("minBridgeMm", "0.8", json!(0.8)),
        ("toleranceMm", "0.1", json!(0.1)),
    ] {
        let steps = undo_steps(&u);
        type_into(&mut u, param, typed);
        assert_eq!(stack(&mut u, r)[0][param], want, "{param}");
        assert_eq!(undo_steps(&u), steps + 1, "{param}: one undo step");
    }
    // Typed past the model's limits, a number stops at them.
    type_into(&mut u, "toleranceMm", "50");
    assert_eq!(stack(&mut u, r)[0]["toleranceMm"], json!(10.0));
}

#[test]
fn stencil_settings_are_grouped_under_small_labels() {
    let (mut u, _) = setup(&["stencil"]);
    open_settings(&mut u, 0);
    let drawn = texts(&flat(&u.frame_output(vec![]).shapes));
    let at = |label: &str| drawn.iter().position(|t| t == label).unwrap_or_else(|| panic!("{label:?} is drawn"));
    assert!(
        at("Colours") < at("Size")
            && at("Size") < at("Clean-up")
            && at("Clean-up") < at("Ties")
            && at("Ties") < at("Registration")
            && at("Registration") < at("Preview"),
        "in order: {drawn:?}"
    );
    assert!(at("Preview") < at("Paper"), "the paper colour is named under Preview");
}

#[test]
fn the_colour_mode_shows_the_threshold_or_the_swatch_list() {
    let (mut u, r) = setup(&["stencil"]);
    open_settings(&mut u, 0);
    assert!(has_field(&mut u, "threshold", 0), "threshold mode shows the threshold");
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let menu = click(&mut u, text_center(&shapes, "Cut the lighter colours"));
    click(&mut u, text_center(&menu, "Cut the colours I pick"));
    assert_eq!(stack(&mut u, r)[0]["colourMode"], json!("pick"));
    assert!(!has_field(&mut u, "threshold", 0), "pick mode hides the threshold");
    // ...and lists the design's colours once the engine has computed the stencil (off the UI
    // thread): the rectangle's grey, with its cut toggle, which picks it.
    let mut toggle = Vec::new();
    for _ in 0..400 {
        toggle = labelled(&mut u, "Cut out #D9D9D9");
        if !toggle.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(toggle.len(), 1, "the rectangle's colour is listed");
    click(&mut u, toggle[0]);
    assert_eq!(stack(&mut u, r)[0]["cutColours"], json!(["#d9d9d9"]));
    // Invert stays in both modes.
    let shapes = flat(&u.frame_output(vec![]).shapes);
    click(&mut u, text_center(&shapes, "Invert (cut the other colours)"));
    assert_eq!(stack(&mut u, r)[0]["invert"], json!(true));
}

fn info(colours: &[(&str, bool, f64)]) -> StencilInfo {
    StencilInfo {
        colours: colours.iter().map(|(hex, cut, area)| StencilColour { hex: hex.to_string(), cut: *cut, area: *area }).collect(),
        warnings: vec!["Image fills are skipped".into()],
        stats: Some(stats(0, 0, 0, 0)),
        sheet: Some((330.0, 245.5)),
        ..StencilInfo::default()
    }
}

#[test]
fn a_swatch_toggle_writes_cut_colours_in_one_step() {
    let (mut u, r) = setup(&["stencil"]);
    u.app.run("modifier.set", json!({"ids": [r], "index": 0, "props": {"colourMode": "pick", "cutColours": ["#ffffff"]}}));
    cache_stencil_info(&u.ctx, &u.app.session, r, info(&[("#1f3a8a", false, 1200.0), ("#ffffff", true, 300.25)]));
    open_settings(&mut u, 0);
    // Notes are drawn only in view: look at the size, then at the foot of the panel.
    for (bottom, want) in [
        (false, &["1F3A8A", "1200 mm²", "FFFFFF", "300.3 mm²", "Sheet 330 × 245.5 mm, with the margin"][..]),
        (true, &["4 pieces, 2 specks dropped, 812.5 mm cut", "Image fills are skipped"][..]),
    ] {
        if bottom {
            to_bottom(&mut u);
        } else {
            into_view(&mut u, Id::unique(("mod", "widthMm", 0)).with("lead"));
        }
        let drawn = texts(&flat(&u.frame_output(vec![]).shapes));
        for t in want {
            assert!(drawn.iter().any(|d| d == t), "{t:?} is drawn: {drawn:?}");
        }
    }
    let steps = undo_steps(&u);
    // The swatch list sits just over the size.
    centred(&mut u, Id::unique(("mod", "widthMm", 0)).with("lead"));
    let blue = labelled(&mut u, "Cut out #1F3A8A");
    assert_eq!(blue.len(), 1, "one cut toggle per colour, named by it");
    click(&mut u, blue[0]);
    assert_eq!(stack(&mut u, r)[0]["cutColours"], json!(["#ffffff", "#1f3a8a"]));
    assert_eq!(undo_steps(&u), steps + 1);
    cache_stencil_info(&u.ctx, &u.app.session, r, info(&[("#1f3a8a", true, 1200.0), ("#ffffff", true, 300.25)]));
    let white = labelled(&mut u, "Cut out #FFFFFF");
    click(&mut u, white[0]);
    assert_eq!(stack(&mut u, r)[0]["cutColours"], json!(["#1f3a8a"]), "a cut colour toggled off leaves the list");
}

#[test]
fn the_preview_shows_its_colours_and_the_picker_edits_them() {
    let (mut u, r) = setup(&["stencil"]);
    open_settings(&mut u, 0);
    to_bottom(&mut u);
    assert!(!labelled(&mut u, "Paper").is_empty(), "the paper preview shows the paper colour");
    assert!(labelled(&mut u, "Paste").is_empty() && labelled(&mut u, "Cloth").is_empty());
    // A hex typed into the paper colour is written lowercase, as the model keeps it.
    let hex = *labelled(&mut u, "Paper").iter().max_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
    click(&mut u, hex);
    u.frame(vec![Event::Text("FF0000".into())]);
    enter(&mut u);
    assert_eq!(stack(&mut u, r)[0]["paperColour"], json!("#ff0000"));
    // The swatch opens FigCraft's colour picker on that parameter; its edits are modifier.set.
    let swatch = *labelled(&mut u, "Paper").iter().min_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
    click(&mut u, swatch);
    let target = crate::colorpicker::PickerTarget::PicabiaModifier { ids: vec![r], index: 0, param: "paperColour".into() };
    assert_eq!(u.app.ui.picker, Some(target));
    let doc = u.app.session.doc().unwrap();
    let paint = super::picker_paint(doc, &[r], 0, "paperColour").unwrap();
    assert_eq!(paint.solid_color().map(|c| c.to_css_hex()), Some("#FF0000".into()));
    let (cmd, params) =
        super::picker_command(&[r], 0, "paperColour", &figcraft_color::Paint::solid(figcraft_color::Color::from_hex("00ff80").unwrap()));
    u.app.run(cmd, params);
    assert_eq!(stack(&mut u, r)[0]["paperColour"], json!("#00ff80"));
    // The cloth preview shows the paste and cloth colours instead.
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let menu = click(&mut u, text_center(&shapes, "Paper preview"));
    click(&mut u, text_center(&menu, "Dyed cloth preview"));
    assert_eq!(stack(&mut u, r)[0]["preview"], json!("cloth"));
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert!(labelled(&mut u, "Paper").is_empty());
    assert_eq!(labelled(&mut u, "Paste").len(), 2, "swatch and hex");
    assert_eq!(labelled(&mut u, "Cloth").len(), 2, "swatch and hex");
}

/// A stencil's stats: 4 pieces, 2 specks, 812.5 mm cut, and these islands.
fn stats(islands: usize, ties: usize, untied: usize, pattern_held: usize) -> super::StencilStats {
    super::StencilStats { cut_pieces: 4, dropped_specks: 2, cut_length_mm: 812.5, islands, ties, untied, pattern_held }
}

#[test]
fn tie_numbers_set_their_params() {
    let (mut u, r) = setup(&["stencil"]);
    open_settings(&mut u, 0);
    for (param, typed, want) in [("tieWidthMm", "1.5", json!(1.5)), ("tiesPerIsland", "3", json!(3)), ("tieSeed", "7", json!(7))] {
        let steps = undo_steps(&u);
        type_into(&mut u, param, typed);
        assert_eq!(stack(&mut u, r)[0][param], want, "{param}");
        assert_eq!(undo_steps(&u), steps + 1, "{param}: one undo step");
    }
    // The model's limits: 1-8 ties, 0.05-50 mm wide.
    type_into(&mut u, "tiesPerIsland", "20");
    assert_eq!(stack(&mut u, r)[0]["tiesPerIsland"], json!(8));
    type_into(&mut u, "tieWidthMm", "0");
    assert_eq!(stack(&mut u, r)[0]["tieWidthMm"], json!(figcraft_modifiers::MIN_TIE_WIDTH_MM));
    // Spread is a checkbox, on by default.
    assert_eq!(stack(&mut u, r)[0]["tieSpread"], json!(true));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    click(&mut u, text_center(&shapes, "Spread the ties around each island"));
    assert_eq!(stack(&mut u, r)[0]["tieSpread"], json!(false));
}

#[test]
fn the_tie_direction_is_any_until_typed_and_the_minus_unsets_it() {
    let (mut u, r) = setup(&["stencil"]);
    open_settings(&mut u, 0);
    assert_eq!(stack(&mut u, r)[0]["tieAngle"], Value::Null);
    assert_eq!(shown(&mut u, "tieAngle", 0), "Any");
    assert!(labelled(&mut u, "Any direction").is_empty(), "nothing to unset");
    type_into(&mut u, "tieAngle", "45");
    assert_eq!(stack(&mut u, r)[0]["tieAngle"], json!(45.0));
    assert_eq!(shown(&mut u, "tieAngle", 0), "45°");
    // Angles wrap like the model's.
    type_into(&mut u, "tieAngle", "370");
    assert_eq!(stack(&mut u, r)[0]["tieAngle"], json!(10.0));
    let steps = undo_steps(&u);
    let minus = labelled(&mut u, "Any direction");
    assert_eq!(minus.len(), 1);
    click(&mut u, minus[0]);
    assert_eq!(stack(&mut u, r)[0]["tieAngle"], Value::Null);
    assert_eq!(undo_steps(&u), steps + 1);
    assert_eq!(shown(&mut u, "tieAngle", 0), "Any");
}

#[test]
fn the_tie_mode_shows_the_ties_the_pattern_or_neither() {
    let (mut u, r) = setup(&["stencil"]);
    open_settings(&mut u, 0);
    let ties = ["tieWidthMm", "tiesPerIsland", "tieAngle", "tieSeed"];
    let pattern = ["patternWidthMm", "patternSpacingMm", "patternAngle"];
    let drawn = |u: &mut Ui| texts(&flat(&u.frame_output(vec![]).shapes));
    for p in ties {
        assert!(has_field(&mut u, p, 0), "ties mode shows {p}");
    }
    for p in pattern {
        assert!(!has_field(&mut u, p, 0), "ties mode hides {p}");
    }
    assert!(!drawn(&mut u).iter().any(|t| t == "Fallback ties"));
    // A tie pattern: its lattice's settings, then the ties for the islands it misses.
    centred(&mut u, Id::unique(("mod", "tieWidthMm", 0)).with("lead"));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let menu = click(&mut u, text_center(&shapes, "Tie the islands"));
    click(&mut u, text_center(&menu, "Tie with a pattern"));
    assert_eq!(stack(&mut u, r)[0]["tieMode"], json!("pattern"));
    for p in pattern.iter().chain(&ties) {
        assert!(has_field(&mut u, p, 0), "pattern mode shows {p}");
    }
    for (param, typed, want) in [("patternWidthMm", "0.8", json!(0.8)), ("patternSpacingMm", "12", json!(12.0)), ("patternAngle", "30", json!(30.0))]
    {
        type_into(&mut u, param, typed);
        assert_eq!(stack(&mut u, r)[0][param], want, "{param}");
    }
    centred(&mut u, Id::unique(("mod", "patternWidthMm", 0)).with("lead"));
    let t = drawn(&mut u);
    let at = |label: &str| t.iter().position(|x| x == label).unwrap_or_else(|| panic!("{label:?} is drawn: {t:?}"));
    assert!(at("Grid") < at("Everywhere") && at("Everywhere") < at("Fallback ties"), "{t:?}");
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let menu = click(&mut u, text_center(&shapes, "Grid"));
    click(&mut u, text_center(&menu, "Vertical lines"));
    assert_eq!(stack(&mut u, r)[0]["patternType"], json!("vertical"));
    centred(&mut u, Id::unique(("mod", "patternWidthMm", 0)).with("lead"));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let menu = click(&mut u, text_center(&shapes, "Everywhere"));
    click(&mut u, text_center(&menu, "Only around islands"));
    assert_eq!(stack(&mut u, r)[0]["patternWhere"], json!("islands"));
    // Gauze holds the islands: no ties, no lattice.
    centred(&mut u, Id::unique(("mod", "patternWidthMm", 0)).with("lead"));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let menu = click(&mut u, text_center(&shapes, "Tie with a pattern"));
    click(&mut u, text_center(&menu, "Hold the islands with gauze"));
    assert_eq!(stack(&mut u, r)[0]["tieMode"], json!("gauze"));
    for p in pattern.iter().chain(&ties) {
        assert!(!has_field(&mut u, p, 0), "gauze hides {p}");
    }
}

#[test]
fn the_stats_line_counts_islands_and_how_they_are_held() {
    use super::{problem_list, stats_line};
    let line = |s, gauze| stats_line(&s, gauze);
    assert_eq!(line(stats(0, 0, 0, 0), false), "4 pieces, 2 specks dropped, 812.5 mm cut");
    assert_eq!(line(stats(2, 4, 0, 0), false), "4 pieces, 2 islands tied with 4 ties, 2 specks dropped, 812.5 mm cut");
    assert_eq!(line(stats(1, 1, 0, 0), false), "4 pieces, 1 island tied with 1 tie, 2 specks dropped, 812.5 mm cut");
    assert_eq!(line(stats(3, 0, 3, 0), true), "4 pieces, 3 islands held by the gauze, 2 specks dropped, 812.5 mm cut");
    assert_eq!(line(stats(3, 0, 0, 3), false), "4 pieces, 3 islands held by the pattern, 2 specks dropped, 812.5 mm cut");
    assert_eq!(line(stats(4, 2, 1, 2), false), "4 pieces, 2 islands held by the pattern, 1 island tied with 2 ties, 2 specks dropped, 812.5 mm cut");
    let untied = |s, gauze| problem_list(&StencilInfo { stats: Some(s), ..StencilInfo::default() }, gauze);
    assert_eq!(untied(stats(3, 2, 1, 0), false), vec![(ProblemKind::Untied, 1)]);
    assert_eq!(super::problem_text(ProblemKind::Untied, 1), "1 island has no tie and would fall out");
    assert_eq!(untied(stats(3, 0, 3, 0), true), vec![], "gauze holds them");
    assert_eq!(untied(stats(3, 6, 0, 0), false), vec![]);
}

#[test]
fn untied_islands_are_warned_of_under_the_row() {
    let (mut u, r) = setup(&["stencil"]);
    let mut i = info(&[("#1f3a8a", true, 1200.0)]);
    i.stats = Some(stats(3, 4, 1, 0));
    // The stencil's own words for it give way to the panel's (translated) ones.
    i.warnings.push("1 islands couldn't be tied and would fall out".into());
    cache_stencil_info(&u.ctx, &u.app.session, r, i.clone());
    open_settings(&mut u, 0);
    to_bottom(&mut u);
    let drawn = texts(&flat(&u.frame_output(vec![]).shapes));
    for t in ["4 pieces, 2 islands tied with 4 ties, 2 specks dropped, 812.5 mm cut", "1 island has no tie and would fall out"] {
        assert!(drawn.iter().any(|d| d == t), "{t:?} is drawn: {drawn:?}");
    }
    assert!(!drawn.iter().any(|d| d.contains("couldn't be tied")), "said once: {drawn:?}");
    // With gauze nothing falls out.
    u.app.run("modifier.set", json!({"ids": [r], "index": 0, "props": {"tieMode": "gauze"}}));
    i.stats = Some(stats(3, 0, 3, 0));
    i.warnings = vec!["3 islands are left untied (gauze mode): a mesh must hold them".into()];
    cache_stencil_info(&u.ctx, &u.app.session, r, i);
    to_bottom(&mut u);
    let drawn = texts(&flat(&u.frame_output(vec![]).shapes));
    assert!(drawn.iter().any(|d| d == "4 pieces, 3 islands held by the gauze, 2 specks dropped, 812.5 mm cut"), "{drawn:?}");
    assert!(!drawn.iter().any(|d| d.contains("would fall out")), "{drawn:?}");
    assert!(!drawn.iter().any(|d| d.contains("left untied")), "{drawn:?}");
}

/// A stencil result with these problems (kind, count, marked places).
fn with_problems(problems: &[(ProblemKind, usize, &[Rect])]) -> StencilInfo {
    let mut i = info(&[("#1f3a8a", true, 1200.0)]);
    i.warnings.clear();
    i.problem_counts = problems.iter().map(|(k, n, _)| (*k, *n)).collect();
    i.problems = problems.iter().flat_map(|(k, _, places)| places.iter().map(move |b| (*k, *b))).collect();
    i
}

fn drawn(u: &mut Ui) -> Vec<String> {
    texts(&flat(&u.frame_output(vec![]).shapes))
}

#[test]
fn problem_rows_show_each_kind_with_places_and_its_count() {
    let (mut u, r) = setup(&["stencil"]);
    let bridges = [Rect::new(10.0, 10.0, 11.0, 11.0); 3];
    let i = with_problems(&[
        (ProblemKind::NarrowPaper, 12, &bridges),
        (ProblemKind::SmallCut, 0, &[]),
        (ProblemKind::SharpCorner, 1, &[Rect::new(5.0, 5.0, 5.0, 5.0)]),
    ]);
    cache_stencil_info(&u.ctx, &u.app.session, r, i);
    // Under the row, open or not.
    to_bottom(&mut u);
    let t = drawn(&mut u);
    let at = |label: &str| t.iter().position(|x| x == label).unwrap_or_else(|| panic!("{label:?} is drawn: {t:?}"));
    assert!(at("12 narrow paper bridges") < at("1 sharp corner"), "in the kinds' order: {t:?}");
    assert!(!t.iter().any(|x| x.contains("small cut")), "no row for a kind with none: {t:?}");
    open_settings(&mut u, 0);
    to_bottom(&mut u);
    let t = drawn(&mut u);
    let at = |label: &str| t.iter().position(|x| x == label).unwrap_or_else(|| panic!("{label:?} is drawn: {t:?}"));
    assert!(at("4 pieces, 2 specks dropped, 812.5 mm cut") < at("12 narrow paper bridges"), "below the stats line: {t:?}");
    for (k, n, want) in
        [(ProblemKind::NarrowCut, 2, "2 narrow cuts"), (ProblemKind::SmallHole, 1, "1 small hole"), (ProblemKind::SmallCut, 5, "5 small cuts")]
    {
        assert_eq!(super::problem_text(k, n), want);
    }
}

#[test]
fn the_untied_islands_row_is_shown_once() {
    let (mut u, r) = setup(&["stencil"]);
    let mut i = with_problems(&[(ProblemKind::Untied, 2, &[Rect::new(0.0, 0.0, 4.0, 4.0), Rect::new(20.0, 0.0, 24.0, 4.0)])]);
    i.stats = Some(stats(3, 2, 2, 0));
    i.warnings.push("2 islands couldn't be tied and would fall out".into());
    cache_stencil_info(&u.ctx, &u.app.session, r, i);
    to_bottom(&mut u);
    let t = drawn(&mut u);
    assert_eq!(t.iter().filter(|x| x.contains("would fall out")).count(), 1, "{t:?}");
    assert!(t.iter().any(|x| x == "2 islands have no tie and would fall out"), "{t:?}");
}

#[test]
fn clicking_a_problem_row_shows_its_markers_in_turn() {
    let (mut u, r) = setup(&["stencil"]);
    let places = [Rect::new(10.0, 10.0, 12.0, 12.0), Rect::new(30.0, 4.0, 34.0, 8.0)];
    let i = with_problems(&[(ProblemKind::NarrowPaper, 2, &places), (ProblemKind::SharpCorner, 1, &[Rect::new(1.0, 1.0, 1.0, 1.0)])]);
    cache_stencil_info(&u.ctx, &u.app.session, r, i);
    let focus = |u: &Ui| super::problem_focus(&u.ctx).map(|f| (f.node, f.kind, f.index));
    assert_eq!(focus(&u), None);
    let steps = undo_steps(&u);
    let revision = u.app.session.ds().unwrap().revision;
    for want in [0, 1, 0] {
        to_bottom(&mut u);
        let at = text_center(&flat(&u.frame_output(vec![]).shapes), "2 narrow paper bridges");
        click(&mut u, at);
        assert_eq!(focus(&u), Some((r, ProblemKind::NarrowPaper, want)));
        // The view centres the place, zoomed in so a tiny one is legible.
        let v = &u.app.session.ds().unwrap().view;
        let c = places[want].center();
        assert!((v.cx - c.x).abs() < 1e-6 && (v.cy - c.y).abs() < 1e-6, "{want}: ({}, {})", v.cx, v.cy);
        assert!(places[want].width().max(places[want].height()) * v.zoom >= 47.9, "{want}: zoom {}", v.zoom);
        to_bottom(&mut u);
        assert!(drawn(&mut u).iter().any(|x| *x == format!("{} of 2", want + 1)));
    }
    // UI state only: nothing in the document changed.
    assert_eq!(undo_steps(&u), steps);
    assert_eq!(u.app.session.ds().unwrap().revision, revision);
    // Another kind starts at its first.
    to_bottom(&mut u);
    let at = text_center(&flat(&u.frame_output(vec![]).shapes), "1 sharp corner");
    click(&mut u, at);
    assert_eq!(focus(&u), Some((r, ProblemKind::SharpCorner, 0)));
    to_bottom(&mut u);
    assert!(!drawn(&mut u).iter().any(|x| x == "2 of 2" || x == "1 of 2"));
}

#[test]
fn a_place_too_big_for_the_view_is_shown_zoomed_out() {
    let (mut u, r) = setup(&["stencil"]);
    let big = Rect::new(0.0, 0.0, 40_000.0, 100.0);
    cache_stencil_info(&u.ctx, &u.app.session, r, with_problems(&[(ProblemKind::SmallHole, 1, &[big])]));
    to_bottom(&mut u);
    let at = text_center(&flat(&u.frame_output(vec![]).shapes), "1 small hole");
    click(&mut u, at);
    let v = &u.app.session.ds().unwrap().view;
    assert!(big.width() * v.zoom <= v.vw, "fits: zoom {} in {}", v.zoom, v.vw);
    assert!((v.cx - 20_000.0).abs() < 1e-6);
}

/// Pick `option` from the dropdown of modifier row `index` showing `current`, with the field
/// `near` (a number of that row) brought into view first.
fn pick(u: &mut Ui, near: &'static str, index: usize, current: &str, option: &str) {
    centred(u, Id::unique(("mod", near, index)).with("lead"));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let menu = click(u, text_center(&shapes, current));
    click(u, text_center(&menu, option));
}

/// Whether the AccessKit node labelled `label` is disabled, and its description.
fn menu_node(u: &mut Ui, label: &str) -> (bool, Option<String>) {
    let out = u.frame_output(vec![]);
    let nodes = out.platform_output.accesskit_update.unwrap().nodes;
    let (_, n) = nodes.iter().find(|(_, n)| n.label() == Some(label)).unwrap_or_else(|| panic!("no {label:?}"));
    (n.is_disabled(), n.description().map(str::to_string))
}

#[test]
fn a_material_preset_fills_the_limits_in_one_step() {
    let (mut u, r) = setup(&["stencil"]);
    open_settings(&mut u, 0);
    let steps = undo_steps(&u);
    pick(&mut u, "marginMm", 0, "Cut from paper", "Cut from film");
    let m = stack(&mut u, r)[0].clone();
    assert_eq!(
        (&m["material"], &m["minCutWidthMm"], &m["minBridgeMm"], &m["tieWidthMm"], &m["minCornerAngle"]),
        (&json!("film"), &json!(0.2), &json!(0.4), &json!(0.6), &json!(20.0))
    );
    assert_eq!(undo_steps(&u), steps + 1, "one undo step");
    pick(&mut u, "marginMm", 0, "Cut from film", "Cut from card");
    assert_eq!(stack(&mut u, r)[0]["minBridgeMm"], json!(1.0));
    // A limit typed off the preset makes the material custom.
    type_into(&mut u, "minCutWidthMm", "0.7");
    assert_eq!(stack(&mut u, r)[0]["material"], json!("custom"));
    centred(&mut u, Id::unique(("mod", "marginMm", 0)).with("lead"));
    assert!(texts(&flat(&u.frame_output(vec![]).shapes)).iter().any(|t| t == "Custom limits"));
}

#[test]
fn a_tile_needs_a_repeat_above() {
    let drawn = |u: &mut Ui| texts(&flat(&u.frame_output(vec![]).shapes));
    // No lattice row: no Repeat choice, and Tile can't be picked (why, as its description).
    let (mut u, r) = setup(&["stencil"]);
    open_settings(&mut u, 0);
    centred(&mut u, Id::unique(("mod", "marginMm", 0)).with("lead"));
    assert!(drawn(&mut u).iter().any(|t| t == "The whole sheet"));
    assert!(!drawn(&mut u).iter().any(|t| t == "Follow the repeat above"), "no repeat to follow");
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let menu = click(&mut u, text_center(&shapes, "The whole sheet"));
    assert_eq!(menu_node(&mut u, "One repeat tile"), (true, Some("A tile needs a Wallpaper or Grid array row above the stencil".into())));
    click(&mut u, text_center(&menu, "One repeat tile"));
    assert_eq!(stack(&mut u, r)[0]["output"], json!("sheet"), "Tile is disabled");
    // Under a grid array: the Repeat choice, and a tile.
    let (mut u, r) = setup(&["gridArray", "stencil"]);
    open_settings(&mut u, 1);
    pick(&mut u, "marginMm", 1, "The whole sheet", "One repeat tile");
    assert_eq!(stack(&mut u, r)[1]["output"], json!("tile"));
    pick(&mut u, "marginMm", 1, "Follow the repeat above", "Ignore the repeat");
    assert_eq!(stack(&mut u, r)[1]["repeat"], json!("off"));
    // The repeat ignored: Tile can't be picked again (Sheet still can).
    centred(&mut u, Id::unique(("mod", "marginMm", 1)).with("lead"));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    click(&mut u, text_center(&shapes, "One repeat tile"));
    assert!(!menu_node(&mut u, "The whole sheet").0, "Sheet still can");
    let (off, why) = menu_node(&mut u, "One repeat tile");
    assert!(off && why.as_deref() == Some("A tile needs the stencil to follow the repeat above"), "{why:?}");
    // A per-copy row breaks the lattice.
    let (mut u, _) = setup(&["gridArray", "randomize", "stencil"]);
    open_settings(&mut u, 2);
    centred(&mut u, Id::unique(("mod", "marginMm", 2)).with("lead"));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    click(&mut u, text_center(&shapes, "The whole sheet"));
    let (off, why) = menu_node(&mut u, "One repeat tile");
    assert!(off && why.is_some_and(|w| w.contains("Randomize")));
}

#[test]
fn limits_and_the_sharpest_corner_set_their_params() {
    let (mut u, r) = setup(&["stencil"]);
    open_settings(&mut u, 0);
    pick(&mut u, "toleranceMm", 0, "Fix what's too fine", "Only flag what's too fine");
    assert_eq!(stack(&mut u, r)[0]["limitMode"], json!("flag"));
    let steps = undo_steps(&u);
    type_into(&mut u, "minCornerAngle", "45");
    assert_eq!(stack(&mut u, r)[0]["minCornerAngle"], json!(45.0));
    assert_eq!(undo_steps(&u), steps + 1);
    type_into(&mut u, "minCornerAngle", "200");
    assert_eq!(stack(&mut u, r)[0]["minCornerAngle"], json!(figcraft_modifiers::MAX_CORNER_ANGLE));
}

#[test]
fn registration_holes_show_their_diameter_while_on() {
    let (mut u, r) = setup(&["stencil"]);
    open_settings(&mut u, 0);
    assert_eq!(stack(&mut u, r)[0]["hoshi"], json!(true));
    assert!(has_field(&mut u, "hoshiDiameterMm", 0));
    type_into(&mut u, "hoshiDiameterMm", "3");
    assert_eq!(stack(&mut u, r)[0]["hoshiDiameterMm"], json!(3.0));
    type_into(&mut u, "hoshiDiameterMm", "50");
    assert_eq!(stack(&mut u, r)[0]["hoshiDiameterMm"], json!(figcraft_modifiers::MAX_HOSHI_DIAMETER_MM));
    centred(&mut u, Id::unique(("mod", "hoshiDiameterMm", 0)).with("lead"));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    click(&mut u, text_center(&shapes, "Registration holes"));
    assert_eq!(stack(&mut u, r)[0]["hoshi"], json!(false));
    assert!(!has_field(&mut u, "hoshiDiameterMm", 0), "off: no diameter");
}
