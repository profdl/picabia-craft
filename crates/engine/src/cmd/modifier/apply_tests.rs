//! PICABIA: `modifier.apply` tests.

use std::collections::BTreeSet;

use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_geom::Affine;
use figcraft_modifiers::PLUGIN_KEY;
use serde_json::{Value, json};

use crate::params::id_of;
use crate::{EngineError, Session};

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn rect(s: &mut Session, x: f64, parent: Option<NodeId>) -> NodeId {
    let mut p = json!({"x": x, "y": 0, "width": 100, "height": 100});
    if let Some(parent) = parent {
        p["parent"] = json!(parent);
    }
    id_of(&ex(s, "shape.rectangle", p)["id"]).unwrap()
}

fn add(s: &mut Session, id: NodeId, kind: &str, props: Value) {
    ex(s, "modifier.add", json!({"ids": [id], "kind": kind, "props": props}));
}

fn doc(s: &Session) -> &Document {
    s.doc().unwrap()
}

fn children(s: &Session, id: NodeId) -> Vec<NodeId> {
    doc(s).node(id).unwrap().children.clone()
}

/// Every layer of the current page with its node, for an exact before/after comparison.
fn snapshot(s: &Session) -> Vec<figcraft_doc::Node> {
    let d = doc(s);
    d.ids().filter_map(|i| d.node(i).cloned()).collect()
}

/// The page-space transforms the layer `id` draws at: one per modifier copy, or its own.
fn drawn(s: &Session, id: NodeId) -> Vec<Affine> {
    let d = doc(s);
    let n = d.node(id).unwrap();
    let parent = d.world_transform(n.parent.unwrap());
    match figcraft_modifiers::evaluate_node(n) {
        Some(o) => o.transforms().map(|t| parent * t * n.transform).collect(),
        None => vec![parent * n.transform],
    }
}

/// What a baked group draws: each child at each of the group's own copies.
fn drawn_group(s: &Session, g: NodeId) -> Vec<Affine> {
    let d = doc(s);
    let gn = d.node(g).unwrap();
    let parent = d.world_transform(gn.parent.unwrap());
    let copies: Vec<Affine> = figcraft_modifiers::evaluate_node(gn).map(|o| o.transforms().collect()).unwrap_or_else(|| vec![Affine::IDENTITY]);
    let mut out = Vec::new();
    for t in copies {
        for c in &gn.children {
            out.push(parent * t * gn.transform * d.node(*c).unwrap().transform);
        }
    }
    out
}

/// Sorted, rounded coefficients: two sets of transforms compare as sets.
fn key(v: &[Affine]) -> Vec<[i64; 6]> {
    let mut k: Vec<[i64; 6]> = v.iter().map(|a| a.as_coeffs().map(|c| (c * 1000.0).round() as i64)).collect();
    k.sort();
    k
}

#[test]
fn apply_makes_a_group_of_named_copies_with_baked_transforms() {
    let mut s = Session::new();
    let page = s.page().unwrap();
    let below = rect(&mut s, -300.0, None);
    let r = rect(&mut s, 0.0, None);
    let above = rect(&mut s, 500.0, None);
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"name": "Tile"}}));
    add(&mut s, r, "linearArray", json!({"count": 5, "offsetX": 120}));
    let want = drawn(&s, r);
    let before = snapshot(&s);
    ex(&mut s, "select.set", json!({"ids": [r]}));
    let out = ex(&mut s, "modifier.apply", json!({}));
    let g = id_of(&out["ids"][0]).unwrap();
    assert_eq!(out["applied"][0]["copies"], 5);
    assert_eq!(s.undo_labels(1), ["Apply modifiers"]);
    assert_eq!(s.selection(), [g], "the group is selected");
    assert!(!doc(&s).contains(r), "the source is replaced");
    assert_eq!(children(&s, page), [below, g, above], "same parent, same z position");
    let gn = doc(&s).node(g).unwrap();
    assert!(matches!(gn.kind, NodeKind::Group));
    assert_eq!(gn.name, "Tile");
    assert!(!gn.plugin_data.contains_key(PLUGIN_KEY), "nothing left live");
    let names: Vec<String> = gn.children.iter().map(|c| doc(&s).node(*c).unwrap().name.clone()).collect();
    assert_eq!(names, ["Tile 1", "Tile 2", "Tile 3", "Tile 4", "Tile 5"]);
    for c in &gn.children {
        assert!(!doc(&s).node(*c).unwrap().plugin_data.contains_key(PLUGIN_KEY), "copies carry no stack");
    }
    assert_eq!(key(&drawn_group(&s, g)), key(&want), "the copies sit where they were drawn");
    let xs: Vec<f64> = gn.children.iter().map(|c| doc(&s).world_bounds(*c).unwrap().x0).collect();
    assert_eq!(xs, [0.0, 120.0, 240.0, 360.0, 480.0]);
    // Undo restores the document exactly.
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s), before);
    assert_eq!(s.selection(), [r]);
    ex(&mut s, "edit.redo", json!({}));
    assert!(doc(&s).contains(g) && !doc(&s).contains(r));
}

