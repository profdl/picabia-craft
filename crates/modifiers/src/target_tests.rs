//! Tests for `copyToShape`: placement modes on rectangle and ellipse targets, alignment, missing
//! targets, cycle refusal and cache invalidation.

use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_geom::{Affine, Point};
use serde_json::{Value, json};

use crate::*;

/// A page with a 20×10 source at (0, 0) and a target of `kind` at (100, 100), 200×100.
fn doc_with(kind: NodeKind) -> (Document, NodeId, NodeId) {
    let mut d = Document::new("cts");
    let page = d.first_page().unwrap();
    let src = d.create(page, NodeKind::Rectangle, "S", 0.0, 0.0, 20.0, 10.0).unwrap();
    let tgt = d.create(page, kind, "T", 100.0, 100.0, 200.0, 100.0).unwrap();
    (d, src, tgt)
}

fn set_stack(d: &mut Document, id: NodeId, mods: Value) {
    d.get_mut(id).unwrap().plugin_data.insert(PLUGIN_KEY.into(), json!({"v": 1, "stack": mods}).to_string());
}

fn cts(target: NodeId, extra: Value) -> Value {
    let mut m = json!({"id": "c", "kind": "copyToShape", "target": target.to_string()});
    if let (Value::Object(o), Value::Object(e)) = (&mut m, extra) {
        o.extend(e);
    }
    json!([m])
}

fn eval(d: &Document, src: NodeId) -> Output {
    let n = d.node(src).unwrap();
    let stack = parse(n.plugin_data.get(PLUGIN_KEY).unwrap()).unwrap();
    evaluate_with(&stack, node_bounds(n), &DocTargets::new(d, src))
}

/// Where each copy puts the source's pivot (its centre, (10, 5)).
fn pivots(o: &Output) -> Vec<Point> {
    o.instances.iter().map(|i| i.transform * Point::new(10.0, 5.0)).collect()
}

fn near(a: Point, b: Point, tol: f64) -> bool {
    (a - b).hypot() < tol
}

fn ellipse() -> NodeKind {
    NodeKind::Ellipse { start: 0.0, sweep: std::f64::consts::TAU, inner: 0.0 }
}

#[test]
fn points_on_a_rectangle_are_its_four_corners() {
    let (mut d, s, t) = doc_with(NodeKind::Rectangle);
    set_stack(&mut d, s, cts(t, json!({"mode": "points", "align": false})));
    let o = eval(&d, s);
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    let p = pivots(&o);
    assert_eq!(p.len(), 4, "{p:?}");
    for c in [Point::new(100.0, 100.0), Point::new(300.0, 100.0), Point::new(300.0, 200.0), Point::new(100.0, 200.0)] {
        assert!(p.iter().any(|q| near(*q, c, 1e-6)), "{c:?} in {p:?}");
    }
    // Not aligned: pure translations.
    assert!(o.instances.iter().all(|i| {
        let c = i.transform.as_coeffs();
        (c[0] - 1.0).abs() < 1e-9 && c[1].abs() < 1e-9
    }));
}

#[test]
fn points_on_an_ellipse_lie_on_it_and_align_follows_the_tangent() {
    let (mut d, s, t) = doc_with(ellipse());
    set_stack(&mut d, s, cts(t, json!({"mode": "points", "align": true})));
    let o = eval(&d, s);
    let p = pivots(&o);
    assert!(p.len() >= 4, "{p:?}");
    let (cx, cy, rx, ry) = (200.0, 150.0, 100.0, 50.0);
    for q in &p {
        let e = ((q.x - cx) / rx).powi(2) + ((q.y - cy) / ry).powi(2);
        assert!((e - 1.0).abs() < 1e-6, "{q:?} is on the ellipse ({e})");
    }
    // At the rightmost anchor the tangent is vertical: the copy is turned a quarter.
    let right = o.instances.iter().zip(&p).find(|(_, q)| near(**q, Point::new(300.0, 150.0), 1e-6)).map(|(i, _)| i.transform).unwrap();
    let c = right.as_coeffs();
    assert!(c[0].abs() < 1e-6 && (c[1].abs() - 1.0).abs() < 1e-6, "{c:?}");
}

#[test]
fn along_a_closed_rectangle_spaces_copies_by_arc_length() {
    let (mut d, s, t) = doc_with(NodeKind::Rectangle);
    // Perimeter 600: 6 copies every 100 points from the top-left corner, clockwise.
    set_stack(&mut d, s, cts(t, json!({"mode": "alongPath", "count": 6, "align": false})));
    let p = pivots(&eval(&d, s));
    let want = [(100.0, 100.0), (200.0, 100.0), (300.0, 100.0), (300.0, 200.0), (200.0, 200.0), (100.0, 200.0)];
    assert_eq!(p.len(), 6);
    for (q, w) in p.iter().zip(want) {
        assert!(near(*q, Point::new(w.0, w.1), 1e-3), "{q:?} vs {w:?}");
    }
}

