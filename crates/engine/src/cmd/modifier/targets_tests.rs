//! PICABIA: copy-to-shape targets through the commands (validation, warnings, the picker list),
//! target ids following duplicate, copy/paste and component instances, and modifier bindings
//! following `modifier.copy`/`paste`/`duplicate`.

use figcraft_doc::NodeId;
use figcraft_modifiers::{ModifierKind, PLUGIN_KEY, binding_key};
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn err(s: &mut Session, cmd: &str, p: Value) -> String {
    match s.execute(cmd, &p) {
        Err(e) => e.to_string(),
        Ok(v) => panic!("{cmd} {p} ran: {v}"),
    }
}

fn shape(s: &mut Session, cmd: &str, x: f64, y: f64, w: f64, h: f64) -> NodeId {
    id_of(&ex(s, cmd, json!({"x": x, "y": y, "width": w, "height": h}))["id"]).unwrap()
}

/// The target of `id`'s copy-to-shape row (the first one).
fn target_of(s: &Session, id: NodeId) -> String {
    let src = s.doc().unwrap().node(id).unwrap().plugin_data.get(PLUGIN_KEY).cloned().unwrap();
    let st = figcraft_modifiers::parse(&src).unwrap();
    st.modifiers
        .iter()
        .find_map(|m| match &m.kind {
            ModifierKind::CopyToShape(c) => Some(c.target.clone()),
            _ => None,
        })
        .unwrap()
}

/// A 20×20 source and an ellipse target beside it, the source copying 12 times along it.
fn along() -> (Session, NodeId, NodeId) {
    let mut s = Session::new();
    let src = shape(&mut s, "shape.rectangle", 0.0, 0.0, 20.0, 20.0);
    let tgt = shape(&mut s, "shape.ellipse", 100.0, 0.0, 200.0, 100.0);
    let out = ex(
        &mut s,
        "modifier.add",
        json!({"ids": [src], "kind": "copyToShape", "props": {"target": tgt.to_string(), "mode": "alongPath", "count": 12}}),
    );
    assert_eq!(out["modifier"], "m1");
    (s, src, tgt)
}

#[test]
fn add_copy_to_shape_places_copies_on_the_target() {
    let (mut s, src, tgt) = along();
    let l = ex(&mut s, "modifier.list", json!({"id": src}));
    assert_eq!(l["instances"], 12, "{l}");
    assert_eq!(l["modifiers"][0]["target"], tgt.to_string());
    assert_eq!(l["warnings"], json!([]));
    // A numeric target reads too.
    let other = shape(&mut s, "shape.rectangle", 0.0, 300.0, 10.0, 10.0);
    ex(&mut s, "modifier.add", json!({"ids": [other], "kind": "copyToShape", "props": {"target": tgt.0, "mode": "points"}}));
    assert_eq!(target_of(&s, other), tgt.to_string());
}

#[test]
fn bad_targets_are_refused_and_roll_back() {
    let (mut s, src, tgt) = along();
    let frame = shape(&mut s, "frame.create", 0.0, 500.0, 300.0, 300.0);
    let inner = shape(&mut s, "shape.rectangle", 10.0, 510.0, 20.0, 20.0);
    ex(&mut s, "node.reparent", json!({"ids": [inner], "parent": frame}));
    let page = s.ds().unwrap().page;
    let before = |s: &Session, id: NodeId| s.doc().unwrap().node(id).unwrap().plugin_data.get(PLUGIN_KEY).cloned();
    let cts = |t: Value| json!({"kind": "copyToShape", "props": {"target": t}});
    for (id, p, want) in [
        (src, cts(json!(src.to_string())), "onto itself"),
        (inner, cts(json!(frame.to_string())), "contains this layer"),
        (frame, cts(json!(inner.to_string())), "inside this layer"),
        (src, cts(json!("99999")), "gone"),
        (src, cts(json!(page.to_string())), "page"),
        (src, cts(json!("x")), "isn't a layer id"),
        // tgt → src makes a loop with src → tgt.
        (tgt, cts(json!(src.to_string())), "copies onto this layer"),
    ] {
        let was = before(&s, id);
        let mut p = p;
        p["ids"] = json!([id]);
        let e = err(&mut s, "modifier.add", p.clone());
        assert!(e.contains(want), "{p}: {e}");
        assert_eq!(before(&s, id), was, "{p} changed nothing");
    }
    // Setting a bad target on an existing row is refused the same way.
    let e = err(&mut s, "modifier.set", json!({"ids": [src], "modifier": "m1", "props": {"target": src.to_string()}}));
    assert!(e.contains("onto itself"), "{e}");
    // Pasting the row onto its own target would loop: refused.
    ex(&mut s, "modifier.copy", json!({"id": src}));
    let e = err(&mut s, "modifier.paste", json!({"ids": [tgt]}));
    assert!(e.contains("onto itself") || e.contains("copies onto this layer"), "{e}");
    // A transitive loop: c → src, then tgt → c (src → tgt → c → src).
    let c = shape(&mut s, "shape.rectangle", 0.0, 900.0, 10.0, 10.0);
    ex(&mut s, "modifier.add", json!({"ids": [c], "kind": "copyToShape", "props": {"target": src.to_string()}}));
    assert!(
        err(&mut s, "modifier.add", json!({"ids": [tgt], "kind": "copyToShape", "props": {"target": c.to_string()}}))
            .contains("copies onto this layer")
    );
}

