//! Designs built through the command API, as a designer's file would be, converted and printed.
//! No font is loaded here (fonts never live in this repository): text is exercised by the game's
//! own check, against the font it pins.

use figcraft_doc::Document;
use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::model::{Arrange, Constructor, HPin, Sizing, UiNode, VPin};
use crate::{Imports, constructor, design, module};

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn id(v: &Value) -> Value {
    v.get("id").cloned().unwrap_or_else(|| panic!("no id in {v}"))
}

/// A screens page with one screen, `hud-play`: a free frame holding an auto layout toolbar of
/// two pressable buttons pinned to the bottom centre (a component set `Button` of `State=Idle`
/// and `State=Selected`, its `@action` a text property), the second shown only while building.
fn hud() -> Document {
    let mut s = Session::new();
    ex(&mut s, "page.rename", json!({"name": "Screens"}));
    let screens = s.page().expect("page");
    ex(&mut s, "page.add", json!({"name": "Components"}));
    let idle =
        id(&ex(&mut s, "node.create", json!({"type": "component", "name": "State=Idle", "width": 120, "height": 48, "props": {"fill": "#404040"}})));
    let selected = id(&ex(
        &mut s,
        "node.create",
        json!({"type": "component", "name": "State=Selected", "x": 200, "width": 120, "height": 48, "props": {"fill": "#F0C020"}}),
    ));
    let set = ex(&mut s, "component.combineVariants", json!({"ids": [idle, selected]}));
    let set = set.get("id").or_else(|| set.get("set")).cloned().unwrap_or_else(|| panic!("no set in {set}"));
    ex(&mut s, "component.addProperty", json!({"id": set, "name": "@action", "type": "text", "default": "pick-saw"}));
    ex(&mut s, "component.addProperty", json!({"id": set, "name": "@shown", "type": "text", "default": ""}));
    ex(&mut s, "page.select", json!({"id": screens}));
    let root = id(&ex(&mut s, "node.create", json!({"type": "frame", "name": "hud-play", "width": 1920, "height": 1080})));
    let bar = id(&ex(
        &mut s,
        "node.create",
        json!({"type": "frame", "name": "Toolbar", "parent": root, "x": 780, "y": 980, "width": 360, "height": 76,
               "props": {"layout": "horizontal", "gap": 8, "padding": 8, "fill": "none", "constraints": {"horizontal": "center", "vertical": "bottom"}}}),
    ));
    let saw = id(&ex(&mut s, "instance.create", json!({"component": idle, "parent": bar})));
    ex(&mut s, "instance.setProperties", json!({"ids": [saw], "properties": {"@action": "pick-saw"}}));
    let hammer = id(&ex(&mut s, "instance.create", json!({"component": idle, "parent": bar})));
    ex(&mut s, "instance.setProperties", json!({"ids": [hammer], "properties": {"@action": "pick-hammer", "@shown": "while-building"}}));
    s.doc().expect("doc").clone()
}

fn kids(n: &UiNode) -> &[UiNode] {
    match n {
        UiNode::Frame { kids, .. } => kids,
        other => panic!("not a frame: {other:?}"),
    }
}

#[test]
fn names_become_constructors() {
    assert_eq!(constructor("hud-play"), Some(Constructor("HudPlay".into())));
    assert_eq!(constructor("boards_left"), Some(Constructor("BoardsLeft".into())));
    assert_eq!(constructor("pick saw"), Some(Constructor("PickSaw".into())));
    assert_eq!(constructor("tool2"), Some(Constructor("Tool2".into())));
    assert_eq!(constructor(""), None);
    assert_eq!(constructor("2d-view"), None);
    assert_eq!(constructor("boards.left"), None);
    assert_eq!(constructor("pick--saw"), None);
}

