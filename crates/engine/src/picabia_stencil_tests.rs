//! PICABIA: stencil layers: gather (shapes, strokes, text, clips, masks, nesting, copies),
//! the result store and its invalidation, `modifier.stencilInfo`, `modifier.stencilFromSelection`,
//! the canvas preview, bounds and hit testing.

use figcraft_color::{Color, ColorStop, Gradient, GradientKind, Paint as Fill, PaintKind};
use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_geom::kurbo::Shape as _;
use figcraft_geom::{Point, Rect};
use figcraft_modifiers::PLUGIN_KEY;
use figcraft_render::picabia::stencil as store;
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;
use crate::picabia_stencil::{compute_now, ensure, gather, info};

const BLACK: &str = "000000";
const WHITE: &str = "FFFFFF";

fn solid(hex: &str) -> Fill {
    Fill::solid(Color::from_hex(hex).unwrap())
}

fn stack(rows: Value) -> String {
    json!({"v": 1, "stack": rows}).to_string()
}

fn stencil_row() -> Value {
    json!({"id": "s1", "enabled": true, "kind": "stencil"})
}

/// A page with a 100 × 100 group at (10, 20) holding what `fill` adds; the group ends in a
/// stencil row (default params: 300 mm wide, so 3 mm per unit).
fn doc_with(fill: impl FnOnce(&mut Document, NodeId)) -> (Document, NodeId) {
    let mut d = Document::new("stencil");
    let page = d.first_page().unwrap();
    let g = d.create(page, NodeKind::Group, "G", 10.0, 20.0, 100.0, 100.0).unwrap();
    fill(&mut d, g);
    d.get_mut(g).unwrap().plugin_data.insert(PLUGIN_KEY.into(), stack(json!([stencil_row()])));
    (d, g)
}

fn rect_in(d: &mut Document, parent: NodeId, r: Rect, hex: &str) -> NodeId {
    let id = d.create(parent, NodeKind::Rectangle, "R", r.x0, r.y0, r.width(), r.height()).unwrap();
    d.get_mut(id).unwrap().fills = vec![solid(hex)];
    id
}

fn area(p: &figcraft_geom::kurbo::BezPath) -> f64 {
    p.area().abs()
}

fn paints(d: &Document, g: NodeId) -> crate::picabia_stencil::Gathered {
    gather(d, d.node(g).unwrap()).unwrap().1
}

#[test]
fn a_rectangle_is_one_paint_in_the_layers_space() {
    let (d, g) = doc_with(|d, g| {
        rect_in(d, g, Rect::new(10.0, 10.0, 60.0, 40.0), BLACK);
    });
    let (frame, got) = gather(&d, d.node(g).unwrap()).unwrap();
    assert_eq!(got.paints.len(), 1);
    let p = &got.paints[0];
    assert_eq!(p.colour, [0.0, 0.0, 0.0, 1.0]);
    assert!((area(&p.path) - 1500.0).abs() < 1e-6);
    // The group's own space: the rectangle where it sits in the group, not on the page.
    let b = p.path.bounding_box();
    assert!((b.x0 - 10.0).abs() < 1e-9 && (b.y0 - 10.0).abs() < 1e-9, "{b:?}");
    assert_eq!(frame.layer_bounds, Rect::new(0.0, 0.0, 100.0, 100.0));
    assert!((frame.mm_per_unit - 3.0).abs() < 1e-12);
    // 15 mm margin = 5 units.
    assert_eq!(frame.sheet, Rect::new(-5.0, -5.0, 105.0, 105.0));
}

#[test]
fn not_a_stencil_gathers_nothing() {
    let mut d = Document::new("x");
    let page = d.first_page().unwrap();
    let r = rect_in(&mut d, page, Rect::new(0.0, 0.0, 10.0, 10.0), BLACK);
    assert!(gather(&d, d.node(r).unwrap()).is_none());
    assert!(compute_now(&d, r).is_none());
    d.get_mut(r).unwrap().plugin_data.insert(PLUGIN_KEY.into(), stack(json!([{"id": "m1", "kind": "linearArray", "count": 3}])));
    assert!(gather(&d, d.node(r).unwrap()).is_none(), "a stack without a stencil row");
    let mut off = stencil_row();
    off["enabled"] = json!(false);
    d.get_mut(r).unwrap().plugin_data.insert(PLUGIN_KEY.into(), stack(json!([off])));
    assert!(gather(&d, d.node(r).unwrap()).is_none(), "a disabled stencil row");
}

#[test]
fn strokes_are_outlined_above_their_fill() {
    let (d, g) = doc_with(|d, g| {
        let r = rect_in(d, g, Rect::new(20.0, 20.0, 80.0, 80.0), WHITE);
        let n = d.get_mut(r).unwrap();
        n.strokes = vec![solid(BLACK)];
        n.stroke.weight = 4.0;
        n.stroke.align = figcraft_doc::StrokeAlign::Center;
    });
    let got = paints(&d, g);
    assert_eq!(got.paints.len(), 2, "fill, then stroke");
    assert_eq!(got.paints[0].colour[0], 1.0);
    assert_eq!(got.paints[1].colour[0], 0.0);
    assert!(got.paints[1].order > got.paints[0].order);
    // A 4-wide band round a 60 × 60 square: 64² − 56².
    let band = area(&figcraft_pathops::simplify(&got.paints[1].path).unwrap());
    assert!((band - (64.0 * 64.0 - 56.0 * 56.0)).abs() < 1.0, "{band}");
}

