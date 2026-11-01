//! PICABIA: the `imageTrace` generator over synthetic images: which layers trace, the density
//! and pixel budget, the traced result and its stats, Apply, chains with arrays and stencils,
//! files, exports and the `.fig` bake.

use base64::Engine as _;
use figcraft_color::{ImagePaint, ImageScaleMode, Paint, PaintKind};
use figcraft_doc::{ImageData, Node, NodeId, NodeKind};
use figcraft_geom::kurbo::Shape as _;
use figcraft_geom::{Point, Rect};
use figcraft_modifiers::{ImageTrace, PLUGIN_KEY};
use figcraft_render::picabia::generated as store;
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;
use crate::picabia_trace::{self as trace, NO_IMAGE, NOTHING};

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

/// A PNG of `w` × `h` px whose pixel (x, y) is `f(x, y)` (straight RGBA; alpha 0 or 255).
fn png(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut pix = figcraft_render::Pixmap::new(w, h).unwrap();
    for (i, px) in pix.data_mut().chunks_exact_mut(4).enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let [r, g, b, a] = f(x, y);
        let pm = |c: u8| (u32::from(c) * u32::from(a) / 255) as u8;
        px.copy_from_slice(&[pm(r), pm(g), pm(b), a]);
    }
    figcraft_render::encode_png(&pix).unwrap()
}

/// A flat logo: white, a red square on the left and a blue disc on the right (hard edges).
fn logo(w: u32, h: u32) -> impl Fn(u32, u32) -> [u8; 4] {
    move |x, y| {
        let (fx, fy) = (x as f64 / w as f64, y as f64 / h as f64);
        let (dx, dy) = (fx - 0.72, (fy - 0.5) * h as f64 / w as f64);
        if (0.1..0.4).contains(&fx) && (0.2..0.8).contains(&fy) {
            [255, 0, 0, 255]
        } else if (dx * dx + dy * dy).sqrt() < 0.18 {
            [0, 0, 255, 255]
        } else {
            [255, 255, 255, 255]
        }
    }
}

/// A synthetic "photo": a smooth multi-colour gradient with soft blobs and a little noise.
pub(crate) fn photo(w: u32, h: u32) -> impl Fn(u32, u32) -> [u8; 4] {
    move |x, y| {
        let (u, v) = (x as f64 / w as f64, y as f64 / h as f64);
        let blob = |cx: f64, cy: f64, r: f64| (-((u - cx).powi(2) + (v - cy).powi(2)) / (r * r)).exp();
        let (b1, b2, b3) = (blob(0.3, 0.35, 0.18), blob(0.7, 0.6, 0.22), blob(0.5, 0.85, 0.12));
        let mut n = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)).wrapping_mul(2_654_435_761);
        n ^= n >> 13;
        let noise = (n % 17) as f64 - 8.0;
        let c = |base: f64| (base + noise).clamp(0.0, 255.0) as u8;
        [c(40.0 + 160.0 * u + 120.0 * b1 - 60.0 * b3), c(60.0 + 120.0 * v + 140.0 * b2), c(200.0 - 150.0 * u + 90.0 * b3 - 40.0 * b1), 255]
    }
}

/// Place `bytes` as an image layer of `w` × `h` points at (`x`, 0).
fn place(s: &mut Session, bytes: &[u8], x: f64, w: f64, h: f64) -> NodeId {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    id_of(&ex(s, "image.place", json!({"data": b64, "x": x, "y": 0, "width": w, "height": h}))["id"]).unwrap()
}

/// Add an image fill of `bytes` on top of layer `id`'s fills.
fn fill_with(s: &mut Session, id: NodeId, bytes: &[u8]) {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    ex(s, "image.place", json!({"target": id, "data": b64}));
}

fn add(s: &mut Session, id: NodeId, kind: &str, props: Value) -> String {
    ex(s, "modifier.add", json!({"ids": [id], "kind": kind, "props": props}))["modifier"].as_str().unwrap().to_string()
}

fn info(s: &mut Session, id: NodeId) -> Value {
    ex(s, "modifier.resultInfo", json!({"id": id}))
}

fn shape(s: &Session, id: NodeId) -> std::sync::Arc<store::GenShape> {
    let doc = s.doc().unwrap();
    crate::picabia_jobs::ensure(doc);
    store::lookup(doc, doc.get(id).unwrap()).unwrap().0
}

fn hexes(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|c| c.as_str().or_else(|| c["hex"].as_str()).unwrap().to_string()).collect()
}

