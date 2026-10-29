//! PICABIA: modifier copies on the canvas and in exports, through the session: clicking or hovering
//! a copy finds its source, a marquee over a copy takes the source, auto layout ignores copies, and
//! PNG/SVG/PDF/Copy as PNG/.fig/Penpot exports include them.

use figcraft_doc::{ExportFormat, NodeId};
use figcraft_modifiers::PLUGIN_KEY;
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;
use crate::tools::{PointerEvent, PointerKind};

/// A 3-copy linear array, 40 apart along x.
pub(crate) const LINEAR3: &str = r#"{"v":1,"stack":[{"id":"m1","enabled":true,"kind":"linearArray","count":3,"offsetX":40,"offsetY":0}]}"#;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn rect(s: &mut Session, x: f64, y: f64, parent: Option<NodeId>) -> NodeId {
    let mut p = json!({"x": x, "y": y, "width": 20, "height": 20, "props": {"fill": "#FF0000"}});
    if let Some(parent) = parent {
        p["parent"] = json!(parent);
    }
    id_of(&ex(s, "shape.rectangle", p)["id"]).unwrap()
}

fn stack(s: &mut Session, id: NodeId, src: &str) {
    s.ds_mut().unwrap().doc_mut().get_mut(id).unwrap().plugin_data.insert(PLUGIN_KEY.into(), src.into());
}

/// A session with a red 20×20 rectangle at (10, 10) arrayed 3 times along x (copies at 50 and 90).
fn arrayed() -> (Session, NodeId) {
    let mut s = Session::new();
    let r = rect(&mut s, 10.0, 10.0, None);
    stack(&mut s, r, LINEAR3);
    ex(&mut s, "select.set", json!({"ids": []}));
    (s, r)
}

fn click(s: &mut Session, x: f64, y: f64) {
    s.pointer(&PointerEvent::at(PointerKind::Down, x, y, 1.0)).unwrap();
    s.pointer(&PointerEvent::at(PointerKind::Up, x, y, 1.0)).unwrap();
}

#[test]
fn clicking_a_copy_selects_the_source_and_hovering_it_highlights_the_source() {
    let (mut s, r) = arrayed();
    s.pointer(&PointerEvent::at(PointerKind::Move, 100.0, 20.0, 1.0)).unwrap();
    assert_eq!(s.ds().unwrap().hover, Some(r));
    click(&mut s, 100.0, 20.0);
    assert_eq!(s.selection(), vec![r]);
    // Between the copies is empty canvas.
    click(&mut s, 40.0, 20.0);
    assert!(s.selection().is_empty());
    // The selection box stays on the source.
    assert_eq!(s.doc().unwrap().world_bounds(r).unwrap(), figcraft_geom::Rect::new(10.0, 10.0, 30.0, 30.0));
}

#[test]
fn a_marquee_over_a_copy_selects_the_source() {
    let (mut s, r) = arrayed();
    let out = ex(&mut s, "select.area", json!({"x": 85, "y": 0, "width": 10, "height": 40}));
    assert_eq!(out["selection"], json!([r]));
    let out = ex(&mut s, "select.area", json!({"x": 35, "y": 0, "width": 10, "height": 40}));
    assert_eq!(out["selection"], json!([]));
}

#[test]
fn a_child_with_a_stack_takes_only_its_own_box_in_auto_layout() {
    let mut s = Session::new();
    let f = id_of(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 200, "height": 100}))["id"]).unwrap();
    ex(&mut s, "node.set", json!({"id": f, "props": {"layout": "horizontal", "padding": 0, "gap": 10, "sizingH": "hug"}}));
    let a = rect(&mut s, 0.0, 0.0, Some(f));
    let b = rect(&mut s, 0.0, 0.0, Some(f));
    let layout = |s: &Session| {
        let d = s.doc().unwrap();
        (d.node(f).unwrap().width, d.world_bounds(a).unwrap(), d.world_bounds(b).unwrap())
    };
    let before = layout(&s);
    stack(&mut s, a, LINEAR3);
    // Any edit re-runs layout: nudge and back.
    ex(&mut s, "node.set", json!({"id": b, "props": {"opacity": 0.5}}));
    ex(&mut s, "node.set", json!({"id": f, "props": {"gap": 10}}));
    assert_eq!(layout(&s), before);
}

#[test]
fn exports_include_the_copies() {
    let (s, r) = arrayed();
    let doc = s.doc().unwrap();
    let png = crate::cmd::file::export_bytes(doc, &[r], ExportFormat::Png, 1.0).unwrap();
    let img = image::load_from_memory(&png).unwrap();
    assert_eq!((img.width(), img.height()), (100, 20));
    let copied = crate::cmd::file::copy_png(doc, &[r]).unwrap();
    assert_eq!((copied.width(), copied.height()), (200, 40), "Copy as PNG (2x) covers every copy");
    let svg = String::from_utf8(crate::cmd::file::export_bytes(doc, &[r], ExportFormat::Svg, 1.0).unwrap()).unwrap();
    assert_eq!(svg.matches("<path").count(), 3, "{svg}");
    assert!(svg.starts_with("<svg width=\"100\""), "Copy as SVG is the same export: {svg}");
    assert!(crate::cmd::file::export_bytes(doc, &[r], ExportFormat::Pdf, 1.0).is_ok());
    let jpg = crate::cmd::file::export_bytes(doc, &[r], ExportFormat::Jpg, 1.0).unwrap();
    assert_eq!(image::load_from_memory(&jpg).unwrap().width(), 100);
}

#[test]
fn fig_and_penpot_exports_bake_the_copies_with_a_warning() {
    let (mut s, _) = arrayed();
    for format in ["fig", "penpot"] {
        let out = ex(&mut s, "file.export", json!({"format": format}));
        let warnings = out["warnings"].as_array().unwrap();
        assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("modifiers")), "{format}: {warnings:?}");
    }
    // The document itself keeps its stack.
    let (s2, r) = arrayed();
    assert!(s2.doc().unwrap().node(r).unwrap().plugin_data.contains_key(PLUGIN_KEY));
}
