//! PICABIA: the Modifiers submenu (Object menu and context menu), the Actions palette listing the
//! modifier commands, no default shortcuts, the layers badge's state, and the Inspect/a11y lines;
//! a selected stencil's canvas markers (tie bars, islands no tie holds).

use egui::Pos2;
use figcraft_engine::Session;
use serde_json::json;

use crate::layers_drag_tests::Ui;
use crate::live_tests::{flat, has_text, texts};
use crate::present_tests::id;

/// The middle of menu text `s`: left of the right-hand panel, which has a Modifiers section too.
fn text_center(shapes: &[egui::Shape], s: &str) -> Pos2 {
    let (_, p, size) = texts(shapes).into_iter().rfind(|(t, p, _)| t == s && p.x < 1100.0).unwrap_or_else(|| panic!("no menu text {s:?}"));
    p + size / 2.0
}

const COMMANDS: &[&str] = &["modifier.add", "modifier.apply", "modifier.remove_all"];

fn rect(s: &mut Session) -> figcraft_doc::NodeId {
    id(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 100, "height": 100})).unwrap())
}

#[test]
fn the_actions_palette_finds_the_modifier_commands() {
    for cmd in COMMANDS {
        let label = figcraft_engine::find_command(cmd).unwrap().label;
        let groups = crate::menus::command_sections(label);
        let key = format!("command:{cmd}");
        assert!(groups.iter().any(|(_, rows)| rows.iter().any(|r| r.key == key)), "{label} finds {cmd}");
    }
}

#[test]
fn modifier_commands_have_no_default_shortcut() {
    for spec in figcraft_engine::all_commands().iter().filter(|c| c.id.starts_with("modifier.")) {
        assert_eq!(crate::shortcuts::shortcut(spec.id), None, "{}", spec.id);
    }
}

#[test]
fn the_submenu_lists_every_kind_and_enables_by_selection() {
    let mut s = Session::new();
    let r = rect(&mut s);
    s.execute("select.none", &json!({})).unwrap();
    let mut u = Ui::new(s);
    let kinds: Vec<&str> = crate::picabia_ui::menu_kinds().map(|k| k.0).collect();
    let mut want = vec!["linearArray", "circularArray", "gridArray", "mirror", "transformEach", "randomize"];
    if figcraft_modifiers::kind_label("wallpaper").is_some() {
        want.push("wallpaper");
    }
    want.push("stencil");
    assert_eq!(kinds, want);
    let on = |u: &Ui| crate::picabia_ui::items(&u.app).iter().filter(|i| !i.1.is_empty()).map(|i| (i.1, i.3)).collect::<Vec<_>>();
    assert!(on(&u).iter().all(|(_, e)| !e), "nothing selected: all off");
    u.app.session.execute("select.set", &json!({"ids": [r]})).unwrap();
    assert!(on(&u).iter().all(|(id, e)| *e == (*id == "modifier.add")), "a plain layer: Add only");
    u.app.session.execute("modifier.add", &json!({"kind": "mirror"})).unwrap();
    assert!(on(&u).iter().all(|(_, e)| *e), "a layer with a stack: all on");
    // Read-only lines for Inspect and the layer row's accessible description.
    let doc = u.app.session.doc().unwrap();
    let sec = crate::picabia_ui::inspect_section(doc, r).unwrap();
    assert_eq!((sec.title, sec.rows[0].label.as_str(), sec.rows[0].value.as_str()), ("Modifiers", "Mirror", "Vertical axis"));
    let n = doc.node(r).unwrap();
    assert_eq!(crate::picabia_ui::a11y_kind("Rectangle", n), "Rectangle, Mirror Vertical axis");
    assert!(crate::picabia_ui::active(n) && crate::picabia_ui::badge_room(n) > 0.0);
}

#[test]
fn the_context_menu_adds_applies_and_removes_modifiers() {
    let mut s = Session::new();
    let r = rect(&mut s);
    s.execute("select.set", &json!({"ids": [r]})).unwrap();
    let mut u = Ui::new(s);
    u.frame(vec![]);
    let open = |u: &mut Ui| {
        u.app.ui.context_menu = Some((Pos2::new(300.0, 200.0), u.app.session.selection()));
        u.frame(vec![]);
        flat(&u.frame_output(vec![]).shapes)
    };
    let click = |u: &mut Ui, label: &str| {
        let shapes = flat(&u.frame_output(vec![]).shapes);
        let at = text_center(&shapes, label);
        u.button(at, true);
        u.button(at, false);
        flat(&u.frame_output(vec![]).shapes)
    };
    let shapes = open(&mut u);
    let y = |s: &str| text_center(&shapes, s).y;
    assert!(y("Detach instance") < y("Modifiers") && y("Modifiers") < y("Show/Hide"), "after the component actions");
    let shapes = click(&mut u, "Modifiers");
    assert!(has_text(&shapes, "Add modifier") && has_text(&shapes, "Apply modifiers") && has_text(&shapes, "Remove all modifiers"));
    let shapes = click(&mut u, "Add modifier");
    assert!(has_text(&shapes, "Circular array"));
    click(&mut u, "Linear array");
    assert!(u.app.ui.context_menu.is_none(), "the menu closes");
    let kinds = u.app.session.execute("modifier.list", &json!({"id": r})).unwrap();
    assert_eq!(kinds["modifiers"][0]["kind"], "linearArray");
    open(&mut u);
    click(&mut u, "Modifiers");
    click(&mut u, "Apply modifiers");
    let doc = u.app.session.doc().unwrap();
    assert!(!doc.contains(r), "applied: the layer became a group of copies");
    assert_eq!(u.app.session.undo_labels(1), ["Apply modifiers"]);
}

