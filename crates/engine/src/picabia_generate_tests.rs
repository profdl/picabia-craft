//! PICABIA: the generator framework (`imageTrace`, `pathfinder` rows):
//! commands and `modifier.resultInfo`, drawing in place of the design, copies on both sides of the
//! generator, the stencil reading generated results, the snapshot, files, Apply, exports and the
//! `.fig` bake.

use base64::Engine as _;
use figcraft_doc::{NodeId, NodeKind};
use figcraft_geom::kurbo::Shape as _;
use figcraft_geom::{Point, Rect};
use figcraft_modifiers::PLUGIN_KEY;
use figcraft_render::picabia::generated as store;
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

/// A red 100 × 100 rectangle at (`x`, 0), filled with a red 100 × 100 px image too (what
/// `imageTrace` traces: black in its default black-and-white mode).
fn rect(s: &mut Session, x: f64) -> NodeId {
    let id = id_of(&ex(s, "shape.rectangle", json!({"x": x, "y": 0, "width": 100, "height": 100, "props": {"fill": "#FF0000"}}))["id"]).unwrap();
    red_image_on(s, id);
    id
}

/// Add a red 100 × 100 px image fill to layer `id`.
fn red_image_on(s: &mut Session, id: NodeId) {
    let mut pix = figcraft_render::Pixmap::new(100, 100).unwrap();
    for px in pix.data_mut().chunks_exact_mut(4) {
        px.copy_from_slice(&[255, 0, 0, 255]);
    }
    let png = figcraft_render::encode_png(&pix).unwrap();
    ex(s, "image.place", json!({"target": id, "data": base64::engine::general_purpose::STANDARD.encode(png)}));
}

/// A group of a red and a blue 40 × 40 square (at (0, 0) and (20, 20)).
fn two_squares(s: &mut Session) -> NodeId {
    let a = id_of(&ex(s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 40, "height": 40, "props": {"fill": "#FF0000"}}))["id"]).unwrap();
    let b = id_of(&ex(s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 40, "height": 40, "props": {"fill": "#0000FF"}}))["id"]).unwrap();
    id_of(&ex(s, "object.group", json!({"ids": [a, b]}))["id"]).unwrap()
}

fn add(s: &mut Session, id: NodeId, kind: &str, props: Value) -> String {
    ex(s, "modifier.add", json!({"ids": [id], "kind": kind, "props": props}))["modifier"].as_str().unwrap().to_string()
}

fn data(s: &Session, id: NodeId) -> Option<String> {
    s.doc().unwrap().node(id).unwrap().plugin_data.get(PLUGIN_KEY).cloned()
}

/// The pixel at page point (`x`, `y`) of `ids` rendered over `area` at 1×.
fn pixel(s: &Session, ids: &[NodeId], area: Rect, x: f64, y: f64) -> [u8; 4] {
    let doc = s.doc().unwrap();
    let pix = figcraft_render::render_area(doc, ids, area, 1.0).unwrap();
    let i = ((y - area.y0) as usize * pix.width() as usize + (x - area.x0) as usize) * 4;
    let d = pix.data();
    [d[i], d[i + 1], d[i + 2], d[i + 3]]
}

fn decode(out: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(out["data"].as_str().unwrap()).unwrap()
}

#[test]
fn both_kinds_add_and_report_their_result() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    assert_eq!(ex(&mut s, "modifier.resultInfo", json!({"id": r}))["modifier"], Value::Null, "no generator yet");
    let m = add(&mut s, r, "imageTrace", json!({"preset": "sixColors"}));
    let l = ex(&mut s, "modifier.list", json!({"id": r}));
    assert_eq!(l["modifiers"][0]["label"], "Image trace");
    assert_eq!(l["modifiers"][0]["colors"], 6);
    assert_eq!(l["modifiers"][0]["mode"], "color");
    let info = ex(&mut s, "modifier.resultInfo", json!({"id": r}));
    assert_eq!(info["modifier"], json!(m));
    assert_eq!((info["kind"].clone(), info["fresh"].clone(), info["pending"].clone()), (json!("imageTrace"), json!(true), json!(false)));
    assert_eq!(info["colours"], json!(["#ff0000"]), "six colours: the image's red, {info}");
    assert!(info["stats"]["paths"].is_number());
    assert_eq!(info["id"], json!(r));
    let g = two_squares(&mut s);
    add(&mut s, g, "pathfinder", json!({"op": "merge"}));
    let info = ex(&mut s, "modifier.resultInfo", json!({"id": g}));
    assert_eq!(info["kind"], "pathfinder");
    assert_eq!((info["stats"]["shapes"].clone(), info["stats"]["faces"].clone()), (json!(2), json!(2)));
    assert_eq!(info["colours"].as_array().unwrap().len(), 2);
    // A disabled generator is none.
    ex(&mut s, "modifier.toggle", json!({"ids": [g], "index": 0}));
    assert_eq!(ex(&mut s, "modifier.resultInfo", json!({"id": g}))["modifier"], Value::Null);
    assert!(s.execute("modifier.add", &json!({"ids": [r], "kind": "imageTrace", "props": {"preset": "posterize", "nope": 1}})).is_err());
}