#[test]
fn text_becomes_glyph_outlines_in_its_fill() {
    let mut s = Session::new();
    let t = id_of(&s.execute("text.create", &json!({"text": "Hi", "x": 0, "y": 0})).unwrap()["id"]).unwrap();
    s.execute("node.set", &json!({"ids": [t], "props": {"fills": [{"type": "solid", "color": "#FF0000"}]}})).ok();
    s.execute("modifier.add", &json!({"ids": [t], "kind": "stencil"})).unwrap();
    let doc = s.doc().unwrap();
    let n = doc.node(t).unwrap();
    // Without fonts (CI without craft-fonts) the text lays out no glyphs: nothing to cut.
    let glyphs = figcraft_text::layout_node(n).map_or(0, |l| l.0.glyphs.len());
    let got = paints(doc, t);
    assert_eq!(got.paints.is_empty(), glyphs == 0, "glyphs: {glyphs}");
    for p in &got.paints {
        assert!(area(&p.path) > 0.0);
    }
}

#[test]
fn a_clipping_frame_cuts_its_children() {
    let (d, g) = doc_with(|d, g| {
        let f = d.create(g, NodeKind::Frame, "F", 0.0, 0.0, 50.0, 50.0).unwrap();
        d.get_mut(f).unwrap().fills = vec![];
        rect_in(d, f, Rect::new(25.0, 25.0, 100.0, 100.0), BLACK);
    });
    let got = paints(&d, g);
    assert_eq!(got.paints.len(), 1);
    assert!((area(&got.paints[0].path) - 625.0).abs() < 1e-6, "clipped to the frame");
}

#[test]
fn a_mask_clips_the_layers_above_it() {
    let (d, g) = doc_with(|d, g| {
        let m = rect_in(d, g, Rect::new(0.0, 0.0, 30.0, 30.0), WHITE);
        d.get_mut(m).unwrap().is_mask = true;
        rect_in(d, g, Rect::new(10.0, 10.0, 100.0, 100.0), BLACK);
    });
    let got = paints(&d, g);
    assert_eq!(got.paints.len(), 1, "the mask itself isn't painted");
    assert!((area(&got.paints[0].path) - 400.0).abs() < 1e-6);
}

#[test]
fn nested_groups_and_hidden_layers() {
    let (d, g) = doc_with(|d, g| {
        let inner = d.create(g, NodeKind::Group, "In", 30.0, 40.0, 50.0, 50.0).unwrap();
        rect_in(d, inner, Rect::new(5.0, 5.0, 15.0, 15.0), BLACK);
        let hidden = rect_in(d, inner, Rect::new(0.0, 0.0, 50.0, 50.0), BLACK);
        d.get_mut(hidden).unwrap().visible = false;
    });
    let got = paints(&d, g);
    assert_eq!(got.paints.len(), 1);
    let b = got.paints[0].path.bounding_box();
    assert!((b.x0 - 35.0).abs() < 1e-9 && (b.y0 - 45.0).abs() < 1e-9, "transforms compose: {b:?}");
}

#[test]
fn copies_from_rows_above_and_stacks_inside_are_gathered() {
    let (mut d, g) = doc_with(|d, g| {
        let r = rect_in(d, g, Rect::new(0.0, 0.0, 10.0, 10.0), BLACK);
        // A stack on a layer inside: three copies.
        d.get_mut(r).unwrap().plugin_data.insert(PLUGIN_KEY.into(), stack(json!([{"id": "m1", "kind": "linearArray", "count": 3, "offsetX": 20}])));
    });
    assert_eq!(paints(&d, g).paints.len(), 3);
    // Two copies of the whole group above the stencil row: six, and the design is wider.
    d.get_mut(g)
        .unwrap()
        .plugin_data
        .insert(PLUGIN_KEY.into(), stack(json!([{"id": "m1", "kind": "linearArray", "count": 2, "offsetX": 150, "offsetY": 0}, stencil_row()])));
    let (frame, got) = gather(&d, d.node(g).unwrap()).unwrap();
    assert_eq!(got.paints.len(), 6);
    assert_eq!(frame.copies.len(), 2);
    assert_eq!(frame.layer_bounds, Rect::new(0.0, 0.0, 250.0, 100.0));
    let xs: Vec<f64> = got.paints.iter().map(|p| p.path.bounding_box().x0).collect();
    assert!(xs.iter().any(|x| (x - 190.0).abs() < 1e-9), "the second copy's last rectangle: {xs:?}");
}

#[test]
fn gradients_cut_as_their_mean_and_images_are_left_out() {
    let (d, g) = doc_with(|d, g| {
        let r = rect_in(d, g, Rect::new(0.0, 0.0, 10.0, 10.0), BLACK);
        let stops = vec![
            ColorStop { position: 0.0, color: Color::from_hex(BLACK).unwrap() },
            ColorStop { position: 1.0, color: Color::from_hex(WHITE).unwrap() },
        ];
        let grad = Gradient { kind: GradientKind::Linear, stops, from: (0.0, 0.5), to: (1.0, 0.5), width: (0.0, 0.0) };
        d.get_mut(r).unwrap().fills = vec![Fill { kind: PaintKind::Gradient(grad), ..solid(BLACK) }];
    });
    let got = paints(&d, g);
    assert_eq!(got.paints.len(), 1);
    assert!((got.paints[0].colour[0] - 0.5).abs() < 1e-6);
    assert!(got.warnings.iter().any(|w| w.contains("gradient")), "{:?}", got.warnings);
}