#[test]
fn along_an_ellipse_with_align_turns_each_copy() {
    let (mut d, s, t) = doc_with(ellipse());
    set_stack(&mut d, s, cts(t, json!({"mode": "alongPath", "count": 12, "align": true, "scale": 0.5})));
    let o = eval(&d, s);
    assert_eq!(o.instances.len(), 12);
    let (cx, cy, rx, ry) = (200.0, 150.0, 100.0, 50.0);
    for q in pivots(&o) {
        let e = ((q.x - cx) / rx).powi(2) + ((q.y - cy) / ry).powi(2);
        assert!((e - 1.0).abs() < 1e-3, "{q:?}");
    }
    // Every copy is scaled by a half (rotation keeps the determinant).
    assert!(o.instances.iter().all(|i| (i.transform.determinant() - 0.25).abs() < 1e-9));
    // Not all the same turn.
    let turns: Vec<f64> = o.instances.iter().map(|i| i.transform.as_coeffs()[1]).collect();
    assert!(turns.iter().any(|b| b.abs() > 0.1));
}

#[test]
fn along_an_open_line_puts_copies_at_both_ends() {
    let (mut d, s, t) = doc_with(NodeKind::Line);
    set_stack(&mut d, s, cts(t, json!({"mode": "alongPath", "count": 3, "align": false})));
    let p = pivots(&eval(&d, s));
    assert_eq!(p.len(), 3);
    assert!(
        near(p[0], Point::new(100.0, 100.0), 1e-3) && near(p[1], Point::new(200.0, 100.0), 1e-3) && near(p[2], Point::new(300.0, 100.0), 1e-3),
        "{p:?}"
    );
}

#[test]
fn populate_puts_count_copies_inside_the_fill_deterministically() {
    for kind in [NodeKind::Rectangle, ellipse()] {
        let (mut d, s, t) = doc_with(kind);
        set_stack(&mut d, s, cts(t, json!({"mode": "populate", "count": 50, "seed": 7})));
        let o = eval(&d, s);
        let p = pivots(&o);
        assert_eq!(p.len(), 50, "{:?}", o.warnings);
        for q in &p {
            let e = ((q.x - 200.0) / 100.0).powi(2) + ((q.y - 150.0) / 50.0).powi(2);
            assert!(q.x >= 100.0 && q.x <= 300.0 && q.y >= 100.0 && q.y <= 200.0, "{q:?}");
            if matches!(d.node(t).unwrap().kind, NodeKind::Ellipse { .. }) {
                assert!(e <= 1.0 + 1e-9, "{q:?} is inside the ellipse");
            }
        }
        assert_eq!(p, pivots(&eval(&d, s)), "same seed, same points");
        set_stack(&mut d, s, cts(t, json!({"mode": "populate", "count": 50, "seed": 8})));
        assert_ne!(p, pivots(&eval(&d, s)), "another seed, other points");
    }
}

#[test]
fn the_target_is_read_in_the_sources_parent_space() {
    let mut d = Document::new("cts");
    let page = d.first_page().unwrap();
    let f = d.create(page, NodeKind::Frame, "F", 50.0, 50.0, 400.0, 400.0).unwrap();
    let s = d.create(f, NodeKind::Rectangle, "S", 0.0, 0.0, 20.0, 10.0).unwrap();
    let t = d.create(page, NodeKind::Rectangle, "T", 100.0, 100.0, 200.0, 100.0).unwrap();
    set_stack(&mut d, s, cts(t, json!({"mode": "points", "align": false})));
    let p = pivots(&eval(&d, s));
    // The target's page corner (100, 100) is (50, 50) in the frame.
    assert!(p.iter().any(|q| near(*q, Point::new(50.0, 50.0), 1e-6)), "{p:?}");
}

#[test]
fn a_deleted_or_unset_target_places_no_copies_with_a_warning() {
    let (mut d, s, t) = doc_with(NodeKind::Rectangle);
    set_stack(&mut d, s, cts(t, json!({"mode": "points"})));
    d.remove(t).unwrap();
    let o = eval(&d, s);
    assert_eq!(o.instances.len(), 1, "only the original");
    assert!(o.instances[0].transform == Affine::IDENTITY);
    assert!(o.warnings.iter().any(|w| w.message.contains("gone")), "{:?}", o.warnings);
    set_stack(&mut d, s, json!([{"id": "c", "kind": "copyToShape"}]));
    assert!(eval(&d, s).warnings.iter().any(|w| w.message.contains("choose a target")));
    // Without a document: no copies, a warning.
    set_stack(&mut d, s, cts(NodeId(999), json!({})));
    let o = evaluate_node(d.node(s).unwrap()).unwrap();
    assert_eq!(o.instances.len(), 1);
    assert!(!o.warnings.is_empty());
}