#[test]
fn the_result_draws_in_place_of_the_design() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    let area = Rect::new(0.0, 0.0, 100.0, 100.0);
    assert_eq!(pixel(&s, &[r], area, 50.0, 50.0), [255, 0, 0, 255]);
    add(&mut s, r, "imageTrace", json!({}));
    crate::picabia_jobs::ensure(s.doc().unwrap());
    let doc = s.doc().unwrap();
    assert!(store::is_fresh(doc, doc.get(r).unwrap()));
    assert_eq!(pixel(&s, &[r], area, 50.0, 50.0), [0, 0, 0, 255], "the traced black, not the red design");
    // Hidden: nothing.
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"visible": false}}));
    assert_eq!(pixel(&s, &[r], area, 50.0, 50.0)[3], 0);
}

#[test]
fn rows_below_copy_the_result_and_rows_above_are_its_input() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    add(&mut s, r, "linearArray", json!({"count": 2, "offsetX": 150}));
    add(&mut s, r, "imageTrace", json!({}));
    add(&mut s, r, "linearArray", json!({"count": 2, "offsetX": 0, "offsetY": 150}));
    crate::picabia_jobs::ensure(s.doc().unwrap());
    let doc = s.doc().unwrap();
    let (shape, _) = store::lookup(doc, doc.get(r).unwrap()).unwrap();
    assert_eq!(shape.items.len(), 2, "one stub item per input copy");
    let world = figcraft_render::export_bounds(doc, r).unwrap();
    assert_eq!(world, Rect::new(0.0, 0.0, 250.0, 250.0));
    for (x, y) in [(50.0, 50.0), (200.0, 50.0), (50.0, 200.0), (200.0, 200.0)] {
        assert_eq!(pixel(&s, &[r], world, x, y), [0, 0, 0, 255], "({x}, {y})");
    }
    assert_eq!(pixel(&s, &[r], world, 125.0, 50.0)[3], 0, "between the copies");
    // Clicking any copy of the result hits the layer; between them nothing.
    let page = doc.first_page().unwrap();
    let hit = |p: Point| figcraft_render::picabia::hit_with_copies(doc, page, p, 0.0, figcraft_doc::geometry::BoolResult::default(), None);
    assert_eq!(hit(Point::new(200.0, 200.0)), Some(r));
    assert_eq!(hit(Point::new(125.0, 50.0)), None);
    let boxes = figcraft_render::picabia::copy_boxes(doc, r);
    assert!(boxes.contains(&Rect::new(150.0, 150.0, 250.0, 250.0)), "{boxes:?}");
}