#[test]
fn a_screen_converts_with_its_pressables_and_pins() {
    let mut doc = hud();
    let d = design(&mut doc).unwrap_or_else(|rs| panic!("refused: {rs:?}"));
    assert_eq!(d.screens.len(), 1);
    let (which, root) = &d.screens[0];
    assert_eq!(which.0, "HudPlay");
    let bar = &kids(root)[0];
    let UiNode::Frame { bounds, arrange, kids: buttons, .. } = bar else { panic!("toolbar is not a frame") };
    assert_eq!((bounds.x, bounds.y, bounds.w, bounds.h), (780.0, 980.0, 360.0, 76.0));
    assert_eq!((bounds.pin_h, bounds.pin_v), (HPin::Centre, VPin::Bottom));
    assert!(matches!(arrange, Arrange::Stack { horizontal: true, gap, padding: [8.0, 8.0, 8.0, 8.0], .. } if *gap == 8.0));
    assert_eq!(buttons.len(), 2);
    let UiNode::Pressable { action, idle, hovered, selected, .. } = &buttons[0] else { panic!("the saw is not pressable: {:?}", buttons[0]) };
    assert_eq!(action.0, "PickSaw");
    let fill = |n: &UiNode| match n {
        UiNode::Frame { surface, bounds, .. } => (surface.fills.first().map(|p| p.rgb), bounds.size_w),
        other => panic!("a variant is not a frame: {other:?}"),
    };
    assert_eq!(fill(idle), (Some(0x404040), Sizing::Fixed));
    assert_eq!(fill(selected).0, Some(0xF0C020), "the selected variant shows its own fill");
    assert_eq!(fill(hovered).0, Some(0x404040), "a state without a variant shows idle");
    let UiNode::Shown { flag, node } = &buttons[1] else { panic!("the hammer is not flagged: {:?}", buttons[1]) };
    assert_eq!(flag.0, "WhileBuilding");
    assert!(matches!(&**node, UiNode::Pressable { action, .. } if action.0 == "PickHammer"));
}

#[test]
fn what_cannot_be_drawn_is_refused_by_name() {
    let mut s = Session::with_document(hud());
    let root = s.doc().expect("doc").node(s.page().expect("page")).map(|p| p.children[0]).expect("a screen");
    ex(&mut s, "node.create", json!({"type": "ellipse", "name": "Dot", "parent": root, "width": 10, "height": 10}));
    let grid = id(&ex(&mut s, "node.create", json!({"type": "frame", "name": "Grid", "parent": root, "props": {"layout": "grid"}})));
    ex(&mut s, "node.set", json!({"ids": [grid], "props": {"effects": [{"type": "dropShadow"}]}}));
    let mut doc = s.doc().expect("doc").clone();
    let rs = design(&mut doc).expect_err("refused");
    let says = |layer: &str, part: &str| rs.iter().any(|r| r.layer == layer && r.why.contains(part));
    assert!(says("Screens/hud-play/Dot", "ellipses"), "{rs:?}");
    assert!(says("Screens/hud-play/Grid", "grid auto layout"), "{rs:?}");
    assert!(says("Screens/hud-play/Grid", "effects"), "{rs:?}");
}

#[test]
fn a_design_without_screens_is_refused() {
    let mut doc = Session::new().doc().expect("doc").clone();
    let rs = design(&mut doc).expect_err("no screens page");
    assert_eq!(rs.len(), 1);
    assert!(rs[0].why.contains("Screens"));
}

#[test]
fn the_module_matches_every_screen_and_names_every_node() {
    let mut doc = hud();
    let d = design(&mut doc).expect("converts");
    let text = module(&d, "hud.figcraft", &Imports { vocabulary: "../Vocabulary".into() });
    assert!(text.starts_with("import Base\nimport ../Vocabulary/Ui.bend as Ui\n"));
    assert!(text.contains("def screen(which: S.Screen) -> Ui.Node:\n  match which:\n    case S.HudPlay{}:\n"));
    assert!(text.contains("Ui.Pressable{S.PickSaw{}, "));
    assert!(text.contains("Ui.Shown{S.WhileBuilding{}, "));
    assert!(text.contains("Ui.HCentre{}, Ui.Bottom{}"));
    // Every def a node refers to is defined before the screen match.
    let defined: Vec<&str> = text.lines().filter_map(|l| l.strip_prefix("def ")).filter_map(|l| l.split('(').next()).collect();
    for used in text.match_indices("node_").map(|(i, _)| text[i..].split('(').next().unwrap_or("")) {
        assert!(defined.contains(&used), "{used} is used but not defined");
    }
}

#[test]
fn floats_print_as_bend_literals() {
    use crate::emit::f32_lit;
    assert_eq!(f32_lit(12.0), "(12.0 : F32)");
    assert_eq!(f32_lit(109.33333), "(109.3333 : F32)");
    assert_eq!(f32_lit(-0.0), "(0.0 : F32)");
    assert_eq!(f32_lit(f64::NAN), "(0.0 : F32)");
    assert_eq!(f32_lit(1e-7), "(0.0 : F32)");
}
