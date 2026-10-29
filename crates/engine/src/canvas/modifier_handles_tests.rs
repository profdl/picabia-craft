//! PICABIA: modifier handles on a selected layer: where they show, which modifier gets them, each
//! drag (one undo step), Esc, Shift and snapping (modelled on `arc_handles_tests.rs`).

use serde_json::{Value, json};

use super::{ModHandle, ModHandles, guides, handles, hit};
use crate::Session;
use crate::params::id_of;
use crate::tools::{PointerEvent, PointerKind};
use figcraft_doc::NodeId;
use figcraft_geom::Point;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

/// A selected 100 × 100 rectangle at the origin (pivot (50, 50)) with modifiers `kinds` added in
/// order, each with its props.
fn layer(s: &mut Session, kinds: &[(&str, Value)]) -> NodeId {
    let id = id_of(&ex(s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 100, "height": 100}))["id"]).unwrap();
    ex(s, "select.set", json!({"ids": [id]}));
    for (k, props) in kinds {
        ex(s, "modifier.add", json!({"kind": k, "props": props}));
    }
    id
}

fn assert_at(p: Option<[f64; 2]>, x: f64, y: f64) {
    let p = p.unwrap_or_else(|| panic!("no handle where ({x}, {y}) was expected"));
    assert!((p[0] - x).abs() < 1e-6 && (p[1] - y).abs() < 1e-6, "{p:?} != ({x}, {y})");
}

fn hs(s: &Session, id: NodeId) -> ModHandles {
    handles(s.doc().unwrap(), &[id]).expect("modifier handles")
}

/// Param `field` of the modifier at `index` on `id`.
fn param(s: &Session, id: NodeId, index: usize, field: &str) -> f64 {
    let n = s.doc().unwrap().node(id).unwrap();
    let stack = figcraft_modifiers::node_stack(n).unwrap().unwrap();
    stack.modifiers[index].kind.params()[field].as_f64().unwrap_or_else(|| panic!("{field} isn't a number"))
}

fn ev(kind: PointerKind, p: (f64, f64), shift: bool) -> PointerEvent {
    let mut e = PointerEvent::at(kind, p.0, p.1, 1.0);
    e.shift = shift;
    e
}

/// Drag with the Move tool from `a` through `via`; the readout seen mid-drag and whether snap
/// lines showed.
fn drag_handle(s: &mut Session, a: (f64, f64), via: &[(f64, f64)], shift: bool) -> (Option<String>, bool) {
    s.pointer(&ev(PointerKind::Down, a, shift)).unwrap();
    let (mut label, mut snapped) = (None, false);
    for p in via {
        s.pointer(&ev(PointerKind::Move, *p, shift)).unwrap();
        label = s.tool.overlay.modifier.as_ref().map(|h| h.label.clone());
        snapped = !s.tool.overlay.guides.is_empty();
    }
    let last = via.last().copied().unwrap_or(a);
    s.pointer(&ev(PointerKind::Up, last, shift)).unwrap();
    assert!(s.tool.overlay.modifier.is_none(), "the readout goes when the drag ends");
    (label, snapped)
}

fn undo_len(s: &Session) -> usize {
    s.ds().unwrap().history.undo.len()
}

#[test]
fn each_kind_shows_its_handles_where_its_guides_are() {
    let mut s = Session::new();
    let id = layer(&mut s, &[("mirror", json!({"axis": "both"}))]);
    let h = hs(&s, id);
    // The knobs sit at the axis lines' ends (1.5 × the layer's size from the pivot, at least 100).
    assert_at(h.at(ModHandle::MirrorVertical), 50.0, -100.0);
    assert_at(h.at(ModHandle::MirrorHorizontal), -100.0, 50.0);

    let mut s = Session::new();
    let id = layer(&mut s, &[("circularArray", json!({"radius": 100}))]);
    let h = hs(&s, id);
    assert_at(h.at(ModHandle::RingCenter), -50.0, 50.0);
    assert_at(h.at(ModHandle::RingRadius), -150.0, 50.0);
    assert_eq!(hit(s.doc().unwrap(), &[id], Point::new(-146.0, 52.0), 1.0), Some((id, "m1".to_string(), ModHandle::RingRadius)));
    assert_eq!(hit(s.doc().unwrap(), &[id], Point::new(-50.0, 45.0), 1.0).map(|h| h.2), Some(ModHandle::RingCenter));
    assert!(hit(s.doc().unwrap(), &[id], Point::new(-100.0, 50.0), 1.0).is_none());
    // Zoomed out, the pick distance is in screen pixels.
    assert!(hit(s.doc().unwrap(), &[id], Point::new(-136.0, 50.0), 0.5).is_some());

    let mut s = Session::new();
    let id = layer(&mut s, &[("linearArray", json!({"count": 3, "offsetX": 100, "offsetY": 0}))]);
    assert_at(hs(&s, id).at(ModHandle::LinearEnd), 250.0, 50.0);
    assert_eq!(hit(s.doc().unwrap(), &[id], Point::new(252.0, 48.0), 1.0).map(|h| h.2), Some(ModHandle::LinearEnd));

    let mut s = Session::new();
    let id = layer(&mut s, &[("gridArray", json!({"rows": 3, "cols": 3, "gapX": 100, "gapY": 100}))]);
    let h = hs(&s, id);
    assert_at(h.at(ModHandle::GridGapX), 500.0, 250.0);
    assert_at(h.at(ModHandle::GridGapY), 250.0, 500.0);
    assert_eq!(guides(s.doc().unwrap(), &[id]).len(), 4, "the lattice outline");
}