fn warned(info: &Value, what: &str) -> bool {
    info["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains(what))
}

/// The pixel at page point (`x`, `y`) of `ids` rendered over `area` at 1×.
fn pixel(s: &Session, ids: &[NodeId], area: Rect, x: f64, y: f64) -> [u8; 4] {
    let doc = s.doc().unwrap();
    crate::picabia_jobs::ensure(doc);
    let pix = figcraft_render::render_area(doc, ids, area, 1.0).unwrap();
    let i = ((y - area.y0) as usize * pix.width() as usize + (x - area.x0) as usize) * 4;
    let d = pix.data();
    [d[i], d[i + 1], d[i + 2], d[i + 3]]
}

fn decode(out: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(out["data"].as_str().unwrap()).unwrap()
}

#[test]
fn a_flat_logo_traces_to_its_colours_at_the_images_pixels() {
    let mut s = Session::new();
    let r = place(&mut s, &png(200, 100, logo(200, 100)), 0.0, 200.0, 100.0);
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"name": "Logo"}}));
    add(&mut s, r, "imageTrace", json!({"preset": "sixColors"}));
    let i = info(&mut s, r);
    assert_eq!(i["kind"], "imageTrace");
    assert_eq!(i["drawsDesign"], false);
    assert_eq!(i["stats"]["tracedPx"], json!({"w": 200, "h": 100}), "{i}");
    let colours = hexes(&i["colours"]);
    for want in ["#ffffff", "#ff0000", "#0000ff"] {
        assert!(colours.contains(&want.to_string()), "{want} in {colours:?}");
    }
    assert_eq!(colours[0], "#ffffff", "largest area first");
    assert_eq!(i["stats"]["colours"], 3, "{i}");
    let paths = i["stats"]["paths"].as_u64().unwrap();
    assert!(paths >= 3 && i["stats"]["anchors"].as_u64().unwrap() > 8, "{i}");
    assert_eq!(i["items"].as_u64().unwrap(), paths, "one input copy");
    let sh = shape(&s, r);
    assert!(sh.items.iter().all(|it| it.source == Some(r) && it.name.starts_with('#') && it.name.len() == 7));
    assert!(sh.items.iter().all(|it| matches!(it.paint, store::GenPaint::Fill { even_odd: false })));
    let red = sh.items.iter().find(|it| it.name == "#ff0000").unwrap();
    let b = red.path.bounding_box();
    assert!((b.x0 - 20.0).abs() < 1.5 && (b.x1 - 80.0).abs() < 1.5 && (b.y0 - 20.0).abs() < 1.5 && (b.y1 - 80.0).abs() < 1.5, "{b:?}");
    // It draws in place of the image: the blue disc's centre, the red square's, the white.
    let area = Rect::new(0.0, 0.0, 200.0, 100.0);
    assert_eq!(pixel(&s, &[r], area, 144.0, 50.0), [0, 0, 255, 255]);
    assert_eq!(pixel(&s, &[r], area, 50.0, 50.0), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, &[r], area, 5.0, 5.0), [255, 255, 255, 255]);
    // Black and white: two colours at most.
    ex(&mut s, "modifier.set", json!({"ids": [r], "index": 0, "props": {"preset": "blackAndWhiteLogo"}}));
    let i = info(&mut s, r);
    assert!(hexes(&i["colours"]).iter().all(|c| c == "#000000"), "white is ignored: {i}");
    // Flat logo mode.
    ex(&mut s, "modifier.set", json!({"ids": [r], "index": 0, "props": {"preset": "flatLogo"}}));
    let i = info(&mut s, r);
    assert!(hexes(&i["colours"]).contains(&"#ff0000".to_string()), "{i}");
}

#[test]
fn the_trace_is_deterministic_and_follows_its_settings() {
    let mut s = Session::new();
    let r = place(&mut s, &png(160, 120, photo(160, 120)), 0.0, 160.0, 120.0);
    add(&mut s, r, "imageTrace", json!({"preset": "sixteenColors"}));
    let doc = s.doc().unwrap().clone();
    let a = crate::picabia_generate::compute(&doc, doc.get(r).unwrap(), &store::frame(&doc, doc.get(r).unwrap()).unwrap());
    let b = crate::picabia_generate::compute(&doc, doc.get(r).unwrap(), &store::frame(&doc, doc.get(r).unwrap()).unwrap());
    assert_eq!(a.items, b.items);
    assert!(a.items.len() > 16, "{}", a.items.len());
    let many = info(&mut s, r)["stats"]["colours"].as_u64().unwrap();
    assert!((8..=16).contains(&many), "{many}");
    ex(&mut s, "modifier.set", json!({"ids": [r], "index": 0, "props": {"colors": 3}}));
    let i = info(&mut s, r);
    assert_eq!(i["stats"]["colours"], 3, "{i}");
    ex(&mut s, "modifier.set", json!({"ids": [r], "index": 0, "props": {"preset": "shadesOfGray"}}));
    let i = info(&mut s, r);
    assert!(hexes(&i["colours"]).iter().all(|c| c[1..3] == c[3..5] && c[3..5] == c[5..7]), "grays: {i}");
    ex(&mut s, "modifier.set", json!({"ids": [r], "index": 0, "props": {"preset": "autoColor"}}));
    assert!(info(&mut s, r)["stats"]["paths"].as_u64().unwrap() > 0);
}