#[test]
fn an_array_above_a_pathfinder_is_folded_into_its_input() {
    let mut s = Session::new();
    let g = two_squares(&mut s);
    add(&mut s, g, "gridArray", json!({"rows": 1, "cols": 3, "gapX": 10}));
    add(&mut s, g, "pathfinder", json!({}));
    let info = ex(&mut s, "modifier.resultInfo", json!({"id": g}));
    assert_eq!(info["stats"]["shapes"], 6, "{info}");
    let l = ex(&mut s, "modifier.list", json!({"id": g}));
    assert_eq!(l["instances"], 1, "one copy of the result");
    let doc = s.doc().unwrap();
    let (shape, _) = store::lookup(doc, doc.get(g).unwrap()).unwrap();
    // Back first: red, blue, then the next copy's red...
    assert_eq!(shape.items[0].rgba, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(shape.items[1].rgba, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(shape.items[0].name, "Rectangle");
    let b = shape.items[4].path.bounding_box();
    assert!((b.x0 - 140.0).abs() < 1e-6, "the third copy, 70 further each: {b:?}");
    // A click on a face keeps the layer inside it (deep select still reaches it); outside the
    // result, the design under the point doesn't count.
    let page = doc.first_page().unwrap();
    let hit = |p: Point, normal| figcraft_render::picabia::hit_with_copies(doc, page, p, 0.0, figcraft_doc::geometry::BoolResult::default(), normal);
    let red = doc.get(g).unwrap().children[0];
    assert_eq!(hit(Point::new(5.0, 5.0), Some(red)), Some(red));
    assert_eq!(hit(Point::new(155.0, 5.0), None), Some(g), "a copy's face hits the group");
    assert_eq!(hit(Point::new(250.0, 5.0), None), None);
}

#[test]
fn a_stencil_below_a_generator_cuts_its_result() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    add(&mut s, r, "imageTrace", json!({}));
    add(&mut s, r, "stencil", json!({}));
    let doc = s.doc().unwrap().clone();
    // The stencil's compute brings the trace up to date first and cuts its grey, not the red.
    let info = crate::picabia_stencil::compute_now(&doc, r).unwrap();
    assert!(store::is_fresh(&doc, doc.get(r).unwrap()));
    assert_eq!(info.colours.len(), 1, "{:?}", info.colours);
    assert_eq!(info.colours[0].hex, "#000000");
    let (_, g) = crate::picabia_stencil::gather(&doc, doc.get(r).unwrap()).unwrap();
    assert_eq!(g.paints.len(), 1);
    assert_eq!(g.sources[0].node, r);
    // The sheet draws, not the trace: the stencil wins.
    crate::picabia_jobs::ensure(&doc);
    assert!(figcraft_render::picabia::stencil::drawn(&doc, doc.get(r).unwrap()).is_some());
    let world = figcraft_render::export_bounds(&doc, r).unwrap();
    assert!(world.width() > 100.0, "the sheet's margin: {world:?}");
}

#[test]
fn a_stencil_over_a_generated_child_reads_its_result() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    add(&mut s, r, "imageTrace", json!({}));
    let g = id_of(&ex(&mut s, "modifier.stencilFromSelection", json!({"ids": [r]}))["id"]).unwrap();
    assert_eq!(g, r, "one layer takes the row itself");
    let mut s = Session::new();
    let a = rect(&mut s, 0.0);
    // A plain red rectangle (the stencil leaves images out).
    let b =
        id_of(&ex(&mut s, "shape.rectangle", json!({"x": 200, "y": 0, "width": 100, "height": 100, "props": {"fill": "#FF0000"}}))["id"]).unwrap();
    add(&mut s, a, "imageTrace", json!({}));
    let g = id_of(&ex(&mut s, "modifier.stencilFromSelection", json!({"ids": [a, b]}))["id"]).unwrap();
    let info = ex(&mut s, "modifier.stencilInfo", json!({"id": g}));
    let hexes: Vec<&str> = info["colours"].as_array().unwrap().iter().map(|c| c["hex"].as_str().unwrap()).collect();
    assert!(hexes.contains(&"#000000") && hexes.contains(&"#ff0000"), "{hexes:?}");
    let doc = s.doc().unwrap();
    assert!(store::is_fresh(doc, doc.get(a).unwrap()), "computed first, in the stencil's job");
}

#[test]
fn a_pathfinder_over_a_traced_child_computes_the_child_first() {
    let mut s = Session::new();
    let g = two_squares(&mut s);
    let red = s.doc().unwrap().get(g).unwrap().children[0];
    red_image_on(&mut s, red);
    add(&mut s, red, "imageTrace", json!({}));
    add(&mut s, g, "pathfinder", json!({}));
    let doc = s.doc().unwrap().clone();
    crate::picabia_generate::compute_now(&doc, g).unwrap();
    assert!(store::is_fresh(&doc, doc.get(red).unwrap()));
    let (shape, _) = store::lookup(&doc, doc.get(g).unwrap()).unwrap();
    assert_eq!(shape.items[0].rgba, [0.0, 0.0, 0.0, 1.0], "the traced child's result, not its red");
    assert_eq!(shape.items[1].rgba, [0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn rows_below_the_generator_dont_recompute_it() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    add(&mut s, r, "imageTrace", json!({}));
    let below = add(&mut s, r, "linearArray", json!({"count": 2, "offsetX": 150}));
    let before = store::inputs(s.doc().unwrap(), r).unwrap();
    crate::picabia_jobs::ensure(s.doc().unwrap());
    ex(&mut s, "modifier.set", json!({"ids": [r], "modifier": below, "props": {"count": 5}}));
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"x": 40, "name": "moved", "opacity": 0.5}}));
    add(&mut s, r, "mirror", json!({}));
    let doc = s.doc().unwrap();
    assert!(store::inputs(doc, r).unwrap() == before, "rows below, place, name and opacity aren't inputs");
    assert!(store::is_fresh(doc, doc.get(r).unwrap()));
    assert!(!crate::picabia_jobs::is_stale(doc, r));
    // The generator's own params and the rows above are.
    ex(&mut s, "modifier.set", json!({"ids": [r], "index": 0, "props": {"threshold": 40}}));
    assert!(store::inputs(s.doc().unwrap(), r).unwrap() != before);
    ex(&mut s, "modifier.set", json!({"ids": [r], "index": 0, "props": {"threshold": 128}}));
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "linearArray", "index": 0}));
    assert!(store::inputs(s.doc().unwrap(), r).unwrap() != before);
    assert!(crate::picabia_jobs::is_stale(s.doc().unwrap(), r));
}

