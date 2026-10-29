//! PICABIA: variable bindings on modifier params (`picabia.modifiers/<id>/<field>` keys in
//! `Node.bound`) and per-copy expressions, through FigCraft's own `variable.bind`.

use figcraft_collab::{Mode, Replica, diff};
use figcraft_doc::NodeId;
use figcraft_modifiers::{ModifierKind, PLUGIN_KEY, binding_key};
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn rect(s: &mut Session) -> NodeId {
    id_of(&ex(s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 100, "height": 100}))["id"]).unwrap()
}

fn count(s: &Session, id: NodeId, at: usize) -> u32 {
    let src = s.doc().unwrap().node(id).unwrap().plugin_data.get(PLUGIN_KEY).cloned().unwrap();
    match &figcraft_modifiers::parse(&src).unwrap().modifiers[at].kind {
        ModifierKind::LinearArray(p) => p.count,
        k => panic!("not a linear array: {k:?}"),
    }
}

fn bound(s: &Session, id: NodeId) -> Vec<String> {
    s.doc().unwrap().node(id).unwrap().bound.keys().cloned().collect()
}

/// A rectangle with a linear array `m1` whose `count` is bound to number variable `copies`
/// (Light = 4, Dark = 7) in a two-mode collection.
fn bound_array() -> (Session, NodeId, String, String) {
    let mut s = Session::new();
    let r = rect(&mut s);
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "linearArray", "props": {"count": 3}}));
    let c = ex(&mut s, "variable.createCollection", json!({"name": "Theme", "modes": ["Light", "Dark"]}));
    let coll = c["id"].as_str().unwrap().to_string();
    let v = ex(&mut s, "variable.create", json!({"collection": coll, "name": "copies", "type": "number", "values": {"Light": 4, "Dark": 7}}));
    let var = v["id"].as_str().unwrap().to_string();
    ex(&mut s, "variable.bind", json!({"ids": [r], "field": binding_key("m1", "count"), "variable": var}));
    (s, r, coll, var)
}

#[test]
fn a_number_variable_drives_count_and_modes_switch_it() {
    let (mut s, r, coll, var) = bound_array();
    assert_eq!(bound(&s, r), ["picabia.modifiers/m1/count"]);
    assert_eq!(count(&s, r, 0), 4, "the default mode's value");
    assert_eq!(ex(&mut s, "modifier.list", json!({"id": r}))["instances"], 4);
    ex(&mut s, "variable.setMode", json!({"collection": coll, "mode": "Dark"}));
    assert_eq!(count(&s, r, 0), 7, "the page's mode switched");
    ex(&mut s, "variable.set", json!({"variable": var, "mode": "Dark", "value": 9}));
    assert_eq!(count(&s, r, 0), 9, "a new value follows");
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(count(&s, r, 0), 7);
}

#[test]
fn reordering_keeps_the_binding_by_modifier_id() {
    let (mut s, r, _, _) = bound_array();
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "mirror", "index": 0}));
    assert_eq!(count(&s, r, 1), 4);
    ex(&mut s, "modifier.move", json!({"ids": [r], "from": 1, "to": 0}));
    ex(&mut s, "modifier.set", json!({"ids": [r], "modifier": "m2", "props": {"offset": 5}}));
    assert_eq!(bound(&s, r), ["picabia.modifiers/m1/count"]);
    assert_eq!(count(&s, r, 0), 4);
}

#[test]
fn removing_a_modifier_drops_its_bindings() {
    let (mut s, r, _, _) = bound_array();
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "mirror"}));
    ex(&mut s, "modifier.remove", json!({"ids": [r], "modifier": "m1"}));
    assert!(bound(&s, r).is_empty(), "{:?}", bound(&s, r));
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(bound(&s, r), ["picabia.modifiers/m1/count"]);
    ex(&mut s, "modifier.remove_all", json!({"ids": [r]}));
    assert!(bound(&s, r).is_empty());
}

#[test]
fn an_explicit_edit_of_a_bound_param_detaches_it() {
    let (mut s, r, _, _) = bound_array();
    ex(&mut s, "modifier.set", json!({"ids": [r], "modifier": "m1", "props": {"offsetX": 30}}));
    assert_eq!(bound(&s, r), ["picabia.modifiers/m1/count"], "another param keeps it");
    ex(&mut s, "modifier.set", json!({"ids": [r], "modifier": "m1", "props": {"count": 2}}));
    assert!(bound(&s, r).is_empty(), "as FigCraft's own fields do (#809)");
    assert_eq!(count(&s, r, 0), 2);
}

#[test]
fn binding_checks_the_modifier_and_the_variable_type() {
    let (mut s, r, coll, _) = bound_array();
    let b = ex(&mut s, "variable.create", json!({"collection": coll, "name": "flag", "type": "boolean", "value": true}));
    let flag = b["id"].as_str().unwrap().to_string();
    let e = s.execute("variable.bind", &json!({"ids": [r], "field": binding_key("m1", "count"), "variable": flag}));
    assert!(e.is_err(), "a boolean doesn't drive a count");
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "mirror"}));
    ex(&mut s, "variable.bind", json!({"ids": [r], "field": binding_key("m2", "keepOriginal"), "variable": flag}));
    for field in [binding_key("m9", "count"), binding_key("m1", "nope"), "picabia.modifiers/m1".to_string()] {
        let e = s.execute("variable.bind", &json!({"ids": [r], "field": field, "variable": flag}));
        assert!(e.is_err(), "{field}");
    }
    // Unbinding goes through the same command.
    ex(&mut s, "variable.bind", json!({"ids": [r], "field": binding_key("m1", "count"), "variable": null}));
    assert_eq!(bound(&s, r), ["picabia.modifiers/m2/keepOriginal"]);
}