#[test]
fn cycles_are_refused_self_ancestor_descendant_and_mutual() {
    let mut d = Document::new("cts");
    let page = d.first_page().unwrap();
    let g = d.create(page, NodeKind::Frame, "G", 0.0, 0.0, 100.0, 100.0).unwrap();
    let a = d.create(g, NodeKind::Rectangle, "A", 0.0, 0.0, 10.0, 10.0).unwrap();
    let b = d.create(page, NodeKind::Rectangle, "B", 200.0, 0.0, 10.0, 10.0).unwrap();
    let c = d.create(page, NodeKind::Rectangle, "C", 400.0, 0.0, 10.0, 10.0).unwrap();
    assert!(check_target(&d, a, a).is_err(), "self");
    assert!(check_target(&d, a, g).is_err(), "ancestor");
    assert!(check_target(&d, g, a).is_err(), "descendant");
    assert!(check_target(&d, a, page).is_err(), "page");
    assert!(check_target(&d, a, NodeId(9999)).is_err(), "missing");
    assert!(check_target(&d, a, b).is_ok());
    // b → a, then a → b is mutual; c → b → a makes a → c a longer loop.
    set_stack(&mut d, b, cts(a, json!({})));
    assert!(check_target(&d, a, b).is_err(), "mutual");
    set_stack(&mut d, c, cts(b, json!({})));
    assert!(check_target(&d, a, c).is_err(), "transitive");
    // Evaluation ignores a cycle in a file, with a warning, and still terminates.
    set_stack(&mut d, a, cts(b, json!({})));
    let o = eval(&d, a);
    assert_eq!(o.instances.len(), 1);
    assert!(o.warnings.iter().any(|w| w.message.contains("copies onto this layer")), "{:?}", o.warnings);
    let list = valid_targets(&d, a);
    assert!(!list.contains(&a) && !list.contains(&g) && !list.contains(&b) && !list.contains(&c), "{list:?}");
    let other = d.create(page, NodeKind::Ellipse { start: 0.0, sweep: std::f64::consts::TAU, inner: 0.0 }, "E", 0.0, 300.0, 10.0, 10.0).unwrap();
    assert!(valid_targets(&d, a).contains(&other));
}

#[test]
fn moving_the_target_re_evaluates_its_copies() {
    let (mut d, s, t) = doc_with(NodeKind::Rectangle);
    set_stack(&mut d, s, cts(t, json!({"mode": "points", "align": false})));
    let before = evaluate_node_in_doc(&d, d.node(s).unwrap()).unwrap();
    d.get_mut(t).unwrap().transform = Affine::translate((150.0, 100.0));
    let after = evaluate_node_in_doc(&d, d.node(s).unwrap()).unwrap();
    assert_ne!(before.instances, after.instances);
    assert!(pivots(&after).iter().any(|q| near(*q, Point::new(150.0, 100.0), 1e-6)));
    // Resizing it too.
    d.get_mut(t).unwrap().width = 50.0;
    let resized = evaluate_node_in_doc(&d, d.node(s).unwrap()).unwrap();
    assert!(pivots(&resized).iter().any(|q| near(*q, Point::new(200.0, 100.0), 1e-6)), "{:?}", pivots(&resized));
}

#[test]
fn copy_to_shape_composes_with_earlier_stages_and_reads_lenient_targets() {
    let (mut d, s, t) = doc_with(NodeKind::Rectangle);
    // A 2-copy linear array, then the 4 corners: 8 instances.
    set_stack(
        &mut d,
        s,
        json!([{"id": "l", "kind": "linearArray", "count": 2, "offsetX": 30}, {"id": "c", "kind": "copyToShape", "target": t.0, "mode": "points"}]),
    );
    assert_eq!(eval(&d, s).instances.len(), 8);
    // A numeric target round-trips as text.
    let st = parse(d.node(s).unwrap().plugin_data.get(PLUGIN_KEY).unwrap()).unwrap();
    assert!(to_json(&st).contains(&format!("\"target\":\"{}\"", t.0)));
    // `target` isn't a bindable field.
    let m = Modifier::new("x", "copyToShape").unwrap();
    assert!(field_type(&m.kind, "target").is_none());
    assert!(field_type(&m.kind, "count").is_some());
}

#[test]
fn bad_copy_to_shape_params_are_refused_by_set_props() {
    let mut m = Modifier::new("x", "copyToShape").unwrap();
    let props = |v: Value| v.as_object().unwrap().clone();
    assert!(m.set_props(&props(json!({"mode": "spiral"}))).is_err());
    assert!(m.set_props(&props(json!({"target": [1]}))).is_err());
    assert!(m.set_props(&props(json!({"target": "42", "mode": "points", "count": 5}))).is_ok());
    let ModifierKind::CopyToShape(p) = &m.kind else { panic!() };
    assert_eq!((p.target.as_str(), p.mode, p.count), ("42", CopyMode::Points, 5));
}