#[test]
fn the_last_enabled_modifier_with_handles_gets_them_and_single_rows_or_columns_have_none() {
    let mut s = Session::new();
    let id = layer(&mut s, &[("mirror", json!({})), ("circularArray", json!({})), ("randomize", json!({"position": 5}))]);
    let h = hs(&s, id);
    assert_eq!((h.index, h.modifier.as_str()), (1, "m2"), "randomize has no handles; the ring is the last that does");
    assert!(h.at(ModHandle::RingCenter).is_some() && h.at(ModHandle::MirrorVertical).is_none());
    ex(&mut s, "modifier.toggle", json!({"index": 1, "enabled": false}));
    assert_eq!(hs(&s, id).index, 0, "a disabled ring hands them to the mirror");

    let mut s = Session::new();
    let id = layer(&mut s, &[("linearArray", json!({"count": 1})), ("gridArray", json!({"rows": 1, "cols": 1}))]);
    assert!(handles(s.doc().unwrap(), &[id]).is_none());
    // Nothing for a plain layer, two layers, or a locked one.
    let mut s = Session::new();
    let plain = layer(&mut s, &[]);
    assert!(handles(s.doc().unwrap(), &[plain]).is_none());
    let id = layer(&mut s, &[("mirror", json!({}))]);
    assert!(handles(s.doc().unwrap(), &[id, plain]).is_none());
    ex(&mut s, "node.set", json!({"ids": [id], "props": {"locked": true}}));
    assert!(handles(s.doc().unwrap(), &[id]).is_none());
}

#[test]
fn dragging_the_radius_knob_sets_the_radius_in_one_undo_step() {
    let mut s = Session::new();
    let id = layer(&mut s, &[("mirror", json!({})), ("circularArray", json!({"radius": 100}))]);
    s.pointer(&ev(PointerKind::Hover, (-150.0, 50.0), false)).unwrap();
    assert_eq!(s.tool.overlay.modifier_hover, Some(ModHandle::RingRadius));
    let before = undo_len(&s);
    let (label, _) = drag_handle(&mut s, (-150.0, 50.0), &[(-200.0, 50.0), (-250.0, 60.0)], false);
    // The knob is 2r from the original's place (pivot + centre offset) along the start direction.
    assert!((param(&s, id, 1, "radius") - 150.0).abs() < 1e-9);
    assert_eq!(label.as_deref(), Some("Radius 150"));
    assert_eq!(undo_len(&s), before + 1, "one undo step for the whole drag");
    assert_eq!(s.undo_labels(1), vec!["Set modifier".to_string()]);
    assert_at(hs(&s, id).at(ModHandle::RingRadius), -250.0, 50.0);
    s.undo().unwrap();
    assert!((param(&s, id, 1, "radius") - 100.0).abs() < 1e-9);
}

