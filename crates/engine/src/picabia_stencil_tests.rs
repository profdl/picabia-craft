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