#[test]
fn every_field_reaches_the_tracer() {
    let p = ImageTrace {
        preset: figcraft_modifiers::TracePreset::Custom,
        mode: figcraft_modifiers::TraceMode::Color,
        palette: figcraft_modifiers::TracePalette::FullTone,
        colors: 12,
        color_detail: 33.0,
        threshold: 77,
        paths: 44.0,
        corners: 55.0,
        noise: 9,
        method: figcraft_modifiers::TraceMethod::Overlapping,
        fills: false,
        strokes: true,
        stroke_width: 7.0,
        snap_curves_to_lines: true,
        ignore_white: true,
        ignore_color: Some("#102030".into()),
    };
    let t = trace::settings(&p);
    use figcraft_vcbridge::trace as vt;
    assert_eq!(
        t,
        vt::TraceSettings {
            mode: vt::Mode::Color,
            palette: vt::Palette::FullTone,
            colors: 12,
            color_detail: 33.0,
            threshold: 77,
            paths: 44.0,
            corners: 55.0,
            noise: 9,
            method: vt::Method::Overlapping,
            fills: false,
            strokes: true,
            stroke_width: 7.0,
            snap_curves_to_lines: true,
            ignore_white: true,
            ignore_color: Some([0x10, 0x20, 0x30]),
        }
    );
    // The presets agree with VectorCraft's (through the bridge).
    for preset in figcraft_modifiers::TracePreset::ALL {
        let Some(fields) = figcraft_modifiers::preset_fields(preset.id()) else { continue };
        assert_eq!(trace::settings(&fields), vt::preset_settings(preset.id()).unwrap(), "{}", preset.id());
    }
}

/// A rectangle node of `w` × `h` with `fills`, in a document holding a `iw` × `ih` image `img`.
fn node_with(doc: &mut figcraft_doc::Document, w: f64, h: f64, fills: Vec<Paint>, iw: u32, ih: u32) -> Node {
    doc.images.insert("img".into(), ImageData { bytes: std::sync::Arc::new(png(2, 2, |_, _| [9, 9, 9, 255])), width: iw, height: ih });
    let mut n = Node::new(NodeId(999), NodeKind::Rectangle, "n", w, h);
    n.fills = fills;
    n
}

fn img(mode: ImageScaleMode) -> ImagePaint {
    ImagePaint { image: "img".into(), scale_mode: mode, scale: 1.0, rotation: 0, crop: None, filters: Default::default() }
}

fn paint(ip: ImagePaint) -> Paint {
    Paint { kind: PaintKind::Image(ip), ..Paint::image("img", ImageScaleMode::Fill) }
}

#[test]
fn the_density_follows_how_each_fill_maps_its_image() {
    let mut doc = figcraft_doc::Document::new("t");
    let d = |doc: &mut figcraft_doc::Document, w: f64, h: f64, fills: Vec<Paint>, iw: u32, ih: u32| {
        let n = node_with(doc, w, h, fills, iw, ih);
        trace::density(doc, &n)
    };
    // Fill covers the box: a 4000 × 2000 image in a 400 × 400 box shows 2000 px over 400 pt.
    assert_eq!(d(&mut doc, 400.0, 400.0, vec![paint(img(ImageScaleMode::Fill))], 4000, 2000), Some(5.0));
    // Fit fits inside it: 4000 px over 400 pt.
    assert_eq!(d(&mut doc, 400.0, 400.0, vec![paint(img(ImageScaleMode::Fit))], 4000, 2000), Some(10.0));
    // Tile at scale 2: a pixel is 2 points.
    assert_eq!(d(&mut doc, 400.0, 400.0, vec![paint(ImagePaint { scale: 2.0, ..img(ImageScaleMode::Tile) })], 4000, 2000), Some(0.5));
    // A quarter turn swaps the sides (Fill: 2000 / 100 vs 4000 / 400).
    assert_eq!(d(&mut doc, 400.0, 100.0, vec![paint(ImagePaint { rotation: 1, ..img(ImageScaleMode::Fill) })], 4000, 2000), Some(5.0));
    // Crop: the image's left half fills the box (x through 0.5 of uv per box unit).
    let crop = ImagePaint { crop: Some([0.5, 0.0, 0.0, 1.0, 0.0, 0.0]), ..img(ImageScaleMode::Crop) };
    assert_eq!(d(&mut doc, 200.0, 200.0, vec![paint(crop)], 800, 400), Some(2.0));
    // The densest fill counts; hidden fills and those of opacity 0 don't.
    let hidden = Paint { visible: false, ..paint(img(ImageScaleMode::Fit)) };
    assert_eq!(d(&mut doc, 400.0, 400.0, vec![paint(img(ImageScaleMode::Fill)), hidden.clone()], 4000, 2000), Some(5.0));
    assert_eq!(d(&mut doc, 400.0, 400.0, vec![hidden], 4000, 2000), None);
    assert_eq!(d(&mut doc, 400.0, 400.0, vec![Paint::solid(figcraft_color::Color::BLACK)], 4000, 2000), None);
    // Clamped.
    assert_eq!(d(&mut doc, 1.0, 1.0, vec![paint(img(ImageScaleMode::Fill))], 20_000, 20_000), Some(trace::MAX_DENSITY));
    assert_eq!(d(&mut doc, 10_000.0, 10_000.0, vec![paint(img(ImageScaleMode::Fill))], 2, 2), Some(trace::MIN_DENSITY));
}