#[test]
fn background_refresh_computes_generated_layers() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    add(&mut s, r, "pathfinder", json!({"op": "unite"}));
    let d = std::sync::Arc::new(s.doc().unwrap().clone());
    crate::picabia_jobs::refresh(&d);
    for _ in 0..400 {
        if store::is_fresh(&d, d.node(r).unwrap()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(store::is_fresh(&d, d.node(r).unwrap()));
}

#[test]
fn the_stack_survives_save_reopen_copy_paste_and_duplicate() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    add(&mut s, r, "linearArray", json!({"count": 2}));
    add(&mut s, r, "imageTrace", json!({"preset": "outline", "ignoreColor": "#ABCDEF"}));
    add(&mut s, r, "pathfinder", json!({"op": "outline", "outlineWidth": 3}));
    let want = data(&s, r).unwrap();
    let dir = std::env::temp_dir().join(format!("picabia-generate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("gen.figcraft");
    ex(&mut s, "file.saveAs", json!({"path": path}));
    let mut t = Session::new();
    ex(&mut t, "file.open", json!({"path": path}));
    assert_eq!(figcraft_modifiers::parse(&data(&t, r).unwrap()).unwrap(), figcraft_modifiers::parse(&want).unwrap());
    let l = ex(&mut t, "modifier.list", json!({"id": r}));
    assert_eq!(l["modifiers"][1]["ignoreColor"], "#abcdef");
    assert_eq!(l["modifiers"][2]["outlineWidth"], 3.0);
    assert!(l["warnings"].as_array().unwrap().iter().any(|w| w["modifier"] == "m3"), "the second generator is ignored: {l}");
    let _ = std::fs::remove_dir_all(&dir);
    ex(&mut s, "edit.copy", json!({"ids": [r]}));
    let pasted = id_of(&ex(&mut s, "edit.paste", json!({}))["ids"][0]).unwrap();
    assert_eq!(data(&s, pasted).as_deref(), Some(want.as_str()));
    let dup = id_of(&ex(&mut s, "edit.duplicate", json!({"ids": [r]}))["ids"][0]).unwrap();
    assert_eq!(data(&s, dup).as_deref(), Some(want.as_str()));
    ex(&mut s, "modifier.copy", json!({"id": r}));
    let other = rect(&mut s, 300.0);
    ex(&mut s, "modifier.paste", json!({"ids": [other]}));
    assert_eq!(data(&s, other).as_deref(), Some(want.as_str()));
}

#[test]
fn apply_replaces_the_layer_by_its_result_in_one_undo_step() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"name": "Photo"}}));
    add(&mut s, r, "linearArray", json!({"count": 2, "offsetX": 150}));
    add(&mut s, r, "imageTrace", json!({}));
    add(&mut s, r, "mirror", json!({}));
    let before = data(&s, r).unwrap();
    let out = ex(&mut s, "modifier.apply", json!({"ids": [r]}));
    assert_eq!(s.undo_labels(1), ["Apply modifiers"]);
    let gid = id_of(&out["ids"][0]).unwrap();
    let doc = s.doc().unwrap();
    assert!(doc.node(r).is_none(), "the layer is replaced");
    let g = doc.get(gid).unwrap();
    assert_eq!(g.name, "Photo (traced)");
    assert!(matches!(g.kind, NodeKind::Group));
    assert_eq!(g.children.len(), 2, "one vector per item");
    let v = doc.get(g.children[1]).unwrap();
    assert!(matches!(v.kind, NodeKind::Vector { .. }), "{:?}", v.kind);
    assert_eq!(v.name, "#000000");
    assert_eq!(doc.world_bounds(v.id).unwrap(), Rect::new(150.0, 0.0, 250.0, 100.0));
    // The rows below moved onto the group, live.
    let l = ex(&mut s, "modifier.list", json!({"id": gid}));
    assert_eq!(l["modifiers"].as_array().unwrap().len(), 1);
    assert_eq!(l["modifiers"][0]["kind"], "mirror");
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(data(&s, r).as_deref(), Some(before.as_str()));
    assert!(s.doc().unwrap().node(gid).is_none());
    // upTo past the generator: the rows below it that the cut covers are applied on the group.
    let out = ex(&mut s, "modifier.apply", json!({"ids": [r], "upTo": 2}));
    let gid = id_of(&out["ids"][0]).unwrap();
    let doc = s.doc().unwrap();
    assert_eq!(doc.get(gid).unwrap().children.len(), 2, "the mirror's two copies of the traced group");
    assert!(!doc.get(gid).unwrap().plugin_data.contains_key(PLUGIN_KEY));
    // upTo above the generator: the plain copies are baked; the generator goes onto the group.
    ex(&mut s, "edit.undo", json!({}));
    let out = ex(&mut s, "modifier.apply", json!({"ids": [r], "upTo": 0}));
    let gid = id_of(&out["ids"][0]).unwrap();
    let l = ex(&mut s, "modifier.list", json!({"id": gid}));
    assert_eq!(l["modifiers"][0]["kind"], "imageTrace");
}