#[test]
fn the_object_menu_has_a_modifiers_submenu() {
    let mut s = Session::new();
    let r = rect(&mut s);
    s.execute("select.set", &json!({"ids": [r]})).unwrap();
    s.execute("modifier.add", &json!({"kind": "gridArray"})).unwrap();
    let mut u = Ui::new(s);
    u.frame(vec![]);
    u.app.ui.main_menu = true;
    crate::menus::open_main_menu(&u.ctx, "Object");
    u.frame(vec![]);
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let y = |s: &str| text_center(&shapes, s).y;
    assert!(y("Suggest auto layout") < y("Modifiers") && y("Modifiers") < y("Create component"), "Object ▸ Modifiers after auto layout");
    let at = text_center(&shapes, "Modifiers");
    u.button(at, true);
    u.button(at, false);
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let at = text_center(&shapes, "Remove all modifiers");
    u.button(at, true);
    u.button(at, false);
    assert!(!u.app.session.doc().unwrap().node(r).unwrap().plugin_data.contains_key(figcraft_modifiers::PLUGIN_KEY));
}

/// A selected 100 × 100 stencil rectangle at (100, 100) whose result (put in the store as the
/// engine would) has a tie bar at (10, 45)-(40, 50) and an untied island at (40, 40)-(60, 60)
/// beside a tied one, in the layer's space.
fn stencil_with_islands(mode: &str) -> (Ui, figcraft_doc::NodeId) {
    use figcraft_render::picabia::stencil as store;
    let mut s = Session::new();
    let r = id(&s.execute("shape.rectangle", &json!({"x": 100, "y": 100, "width": 100, "height": 100})).unwrap());
    s.execute("modifier.add", &json!({"ids": [r], "kind": "stencil", "props": {"tieMode": mode}})).unwrap();
    s.execute("select.set", &json!({"ids": [r]})).unwrap();
    let doc = s.doc().unwrap();
    let mut info = (*figcraft_engine::picabia_stencil::compute_now(doc, r).unwrap()).clone();
    let island = |b, tied| figcraft_engine::picabia_stencil::Island { bounds: b, tied, pattern_held: false, nested_depth: 0 };
    info.islands = vec![island(figcraft_geom::Rect::new(40.0, 40.0, 60.0, 60.0), 0), island(figcraft_geom::Rect::new(70.0, 70.0, 80.0, 80.0), 2)];
    info.ties = figcraft_geom::kurbo::Shape::to_path(&figcraft_geom::Rect::new(10.0, 45.0, 40.0, 50.0), 0.1);
    let (shape, _) = store::lookup(doc, doc.node(r).unwrap()).unwrap();
    let extra: std::sync::Arc<dyn std::any::Any + Send + Sync> = std::sync::Arc::new(info);
    let shape = store::StencilShape { cut: shape.cut.clone(), paper: shape.paper.clone(), sheet: shape.sheet, extra: Some(extra) };
    store::store(r, store::inputs(doc, r).unwrap(), shape);
    let mut u = Ui::new(s);
    u.frame(vec![]);
    (u, r)
}

/// The line segments of `shapes` stroked in `colour` from near `a` to near `b` (or back).
fn segments_in(shapes: &[egui::Shape], colour: egui::Color32, along: impl Fn(Pos2) -> bool) -> usize {
    shapes
        .iter()
        .filter(|s| matches!(s, egui::Shape::LineSegment { points: [p, q], stroke } if stroke.color == colour && along(*p) && along(*q)))
        .count()
}

#[test]
fn a_selected_stencil_marks_its_ties_and_flags_untied_islands() {
    let (mut u, _) = stencil_with_islands("ties");
    let warn = crate::theme::Tokens::get(&u.ctx).warning;
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let at = |x: f64, y: f64| u.app.to_screen(x, y);
    // The untied island's top edge, dashed in the warning colour; the tied one isn't flagged.
    let (a, b) = (at(140.0, 140.0), at(160.0, 140.0));
    let on = |p: Pos2, a: Pos2, b: Pos2| (p.y - a.y).abs() < 0.6 && p.x >= a.x - 0.6 && p.x <= b.x + 0.6;
    assert!(segments_in(&shapes, warn, |p| on(p, a, b)) > 2, "the untied island is outlined, dashed");
    let (c, d) = (at(170.0, 170.0), at(180.0, 170.0));
    assert_eq!(segments_in(&shapes, warn, |p| on(p, c, d)), 0, "a tied island isn't flagged");
    // The tie bar: a thin outline through its corners.
    let corners = [at(110.0, 145.0), at(140.0, 145.0), at(140.0, 150.0), at(110.0, 150.0)];
    let tie = shapes.iter().any(|s| match s {
        egui::Shape::Path(p) => corners.iter().all(|c| p.points.iter().any(|q| q.distance(*c) < 0.6)),
        _ => false,
    });
    assert!(tie, "the tie bar is tinted");
    // Not selected: no markers.
    u.app.run("select.set", json!({"ids": []}));
    let shapes = flat(&u.frame_output(vec![]).shapes);
    assert_eq!(segments_in(&shapes, warn, |p| on(p, a, b)), 0, "only while selected");
}

#[test]
fn islands_held_by_gauze_are_not_flagged() {
    let (mut u, _) = stencil_with_islands("gauze");
    let warn = crate::theme::Tokens::get(&u.ctx).warning;
    let shapes = flat(&u.frame_output(vec![]).shapes);
    let (a, b) = (u.app.to_screen(140.0, 140.0), u.app.to_screen(160.0, 140.0));
    let on = |p: Pos2| (p.y - a.y).abs() < 0.6 && p.x >= a.x - 0.6 && p.x <= b.x + 0.6;
    assert_eq!(segments_in(&shapes, warn, on), 0);
}