#[test]
fn the_binding_survives_save_and_reopen() {
    let (mut s, r, coll, _) = bound_array();
    ex(&mut s, "variable.setMode", json!({"collection": coll, "mode": "Dark"}));
    let dir = std::env::temp_dir().join(format!("picabia-bindings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bound.figcraft");
    ex(&mut s, "file.saveAs", json!({"path": path}));
    let mut t = Session::new();
    ex(&mut t, "file.open", json!({"path": path}));
    assert_eq!(bound(&t, r), ["picabia.modifiers/m1/count"]);
    assert_eq!(count(&t, r, 0), 7);
    // The mode was set on the selected rectangle itself.
    ex(&mut t, "variable.setMode", json!({"ids": [r], "collection": coll, "mode": "Light"}));
    assert_eq!(count(&t, r, 0), 4, "still live after reopening");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_binding_syncs_through_collab_changes() {
    let mut s = Session::with_document(crate::collab_tests::base());
    let replica = Replica::new(s.doc().unwrap().clone());
    let r = rect(&mut s);
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "linearArray"}));
    let c = ex(&mut s, "variable.createCollection", json!({"name": "Sizes", "modes": ["A", "B"]}));
    let coll = c["id"].as_str().unwrap().to_string();
    let v = ex(&mut s, "variable.create", json!({"collection": coll, "name": "n", "type": "number", "values": {"A": 5, "B": 8}}));
    let var = v["id"].as_str().unwrap().to_string();
    ex(&mut s, "variable.bind", json!({"ids": [r], "field": binding_key("m1", "count"), "variable": var}));
    let d = diff(&replica, s.doc().unwrap()).unwrap();
    let mut other = replica.clone();
    other.apply(&d.changes, Mode::Strict).unwrap();
    let n = other.document().node(r).unwrap();
    assert_eq!(n.bound.get("picabia.modifiers/m1/count"), Some(&var));
    assert_eq!(n.plugin_data.get(PLUGIN_KEY), s.doc().unwrap().node(r).unwrap().plugin_data.get(PLUGIN_KEY));
    // A peer opening the synced document resolves it in its own session.
    let mut peer = Session::with_document(other.document().clone());
    ex(&mut peer, "variable.setMode", json!({"collection": coll, "mode": "B"}));
    assert_eq!(count(&peer, r, 0), 8);
}

#[test]
fn modifier_set_takes_per_copy_expressions() {
    let mut s = Session::new();
    let r = rect(&mut s);
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "linearArray", "props": {"count": 4}}));
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "transformEach"}));
    ex(&mut s, "modifier.set", json!({"ids": [r], "modifier": "m2", "props": {"rotateExpr": "i * 15"}}));
    let l = ex(&mut s, "modifier.list", json!({"id": r}));
    assert_eq!(l["modifiers"][1]["rotateExpr"], "i * 15");
    assert_eq!(l["instances"], 4);
    for bad in [json!({"rotateExpr": "i *"}), json!({"rotateExpr": [1]}), json!({"rotateExpr": "x".repeat(1000)}), json!({"countExpr": "i"})] {
        assert!(s.execute("modifier.set", &json!({"ids": [r], "modifier": "m2", "props": bad})).is_err(), "{bad}");
    }
    ex(&mut s, "modifier.set", json!({"ids": [r], "modifier": "m2", "props": {"rotateExpr": ""}}));
    assert!(ex(&mut s, "modifier.list", json!({"id": r}))["modifiers"][1].get("rotateExpr").is_none());
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(ex(&mut s, "modifier.list", json!({"id": r}))["modifiers"][1]["rotateExpr"], "i * 15");
}

#[test]
fn a_bound_value_feeds_an_expression() {
    let (mut s, r, coll, _) = bound_array();
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "transformEach"}));
    let v = ex(&mut s, "variable.create", json!({"collection": coll, "name": "step", "type": "number", "values": {"Light": 10, "Dark": 20}}));
    let step = v["id"].as_str().unwrap().to_string();
    ex(&mut s, "variable.bind", json!({"ids": [r], "field": binding_key("m2", "rotate"), "variable": step}));
    ex(&mut s, "modifier.set", json!({"ids": [r], "modifier": "m2", "props": {"rotateExpr": "value * i"}}));
    let src = s.doc().unwrap().node(r).unwrap().plugin_data.get(PLUGIN_KEY).cloned().unwrap();
    let st = figcraft_modifiers::parse(&src).unwrap();
    assert!(matches!(&st.modifiers[1].kind, ModifierKind::TransformEach(p) if p.rotate == 10.0 && p.rotate_expr.as_deref() == Some("value * i")));
    assert_eq!(bound(&s, r).len(), 2, "setting the expression keeps the field's binding");
    ex(&mut s, "variable.setMode", json!({"collection": coll, "mode": "Dark"}));
    let src = s.doc().unwrap().node(r).unwrap().plugin_data.get(PLUGIN_KEY).cloned().unwrap();
    let out = figcraft_modifiers::evaluate(&figcraft_modifiers::parse(&src).unwrap(), figcraft_geom::Rect::new(0.0, 0.0, 100.0, 100.0));
    let c = out.instances[2].transform.as_coeffs();
    assert!((c[1].atan2(c[0]).to_degrees() - 40.0).abs() < 1e-9, "copy 2 of 7 turns 2 × 20°");
}