#[test]
fn rotated_and_mirrored_copies_bake_exactly() {
    let mut s = Session::new();
    let r = rect(&mut s, 40.0, None);
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"rotation": 30}}));
    add(&mut s, r, "circularArray", json!({"count": 6, "radius": 150}));
    add(&mut s, r, "mirror", json!({"axis": "horizontal", "offset": 80}));
    add(&mut s, r, "randomize", json!({"seed": 7, "rotation": 40, "scale": 0.3}));
    let want = drawn(&s, r);
    assert_eq!(want.len(), 12);
    let g = id_of(&ex(&mut s, "modifier.apply", json!({"ids": [r]}))["ids"][0]).unwrap();
    assert_eq!(children(&s, g).len(), 12);
    assert_eq!(key(&drawn_group(&s, g)), key(&want));
}

#[test]
fn up_to_bakes_a_prefix_and_the_rest_stays_live_on_the_group() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0, None);
    add(&mut s, r, "linearArray", json!({"count": 3, "offsetX": 150}));
    // Without Align: an aligned ring turns each block about the group's centre (see `apply.rs`).
    add(&mut s, r, "circularArray", json!({"count": 4, "radius": 300, "align": false}));
    add(&mut s, r, "mirror", json!({"axis": "vertical", "offset": 900}));
    let want = drawn(&s, r);
    assert_eq!(want.len(), 24);
    let out = ex(&mut s, "modifier.apply", json!({"ids": [r], "upTo": 0}));
    let g = id_of(&out["ids"][0]).unwrap();
    assert_eq!(children(&s, g).len(), 3, "only the linear array is baked");
    let l = ex(&mut s, "modifier.list", json!({"id": g}));
    let kinds: Vec<&str> = l["modifiers"].as_array().unwrap().iter().map(|m| m["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["circularArray", "mirror"], "the rows after upTo move onto the group");
    assert_eq!(l["instances"], 8);
    assert_eq!(key(&drawn_group(&s, g)), key(&want), "the result looks unchanged");
    // upTo past the end is refused; upTo at the end bakes everything.
    let r2 = rect(&mut s, 2000.0, None);
    add(&mut s, r2, "gridArray", json!({"rows": 2, "cols": 2}));
    let e = s.execute("modifier.apply", &json!({"ids": [r2], "upTo": 1})).unwrap_err().to_string();
    assert!(e.contains("upTo"), "{e}");
    let g2 = id_of(&ex(&mut s, "modifier.apply", json!({"ids": [r2], "upTo": 0}))["ids"][0]).unwrap();
    assert_eq!(children(&s, g2).len(), 4);
    assert!(!doc(&s).node(g2).unwrap().plugin_data.contains_key(PLUGIN_KEY));
}

#[test]
fn nested_layers_get_new_unique_ids_and_keep_their_own_stacks() {
    let mut s = Session::new();
    let f = id_of(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 300, "height": 200}))["id"]).unwrap();
    let a = rect(&mut s, 10.0, Some(f));
    let b = rect(&mut s, 150.0, Some(f));
    add(&mut s, b, "mirror", json!({}));
    add(&mut s, f, "linearArray", json!({"count": 3, "offsetY": 300}));
    let g = id_of(&ex(&mut s, "modifier.apply", json!({"ids": [f]}))["ids"][0]).unwrap();
    let all: Vec<NodeId> = doc(&s).subtree(g);
    assert_eq!(all.len(), 1 + 3 * 3);
    let set: BTreeSet<NodeId> = all.iter().copied().collect();
    assert_eq!(set.len(), all.len(), "ids are unique");
    for old in [f, a, b] {
        assert!(!set.contains(&old) && !doc(&s).contains(old));
    }
    for copy in children(&s, g) {
        let n = doc(&s).node(copy).unwrap();
        assert!(!n.plugin_data.contains_key(PLUGIN_KEY), "the applied stack is gone from the copy");
        let inner = doc(&s).node(*n.children.get(1).unwrap()).unwrap();
        assert!(inner.plugin_data.contains_key(PLUGIN_KEY), "a child's own stack is not this apply's");
        assert_eq!(inner.parent, Some(copy));
    }
}

#[test]
fn a_layer_in_auto_layout_is_replaced_in_place() {
    let mut s = Session::new();
    let f = id_of(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 600, "height": 200}))["id"]).unwrap();
    let a = rect(&mut s, 0.0, Some(f));
    let b = rect(&mut s, 0.0, Some(f));
    let c = rect(&mut s, 0.0, Some(f));
    ex(&mut s, "layout.add", json!({"ids": [f], "direction": "horizontal"}));
    add(&mut s, b, "gridArray", json!({"rows": 2, "cols": 2, "gapX": 110, "gapY": 110}));
    let g = id_of(&ex(&mut s, "modifier.apply", json!({"ids": [b]}))["ids"][0]).unwrap();
    assert_eq!(children(&s, f), [a, g, c]);
    assert_eq!(children(&s, g).len(), 4);
    let gb = doc(&s).world_bounds(g).unwrap();
    assert!((gb.width() - 310.0).abs() < 1e-6 && (gb.height() - 310.0).abs() < 1e-6, "{gb:?}");
    // The auto layout places the group after `a`, and `c` after the group.
    let (ab, cb) = (doc(&s).world_bounds(a).unwrap(), doc(&s).world_bounds(c).unwrap());
    assert!(gb.x0 >= ab.x1 && cb.x0 >= gb.x1, "{ab:?} {gb:?} {cb:?}");
}