#[test]
fn a_deleted_target_leaves_a_warning_and_other_edits_still_work() {
    let (mut s, src, tgt) = along();
    ex(&mut s, "edit.delete", json!({"ids": [tgt]}));
    let l = ex(&mut s, "modifier.list", json!({"id": src}));
    assert_eq!(l["instances"], 1, "{l}");
    assert!(l["warnings"][0]["message"].as_str().unwrap().contains("gone"), "{l}");
    // Unrelated edits on the layer aren't blocked by the dangling target.
    ex(&mut s, "modifier.add", json!({"ids": [src], "kind": "mirror"}));
    ex(&mut s, "modifier.toggle", json!({"ids": [src], "index": 0}));
    let t = ex(&mut s, "modifier.targets", json!({"id": src, "index": 0}));
    assert_eq!((t["valid"].clone(), t["reason"].as_str().is_some_and(|r| r.contains("gone"))), (json!(false), true), "{t}");
    ex(&mut s, "edit.undo", json!({}));
    ex(&mut s, "edit.undo", json!({}));
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(ex(&mut s, "modifier.list", json!({"id": src}))["instances"], 12, "undoing the delete brings the copies back");
}

#[test]
fn modifier_targets_lists_what_the_picker_offers() {
    let (mut s, src, tgt) = along();
    let other = shape(&mut s, "shape.rectangle", 0.0, 300.0, 10.0, 10.0);
    ex(&mut s, "modifier.add", json!({"ids": [other], "kind": "copyToShape", "props": {"target": src.to_string()}}));
    let t = ex(&mut s, "modifier.targets", json!({"id": src, "modifier": "m1"}));
    let ids: Vec<String> = t["targets"].as_array().unwrap().iter().map(|r| r["id"].to_string()).collect();
    assert!(ids.contains(&tgt.0.to_string()), "{t}");
    assert!(!ids.contains(&src.0.to_string()), "not itself: {t}");
    assert!(!ids.contains(&other.0.to_string()), "not a layer copying onto it: {t}");
    assert_eq!((t["target"].clone(), t["valid"].clone()), (json!(tgt.to_string()), json!(true)));
    // The Rust API the panel calls agrees.
    let doc = s.doc().unwrap();
    let choices = super::target_choices(doc, src);
    assert!(choices.iter().any(|(id, name)| *id == tgt && !name.is_empty()));
    assert!(super::check_target(doc, src, other).is_err());
}

#[test]
fn duplicating_source_and_target_together_points_the_copy_at_the_copied_target() {
    let (mut s, src, tgt) = along();
    let out = ex(&mut s, "edit.duplicate", json!({"ids": [src, tgt], "map": true}));
    let map = &out["map"];
    let new_src = id_of(&map[src.to_string()]).unwrap_or_else(|| panic!("{out}"));
    let new_tgt = id_of(&map[tgt.to_string()]).unwrap();
    assert_eq!(target_of(&s, new_src), new_tgt.to_string());
    assert_eq!(target_of(&s, src), tgt.to_string(), "the original is unchanged");
    // The source alone keeps copying onto the original target.
    let alone = id_of(&ex(&mut s, "edit.duplicate", json!({"ids": [src]}))["ids"][0]).unwrap();
    assert_eq!(target_of(&s, alone), tgt.to_string());
}

#[test]
fn copy_paste_of_source_and_target_points_the_paste_at_the_pasted_target() {
    let (mut s, src, tgt) = along();
    ex(&mut s, "edit.copy", json!({"ids": [src, tgt]}));
    let pasted: Vec<NodeId> = ex(&mut s, "edit.paste", json!({}))["ids"].as_array().unwrap().iter().filter_map(id_of).collect();
    assert_eq!(pasted.len(), 2);
    let doc = s.doc().unwrap();
    let (p_src, p_tgt) =
        if doc.node(pasted[0]).unwrap().plugin_data.contains_key(PLUGIN_KEY) { (pasted[0], pasted[1]) } else { (pasted[1], pasted[0]) };
    assert_eq!(target_of(&s, p_src), p_tgt.to_string());
    assert_eq!(ex(&mut s, "modifier.list", json!({"id": p_src}))["instances"], 12);
    // Only the source: it keeps the original target.
    ex(&mut s, "edit.copy", json!({"ids": [src]}));
    let only = id_of(&ex(&mut s, "edit.paste", json!({}))["ids"][0]).unwrap();
    assert_eq!(target_of(&s, only), tgt.to_string());
}