/// A black square with a white one inside: by default (threshold 0.5) the white is cut.
fn two_tone() -> (Document, NodeId, NodeId) {
    let mut inner = NodeId(0);
    let (d, g) = doc_with(|d, g| {
        rect_in(d, g, Rect::new(0.0, 0.0, 100.0, 100.0), BLACK);
        inner = rect_in(d, g, Rect::new(25.0, 25.0, 75.0, 75.0), WHITE);
    });
    (d, g, inner)
}

#[test]
fn results_are_fresh_until_the_design_changes() {
    let (mut d, g, inner) = two_tone();
    let n = d.node(g).unwrap().clone();
    let first = compute_now(&d, g).unwrap();
    // The white square and four 1.5 mm registration holes in the margin corners.
    assert_eq!(first.stats.cut_pieces, 5);
    let holes = 4.0 * std::f64::consts::PI * 0.75 * 0.75;
    assert!((first.stats.area_cut_mm2 - 2500.0 * 9.0 - holes).abs() < 1.0, "{:?}", first.stats);
    assert!(store::is_fresh(&d, &n));
    let (i, fresh) = info(&d, &n).unwrap();
    assert!(fresh && i.colours.iter().any(|c| c.hex == "#ffffff" && c.cut));
    // Preview colours and moving the layer don't change the cut.
    let mut moved = d.clone();
    moved.get_mut(g).unwrap().transform = figcraft_geom::Affine::translate((300.0, 7.0));
    let src = stack(json!([{"id": "s1", "kind": "stencil", "preview": "cloth", "paperColour": "#123456"}]));
    moved.get_mut(g).unwrap().plugin_data.insert(PLUGIN_KEY.into(), src);
    assert!(store::is_fresh(&moved, moved.node(g).unwrap()), "same geometry");
    // A source colour edit: stale (the old result still draws), then fresh again with the change.
    d.get_mut(inner).unwrap().fills = vec![solid("202020")];
    let n = d.node(g).unwrap().clone();
    assert!(!store::is_fresh(&d, &n));
    assert!(store::lookup(&d, &n).is_some(), "the stale result is still there to draw");
    ensure(&d);
    let (i, fresh) = info(&d, &n).unwrap();
    assert!(fresh);
    assert!(i.colours.iter().all(|c| !c.cut), "nothing light left: {:?}", i.colours);
    // A cut param: stale.
    d.get_mut(g).unwrap().plugin_data.insert(PLUGIN_KEY.into(), stack(json!([{"id": "s1", "kind": "stencil", "threshold": 0.05}])));
    assert!(!store::is_fresh(&d, d.node(g).unwrap()));
}

#[test]
fn the_canvas_draws_the_sheet_in_place_of_the_design() {
    let (d, g, _) = two_tone();
    ensure(&d);
    let world = figcraft_render::export_bounds(&d, g).unwrap();
    // The group at (10, 20), 100 wide, with a 5-unit margin.
    assert_eq!(world, Rect::new(5.0, 15.0, 115.0, 125.0));
    let pix = figcraft_render::render_area(&d, &[g], world, 1.0).unwrap();
    let px = |x: f64, y: f64| {
        let i = ((y - world.y0) as usize * pix.width() as usize + (x - world.x0) as usize) * 4;
        pix.data()[i..i + 4].to_vec()
    };
    // Margin: paper (kakishibu brown, #c8a165), beside the registration hole at (7.5, 17.5).
    assert_eq!(px(9.0, 17.0), [0xc8, 0xa1, 0x65, 0xff]);
    // Where the black square was: paper too.
    assert_eq!(px(15.0, 25.0), [0xc8, 0xa1, 0x65, 0xff]);
    // The white square is cut: the canvas shows through.
    assert_eq!(px(60.0, 70.0)[3], 0, "cut out");
    // Cloth preview: cloth with the paste through the cut.
    let mut d2 = d.clone();
    d2.get_mut(g).unwrap().plugin_data.insert(PLUGIN_KEY.into(), stack(json!([{"id": "s1", "kind": "stencil", "preview": "cloth"}])));
    let pix = figcraft_render::render_area(&d2, &[g], world, 1.0).unwrap();
    let px = |x: f64, y: f64| {
        let i = ((y - world.y0) as usize * pix.width() as usize + (x - world.x0) as usize) * 4;
        pix.data()[i..i + 4].to_vec()
    };
    assert_eq!(px(9.0, 17.0), [0xf4, 0xef, 0xe6, 0xff], "cloth");
    assert_eq!(px(60.0, 70.0), [0x1f, 0x3a, 0x8a, 0xff], "paste");
}