#[test]
fn instances_stay_linked_and_main_components_are_refused() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0, None);
    let comp = id_of(&ex(&mut s, "component.create", json!({"ids": [r]}))["id"]).unwrap();
    let inst = id_of(&ex(&mut s, "instance.create", json!({"component": comp, "x": 400, "y": 0}))["id"]).unwrap();
    add(&mut s, inst, "linearArray", json!({"count": 3}));
    let g = id_of(&ex(&mut s, "modifier.apply", json!({"ids": [inst]}))["ids"][0]).unwrap();
    for c in children(&s, g) {
        let linked = matches!(doc(&s).node(c).unwrap().kind, NodeKind::Instance { component, .. } if component == comp);
        assert!(linked, "copies of an instance are instances of the same component");
    }
    add(&mut s, comp, "linearArray", json!({"count": 3}));
    let e = s.execute("modifier.apply", &json!({"ids": [comp]})).unwrap_err().to_string();
    assert!(e.contains("main component"), "{e}");
    assert!(doc(&s).contains(comp));
}

#[test]
fn nothing_to_apply_is_refused_cleanly() {
    let mut s = Session::new();
    let plain = rect(&mut s, 0.0, None);
    let off = rect(&mut s, 200.0, None);
    add(&mut s, off, "linearArray", json!({"count": 4}));
    ex(&mut s, "modifier.toggle", json!({"ids": [off], "index": 0}));
    let one = rect(&mut s, 400.0, None);
    add(&mut s, one, "linearArray", json!({"count": 1}));
    let labels = s.undo_labels(10).len();
    for id in [plain, off, one] {
        match s.execute("modifier.apply", &json!({"ids": [id]})) {
            Err(EngineError::Disabled(..)) => {}
            other => panic!("{id}: {other:?}"),
        }
        assert!(doc(&s).contains(id));
    }
    assert_eq!(s.undo_labels(10).len(), labels, "no undo step");
    // Mixed: the layers with copies are applied, the others left alone.
    let live = rect(&mut s, 600.0, None);
    add(&mut s, live, "linearArray", json!({"count": 2}));
    let out = ex(&mut s, "modifier.apply", json!({"ids": [plain, live]}));
    assert_eq!(out["ids"].as_array().unwrap().len(), 1);
    assert!(doc(&s).contains(plain) && !doc(&s).contains(live));
}

#[test]
fn apply_refuses_more_layers_than_the_cap_before_copying() {
    let mut s = Session::new();
    let f = id_of(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 100, "height": 100}))["id"]).unwrap();
    for i in 0..10 {
        rect(&mut s, f64::from(i), Some(f));
    }
    add(&mut s, f, "gridArray", json!({"rows": 100, "cols": 100}));
    let nodes = doc(&s).ids().count();
    let e = s.execute("modifier.apply", &json!({"ids": [f]})).unwrap_err().to_string();
    assert!(e.contains(&super::MAX_APPLY_LAYERS.to_string()), "{e}");
    assert_eq!(doc(&s).ids().count(), nodes);
}

#[test]
fn a_damaged_stack_is_refused() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0, None);
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"pluginData": {PLUGIN_KEY: "{broken"}}}));
    assert!(s.execute("modifier.apply", &json!({"ids": [r]})).is_err());
    assert!(doc(&s).contains(r));
}

#[test]
fn bindings_leave_the_copies_and_follow_the_rows_that_stay_live() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0, None);
    add(&mut s, r, "linearArray", json!({"count": 3}));
    add(&mut s, r, "gridArray", json!({"rows": 2, "cols": 2}));
    let v = figcraft_doc::VariableId::default();
    {
        let n = s.doc_mut().unwrap().get_mut(r).unwrap();
        n.bound.insert("picabia.modifiers/m1/count".into(), v.clone());
        n.bound.insert("picabia.modifiers/m2/rows".into(), v.clone());
    }
    let g = id_of(&ex(&mut s, "modifier.apply", json!({"ids": [r], "upTo": 0}))["ids"][0]).unwrap();
    for c in children(&s, g) {
        assert!(doc(&s).node(c).unwrap().bound.is_empty(), "the copies' modifier bindings are gone");
    }
    let keys: Vec<&String> = doc(&s).node(g).unwrap().bound.keys().collect();
    assert_eq!(keys, ["picabia.modifiers/m2/rows"]);
}
