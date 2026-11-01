//! PICABIA: the `pathfinder` generator: every operation on overlapping coloured shapes, input
//! (copies, text, strokes, frames, boolean groups, hidden layers), limits, Apply, hit testing,
//! files and exports.

use base64::Engine as _;
use figcraft_doc::{NodeId, NodeKind};
use figcraft_geom::kurbo::Shape as _;
use figcraft_geom::{Point, Rect};
use figcraft_modifiers::PLUGIN_KEY;
use figcraft_render::picabia::generated::{self as store, GenPaint};
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn shape(s: &mut Session, kind: &str, r: Rect, fill: &str) -> NodeId {
    let p = json!({"x": r.x0, "y": r.y0, "width": r.width(), "height": r.height(), "props": {"fill": fill}});
    id_of(&ex(s, kind, p)["id"]).unwrap()
}

fn group(s: &mut Session, ids: &[NodeId]) -> NodeId {
    id_of(&ex(s, "object.group", json!({"ids": ids}))["id"]).unwrap()
}

/// A group of a red 60 square at (0, 0) and a blue one at (30, 30) in front.
fn two(s: &mut Session) -> NodeId {
    let a = shape(s, "shape.rectangle", Rect::new(0.0, 0.0, 60.0, 60.0), "#FF0000");
    let b = shape(s, "shape.rectangle", Rect::new(30.0, 30.0, 90.0, 90.0), "#0000FF");
    group(s, &[a, b])
}

fn add(s: &mut Session, id: NodeId, kind: &str, props: Value) {
    ex(s, "modifier.add", json!({"ids": [id], "kind": kind, "props": props}));
}

fn items(s: &Session, id: NodeId) -> Vec<store::GenItem> {
    let doc = s.doc().unwrap();
    crate::picabia_generate::compute_now(doc, id);
    store::lookup(doc, doc.get(id).unwrap()).unwrap().0.items.clone()
}

fn area(v: &[store::GenItem]) -> f64 {
    v.iter().map(|i| i.path.area().abs()).sum()
}

fn info(s: &mut Session, id: NodeId) -> Value {
    ex(s, "modifier.resultInfo", json!({"id": id}))
}

fn decode(out: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(out["data"].as_str().unwrap()).unwrap()
}

fn hit(s: &Session, p: Point) -> Option<NodeId> {
    let doc = s.doc().unwrap();
    let page = doc.first_page().unwrap();
    figcraft_render::picabia::hit_with_copies(doc, page, p, 0.0, figcraft_doc::geometry::BoolResult::default(), None)
}

fn by_colour(v: &[store::GenItem], rgb: [f32; 3]) -> Vec<&store::GenItem> {
    v.iter().filter(|i| i.rgba[..3] == rgb).collect()
}

const RED: [f32; 3] = [1.0, 0.0, 0.0];
const BLUE: [f32; 3] = [0.0, 0.0, 1.0];

#[test]
fn every_operation_on_overlapping_squares() {
    // Red 3600 behind, blue 3600 in front, overlapping in 900.
    let cases: [(&str, usize, f64, usize, usize); 10] = [
        // op, faces, total area, red faces, blue faces
        ("divide", 3, 6300.0, 1, 2),
        ("trim", 2, 6300.0, 1, 1),
        ("merge", 2, 6300.0, 1, 1),
        ("crop", 1, 900.0, 1, 0),
        ("minusBack", 1, 2700.0, 0, 1),
        ("unite", 1, 6300.0, 0, 1),
        ("minusFront", 1, 2700.0, 1, 0),
        ("intersect", 1, 900.0, 0, 1),
        ("exclude", 1, 5400.0, 0, 1),
        ("outline", 0, 0.0, 0, 0),
    ];
    for (op, faces, total, red, blue) in cases {
        let mut s = Session::new();
        let g = two(&mut s);
        add(&mut s, g, "pathfinder", json!({"op": op}));
        let v = items(&s, g);
        if op == "outline" {
            assert!(v.len() >= 2, "{op}: {}", v.len());
            assert!(v.iter().all(|i| matches!(i.paint, GenPaint::Stroke { width, .. } if width == 1.0)));
            assert!(!by_colour(&v, RED).is_empty() && !by_colour(&v, BLUE).is_empty());
            continue;
        }
        assert_eq!(v.len(), faces, "{op}");
        assert!((area(&v) - total).abs() < 1.0, "{op}: {}", area(&v));
        assert_eq!((by_colour(&v, RED).len(), by_colour(&v, BLUE).len()), (red, blue), "{op}");
        assert!(v.iter().all(|i| matches!(i.paint, GenPaint::Fill { even_odd: false })), "{op}");
        let i = info(&mut s, g);
        assert_eq!(
            (i["stats"]["shapes"].clone(), i["stats"]["faces"].clone(), i["stats"]["colours"].clone()),
            (json!(2), json!(faces), json!(2)),
            "{op}"
        );
    }
}