#[test]
fn the_raster_is_capped_before_it_is_made() {
    let b = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    assert_eq!(trace::raster_size(b, 2.0, 16 << 20), Some((2.0, 2000, 2000, false)));
    let (d, w, h, scaled) = trace::raster_size(b, 16.0, 16 << 20).unwrap();
    assert!(scaled && u64::from(w) * u64::from(h) <= 16 << 20 && w == h && d < 4.1, "{d} {w} {h}");
    let (_, w, h, scaled) = trace::raster_size(Rect::new(0.0, 0.0, 100_000.0, 1.0), 1.0, 16 << 20).unwrap();
    assert!(scaled && w <= trace::MAX_SIDE && h >= 1, "{w} {h}");
    let (_, w, h, _) = trace::raster_size(Rect::new(0.0, 0.0, 1e9, 1e-3), 16.0, 10_000).unwrap();
    assert!(u64::from(w) * u64::from(h) <= 10_000 && w <= trace::MAX_SIDE, "{w} {h}");
    assert_eq!(trace::raster_size(Rect::new(0.0, 0.0, f64::NAN, 1.0), 1.0, 100), None);
    assert_eq!(trace::raster_size(Rect::ZERO, 1.0, 100), None);
}

#[test]
fn a_huge_image_traces_scaled_down_with_a_warning() {
    let mut s = Session::new();
    let r = place(&mut s, &png(64, 64, logo(64, 64)), 0.0, 1000.0, 1000.0);
    // The file claims 20000 px a side (its pixels are a small image, drawn scaled up).
    {
        let doc = s.doc_mut().unwrap();
        let key = match &doc.get(r).unwrap().fills[0].kind {
            PaintKind::Image(ip) => ip.image.clone(),
            _ => panic!("not this kind"),
        };
        let im = doc.images.get_mut(&key).unwrap();
        im.width = 20_000;
        im.height = 20_000;
    }
    add(&mut s, r, "imageTrace", json!({"preset": "threeColors"}));
    let doc = s.doc().unwrap().clone();
    let n = doc.get(r).unwrap();
    assert_eq!(trace::density(&doc, n), Some(trace::MAX_DENSITY));
    let frame = store::frame(&doc, n).unwrap();
    let p = match &frame.kind {
        figcraft_modifiers::GeneratorKind::ImageTrace(p) => p.clone(),
        _ => panic!("not this kind"),
    };
    let out = trace::compute_within(&doc, n, &frame, &p, 40_000);
    let st = &out.stats["tracedPx"];
    let (w, h) = (st["w"].as_u64().unwrap(), st["h"].as_u64().unwrap());
    assert!(w * h <= 40_000 && w == h && w > 150, "{st}");
    assert!(out.warnings.iter().any(|w| w.contains("scaled down")), "{:?}", out.warnings);
    assert!(!out.items.is_empty());
    // In layer space all the same: the red square spans 10–40 % of the box.
    let red = out.items.iter().find(|it| it.name == "#ff0000").unwrap().path.bounding_box();
    assert!((red.x0 - 100.0).abs() < 20.0 && (red.x1 - 400.0).abs() < 20.0, "{red:?}");
    // Natively: 16 megapixels, and the side over 1500 px warns that it's slower.
    let out = trace::compute(&doc, n, &frame, &p);
    let st = &out.stats["tracedPx"];
    assert!(st["w"].as_u64().unwrap() * st["h"].as_u64().unwrap() <= 16 << 20, "{st}");
    assert!(out.warnings.iter().any(|w| w.contains("more slowly")), "{:?}", out.warnings);
}