#[test]
fn centre_offset_mirror_and_grid_drags_write_their_params() {
    let mut s = Session::new();
    let id = layer(&mut s, &[("circularArray", json!({"radius": 100}))]);
    drag_handle(&mut s, (-50.0, 50.0), &[(-20.0, 30.0)], false);
    assert_eq!((param(&s, id, 0, "centerX"), param(&s, id, 0, "centerY")), (30.0, -20.0));
    assert_at(hs(&s, id).at(ModHandle::RingCenter), -20.0, 30.0);

    let mut s = Session::new();
    let id = layer(&mut s, &[("linearArray", json!({"count": 5, "offsetX": 50, "offsetY": 0}))]);
    // Grabbed 2 px off the knob: the knob keeps that distance from the pointer.
    drag_handle(&mut s, (252.0, 50.0), &[(452.0, 250.0)], false);
    assert_eq!((param(&s, id, 0, "offsetX"), param(&s, id, 0, "offsetY")), (100.0, 50.0));
    assert_eq!(param(&s, id, 0, "count"), 5.0, "count stays");

    let mut s = Session::new();
    let id = layer(&mut s, &[("mirror", json!({"axis": "vertical"}))]);
    drag_handle(&mut s, (50.0, -100.0), &[(80.0, -140.0)], false);
    assert_eq!(param(&s, id, 0, "offset"), 30.0);

    let mut s = Session::new();
    let id = layer(&mut s, &[("gridArray", json!({"rows": 2, "cols": 3, "gapX": 100, "gapY": 100}))]);
    // The right side of the lattice moves: 2 steps of (width + gap).
    drag_handle(&mut s, (500.0, 150.0), &[(340.0, 170.0)], false);
    assert_eq!((param(&s, id, 0, "gapX"), param(&s, id, 0, "gapY")), (20.0, 100.0));
}

#[test]
fn escape_cancels_a_drag_back_to_its_start() {
    let mut s = Session::new();
    let id = layer(&mut s, &[("circularArray", json!({"radius": 100}))]);
    let before = undo_len(&s);
    s.pointer(&ev(PointerKind::Down, (-150.0, 50.0), false)).unwrap();
    s.pointer(&ev(PointerKind::Move, (-300.0, 50.0), false)).unwrap();
    assert!((param(&s, id, 0, "radius") - 175.0).abs() < 1e-9, "live while dragging");
    s.escape().unwrap();
    assert_eq!(param(&s, id, 0, "radius"), 100.0);
    assert!(!s.tool.busy() && s.tool.overlay.modifier.is_none());
    assert_eq!(undo_len(&s), before, "no undo step");
    assert_eq!(s.selection(), vec![id], "the layer stays selected");
}

#[test]
fn shift_keeps_the_offset_at_45_degrees_the_centre_on_an_axis_and_grid_gaps_equal() {
    let mut s = Session::new();
    let id = layer(&mut s, &[("linearArray", json!({"count": 3, "offsetX": 100, "offsetY": 0}))]);
    let (label, _) = drag_handle(&mut s, (250.0, 50.0), &[(250.0, 30.0)], true);
    assert_eq!(param(&s, id, 0, "offsetY"), 0.0, "the line stays horizontal");
    assert!((param(&s, id, 0, "offsetX") - (200.0f64.hypot(20.0) / 2.0)).abs() < 1e-9);
    assert!(label.is_some_and(|l| l.starts_with("Offset 100.5, 0")));
    drag_handle(&mut s, (250.0, 50.0), &[(240.0, 230.0)], true);
    let (ox, oy) = (param(&s, id, 0, "offsetX"), param(&s, id, 0, "offsetY"));
    assert!((ox - oy).abs() < 1e-9 && ox > 0.0, "45°: {ox}, {oy}");

    let mut s = Session::new();
    let id = layer(&mut s, &[("circularArray", json!({"radius": 100}))]);
    drag_handle(&mut s, (-50.0, 50.0), &[(-20.0, 58.0)], true);
    assert_eq!((param(&s, id, 0, "centerX"), param(&s, id, 0, "centerY")), (30.0, 0.0));

    let mut s = Session::new();
    let id = layer(&mut s, &[("gridArray", json!({"rows": 3, "cols": 3, "gapX": 100, "gapY": 100}))]);
    drag_handle(&mut s, (500.0, 250.0), &[(380.0, 250.0)], true);
    assert_eq!((param(&s, id, 0, "gapX"), param(&s, id, 0, "gapY")), (40.0, 40.0));
}

#[test]
fn handles_snap_to_a_nearby_layer_edge_with_snap_lines() {
    let mut s = Session::new();
    let id = layer(&mut s, &[("gridArray", json!({"rows": 3, "cols": 3, "gapX": 100, "gapY": 100}))]);
    ex(&mut s, "shape.rectangle", json!({"x": 420, "y": 0, "width": 50, "height": 50}));
    ex(&mut s, "select.set", json!({"ids": [id]}));
    // The lattice's right side lands on the other rectangle's left edge (3 px away).
    let (_, snapped) = drag_handle(&mut s, (500.0, 250.0), &[(417.0, 250.0)], false);
    assert_eq!(param(&s, id, 0, "gapX"), 60.0);
    assert!(snapped, "snap lines show while it snaps");
    // Cmd/Ctrl turns snapping off.
    s.pointer(&{
        let mut e = ev(PointerKind::Down, (420.0, 250.0), false);
        e.cmd = true;
        e
    })
    .unwrap();
    let mut m = ev(PointerKind::Move, (417.0, 250.0), false);
    m.cmd = true;
    s.pointer(&m).unwrap();
    s.pointer(&{
        let mut e = ev(PointerKind::Up, (417.0, 250.0), false);
        e.cmd = true;
        e
    })
    .unwrap();
    assert_eq!(param(&s, id, 0, "gapX"), 58.5);

    // A mirror axis snaps to the layer's own edge.
    let mut s = Session::new();
    let id = layer(&mut s, &[("mirror", json!({}))]);
    drag_handle(&mut s, (50.0, -100.0), &[(97.0, -100.0)], false);
    assert_eq!(param(&s, id, 0, "offset"), 50.0);
}