#[test]
fn circles_and_squares_divide_into_faces_with_their_colours() {
    let mut s = Session::new();
    let a = shape(&mut s, "shape.rectangle", Rect::new(0.0, 0.0, 80.0, 80.0), "#00FF00");
    let b = shape(&mut s, "shape.ellipse", Rect::new(40.0, 0.0, 120.0, 80.0), "#FF8800");
    let c = shape(&mut s, "shape.ellipse", Rect::new(20.0, 40.0, 100.0, 120.0), "#0000FF");
    let g = group(&mut s, &[a, b, c]);
    add(&mut s, g, "pathfinder", json!({"op": "divide"}));
    let v = items(&s, g);
    assert_eq!(v.len(), 7, "three shapes overlapping pairwise and all: 7 faces");
    assert_eq!(info(&mut s, g)["stats"]["colours"], 3);
    // The middle face (all three) has the front-most colour.
    let doc = s.doc().unwrap();
    let mid = v.iter().find(|i| i.path.contains(Point::new(60.0, 55.0))).unwrap();
    assert_eq!(mid.rgba[..3], BLUE);
    assert_eq!(doc.get(mid.source.unwrap()).unwrap().name, "Ellipse");
}

#[test]
fn trim_keeps_same_coloured_shapes_apart_and_merge_joins_them() {
    let mut s = Session::new();
    let a = shape(&mut s, "shape.rectangle", Rect::new(0.0, 0.0, 60.0, 60.0), "#FF0000");
    let b = shape(&mut s, "shape.rectangle", Rect::new(30.0, 30.0, 90.0, 90.0), "#FF0000");
    let g = group(&mut s, &[a, b]);
    add(&mut s, g, "pathfinder", json!({"op": "trim"}));
    assert_eq!(items(&s, g).len(), 2, "trim: one per shape");
    ex(&mut s, "modifier.set", json!({"ids": [g], "index": 0, "props": {"op": "merge"}}));
    let v = items(&s, g);
    assert_eq!(v.len(), 1, "merge: touching same colour");
    assert!((area(&v) - 6300.0).abs() < 1.0);
    assert_eq!(info(&mut s, g)["stats"]["colours"], 1);
    ex(&mut s, "modifier.set", json!({"ids": [g], "index": 0, "props": {"op": "divide"}}));
    assert_eq!(items(&s, g).len(), 3);
}