#[test]
fn layers_without_a_visible_image_draw_their_design() {
    let mut s = Session::new();
    let r = id_of(&ex(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 50, "height": 50, "props": {"fill": "#00FF00"}}))["id"]).unwrap();
    add(&mut s, r, "imageTrace", json!({}));
    let i = info(&mut s, r);
    assert_eq!((i["drawsDesign"].clone(), i["items"].clone()), (json!(true), json!(0)));
    assert!(warned(&i, NO_IMAGE), "{i}");
    assert_eq!(pixel(&s, &[r], Rect::new(0.0, 0.0, 50.0, 50.0), 25.0, 25.0), [0, 255, 0, 255]);
    assert!(s.execute("modifier.apply", &json!({"ids": [r]})).is_err(), "nothing to apply");
    // A hidden image fill doesn't count either.
    fill_with(&mut s, r, &png(10, 10, |_, _| [0, 0, 0, 255]));
    assert_eq!(info(&mut s, r)["drawsDesign"], false);
    let mut fills = s.doc().unwrap().get(r).unwrap().fills.clone();
    fills[1].visible = false;
    s.doc_mut().unwrap().get_mut(r).unwrap().fills = fills;
    let i = info(&mut s, r);
    assert!(i["drawsDesign"] == true && warned(&i, NO_IMAGE), "{i}");
}

#[test]
fn empty_transparent_and_one_colour_images() {
    let mut s = Session::new();
    // Fully transparent: nothing drawn, with a warning.
    let r = place(&mut s, &png(40, 40, |_, _| [0, 0, 0, 0]), 0.0, 40.0, 40.0);
    add(&mut s, r, "imageTrace", json!({"preset": "sixColors"}));
    let i = info(&mut s, r);
    assert_eq!((i["items"].clone(), i["drawsDesign"].clone()), (json!(0), json!(false)), "{i}");
    assert!(warned(&i, NOTHING), "{i}");
    // One colour: one path over the box.
    let g = place(&mut s, &png(40, 40, |_, _| [10, 200, 30, 255]), 100.0, 40.0, 40.0);
    add(&mut s, g, "imageTrace", json!({"preset": "sixColors"}));
    let i = info(&mut s, g);
    assert_eq!((i["stats"]["paths"].clone(), i["colours"].clone()), (json!(1), json!(["#0ac81e"])), "{i}");
    let b = shape(&s, g).items[0].path.bounding_box();
    assert!((b.width() - 40.0).abs() < 0.5 && (b.height() - 40.0).abs() < 0.5, "{b:?}");
    // All white, ignored: nothing.
    let w = place(&mut s, &png(40, 40, |_, _| [255, 255, 255, 255]), 200.0, 40.0, 40.0);
    add(&mut s, w, "imageTrace", json!({"preset": "sixColors", "ignoreWhite": true}));
    let i = info(&mut s, w);
    assert!(i["items"] == 0 && warned(&i, NOTHING), "{i}");
    // The colour ignored instead of white.
    ex(&mut s, "modifier.set", json!({"ids": [g], "index": 0, "props": {"ignoreWhite": true, "ignoreColor": "#0ac81e"}}));
    assert_eq!(info(&mut s, g)["items"], 0);
}

#[test]
fn the_layer_traces_as_it_draws_shape_strokes_and_children() {
    let mut s = Session::new();
    // An ellipse: outside it is transparent and isn't traced.
    let e = id_of(&ex(&mut s, "shape.ellipse", json!({"x": 0, "y": 0, "width": 100, "height": 100}))["id"]).unwrap();
    s.doc_mut().unwrap().get_mut(e).unwrap().fills.clear();
    fill_with(&mut s, e, &png(100, 100, |_, _| [200, 0, 0, 255]));
    add(&mut s, e, "imageTrace", json!({"preset": "sixColors"}));
    let sh = shape(&s, e);
    assert_eq!(sh.items.len(), 1);
    let path = &sh.items[0].path;
    assert!(path.winding(Point::new(50.0, 50.0)) != 0);
    assert!(path.winding(Point::new(4.0, 4.0)) == 0, "the corner is outside the ellipse");
    assert!((path.area().abs() - std::f64::consts::PI * 2500.0).abs() < 150.0, "{}", path.area());
    // A frame with an image fill and a blue child: the child is traced too.
    let f = id_of(&ex(&mut s, "frame.create", json!({"x": 300, "y": 0, "width": 100, "height": 100}))["id"]).unwrap();
    s.doc_mut().unwrap().get_mut(f).unwrap().fills.clear();
    fill_with(&mut s, f, &png(100, 100, |_, _| [255, 255, 0, 255]));
    ex(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 30, "height": 30, "parent": f, "props": {"fill": "#0000FF"}}));
    add(&mut s, f, "imageTrace", json!({"preset": "sixColors"}));
    let i = info(&mut s, f);
    let c = hexes(&i["colours"]);
    assert!(c.contains(&"#ffff00".to_string()) && c.contains(&"#0000ff".to_string()), "{c:?}");
    // Its own effects are left out: a drop shadow doesn't trace.
    ex(&mut s, "effect.add", json!({"ids": [f], "type": "dropShadow"}));
    assert_eq!(hexes(&info(&mut s, f)["colours"]), c);
}