#[test]
fn the_sheet_counts_for_bounds_and_hit_testing() {
    let (d, g, _) = two_tone();
    let page = d.first_page().unwrap();
    // On the margin, outside the design: the layer.
    let hit = figcraft_render::picabia::hit_with_copies(&d, page, Point::new(7.0, 17.0), 0.0, figcraft_doc::geometry::BoolResult::default(), None);
    assert_eq!(hit, Some(g));
    let off = figcraft_render::picabia::hit_with_copies(&d, page, Point::new(1.0, 1.0), 0.0, figcraft_doc::geometry::BoolResult::default(), None);
    assert_eq!(off, None);
    let boxes = figcraft_render::picabia::copy_boxes(&d, g);
    assert!(boxes.contains(&Rect::new(5.0, 15.0, 115.0, 125.0)), "marquee: {boxes:?}");
}

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn rect(s: &mut Session, x: f64, hex: &str) -> NodeId {
    let id = id_of(&ex(s, "shape.rectangle", json!({"x": x, "y": 0, "width": 100, "height": 100}))["id"]).unwrap();
    ex(s, "node.set", json!({"ids": [id], "props": {"fills": [{"type": "solid", "color": format!("#{hex}")}]}}));
    id
}

#[test]
fn stencil_info_reports_colours_stats_and_size() {
    let mut s = Session::new();
    let a = rect(&mut s, 0.0, BLACK);
    let b = rect(&mut s, 100.0, WHITE);
    let none = ex(&mut s, "modifier.stencilInfo", json!({"id": a}));
    assert_eq!(none["stencil"], Value::Null);
    let made = ex(&mut s, "modifier.stencilFromSelection", json!({"ids": [a, b]}));
    let g = id_of(&made["id"]).unwrap();
    assert_eq!(made["modifier"], "m1");
    let i = ex(&mut s, "modifier.stencilInfo", json!({"id": g}));
    assert_eq!(i["stencil"], "m1");
    let colours = i["colours"].as_array().unwrap();
    assert_eq!(colours.len(), 2, "{i}");
    let white = colours.iter().find(|c| c["hex"] == "#ffffff").unwrap();
    assert_eq!(white["cut"], true);
    assert!((white["area"].as_f64().unwrap() - 150.0 * 150.0).abs() < 1.0, "100 units at 1.5 mm/unit: {white}");
    assert_eq!(i["stats"]["cutPieces"], 5, "the white square and four registration holes");
    assert!(i["stats"]["cutLengthMm"].as_f64().unwrap() > 0.0);
    // 200 × 100 design, 300 mm wide, plus 15 mm all round.
    assert!((i["sheetMm"]["w"].as_f64().unwrap() - 330.0).abs() < 1e-6);
    assert!((i["sheetMm"]["h"].as_f64().unwrap() - 180.0).abs() < 1e-6);
    // The panel's non-blocking read gives the same.
    let v = crate::picabia_stencil::info_value(s.doc().unwrap(), g);
    assert_eq!(v["fresh"], true);
    assert_eq!(v["colours"], i["colours"]);
}

#[test]
fn stencil_from_selection_groups_then_adds_the_row_in_one_step() {
    let mut s = Session::new();
    let a = rect(&mut s, 0.0, BLACK);
    let b = rect(&mut s, 150.0, WHITE);
    let before = s.doc().unwrap().get(s.page().unwrap()).unwrap().children.len();
    let made = ex(&mut s, "modifier.stencilFromSelection", json!({"ids": [a, b], "props": {"widthMm": 500}}));
    let g = id_of(&made["id"]).unwrap();
    assert_eq!(s.undo_labels(1), ["Make stencil"]);
    assert_eq!(s.selection(), vec![g]);
    let doc = s.doc().unwrap();
    let n = doc.get(g).unwrap();
    assert!(matches!(n.kind, NodeKind::Group));
    assert_eq!(n.children, vec![a, b]);
    let st = figcraft_modifiers::node_stack(n).unwrap().unwrap();
    assert_eq!(st.stencil().unwrap().1.width_mm, 500.0);
    // Again: refused, it is one already.
    assert!(s.execute("modifier.stencilFromSelection", &json!({"ids": [g]})).is_err());
    ex(&mut s, "edit.undo", json!({}));
    let doc = s.doc().unwrap();
    assert!(doc.node(g).is_none(), "one undo removes the group and the row");
    assert_eq!(doc.get(s.page().unwrap()).unwrap().children.len(), before);
    // One layer takes the row itself.
    let made = ex(&mut s, "modifier.stencilFromSelection", json!({"ids": [a]}));
    assert_eq!(id_of(&made["id"]), Some(a));
    assert!(store::is_stencil(s.doc().unwrap().get(a).unwrap()));
    // Nothing selected, a page: refused.
    ex(&mut s, "select.set", json!({"ids": []}));
    assert!(s.execute("modifier.stencilFromSelection", &json!({})).is_err());
    let page = s.page().unwrap();
    assert!(s.execute("modifier.stencilFromSelection", &json!({"ids": [page]})).is_err());
}

#[test]
fn exports_compute_stale_stencils_first() {
    let mut s = Session::new();
    let a = rect(&mut s, 0.0, BLACK);
    let b = rect(&mut s, 100.0, WHITE);
    let g = id_of(&ex(&mut s, "modifier.stencilFromSelection", json!({"ids": [a, b]}))["id"]).unwrap();
    let out = ex(&mut s, "file.export", json!({"ids": [g], "format": "png"}));
    assert!(out["data"].as_str().is_some_and(|d| !d.is_empty()));
    let doc = s.doc().unwrap();
    assert!(store::is_fresh(doc, doc.get(g).unwrap()));
}

