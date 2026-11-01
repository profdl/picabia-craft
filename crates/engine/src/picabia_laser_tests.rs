//! PICABIA: laser export of stencil layers (`modifier.stencilExport`) and the laser test card
//! (`modifier.stencilTestCard`): files and their true size, base64 data, dialog requests, bed
//! splits, tile output, kerf from the material, remembered settings, errors.

use base64::Engine as _;
use figcraft_doc::NodeId;
use serde_json::{Value, json};

use crate::params::id_of;
use crate::picabia_laser::sheet_path;
use crate::{EngineError, Prefs, Session};

const BLACK: &str = "000000";
const WHITE: &str = "FFFFFF";

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn rect(s: &mut Session, x: f64, y: f64, w: f64, hex: &str) -> NodeId {
    let id = id_of(&ex(s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": w}))["id"]).unwrap();
    ex(s, "node.set", json!({"ids": [id], "props": {"fills": [{"type": "solid", "color": format!("#{hex}")}]}}));
    id
}

/// A black and a white 100-unit square side by side made a stencil (default params: 300 mm wide,
/// so 1.5 mm per unit; the sheet 330 × 180 mm with its 15 mm margin; the white square is cut).
fn stencil(s: &mut Session, props: Value) -> NodeId {
    let a = rect(s, 0.0, 0.0, 100.0, BLACK);
    let b = rect(s, 100.0, 0.0, 100.0, WHITE);
    id_of(&ex(s, "modifier.stencilFromSelection", json!({"ids": [a, b], "props": props}))["id"]).unwrap()
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("picabia-laser-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn decode(f: &Value) -> String {
    String::from_utf8(base64::engine::general_purpose::STANDARD.decode(f["data"].as_str().unwrap()).unwrap()).unwrap()
}

fn close(a: &Value, b: f64) -> bool {
    a.as_f64().is_some_and(|a| (a - b).abs() < 1e-6)
}

#[test]
fn export_writes_a_true_size_sheet_with_its_cut_layer() {
    let mut s = Session::new();
    let g = stencil(&mut s, json!({}));
    let dir = tmp("one");
    let path = dir.join("rose.svg");
    let r = ex(&mut s, "modifier.stencilExport", json!({"id": g, "path": path.to_string_lossy()}));
    assert_eq!(r["sheets"], 1, "{r}");
    let f = &r["files"][0];
    assert_eq!(f["path"], json!(path.to_string_lossy()));
    assert_eq!(f["name"], "rose.svg");
    assert_eq!(f["sheet"], "1 of 1");
    // The sheet plus the kerf (half each side) of paper's 0.10 mm.
    assert!(close(&f["wMm"], 330.1) && close(&f["hMm"], 180.1), "{f}");
    assert!(f["contours"].as_u64().unwrap() >= 6, "the white square, four registration holes and the outline: {f}");
    assert_eq!(f["loosePieces"], 0);
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(svg.contains("width=\"330.1mm\" height=\"180.1mm\" viewBox=\"0 0 330.1 180.1\""), "{svg}");
    assert!(svg.contains("inkscape:label=\"Cut\"") && svg.contains("stroke=\"#ff0000\""), "{svg}");
    assert!(svg.contains("inkscape:label=\"Engrave\""), "the label is engraved: {svg}");
    assert_eq!(r["params"]["kerfMm"], 0.1);
    assert_eq!(r["params"]["cutColour"], "#ff0000");
    // Remembered for next time.
    assert_eq!(s.prefs.picabia_laser, r["params"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn data_mode_returns_the_files_as_base64() {
    let mut s = Session::new();
    let g = stencil(&mut s, json!({}));
    let r = ex(&mut s, "modifier.stencilExport", json!({"id": g, "data": true, "cutColour": "#00ff00", "label": false}));
    let f = &r["files"][0];
    assert_eq!(f["name"], "Stencil laser.svg");
    assert!(f.get("path").is_none());
    let svg = decode(f);
    assert!(svg.starts_with("<?xml") && svg.contains("width=\"330.1mm\""), "{svg}");
    assert!(svg.contains("stroke=\"#00ff00\"") && !svg.contains("inkscape:label=\"Engrave\""), "{svg}");
    assert_eq!(r["params"]["label"], false);
    assert!(s.ui_requests.iter().all(|u| !u.kind.starts_with("dialog.stencil")));
}

#[test]
fn without_path_or_data_it_only_asks_for_the_dialog() {
    let mut s = Session::new();
    let g = stencil(&mut s, json!({"material": "card"}));
    s.ui_requests.clear();
    let r = ex(&mut s, "modifier.stencilExport", json!({}));
    assert_eq!(r, json!({"dialog": true}));
    let u = s.ui_requests.iter().find(|u| u.kind == "dialog.stencilExport").unwrap();
    assert_eq!(id_of(&u.data["id"]), Some(g));
    assert_eq!(u.data["params"]["kerfMm"], 0.15, "card's kerf: {}", u.data);
    assert_eq!(u.data["params"]["bedWidthMm"], 600.0);
    assert_eq!(s.prefs.picabia_laser, Value::Null, "nothing exported, nothing remembered");
    // The test card too.
    s.ui_requests.clear();
    let r = ex(&mut s, "modifier.stencilTestCard", json!({}));
    assert_eq!(r, json!({"dialog": true}));
    let u = s.ui_requests.iter().find(|u| u.kind == "dialog.stencilTestCard").unwrap();
    assert_eq!(u.data["material"], "card", "the selected stencil's material");
    assert_eq!(u.data["params"]["kerfMm"], 0.15);
}

#[test]
fn kerf_follows_the_material_then_the_params() {
    let mut s = Session::new();
    let g = stencil(&mut s, json!({"material": "film"}));
    let kerf = |s: &mut Session, extra: Value| {
        let mut p = json!({"id": g, "data": true});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        ex(s, "modifier.stencilExport", p)["params"]["kerfMm"].as_f64().unwrap()
    };
    assert_eq!(kerf(&mut s, json!({})), 0.08);
    assert_eq!(kerf(&mut s, json!({"kerfMm": 0.2})), 0.2);
    // The material's kerf wins over the remembered one...
    assert_eq!(kerf(&mut s, json!({})), 0.08);
    ex(&mut s, "modifier.set", json!({"ids": [g], "index": 0, "props": {"material": "card"}}));
    assert_eq!(kerf(&mut s, json!({})), 0.15);
    // ...but a custom material keeps the remembered kerf.
    assert_eq!(kerf(&mut s, json!({"kerfMm": 0.25})), 0.25);
    ex(&mut s, "modifier.set", json!({"ids": [g], "index": 0, "props": {"material": "custom"}}));
    assert_eq!(kerf(&mut s, json!({})), 0.25);
    // Out of range is clamped, junk keeps the rest.
    assert_eq!(kerf(&mut s, json!({"kerfMm": 9})), 2.0);
    assert_eq!(kerf(&mut s, json!({"kerfMm": "x"})), 2.0);
}

#[test]
fn the_other_remembered_settings_carry_over() {
    let mut s = Session::new();
    let g = stencil(&mut s, json!({}));
    ex(&mut s, "modifier.stencilExport", json!({"id": g, "data": true, "bedWidthMm": 0, "bedHeightMm": 0, "scoreColour": "#123456", "overlapMm": 4}));
    let r = ex(&mut s, "modifier.stencilExport", json!({"id": g, "data": true}));
    assert_eq!(r["params"]["bedWidthMm"], 0.0);
    assert_eq!(r["params"]["scoreColour"], "#123456");
    assert_eq!(r["params"]["overlapMm"], 4.0);
    // Prefs written and read back keep them.
    let back: Prefs = serde_json::from_str(&serde_json::to_string(&s.prefs).unwrap()).unwrap();
    assert_eq!(back.picabia_laser, s.prefs.picabia_laser);
}

#[test]
fn a_sheet_bigger_than_the_bed_splits_into_numbered_files() {
    let mut s = Session::new();
    let g = stencil(&mut s, json!({}));
    let dir = tmp("split");
    let path = dir.join("big.svg");
    let r = ex(&mut s, "modifier.stencilExport", json!({"id": g, "path": path.to_string_lossy(), "bedWidthMm": 200, "bedHeightMm": 200}));
    let n = r["sheets"].as_u64().unwrap() as usize;
    assert!(n >= 2, "{r}");
    let files = r["files"].as_array().unwrap();
    assert_eq!(files.len(), n);
    for (k, f) in files.iter().enumerate() {
        let want = dir.join(format!("big {}.svg", k + 1));
        assert_eq!(f["path"], json!(want.to_string_lossy()), "{f}");
        assert_eq!(f["sheet"], format!("{} of {n}", k + 1));
        assert!(f["wMm"].as_f64().unwrap() <= 200.0 + 1e-6 && f["hMm"].as_f64().unwrap() <= 200.0 + 1e-6, "{f}");
        let svg = std::fs::read_to_string(&want).unwrap();
        assert!(svg.contains("inkscape:label=\"Cut\""), "{svg}");
    }
    assert!(!path.exists(), "a split job writes only the numbered files");
    // Data mode names them the same way.
    let r = ex(&mut s, "modifier.stencilExport", json!({"id": g, "data": true}));
    assert_eq!(r["files"][0]["name"], "Stencil laser 1.svg");
    assert_eq!(r["files"][1]["name"], "Stencil laser 2.svg");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tile_output_exports_the_tile() {
    let mut s = Session::new();
    let ids: Vec<NodeId> = [(0.0, 0.0, 20.0, BLACK), (14.0, 14.0, 6.0, WHITE), (18.0, 18.0, 2.0, BLACK)]
        .into_iter()
        .map(|(x, y, w, hex)| rect(&mut s, x, y, w, hex))
        .collect();
    let g = id_of(&ex(&mut s, "modifier.stencilFromSelection", json!({"ids": ids, "props": {"output": "tile", "tieSeed": 99}}))["id"]).unwrap();
    ex(&mut s, "modifier.add", json!({"ids": [g], "kind": "gridArray", "props": {"rows": 3, "cols": 3, "gapX": 0, "gapY": 0}, "index": 0}));
    let r = ex(&mut s, "modifier.stencilExport", json!({"id": g, "data": true}));
    assert_eq!(r["sheets"], 1, "{r}");
    // The 130 mm tile (not the 330 mm sheet) plus paper's kerf.
    assert!(close(&r["files"][0]["wMm"], 130.1) && close(&r["files"][0]["hMm"], 130.1), "{r}");
    assert!(decode(&r["files"][0]).contains("width=\"130.1mm\""));
}

#[test]
fn what_cannot_be_exported_is_an_error() {
    let mut s = Session::new();
    let plain = rect(&mut s, 0.0, 0.0, 50.0, BLACK);
    let e = s.execute("modifier.stencilExport", &json!({"id": plain, "data": true})).unwrap_err();
    assert!(matches!(e, EngineError::Disabled(..)), "{e}");
    s.set_selection(vec![]);
    assert!(s.execute("modifier.stencilExport", &json!({"data": true})).is_err(), "nothing selected");
    // All paper, no registration holes: nothing to cut.
    let solid = rect(&mut s, 300.0, 0.0, 50.0, BLACK);
    let g = id_of(&ex(&mut s, "modifier.stencilFromSelection", json!({"ids": [solid], "props": {"hoshi": false}}))["id"]).unwrap();
    let e = s.execute("modifier.stencilExport", &json!({"id": g, "data": true})).unwrap_err();
    assert!(e.to_string().contains("nothing to cut"), "{e}");
    // Too many sheets for the bed.
    let g = stencil(&mut s, json!({}));
    let e = s.execute("modifier.stencilExport", &json!({"id": g, "data": true, "bedWidthMm": 10, "bedHeightMm": 10})).unwrap_err();
    assert!(matches!(e, EngineError::Disabled(..)), "{e}");
    assert_eq!(s.prefs.picabia_laser, Value::Null, "a refused export remembers nothing");
    // Junk outputs.
    for p in [json!({"id": g, "path": 5}), json!({"id": g, "data": "yes please"}), json!({"id": g, "path": ""})] {
        assert!(matches!(s.execute("modifier.stencilExport", &p), Err(EngineError::BadParams { .. })), "{p}");
    }
    assert!(s.execute("modifier.stencilExport", &json!({"id": g, "data": true, "colour": 1})).is_err(), "unknown params are refused");
}

#[test]
fn the_test_card_for_each_material() {
    let mut s = Session::new();
    for (m, kerf) in [("paper", 0.1), ("film", 0.08), ("card", 0.15)] {
        let r = ex(&mut s, "modifier.stencilTestCard", json!({"material": m, "data": true}));
        assert_eq!(r["material"], m);
        assert_eq!(r["params"]["kerfMm"], kerf, "{r}");
        let f = &r["files"][0];
        assert_eq!(f["name"], format!("Test card {m}.svg"));
        assert_eq!(f["sheet"], "test card");
        assert!((f["wMm"].as_f64().unwrap() - figcraft_stencil::CARD_W_MM).abs() < 1.0, "{f}");
        assert!((f["hMm"].as_f64().unwrap() - figcraft_stencil::CARD_H_MM).abs() < 1.0, "{f}");
        let svg = decode(f);
        assert!(svg.contains("inkscape:label=\"Engrave\"") && svg.contains("inkscape:label=\"Cut\""), "{svg}");
        assert!(f["contours"].as_u64().unwrap() > 20, "{f}");
    }
    // Without a stencil selected the card is paper's; a typed kerf wins.
    s.set_selection(vec![]);
    let r = ex(&mut s, "modifier.stencilTestCard", json!({"data": true, "kerfMm": 0.12}));
    assert_eq!(r["material"], "paper");
    assert_eq!(r["params"]["kerfMm"], 0.12);
    // Custom needs a stencil's limits.
    let e = s.execute("modifier.stencilTestCard", &json!({"material": "custom", "data": true})).unwrap_err();
    assert!(matches!(e, EngineError::Disabled(..)), "{e}");
    assert!(matches!(s.execute("modifier.stencilTestCard", &json!({"material": "wood", "data": true})), Err(EngineError::BadParams { .. })));
    // A custom stencil selected: its limits, the remembered kerf (0.12 from the last card).
    let g = stencil(&mut s, json!({"material": "custom", "minBridgeMm": 0.7}));
    let r = ex(&mut s, "modifier.stencilTestCard", json!({"id": g, "data": true}));
    assert_eq!(r["material"], "custom");
    assert_eq!(r["params"]["kerfMm"], 0.12);
    // To a file.
    let dir = tmp("card");
    let path = dir.join("card.svg");
    let r = ex(&mut s, "modifier.stencilTestCard", json!({"material": "film", "path": path.to_string_lossy(), "engraveColour": "#0000aa"}));
    assert_eq!(r["files"][0]["name"], "card.svg");
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(svg.contains("width=\"120mm\"") || svg.contains("width=\"120."), "{svg}");
    assert!(svg.contains("stroke=\"#0000aa\""));
    assert_eq!(s.prefs.picabia_laser["engraveColour"], "#0000aa");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prefs_round_trip_with_and_without_laser_settings() {
    let none = Prefs::default();
    let text = serde_json::to_string(&none).unwrap();
    assert!(!text.contains("picabia_laser"), "null isn't written");
    assert_eq!(serde_json::from_str::<Prefs>(&text).unwrap(), none);
    assert_eq!(serde_json::from_str::<Prefs>("{}").unwrap().picabia_laser, Value::Null, "older files");
    let with = Prefs { picabia_laser: json!({"kerfMm": 0.2, "label": false}), ..Prefs::default() };
    assert_eq!(serde_json::from_str::<Prefs>(&serde_json::to_string(&with).unwrap()).unwrap(), with);
}

#[test]
fn sheet_paths_gain_their_number_before_the_extension() {
    assert_eq!(sheet_path("/a/rose.svg", 1, 1), "/a/rose.svg");
    assert_eq!(sheet_path("/a/rose.svg", 2, 3), "/a/rose 2.svg");
    assert_eq!(sheet_path("/a/rose", 1, 2), "/a/rose 1");
    assert_eq!(sheet_path("/a.b/rose", 1, 2), "/a.b/rose 1");
    assert_eq!(sheet_path("rose.tar.svg", 3, 3), "rose.tar 3.svg");
    assert_eq!(sheet_path(".svg", 1, 2), ".svg 1");
}