#[test]
fn strokes_trace_as_centre_lines_in_layer_units() {
    let mut s = Session::new();
    // Thin black lines on white, in a box at half the image's size (density 2).
    let line =
        png(
            200,
            200,
            |x, y| if (x % 50 < 4 && y > 20 && y < 180) || (y % 50 < 4 && x > 20 && x < 180) { [0, 0, 0, 255] } else { [255, 255, 255, 255] },
        );
    let r = place(&mut s, &line, 0.0, 100.0, 100.0);
    add(&mut s, r, "imageTrace", json!({"preset": "outline"}));
    let sh = shape(&s, r);
    let strokes: Vec<_> = sh.items.iter().filter(|it| matches!(it.paint, store::GenPaint::Stroke { .. })).collect();
    assert!(!strokes.is_empty(), "{:?}", sh.items.iter().map(|i| i.paint).collect::<Vec<_>>());
    for it in &strokes {
        let store::GenPaint::Stroke { width, cap, join } = it.paint else { panic!("not this kind") };
        assert!(width > 0.5 && width < 6.0, "image px halved: {width}");
        assert!(matches!(cap, figcraft_doc::StrokeCap::Round | figcraft_doc::StrokeCap::Square));
        assert_eq!(join, figcraft_doc::StrokeJoin::Round);
    }
    assert!(sh.bounds.x1 <= 101.0 && sh.bounds.y1 <= 101.0, "in layer units: {:?}", sh.bounds);
}