#[test]
fn apply_on_a_pathfinder_names_faces_after_their_sources() {
    let mut s = Session::new();
    let g = two_squares(&mut s);
    ex(&mut s, "node.set", json!({"ids": [g], "props": {"name": "Shapes"}}));
    add(&mut s, g, "pathfinder", json!({}));
    let out = ex(&mut s, "modifier.apply", json!({"ids": [g]}));
    let gid = id_of(&out["ids"][0]).unwrap();
    let doc = s.doc().unwrap();
    let grp = doc.get(gid).unwrap();
    assert_eq!(grp.name, "Shapes");
    let names: Vec<&str> = grp.children.iter().map(|c| doc.get(*c).unwrap().name.as_str()).collect();
    assert_eq!(names, ["Rectangle", "Rectangle"]);
    let fill = &doc.get(grp.children[1]).unwrap().fills[0];
    assert!(matches!(&fill.kind, figcraft_color::PaintKind::Solid { color } if color.b == 1.0 && color.r == 0.0));
}

#[test]
fn svg_and_pdf_exports_write_the_result() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    add(&mut s, r, "imageTrace", json!({}));
    add(&mut s, r, "linearArray", json!({"count": 2, "offsetX": 150}));
    let out = ex(&mut s, "file.export", json!({"ids": [r], "format": "svg"}));
    let svg = String::from_utf8(decode(&out)).unwrap();
    assert_eq!(svg.matches("fill=\"#000000\"").count(), 2, "{svg}");
    assert!(!svg.contains("#ff0000") && !svg.contains("#FF0000"), "not the design: {svg}");
    assert!(svg.contains("width=\"250\""), "{svg}");
    let out = ex(&mut s, "file.export", json!({"ids": [r], "format": "pdf"}));
    assert!(!decode(&out).is_empty());
    let out = ex(&mut s, "file.export", json!({"ids": [r], "format": "png"}));
    let png = image::load_from_memory(&decode(&out)).unwrap().to_rgba8();
    assert_eq!(png.get_pixel(200, 50).0, [0, 0, 0, 255]);
}

#[test]
fn fig_and_penpot_bake_the_result() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    add(&mut s, r, "imageTrace", json!({}));
    add(&mut s, r, "linearArray", json!({"count": 3, "offsetX": 150}));
    let doc = s.doc().unwrap();
    let (baked, warnings) = crate::picabia_bake::baked(doc);
    assert!(warnings.iter().any(|w| w.contains("generated layer")), "{warnings:?}");
    let page = baked.first_page().unwrap();
    let top = baked.get(baked.get(page).unwrap().children[0]).unwrap();
    // The copies' group, holding three copies of the traced group, each one grey vector.
    assert_eq!(top.children.len(), 3, "{:?}", top.name);
    let copy = baked.get(top.children[2]).unwrap();
    assert!(matches!(copy.kind, NodeKind::Group));
    let v = baked.get(copy.children[0]).unwrap();
    assert!(matches!(v.kind, NodeKind::Vector { .. }));
    assert!(baked.nodes().all(|n| !n.plugin_data.contains_key(PLUGIN_KEY)));
    for format in ["fig", "penpot"] {
        let out = ex(&mut s, "file.export", json!({"format": format}));
        let warnings = out["warnings"].as_array().unwrap();
        assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("generated layer")), "{format}: {warnings:?}");
    }
    assert!(s.doc().unwrap().get(r).unwrap().plugin_data.contains_key(PLUGIN_KEY), "the document keeps its stack");
}