#[test]
fn a_component_instance_copies_onto_its_own_layers() {
    let mut s = Session::new();
    let f = shape(&mut s, "frame.create", 0.0, 0.0, 400.0, 200.0);
    let src = shape(&mut s, "shape.rectangle", 10.0, 10.0, 20.0, 20.0);
    let tgt = shape(&mut s, "shape.ellipse", 100.0, 50.0, 200.0, 100.0);
    ex(&mut s, "node.reparent", json!({"ids": [src, tgt], "parent": f}));
    ex(&mut s, "modifier.add", json!({"ids": [src], "kind": "copyToShape", "props": {"target": tgt.to_string(), "count": 6}}));
    let comp = id_of(&ex(&mut s, "component.create", json!({"ids": [f]}))["id"]).unwrap_or(f);
    let inst = id_of(&ex(&mut s, "instance.create", json!({"component": comp, "x": 0, "y": 500}))["id"]).unwrap();
    let doc = s.doc().unwrap();
    let kids = doc.node(inst).unwrap().children.clone();
    let i_src = *kids.iter().find(|k| doc.node(**k).unwrap().plugin_data.contains_key(PLUGIN_KEY)).unwrap();
    let i_tgt = *kids.iter().find(|k| **k != i_src).unwrap();
    assert_eq!(target_of(&s, i_src), i_tgt.to_string(), "the instance's source copies onto the instance's target");
    assert_eq!(target_of(&s, src), tgt.to_string(), "the main is unchanged");
    let l = ex(&mut s, "modifier.list", json!({"id": i_src}));
    assert_eq!(l["instances"], 6, "{l}");
    // Editing the main's stack re-syncs the instance, still remapped.
    ex(&mut s, "modifier.set", json!({"ids": [src], "modifier": "m1", "props": {"count": 4}}));
    let doc = s.doc().unwrap();
    let i_src = *doc.node(inst).unwrap().children.iter().find(|k| doc.node(**k).unwrap().plugin_data.contains_key(PLUGIN_KEY)).unwrap();
    assert_eq!(target_of(&s, i_src), i_tgt.to_string());
    assert_eq!(ex(&mut s, "modifier.list", json!({"id": i_src}))["instances"], 4);
}

/// A rectangle with a linear array `m1` whose `count` is bound to a number variable (4).
fn bound_array() -> (Session, NodeId, String) {
    let mut s = Session::new();
    let r = shape(&mut s, "shape.rectangle", 0.0, 0.0, 100.0, 100.0);
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "linearArray", "props": {"count": 3}}));
    let c = ex(&mut s, "variable.createCollection", json!({"name": "Theme", "modes": ["Light"]}));
    let coll = c["id"].as_str().unwrap().to_string();
    let v = ex(&mut s, "variable.create", json!({"collection": coll, "name": "copies", "type": "number", "values": {"Light": 4}}));
    let var = v["id"].as_str().unwrap().to_string();
    ex(&mut s, "variable.bind", json!({"ids": [r], "field": binding_key("m1", "count"), "variable": var}));
    (s, r, var)
}

fn bound(s: &Session, id: NodeId) -> Vec<(String, String)> {
    s.doc().unwrap().node(id).unwrap().bound.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

#[test]
fn copy_paste_and_duplicate_carry_bindings_to_the_new_modifier_id() {
    let (mut s, r, var) = bound_array();
    let other = shape(&mut s, "shape.rectangle", 300.0, 0.0, 100.0, 100.0);
    ex(&mut s, "modifier.add", json!({"ids": [other], "kind": "mirror"}));
    ex(&mut s, "modifier.copy", json!({"id": r, "index": 0}));
    ex(&mut s, "modifier.paste", json!({"ids": [other]}));
    // m1 was taken on the other layer: the pasted row is m2, and its binding follows it.
    assert_eq!(bound(&s, other), [(binding_key("m2", "count"), var.clone())]);
    let l = ex(&mut s, "modifier.list", json!({"id": other}));
    assert_eq!(l["modifiers"][1]["count"], 4, "the bound value is in place: {l}");
    assert!(l["modifiers"][1].get("__picabiaBound").is_none(), "the carried bindings aren't stored in the row: {l}");
    // Duplicate a row: the copy (m2, below it) is bound too.
    let d = ex(&mut s, "modifier.duplicate", json!({"ids": [r], "index": 0}));
    assert_eq!(d["modifier"], "m2");
    let keys: Vec<String> = bound(&s, r).into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, [binding_key("m1", "count"), binding_key("m2", "count")]);
    assert_eq!(s.undo_labels(1), ["Duplicate modifier"]);
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(bound(&s, r).len(), 1);
}