#[test]
fn apply_bakes_the_trace_and_undo_brings_the_image_back() {
    let mut s = Session::new();
    let r = place(&mut s, &png(120, 60, logo(120, 60)), 10.0, 120.0, 60.0);
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"name": "Badge"}}));
    add(&mut s, r, "imageTrace", json!({"preset": "sixColors"}));
    add(&mut s, r, "linearArray", json!({"count": 2, "offsetX": 200}));
    let items = shape(&s, r).items.len();
    let out = ex(&mut s, "modifier.apply", json!({"ids": [r], "upTo": 0}));
    let gid = id_of(&out["ids"][0]).unwrap();
    let doc = s.doc().unwrap();
    assert!(doc.node(r).is_none(), "the image layer is removed");
    let g = doc.get(gid).unwrap();
    assert_eq!(g.name, "Badge (traced)");
    assert_eq!(g.children.len(), items);
    let names: Vec<&str> = g.children.iter().map(|c| doc.get(*c).unwrap().name.as_str()).collect();
    assert_eq!(names[0], "#ffffff", "bottom first: the white back");
    assert!(names.contains(&"#ff0000") && names.contains(&"#0000ff"), "{names:?}");
    assert!(g.children.iter().all(|c| matches!(doc.get(*c).unwrap().kind, NodeKind::Vector { .. })));
    let red = g.children.iter().find(|c| doc.get(**c).unwrap().name == "#ff0000").unwrap();
    let rb = doc.world_bounds(*red).unwrap();
    assert!((rb.x0 - 22.0).abs() < 1.5 && (rb.y0 - 12.0).abs() < 1.5, "placed where it was: {rb:?}");
    let l = ex(&mut s, "modifier.list", json!({"id": gid}));
    assert_eq!(l["modifiers"][0]["kind"], "linearArray", "the row below moves onto the group: {l}");
    // Save and reopen keep the vectors.
    let dir = std::env::temp_dir().join(format!("picabia-trace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("traced.figcraft");
    ex(&mut s, "file.saveAs", json!({"path": path}));
    let mut t = Session::new();
    ex(&mut t, "file.open", json!({"path": path}));
    let tg = t.doc().unwrap().get(gid).unwrap();
    assert_eq!((tg.name.as_str(), tg.children.len()), ("Badge (traced)", items));
    let _ = std::fs::remove_dir_all(&dir);
    // Undo: the image and its stack are back.
    ex(&mut s, "edit.undo", json!({}));
    let doc = s.doc().unwrap();
    assert!(doc.node(gid).is_none());
    let n = doc.get(r).unwrap();
    assert!(matches!(n.fills[0].kind, PaintKind::Image(_)));
    assert!(n.plugin_data.contains_key(PLUGIN_KEY));
    // Apply up to the array below: its two copies of the traced group, nothing left live.
    let out = ex(&mut s, "modifier.apply", json!({"ids": [r], "upTo": 1}));
    let gid = id_of(&out["ids"][0]).unwrap();
    let doc = s.doc().unwrap();
    assert_eq!(doc.get(gid).unwrap().children.len(), 2);
    assert!(!doc.get(gid).unwrap().plugin_data.contains_key(PLUGIN_KEY));
}

#[test]
fn a_stencil_below_cuts_the_traced_colours() {
    let mut s = Session::new();
    let r = place(&mut s, &png(200, 100, logo(200, 100)), 0.0, 200.0, 100.0);
    add(&mut s, r, "imageTrace", json!({"preset": "threeColors"}));
    add(&mut s, r, "stencil", json!({}));
    let i = ex(&mut s, "modifier.stencilInfo", json!({"id": r}));
    let c = hexes(&i["colours"]);
    assert!(c.contains(&"#ff0000".to_string()) && c.contains(&"#0000ff".to_string()), "{i}");
    let doc = s.doc().unwrap();
    crate::picabia_jobs::ensure(doc);
    let drawn = figcraft_render::picabia::stencil::drawn(doc, doc.get(r).unwrap()).unwrap();
    assert!(!drawn.shape.cut.elements().is_empty(), "the cut");
}

#[test]
fn arrays_above_and_below_the_trace() {
    let mut s = Session::new();
    // Below: copies of the result, and a click on a copy hits the layer.
    let r = place(&mut s, &png(100, 100, logo(100, 100)), 0.0, 100.0, 100.0);
    add(&mut s, r, "imageTrace", json!({"preset": "sixColors"}));
    let grid = add(&mut s, r, "gridArray", json!({"rows": 1, "cols": 3, "gapX": 20}));
    let paths = info(&mut s, r)["stats"]["paths"].as_u64().unwrap() as usize;
    assert_eq!(shape(&s, r).items.len(), paths);
    let doc = s.doc().unwrap();
    let boxes = figcraft_render::picabia::copy_boxes(doc, r);
    assert!(boxes.contains(&Rect::new(240.0, 0.0, 340.0, 100.0)), "{boxes:?}");
    let world = figcraft_render::export_bounds(doc, r).unwrap();
    assert_eq!(world, Rect::new(0.0, 0.0, 340.0, 100.0));
    let page = doc.first_page().unwrap();
    let hit = |p: Point| figcraft_render::picabia::hit_with_copies(doc, page, p, 0.0, figcraft_doc::geometry::BoolResult::default(), None);
    assert_eq!(hit(Point::new(290.0, 50.0)), Some(r), "the third copy");
    assert_eq!(hit(Point::new(110.0, 50.0)), None, "the gap");
    assert_eq!(pixel(&s, &[r], world, 265.0, 50.0), [255, 0, 0, 255], "the third copy's red");
    // Changing the row below doesn't retrace: the result stays fresh (the same one).
    let kept = shape(&s, r);
    ex(&mut s, "modifier.set", json!({"ids": [r], "modifier": grid, "props": {"cols": 5}}));
    let doc = s.doc().unwrap();
    assert!(!crate::picabia_jobs::is_stale(doc, r), "nothing to recompute");
    let (now, fresh) = store::lookup(doc, doc.get(r).unwrap()).unwrap();
    assert!(fresh && std::sync::Arc::ptr_eq(&now, &kept), "the same result");
    // Above: traced once, drawn at each input copy.
    let mut s = Session::new();
    let r = place(&mut s, &png(100, 100, logo(100, 100)), 0.0, 100.0, 100.0);
    add(&mut s, r, "gridArray", json!({"rows": 1, "cols": 3, "gapX": 20}));
    add(&mut s, r, "imageTrace", json!({"preset": "sixColors"}));
    let i = info(&mut s, r);
    let paths = i["stats"]["paths"].as_u64().unwrap() as usize;
    assert_eq!(i["items"].as_u64().unwrap() as usize, 3 * paths, "{i}");
    assert_eq!(i["stats"]["tracedPx"], json!({"w": 100, "h": 100}), "one image traced, not the row of three");
    let world = figcraft_render::export_bounds(s.doc().unwrap(), r).unwrap();
    assert_eq!(pixel(&s, &[r], world, 265.0, 50.0), [255, 0, 0, 255]);
}

#[test]
fn exports_and_the_fig_bake_write_the_traced_paths() {
    let mut s = Session::new();
    let r = place(&mut s, &png(100, 50, logo(100, 50)), 0.0, 100.0, 50.0);
    add(&mut s, r, "imageTrace", json!({"preset": "threeColors"}));
    let out = ex(&mut s, "file.export", json!({"ids": [r], "format": "svg"}));
    let svg = String::from_utf8(decode(&out)).unwrap();
    assert!(svg.contains("fill=\"#ff0000\"") && svg.contains("fill=\"#0000ff\""), "{svg}");
    assert!(!svg.contains("<image"), "the trace, not the image: {svg}");
    let out = ex(&mut s, "file.export", json!({"ids": [r], "format": "pdf"}));
    let pdf = decode(&out);
    assert!(pdf.starts_with(b"%PDF") && !pdf.windows(7).any(|w| w == b"/Image "), "no image in the pdf");
    let (baked, _) = crate::picabia_bake::baked(s.doc().unwrap());
    let page = baked.first_page().unwrap();
    let g = baked.get(baked.get(page).unwrap().children[0]).unwrap();
    assert!(matches!(g.kind, NodeKind::Group), "{:?}", g.kind);
    assert!(g.name.ends_with("(traced)"));
    assert!(g.children.iter().all(|c| matches!(baked.get(*c).unwrap().kind, NodeKind::Vector { .. })));
}

#[test]
fn the_row_survives_files_copies_and_undo() {
    let mut s = Session::new();
    let r = place(&mut s, &png(60, 60, logo(60, 60)), 0.0, 60.0, 60.0);
    add(&mut s, r, "imageTrace", json!({"preset": "sixColors"}));
    let want = info(&mut s, r)["colours"].clone();
    ex(&mut s, "modifier.set", json!({"ids": [r], "index": 0, "props": {"colors": 2}}));
    assert_ne!(info(&mut s, r)["colours"], want);
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(info(&mut s, r)["colours"], want, "undo of set retraces as before");
    let dup = id_of(&ex(&mut s, "edit.duplicate", json!({"ids": [r]}))["ids"][0]).unwrap();
    assert_eq!(info(&mut s, dup)["colours"], want);
    ex(&mut s, "edit.copy", json!({"ids": [r]}));
    let pasted = id_of(&ex(&mut s, "edit.paste", json!({}))["ids"][0]).unwrap();
    assert_eq!(info(&mut s, pasted)["colours"], want);
    let dir = std::env::temp_dir().join(format!("picabia-trace-row-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("row.figcraft");
    ex(&mut s, "file.saveAs", json!({"path": path}));
    let mut t = Session::new();
    ex(&mut t, "file.open", json!({"path": path}));
    assert_eq!(info(&mut t, r)["colours"], want);
    let _ = std::fs::remove_dir_all(&dir);
    ex(&mut s, "edit.undo", json!({}));
    ex(&mut s, "edit.undo", json!({}));
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(info(&mut s, r)["modifier"], Value::Null, "undo of add");
}

#[test]
fn junk_params_on_an_image_never_panic() {
    let junk = [
        json!({"preset": [1], "colors": 1e308, "threshold": -1e308, "noise": "NaN", "ignoreColor": 5, "mode": {"x": 1}}),
        json!({"preset": "outline", "fills": false, "strokes": false, "strokeWidth": 1e308, "palette": "documentLibrary"}),
        json!({"mode": "logo", "colors": 1e9, "paths": -5, "corners": 1e308, "ignoreWhite": true, "ignoreColor": "#zzzzzz"}),
        json!({"mode": "color", "palette": "automatic", "colorDetail": "inf", "noise": 0, "method": 9, "strokes": true}),
        json!({"mode": "grayscale", "colors": 0, "strokeWidth": -1, "snapCurvesToLines": "x"}),
    ];
    for j in junk {
        let mut s = Session::new();
        let r = place(&mut s, &png(30, 20, photo(30, 20)), 0.0, 30.0, 20.0);
        let _ = s.execute("modifier.add", &json!({"ids": [r], "kind": "imageTrace", "props": j}));
        if let Err(crate::EngineError::Internal { cmd, msg }) = s.execute("modifier.resultInfo", &json!({"id": r})) {
            panic!("{cmd} panicked on {j}: {msg}");
        }
    }
}

/// The release-build timing: a 4000 × 3000 synthetic photo, end to end through
/// `modifier.resultInfo` (`cargo test --release -p figcraft-engine trace_timing -- --ignored
/// --nocapture`).
#[test]
#[ignore = "timing: run in release with --nocapture"]
fn trace_timing() {
    for preset in ["autoColor", "sixteenColors", "highFidelityPhoto", "blackAndWhiteLogo"] {
        let mut s = Session::new();
        let bytes = png(4000, 3000, photo(4000, 3000));
        let r = place(&mut s, &bytes, 0.0, 400.0, 300.0);
        add(&mut s, r, "imageTrace", json!({"preset": preset}));
        let t = std::time::Instant::now();
        let raster = trace::raster(s.doc().unwrap(), r, 10.0, figcraft_vcbridge::trace::MAX_TRACE_PIXELS).unwrap();
        println!("  render alone {} x {}: {} ms", raster.rgba.width, raster.rgba.height, t.elapsed().as_millis());
        let t = std::time::Instant::now();
        let i = info(&mut s, r);
        let ms = t.elapsed().as_millis();
        println!(
            "{preset}: {ms} ms, {} paths, {} anchors, {} colours, traced {}",
            i["stats"]["paths"], i["stats"]["anchors"], i["stats"]["colours"], i["stats"]["tracedPx"]
        );
    }
}