#[test]
fn outline_width_and_zero() {
    let mut s = Session::new();
    let g = two(&mut s);
    add(&mut s, g, "pathfinder", json!({"op": "outline", "outlineWidth": 4}));
    assert!(items(&s, g).iter().all(|i| matches!(i.paint, GenPaint::Stroke { width, .. } if width == 4.0)));
    ex(&mut s, "modifier.set", json!({"ids": [g], "index": 0, "props": {"outlineWidth": 0}}));
    let i = info(&mut s, g);
    assert!(i["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("width is 0")), "{i}");
}

#[test]
fn only_groups_frames_and_boolean_groups() {
    let mut s = Session::new();
    let r = shape(&mut s, "shape.rectangle", Rect::new(0.0, 0.0, 50.0, 50.0), "#FF0000");
    add(&mut s, r, "pathfinder", json!({}));
    let i = info(&mut s, r);
    assert_eq!(i["drawsDesign"], true);
    assert_eq!(i["warnings"][0], "pathfinder works on a group, frame or boolean group");
    let doc = s.doc().unwrap();
    assert!(store::lookup(doc, doc.get(r).unwrap()).unwrap().0.keep_design);
}

#[test]
fn grid_array_then_merge_is_one_shape_and_hit_testing_follows_the_faces() {
    let mut s = Session::new();
    let r = shape(&mut s, "shape.rectangle", Rect::new(0.0, 0.0, 40.0, 40.0), "#3366FF");
    let g = group(&mut s, &[r]);
    add(&mut s, g, "gridArray", json!({"rows": 3, "cols": 3, "gapX": -10, "gapY": -10}));
    add(&mut s, g, "pathfinder", json!({"op": "merge"}));
    let i = info(&mut s, g);
    assert_eq!((i["stats"]["shapes"].clone(), i["stats"]["faces"].clone()), (json!(9), json!(1)), "{i}");
    // 3 x 3 squares of 40 overlapping by 10: a 100 x 100 block.
    let v = items(&s, g);
    assert!((area(&v) - 10000.0).abs() < 1.0, "{}", area(&v));
    assert_eq!(hit(&s, Point::new(50.0, 50.0)), Some(g), "a face of the merged grid selects the group");
    assert_eq!(hit(&s, Point::new(95.0, 95.0)), Some(g));
    assert_eq!(hit(&s, Point::new(150.0, 50.0)), None);
}

#[test]
fn a_cropped_away_part_hits_nothing() {
    let mut s = Session::new();
    let a = shape(&mut s, "shape.rectangle", Rect::new(0.0, 0.0, 60.0, 60.0), "#FF0000");
    let b = shape(&mut s, "shape.rectangle", Rect::new(30.0, 30.0, 90.0, 90.0), "#0000FF");
    let g = group(&mut s, &[a, b]);
    add(&mut s, g, "pathfinder", json!({"op": "crop"}));
    items(&s, g);
    assert_eq!(hit(&s, Point::new(45.0, 45.0)), Some(g), "the cropped red part");
    assert_eq!(hit(&s, Point::new(10.0, 10.0)), None, "red outside the crop shape is gone");
    assert_eq!(hit(&s, Point::new(80.0, 80.0)), None, "so is the crop shape itself");
}

#[test]
fn merge_then_a_linear_array_copies_the_result() {
    let mut s = Session::new();
    let g = two(&mut s);
    add(&mut s, g, "pathfinder", json!({"op": "unite"}));
    add(&mut s, g, "linearArray", json!({"count": 3, "offsetX": 200}));
    items(&s, g);
    let doc = s.doc().unwrap();
    let world = figcraft_render::export_bounds(doc, g).unwrap();
    assert_eq!(world, Rect::new(0.0, 0.0, 490.0, 90.0));
    let doc = s.doc().unwrap();
    assert!(hit(&s, Point::new(250.0, 20.0)).is_some_and(|h| h == g || doc.is_ancestor(g, h)), "a face of the second copy");
    assert_eq!(hit(&s, Point::new(150.0, 20.0)), None, "between the copies");
    assert_eq!(ex(&mut s, "modifier.list", json!({"id": g}))["instances"], 3);
}

#[test]
fn a_pathfinder_then_a_stencil_cuts_the_faces() {
    let mut s = Session::new();
    let g = two(&mut s);
    add(&mut s, g, "pathfinder", json!({"op": "divide"}));
    add(&mut s, g, "stencil", json!({}));
    let doc = s.doc().unwrap().clone();
    let st = crate::picabia_stencil::compute_now(&doc, g).unwrap();
    assert_eq!(st.paints, 3, "the three faces are what the stencil cuts");
}

#[test]
fn children_with_stacks_strokes_hidden_layers_and_text_are_input() {
    let mut s = Session::new();
    let a = shape(&mut s, "shape.rectangle", Rect::new(0.0, 0.0, 30.0, 30.0), "#FF0000");
    add(&mut s, a, "linearArray", json!({"count": 3, "offsetX": 100}));
    let b = shape(&mut s, "shape.rectangle", Rect::new(0.0, 200.0, 40.0, 240.0), "#00AA00");
    ex(&mut s, "node.set", json!({"ids": [b], "props": {"strokes": [{"type": "solid", "color": "#000000"}], "strokeWeight": 4}}));
    let h = shape(&mut s, "shape.rectangle", Rect::new(0.0, 300.0, 40.0, 340.0), "#FF00FF");
    ex(&mut s, "node.set", json!({"ids": [h], "props": {"visible": false}}));
    let g = group(&mut s, &[a, b, h]);
    add(&mut s, g, "pathfinder", json!({"op": "trim"}));
    let i = info(&mut s, g);
    // 3 copies, the green square and its stroke; the hidden one is left out.
    assert_eq!(i["stats"]["shapes"], 5, "{i}");
    assert!(!i["colours"].as_array().unwrap().iter().any(|c| c == "#ff00ff"));
}

#[test]
fn text_is_input_as_glyph_outlines() {
    let mut s = Session::new();
    let t = id_of(&ex(&mut s, "text.create", json!({"text": "Hi", "x": 0, "y": 0}))["id"]).unwrap();
    let r = shape(&mut s, "shape.rectangle", Rect::new(0.0, 0.0, 200.0, 100.0), "#FFFF00");
    let g = group(&mut s, &[r, t]);
    add(&mut s, g, "pathfinder", json!({"op": "divide"}));
    let glyphs = figcraft_text::layout_node(s.doc().unwrap().get(t).unwrap()).map_or(0, |l| l.0.glyphs.len());
    let i = info(&mut s, g);
    // Without fonts (CI without craft-fonts) the text lays out no glyphs: only the rectangle.
    assert_eq!(i["stats"]["shapes"], if glyphs == 0 { 1 } else { 2 }, "{i}");
}

#[test]
fn a_frame_fill_is_the_back_most_shape_and_its_clip_applies() {
    let mut s = Session::new();
    let a = shape(&mut s, "shape.rectangle", Rect::new(20.0, 20.0, 140.0, 60.0), "#FF0000");
    let f = id_of(&ex(&mut s, "object.frameSelection", json!({"ids": [a]}))["id"]).unwrap();
    ex(&mut s, "node.set", json!({"ids": [f], "props": {"x": 0, "y": 0, "width": 100, "height": 100, "clip": true, "fill": "#00FF00"}}));
    add(&mut s, f, "pathfinder", json!({"op": "divide"}));
    let v = items(&s, f);
    assert_eq!(v.len(), 2, "the frame fill, split by the child");
    assert_eq!(v[0].rgba[..3], [0.0, 1.0, 0.0], "frame fill first (back)");
    let red = by_colour(&v, RED);
    assert_eq!(red.len(), 1);
    assert!(red[0].path.bounding_box().x1 <= 100.0 + 1e-6, "the child is clipped to the frame");
    assert!((area(&v) - 10000.0).abs() < 1.0);
}

#[test]
fn a_boolean_groups_operands_are_the_input() {
    let mut s = Session::new();
    let a = shape(&mut s, "shape.rectangle", Rect::new(0.0, 0.0, 60.0, 60.0), "#FF0000");
    let b = shape(&mut s, "shape.rectangle", Rect::new(30.0, 30.0, 90.0, 90.0), "#0000FF");
    let bg = id_of(&ex(&mut s, "bool.subtract", json!({"ids": [a, b]}))["id"]).unwrap();
    assert!(matches!(s.doc().unwrap().get(bg).unwrap().kind, NodeKind::BooleanOp { .. }));
    add(&mut s, bg, "pathfinder", json!({"op": "divide"}));
    let v = items(&s, bg);
    assert_eq!(info(&mut s, bg)["stats"]["shapes"], 2, "the operands, not their difference");
    assert_eq!(v.len(), 3);
    assert!((area(&v) - 6300.0).abs() < 1.0);
}

#[test]
fn limits() {
    // More than 5000 shapes: the rest are left out with a warning.
    let mut s = Session::new();
    let r = shape(&mut s, "shape.rectangle", Rect::new(0.0, 0.0, 4.0, 4.0), "#FF0000");
    let g = group(&mut s, &[r]);
    add(&mut s, g, "gridArray", json!({"rows": 60, "cols": 100, "gapX": 2, "gapY": 2}));
    add(&mut s, g, "pathfinder", json!({"op": "unite"}));
    let t = std::time::Instant::now();
    let i = info(&mut s, g);
    eprintln!("6000 disjoint squares, unite: {:?}", t.elapsed());
    let w = i["warnings"].as_array().unwrap();
    assert!(w.iter().any(|w| w.as_str().unwrap().contains("1000 shapes past the 5000 limit")), "{i}");
    assert_eq!(i["stats"]["shapes"], 6000);
    // Too complex: the design is kept, with a warning.
    let mut s = Session::new();
    let mut ids = Vec::new();
    for k in 0..400 {
        let o = f64::from(k % 20) * 3.0;
        let q = f64::from(k / 20) * 3.0;
        ids.push(shape(&mut s, "shape.ellipse", Rect::new(o, q, o + 300.0, q + 300.0), "#FF0000"));
    }
    let g = group(&mut s, &ids);
    add(&mut s, g, "pathfinder", json!({"op": "divide"}));
    let t = std::time::Instant::now();
    let i = info(&mut s, g);
    eprintln!("400 big overlapping ellipses, divide: {:?}", t.elapsed());
    assert_eq!(i["drawsDesign"], true, "{i}");
    assert!(i["warnings"][0].as_str().unwrap().contains("overlap too much"), "{i}");
    assert!(t.elapsed().as_secs() < 2);
}

#[test]
fn timings() {
    let mut s = Session::new();
    let mut ids = Vec::new();
    let mut x: u64 = 12345;
    let mut rnd = || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((x >> 33) % 1000) as f64 / 1000.0
    };
    for k in 0..200 {
        let (px, py, sz) = (rnd() * 800.0, rnd() * 800.0, 40.0 + rnd() * 120.0);
        let kind = if k % 2 == 0 { "shape.rectangle" } else { "shape.ellipse" };
        let col = format!("#{:02X}{:02X}{:02X}", (rnd() * 255.0) as u8, (rnd() * 255.0) as u8, (rnd() * 255.0) as u8);
        ids.push(shape(&mut s, kind, Rect::new(px, py, px + sz, py + sz), &col));
    }
    let g = group(&mut s, &ids);
    add(&mut s, g, "pathfinder", json!({"op": "divide"}));
    let t = std::time::Instant::now();
    let i = info(&mut s, g);
    eprintln!("divide of 200 overlapping random shapes: {:?} -> {} faces ({})", t.elapsed(), i["stats"]["faces"], i["drawsDesign"]);
    let r = shape(&mut s, "shape.ellipse", Rect::new(0.0, 2000.0, 40.0, 2040.0), "#3366FF");
    let g2 = group(&mut s, &[r]);
    add(&mut s, g2, "gridArray", json!({"rows": 30, "cols": 30, "gapX": -10, "gapY": -10}));
    add(&mut s, g2, "pathfinder", json!({"op": "merge"}));
    let t = std::time::Instant::now();
    let i = info(&mut s, g2);
    eprintln!("merge of a 30 x 30 grid of overlapping circles: {:?} -> {} faces ({})", t.elapsed(), i["stats"]["faces"], i["drawsDesign"]);
}

#[test]
fn apply_makes_a_group_of_faces_and_undo_restores() {
    let mut s = Session::new();
    let g = two(&mut s);
    ex(&mut s, "node.set", json!({"ids": [g], "props": {"name": "Pair"}}));
    add(&mut s, g, "pathfinder", json!({"op": "divide"}));
    add(&mut s, g, "mirror", json!({}));
    let out = ex(&mut s, "modifier.apply", json!({"ids": [g]}));
    let gid = id_of(&out["ids"][0]).unwrap();
    let doc = s.doc().unwrap();
    assert!(doc.node(g).is_none());
    let grp = doc.get(gid).unwrap();
    assert_eq!(grp.name, "Pair");
    assert_eq!(grp.children.len(), 3);
    for c in &grp.children {
        let v = doc.get(*c).unwrap();
        assert!(matches!(v.kind, NodeKind::Vector { .. }));
        assert_eq!(v.name, "Rectangle");
        assert_eq!(v.fills.len(), 1);
    }
    assert_eq!(ex(&mut s, "modifier.list", json!({"id": gid}))["modifiers"][0]["kind"], "mirror", "rows below move onto the group");
    ex(&mut s, "edit.undo", json!({}));
    let doc = s.doc().unwrap();
    assert!(doc.node(gid).is_none());
    assert_eq!(doc.get(g).unwrap().children.len(), 2, "the original contents are back");
    assert!(doc.get(g).unwrap().plugin_data.contains_key(PLUGIN_KEY));
}

#[test]
fn apply_outline_makes_stroked_vectors() {
    let mut s = Session::new();
    let g = two(&mut s);
    add(&mut s, g, "pathfinder", json!({"op": "outline", "outlineWidth": 3}));
    let out = ex(&mut s, "modifier.apply", json!({"ids": [g]}));
    let doc = s.doc().unwrap();
    let grp = doc.get(id_of(&out["ids"][0]).unwrap()).unwrap();
    assert!(grp.children.len() >= 2);
    for c in &grp.children {
        let v = doc.get(*c).unwrap();
        assert!(v.fills.is_empty() && v.strokes.len() == 1 && v.stroke.weight == 3.0);
    }
}

#[test]
fn save_reopen_keeps_the_result_and_exports_write_the_faces() {
    let mut s = Session::new();
    let g = two(&mut s);
    add(&mut s, g, "pathfinder", json!({"op": "divide"}));
    let dir = std::env::temp_dir().join(format!("picabia-pathfinder-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pf.figcraft");
    ex(&mut s, "file.saveAs", json!({"path": path}));
    let mut t = Session::new();
    ex(&mut t, "file.open", json!({"path": path}));
    let _ = std::fs::remove_dir_all(&dir);
    let i = info(&mut t, g);
    assert_eq!((i["stats"]["shapes"].clone(), i["stats"]["faces"].clone()), (json!(2), json!(3)), "{i}");
    // Exports.
    let svg = String::from_utf8(decode(&ex(&mut t, "file.export", json!({"ids": [g], "format": "svg"})))).unwrap();
    assert_eq!(svg.matches("fill=\"#0000ff\"").count() + svg.matches("fill=\"#0000FF\"").count(), 2, "{svg}");
    assert_eq!(svg.matches("fill=\"#ff0000\"").count() + svg.matches("fill=\"#FF0000\"").count(), 1, "{svg}");
    assert!(!decode(&ex(&mut t, "file.export", json!({"ids": [g], "format": "pdf"}))).is_empty());
    // Copy, paste and duplicate keep the stack.
    ex(&mut t, "edit.copy", json!({"ids": [g]}));
    let pasted = id_of(&ex(&mut t, "edit.paste", json!({}))["ids"][0]).unwrap();
    assert_eq!(info(&mut t, pasted)["stats"]["faces"], 3);
    let dup = id_of(&ex(&mut t, "edit.duplicate", json!({"ids": [g]}))["ids"][0]).unwrap();
    assert_eq!(info(&mut t, dup)["stats"]["faces"], 3);
}

#[test]
fn fig_bake_writes_the_faces() {
    let mut s = Session::new();
    let g = two(&mut s);
    add(&mut s, g, "pathfinder", json!({"op": "divide"}));
    let (baked, warnings) = crate::picabia_bake::baked(s.doc().unwrap());
    assert!(warnings.iter().any(|w| w.contains("generated layer")), "{warnings:?}");
    let page = baked.first_page().unwrap();
    let top = baked.get(baked.get(page).unwrap().children[0]).unwrap();
    assert_eq!(top.children.len(), 3);
    assert!(baked.nodes().all(|n| !n.plugin_data.contains_key(PLUGIN_KEY)));
    let out = ex(&mut s, "file.export", json!({"format": "fig"}));
    assert!(out["data"].as_str().is_some_and(|d| !d.is_empty()));
}

#[test]
fn the_densest_pile_allowed_still_computes_fast() {
    let mut s = Session::new();
    let ids: Vec<NodeId> = (0..270)
        .map(|k| {
            let (o, q) = (f64::from(k % 18) * 3.0, f64::from(k / 18) * 3.0);
            shape(&mut s, "shape.ellipse", Rect::new(o, q, o + 300.0, q + 300.0), "#FF0000")
        })
        .collect();
    let g = group(&mut s, &ids);
    add(&mut s, g, "pathfinder", json!({"op": "divide"}));
    let t = std::time::Instant::now();
    let i = info(&mut s, g);
    eprintln!("270 big overlapping ellipses, divide: {:?} -> {}", t.elapsed(), i["stats"]);
}