#[test]
fn refresh_computes_in_the_background() {
    let (d, g, _) = two_tone();
    let d = std::sync::Arc::new(d);
    crate::picabia_stencil::refresh(&d);
    for _ in 0..400 {
        if store::is_fresh(&d, d.node(g).unwrap()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(store::is_fresh(&d, d.node(g).unwrap()));
}

#[test]
fn svg_and_pdf_exports_draw_the_sheet() {
    let mut s = Session::new();
    let a = rect(&mut s, 0.0, BLACK);
    let b = rect(&mut s, 100.0, WHITE);
    let g = id_of(&ex(&mut s, "modifier.stencilFromSelection", json!({"ids": [a, b]}))["id"]).unwrap();
    let out = ex(&mut s, "file.export", json!({"ids": [g], "format": "svg"}));
    use base64::Engine as _;
    let svg = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(out["data"].as_str().unwrap()).unwrap()).unwrap();
    assert!(svg.contains("fill=\"#c8a165\"") && svg.contains("fill-rule=\"evenodd\""), "{svg}");
    assert!(!svg.contains("fill=\"#000000\""), "the design itself isn't written: {svg}");
    // The export covers the sheet: 200 wide at 1.5 mm/unit plus 10 units each side.
    assert!(svg.contains("width=\"220\""), "{svg}");
    let out = ex(&mut s, "file.export", json!({"ids": [g], "format": "pdf"}));
    assert!(out["data"].as_str().is_some_and(|d| !d.is_empty()));
}

#[test]
fn stencil_info_lists_islands_and_their_stats() {
    let (d, g) = doc_with(|d, g| {
        rect_in(d, g, Rect::new(0.0, 0.0, 100.0, 100.0), BLACK);
    });
    let mut i = (*compute_now(&d, g).unwrap()).clone();
    i.islands = vec![
        figcraft_stencil::Island { bounds: Rect::new(40.0, 40.0, 60.0, 70.0), tied: 2, pattern_held: false, nested_depth: 0 },
        figcraft_stencil::Island { bounds: Rect::new(45.0, 45.0, 50.0, 50.0), tied: 0, pattern_held: true, nested_depth: 1 },
        figcraft_stencil::Island { bounds: Rect::new(f64::NAN, 0.0, 1.0, 1.0), tied: 0, pattern_held: false, nested_depth: 0 },
    ];
    i.stats.islands = 3;
    i.stats.ties = 2;
    i.stats.pattern_held = 1;
    i.stats.untied = 1;
    let v = crate::picabia_stencil::info_json(&i);
    assert_eq!(
        v["islands"],
        json!([
            {"x": 40.0, "y": 40.0, "w": 20.0, "h": 30.0, "tied": 2, "patternHeld": false, "depth": 0},
            {"x": 45.0, "y": 45.0, "w": 5.0, "h": 5.0, "tied": 0, "patternHeld": true, "depth": 1},
            {"x": 0.0, "y": 0.0, "w": 0.0, "h": 1.0, "tied": 0, "patternHeld": false, "depth": 0},
        ]),
        "layer space; what isn't a number reads 0"
    );
    assert_eq!(v["stats"]["islands"], 3);
    assert_eq!(v["stats"]["ties"], 2);
    assert_eq!(v["stats"]["patternHeld"], 1);
    assert_eq!(v["stats"]["untied"], 1);
    // The command lists them too (none in a plain square).
    let w = crate::picabia_stencil::info_value(&d, g);
    assert_eq!(w["islands"], json!([]));
    assert_eq!(w["stats"]["islands"], 0);
}

#[test]
fn a_ring_around_an_island_reports_it_tied_through_stencil_info() {
    // A black square with a white ring cut out of it and a black island in the ring.
    let mut s = Session::new();
    let ids: Vec<NodeId> = [(0.0, 0.0, 100.0, BLACK), (20.0, 20.0, 60.0, WHITE), (40.0, 40.0, 20.0, BLACK)]
        .into_iter()
        .map(|(x, y, w, hex)| {
            let id = id_of(&ex(&mut s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": w}))["id"]).unwrap();
            ex(&mut s, "node.set", json!({"ids": [id], "props": {"fills": [{"type": "solid", "color": format!("#{hex}")}]}}));
            id
        })
        .collect();
    let g = id_of(&ex(&mut s, "modifier.stencilFromSelection", json!({"ids": ids}))["id"]).unwrap();
    let i = ex(&mut s, "modifier.stencilInfo", json!({"id": g}));
    let islands = i["islands"].as_array().unwrap();
    assert_eq!(islands.len(), 1, "{i}");
    let isl = &islands[0];
    for (k, want) in [("x", 40.0), ("y", 40.0), ("w", 20.0), ("h", 20.0)] {
        assert!((isl[k].as_f64().unwrap() - want).abs() < 1.0, "{k}, layer space: {isl}");
    }
    assert_eq!(isl["depth"], 0);
    assert_eq!(isl["tied"], 2, "two ties by default: {isl}");
    assert_eq!(i["stats"]["islands"], 1);
    assert_eq!(i["stats"]["ties"], 2);
    assert_eq!(i["stats"]["untied"], 0);
    // The tie bars are kept for the canvas markers.
    let doc = s.doc().unwrap();
    let (info, _) = info(doc, doc.node(g).unwrap()).unwrap();
    assert!(area(&info.ties) > 0.0);
}

// P11: repeat, tile, problems, registration holes, materials.

/// A 20 × 20 group at (10, 20): black, with a white 6-unit square in each corner holding a black
/// 2-unit square. Repeated by `rows` (a gridArray of gap 0 or a wallpaper) the corners meet as a
/// white ring around a black island centred on each inner lattice point: across the cell edges.
fn corner_motif(rows: Value) -> (Document, NodeId) {
    let mut d = Document::new("repeat");
    let page = d.first_page().unwrap();
    let g = d.create(page, NodeKind::Group, "G", 10.0, 20.0, 20.0, 20.0).unwrap();
    rect_in(&mut d, g, Rect::new(0.0, 0.0, 20.0, 20.0), BLACK);
    for (x, y) in [(0.0, 0.0), (14.0, 0.0), (0.0, 14.0), (14.0, 14.0)] {
        rect_in(&mut d, g, Rect::new(x, y, x + 6.0, y + 6.0), WHITE);
    }
    for (x, y) in [(0.0, 0.0), (18.0, 0.0), (0.0, 18.0), (18.0, 18.0)] {
        rect_in(&mut d, g, Rect::new(x, y, x + 2.0, y + 2.0), BLACK);
    }
    d.get_mut(g).unwrap().plugin_data.insert(PLUGIN_KEY.into(), stack(rows));
    (d, g)
}

fn grid_row(rows: u32, cols: u32) -> Value {
    json!({"id": "g1", "enabled": true, "kind": "gridArray", "rows": rows, "cols": cols, "gapX": 0, "gapY": 0})
}

fn set_stack(d: &mut Document, g: NodeId, rows: Value) {
    d.get_mut(g).unwrap().plugin_data.insert(PLUGIN_KEY.into(), stack(rows));
}

fn info_of(d: &Document, g: NodeId) -> Value {
    crate::picabia_stencil::info_json(&compute_now(d, g).unwrap())
}

#[test]
fn stencil_info_lists_registration_holes_problems_and_materials() {
    let (d, g, _) = two_tone();
    let v = info_of(&d, g);
    // Four registration holes, at the design's corners pushed half the 5-unit margin out.
    let hoshi = v["hoshi"].as_array().unwrap();
    assert_eq!(hoshi.len(), 4, "{v}");
    assert!(hoshi.iter().any(|h| (h["x"].as_f64().unwrap() + 2.5).abs() < 1e-6 && (h["y"].as_f64().unwrap() + 2.5).abs() < 1e-6), "{hoshi:?}");
    // Every kind counted, none here.
    let counts = v["stats"]["problems"].as_object().unwrap();
    let kinds = ["narrowPaper", "narrowCut", "smallCut", "smallHole", "sharpCorner", "untied"];
    assert_eq!(counts.len(), 6);
    for k in kinds {
        assert!(counts.contains_key(k), "{k}: {counts:?}");
    }
    assert_eq!(v["problems"], json!([]));
    assert_eq!(v["stats"]["repeatIslands"], 0);
    // No repeat: no lattice, cell or tile.
    assert_eq!((v["repeat"].clone(), v["cell"].clone(), v["tile"].clone()), (Value::Null, Value::Null, Value::Null));
    // The presets (custom has none); paper is the defaults.
    let m = v["materials"].as_array().unwrap();
    assert_eq!(m.iter().map(|r| r["id"].as_str().unwrap()).collect::<Vec<_>>(), ["paper", "film", "card"]);
    assert_eq!(m[0], json!({"id": "paper", "minCutWidthMm": 0.3, "minBridgeMm": 0.5, "tieWidthMm": 1.0, "minCornerAngle": 30.0, "kerfMm": 0.1}));
    assert_eq!(m[1]["minCutWidthMm"], 0.2);
    // The canvas reads them without JSON.
    let (i, _) = info(&d, d.node(g).unwrap()).unwrap();
    assert_eq!(i.hoshi.len(), 4);
    assert!(i.problems.is_empty() && i.cell.is_none() && i.repeat.is_none());
}

#[test]
fn flag_mode_reports_problems_by_kind() {
    // A white sliver 0.05 units (0.15 mm at 3 mm per unit) wide: narrower than the 0.3 mm minimum.
    let (mut d, g) = doc_with(|d, g| {
        rect_in(d, g, Rect::new(0.0, 0.0, 100.0, 100.0), BLACK);
        rect_in(d, g, Rect::new(20.0, 50.0, 80.0, 50.05), WHITE);
    });
    let fixed = info_of(&d, g);
    set_stack(&mut d, g, json!([{"id": "s1", "kind": "stencil", "limitMode": "flag"}]));
    let v = info_of(&d, g);
    let problems = v["problems"].as_array().unwrap();
    assert!(!problems.is_empty(), "{v}");
    let kinds = crate::picabia_stencil::ProblemKind::ALL.map(|k| k.id());
    for p in problems {
        assert!(kinds.contains(&p["kind"].as_str().unwrap()), "{p}");
        // Layer space, near the sliver.
        assert!(p["x"].as_f64().unwrap() >= 19.0 && p["x"].as_f64().unwrap() <= 81.0 && (p["y"].as_f64().unwrap() - 50.0).abs() < 2.0, "{p}");
    }
    assert!(v["stats"]["problems"]["narrowCut"].as_u64().unwrap() > 0, "{v}");
    let listed = problems.iter().filter(|p| p["kind"] == "narrowCut").count() as u64;
    assert_eq!(v["stats"]["problems"]["narrowCut"].as_u64().unwrap(), listed);
    // Fixed, the sliver is cleaned away instead.
    assert_eq!(fixed["stats"]["problems"]["narrowCut"], 0, "{fixed}");
    let (i, _) = info(&d, d.node(g).unwrap()).unwrap();
    assert_eq!(i.problems.len(), problems.len());
}

#[test]
fn a_grid_repeated_island_across_the_cell_edge_is_tied_repeat_aware() {
    let (d, g) = corner_motif(json!([grid_row(3, 3), stencil_row()]));
    let frame = store::frame(&d, d.node(g).unwrap()).unwrap();
    // The grid's lattice in the group's own space (the parent's moved by the group's place).
    let r = frame.repeat.unwrap();
    assert_eq!((r.origin, r.a, r.b), (Point::ORIGIN, figcraft_geom::Vec2::new(20.0, 0.0), figcraft_geom::Vec2::new(0.0, 20.0)));
    assert_eq!(frame.layer_bounds, Rect::new(0.0, 0.0, 60.0, 60.0));
    let v = info_of(&d, g);
    // Four inner lattice points, each an island across four cells; one in the centre cell.
    assert_eq!(v["stats"]["islands"], 4, "{v}");
    assert_eq!(v["stats"]["repeatIslands"], 1, "{v}");
    assert_eq!(v["stats"]["untied"], 0, "{v}");
    assert_eq!(v["stats"]["problems"]["untied"], 0);
    assert_eq!(v["repeat"], json!({"ax": 20.0, "ay": 0.0, "bx": 0.0, "by": 20.0}));
    // The centre cell [20, 40]².
    let cell = v["cell"].as_array().unwrap();
    assert_eq!(cell.len(), 4);
    let (xs, ys): (Vec<f64>, Vec<f64>) = cell.iter().map(|p| (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap())).unzip();
    assert_eq!((xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max)), (20.0, 40.0));
    assert_eq!((ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max)), (20.0, 40.0));
    assert_eq!(v["tile"], Value::Null, "sheet output");
    // Repeat off: judged on the sheet as drawn, no lattice.
    let (d2, g2) = corner_motif(json!([grid_row(3, 3), {"id": "s1", "kind": "stencil", "repeat": "off"}]));
    let off = info_of(&d2, g2);
    assert_eq!((off["stats"]["repeatIslands"].clone(), off["repeat"].clone(), off["cell"].clone()), (json!(0), Value::Null, Value::Null));
    assert_eq!(off["stats"]["untied"], 0);
}

#[test]
fn tile_output_places_the_sheet_on_the_repeat_cell() {
    let (mut d, g) = corner_motif(json!([grid_row(3, 3), {"id": "s1", "kind": "stencil", "output": "tile"}]));
    let n = d.node(g).unwrap().clone();
    // 60 units across at 300 mm: 5 mm per unit, the 15 mm margin is 3 units. Known before compute.
    let frame = store::frame(&d, &n).unwrap();
    assert_eq!(frame.sheet, Rect::new(17.0, 17.0, 43.0, 43.0));
    assert!(frame.cell.is_some());
    assert_eq!(store::parent_sheet(&d, &n), Some(Rect::new(27.0, 37.0, 53.0, 63.0)));
    let v = info_of(&d, g);
    // The tile's sheet: (20 + 2·3) units at 5 mm; the design's sheet stays for the panel.
    assert_eq!(v["tile"], json!({"w": 130.0, "h": 130.0}), "{v}");
    assert!((v["sheetMm"]["w"].as_f64().unwrap() - 330.0).abs() < 1e-9);
    // Read from the compute itself, and just before the hit tests (which draw the stored result):
    // the store is shared by the tests running at once and keeps 64 results.
    let (_, _, sheet) = crate::picabia_stencil::compute_cut_now(&d, g).unwrap();
    assert_eq!(sheet, frame.sheet, "what is cut is where the frame put it");
    assert!(store::lookup(&d, &n).is_some());
    // Bounds and hit testing follow the tile.
    let world = figcraft_render::export_bounds(&d, g).unwrap();
    assert!((world.x0 - 27.0).abs() < 1e-9 && (world.x1 - 53.0).abs() < 1e-9, "{world:?}");
    let page = d.first_page().unwrap();
    let hit = |d: &Document, x: f64, y: f64| {
        figcraft_render::picabia::hit_with_copies(d, page, Point::new(x, y), 0.0, figcraft_doc::geometry::BoolResult::default(), None)
    };
    assert_eq!(hit(&d, 28.0, 38.0), Some(g), "on the tile's margin");
    assert_eq!(hit(&d, 8.0, 18.0), None, "where the whole sheet's margin was");
    assert_eq!(hit(&d, 15.0, 25.0), None, "the design off the tile isn't drawn");
    let boxes = figcraft_render::picabia::copy_boxes(&d, g);
    assert!(boxes.contains(&Rect::new(27.0, 37.0, 53.0, 63.0)), "marquee: {boxes:?}");
    // Output and repeat changes recompute, and move the sheet back. (A tie seed no other test uses:
    // the store is shared by the tests running at once.)
    set_stack(&mut d, g, json!([grid_row(3, 3), {"id": "s1", "kind": "stencil", "tieSeed": 4711}]));
    let n = d.node(g).unwrap().clone();
    assert!(!store::is_fresh(&d, &n), "output sheet: stale");
    assert_eq!(store::frame(&d, &n).unwrap().sheet, Rect::new(-3.0, -3.0, 63.0, 63.0));
    let i = compute_now(&d, g).unwrap();
    assert!(i.tile_mm.is_none() && i.repeat.is_some());
    set_stack(&mut d, g, json!([grid_row(3, 3), {"id": "s1", "kind": "stencil", "tieSeed": 4711, "repeat": "off"}]));
    let n = d.node(g).unwrap().clone();
    assert!(!store::is_fresh(&d, &n), "repeat off: stale");
    let i = compute_now(&d, g).unwrap();
    assert!(i.repeat.is_none() && i.cell.is_none() && i.stats.repeat_islands == 0);
    // Tile without a repeat: the whole sheet.
    set_stack(&mut d, g, json!([grid_row(3, 3), {"id": "s1", "kind": "stencil", "repeat": "off", "output": "tile"}]));
    let n = d.node(g).unwrap().clone();
    assert_eq!(store::frame(&d, &n).unwrap().sheet, Rect::new(-3.0, -3.0, 63.0, 63.0));
    assert_eq!(info_of(&d, g)["tile"], Value::Null);
}

#[test]
fn the_tile_frame_matches_what_the_stencil_cuts() {
    // Lattices of each shape: grid, one row, one column, an oblique wallpaper, a rotated layer.
    let cases = [
        json!([grid_row(3, 4)]),
        json!([grid_row(1, 4)]),
        json!([grid_row(3, 1)]),
        json!([{"id": "w1", "kind": "wallpaper", "group": "p1", "cols": 3, "rows": 3, "cellWidth": 20, "cellHeight": 20, "angle": 60}]),
        json!([{"id": "w1", "kind": "wallpaper", "group": "p6", "cols": 2, "rows": 2, "cellWidth": 25, "cellHeight": 25}]),
    ];
    for (k, rows) in cases.iter().enumerate() {
        for rotate in [false, true] {
            let mut rows = rows.as_array().unwrap().clone();
            rows.push(json!({"id": "s1", "kind": "stencil", "output": "tile"}));
            let (mut d, g) = corner_motif(Value::Array(rows));
            if rotate {
                let t = d.node(g).unwrap().transform;
                d.get_mut(g).unwrap().transform = t * figcraft_geom::Affine::rotate(0.3);
            }
            let n = d.node(g).unwrap().clone();
            let frame = store::frame(&d, &n).unwrap();
            assert!(frame.repeat.is_some(), "case {k}");
            let i = compute_now(&d, g).unwrap();
            let (shape, _) = store::lookup(&d, &n).unwrap();
            let close = |a: Rect, b: Rect| [a.x0 - b.x0, a.y0 - b.y0, a.x1 - b.x1, a.y1 - b.y1].iter().all(|v| v.abs() < 1e-6);
            assert!(close(shape.sheet, frame.sheet), "case {k} rotate {rotate}: cut {:?}, frame {:?}; {:?}", shape.sheet, frame.sheet, i.warnings);
            assert!(i.tile_mm.is_some(), "case {k}: {:?}", i.warnings);
            let (cell, fc) = (i.cell.unwrap(), frame.cell.unwrap());
            for p in fc {
                assert!(cell.iter().any(|q| (*q - p).hypot() < 1e-6), "case {k}: {cell:?} vs {fc:?}");
            }
        }
    }
}

#[test]
fn svg_and_pdf_exports_write_the_tile_where_it_is_cut() {
    let mut s = Session::new();
    let ids: Vec<NodeId> = [(0.0, 0.0, 20.0, BLACK), (14.0, 14.0, 6.0, WHITE), (18.0, 18.0, 2.0, BLACK)]
        .into_iter()
        .map(|(x, y, w, hex)| {
            let id = id_of(&ex(&mut s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": w}))["id"]).unwrap();
            ex(&mut s, "node.set", json!({"ids": [id], "props": {"fills": [{"type": "solid", "color": format!("#{hex}")}]}}));
            id
        })
        .collect();
    let g = id_of(&ex(&mut s, "modifier.stencilFromSelection", json!({"ids": ids, "props": {"output": "tile", "tieSeed": 99}}))["id"]).unwrap();
    ex(&mut s, "modifier.add", json!({"ids": [g], "kind": "gridArray", "props": {"rows": 3, "cols": 3, "gapX": 0, "gapY": 0}, "index": 0}));
    let i = ex(&mut s, "modifier.stencilInfo", json!({"id": g}));
    assert_eq!(i["tile"], json!({"w": 130.0, "h": 130.0}), "{i}");
    let out = ex(&mut s, "file.export", json!({"ids": [g], "format": "svg"}));
    use base64::Engine as _;
    let svg = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(out["data"].as_str().unwrap()).unwrap()).unwrap();
    // The tile: the 20-unit cell plus a 3-unit margin each side, not the 66-unit sheet.
    assert!(svg.contains("width=\"26\"") && svg.contains("height=\"26\""), "{svg}");
    assert!(svg.contains("fill=\"#c8a165\"") && !svg.contains("fill=\"#000000\""), "{svg}");
    let out = ex(&mut s, "file.export", json!({"ids": [g], "format": "pdf"}));
    let pdf = base64::engine::general_purpose::STANDARD.decode(out["data"].as_str().unwrap()).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/MediaBox [0 0 26 26]") || text.contains("/MediaBox[0 0 26 26]"), "the page is the tile");
}