#[test]
fn handles_follow_a_rotated_parent_and_tool_info_lists_them() {
    let mut s = Session::new();
    let f = id_of(&ex(&mut s, "frame.create", json!({"x": 1000, "y": 0, "width": 400, "height": 400}))["id"]).unwrap();
    let id = id_of(&ex(&mut s, "shape.rectangle", json!({"x": 100, "y": 100, "width": 100, "height": 100, "parent": f}))["id"]).unwrap();
    ex(&mut s, "select.set", json!({"ids": [id]}));
    ex(&mut s, "modifier.add", json!({"kind": "linearArray", "props": {"count": 2, "offsetX": 100, "offsetY": 0}}));
    assert_eq!(s.tool_info()["modifierHandles"]["handles"][0][0], json!("linearEnd"));
    // The knob sits on the last copy's centre, also once the frame turns (copies go in its space).
    for turn in [0.0, 90.0] {
        ex(&mut s, "node.set", json!({"ids": [f], "props": {"rotation": turn}}));
        ex(&mut s, "select.set", json!({"ids": [id]}));
        let doc = s.doc().unwrap().clone();
        let p = hs(&s, id).at(ModHandle::LinearEnd).unwrap();
        let copies = figcraft_render::picabia::page_copies(&doc, id);
        let centre = doc.world_transform(id) * Point::new(50.0, 50.0);
        let last = copies.last().map(|c| *c * centre).unwrap();
        assert!((p[0] - last.x).abs() < 1e-6 && (p[1] - last.y).abs() < 1e-6, "{turn}°: {p:?} vs {last:?}");
    }
}

#[test]
fn drag_math_survives_junk() {
    let mut s = Session::new();
    let id = layer(&mut s, &[("circularArray", json!({"radius": 100})), ("gridArray", json!({}))]);
    for p in [(f64::NAN, 0.0), (f64::INFINITY, 1.0), (1e300, -1e300)] {
        s.pointer(&ev(PointerKind::Down, (500.0, 250.0), false)).unwrap();
        let _ = s.pointer(&ev(PointerKind::Move, p, false));
        let _ = s.pointer(&ev(PointerKind::Up, p, true));
        let gx = param(&s, id, 1, "gapX");
        assert!(gx.is_finite(), "{gx}");
    }
    assert!(hit(s.doc().unwrap(), &[id], Point::new(f64::NAN, 0.0), f64::NAN).is_none());
}

#[test]
fn zoom_to_fit_takes_in_the_copies() {
    let mut s = Session::new();
    layer(&mut s, &[("linearArray", json!({"count": 3, "offsetX": 1000, "offsetY": 0}))]);
    ex(&mut s, "view.zoomFit", json!({}));
    let v = &s.ds().unwrap().view;
    assert!((v.cx - 1050.0).abs() < 1e-6 && (v.cy - 50.0).abs() < 1e-6, "centred on the original and its copies: {}, {}", v.cx, v.cy);
    s.ds_mut().unwrap().zoom_to_fit();
    assert!((s.ds().unwrap().view.cx - 1050.0).abs() < 1e-6);
}

#[test]
fn in_outline_mode_a_copy_is_clicked_on_its_outline() {
    let mut s = Session::new();
    let id = layer(&mut s, &[("linearArray", json!({"count": 2, "offsetX": 300, "offsetY": 0}))]);
    ex(&mut s, "select.set", json!({"ids": []}));
    ex(&mut s, "view.outlines", json!({"on": true}));
    let (doc, page) = (s.doc().unwrap().clone(), s.page().unwrap());
    // The copy's left edge (x 300) is hit; its inside is not (outline mode draws no fills).
    assert_eq!(s.hit(&doc, page, Point::new(300.5, 50.0), 2.0), Some(id));
    assert_eq!(s.hit(&doc, page, Point::new(350.0, 50.0), 2.0), None);
    ex(&mut s, "view.outlines", json!({"on": false}));
    assert_eq!(s.hit(&doc, page, Point::new(350.0, 50.0), 2.0), Some(id));
}
