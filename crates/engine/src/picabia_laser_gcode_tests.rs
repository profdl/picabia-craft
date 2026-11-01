//! PICABIA: `modifier.stencilGcode`, the test card and the power ladder as G-code, the machine
//! profile (`laser.profile`, `Prefs::picabia_machine`) and the G-code file bounds `laser.start`
//! checks.

use base64::Engine as _;
use figcraft_doc::NodeId;
use serde_json::{Value, json};

use crate::params::id_of;
use crate::picabia_laser_gcode::{MachineProfile, gcode_bounds};
use crate::{EngineError, Prefs, Session};

const BLACK: &str = "000000";
const WHITE: &str = "FFFFFF";

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn rect(s: &mut Session, x: f64, y: f64, w: f64, h: f64, hex: &str) -> NodeId {
    let id = id_of(&ex(s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": h}))["id"]).unwrap();
    ex(s, "node.set", json!({"ids": [id], "props": {"fills": [{"type": "solid", "color": format!("#{hex}")}]}}));
    id
}

/// A black and a white 100-unit square made a stencil (300 mm wide: a 330 × 180 mm sheet).
pub(crate) fn stencil(s: &mut Session, props: Value) -> NodeId {
    let a = rect(s, 0.0, 0.0, 100.0, 100.0, BLACK);
    let b = rect(s, 100.0, 0.0, 100.0, 100.0, WHITE);
    id_of(&ex(s, "modifier.stencilFromSelection", json!({"ids": [a, b], "props": props}))["id"]).unwrap()
}

/// The P12 sample: a 200 × 120 mm design (1 mm a unit) with a 15 mm margin, the white hole in the
/// top half of the right side, no registration holes.
fn p12_sample(s: &mut Session) -> NodeId {
    let ids = vec![rect(s, 0.0, 0.0, 100.0, 120.0, BLACK), rect(s, 100.0, 60.0, 100.0, 60.0, BLACK), rect(s, 100.0, 0.0, 100.0, 60.0, WHITE)];
    id_of(
        &ex(s, "modifier.stencilFromSelection", json!({"ids": ids, "props": {"widthMm": 200, "marginMm": 15, "hoshi": false, "material": "paper"}}))
            ["id"],
    )
    .unwrap()
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("picabia-gcode-{}-{name}", std::process::id()));
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

/// The X and Y of a motion line (`G0`/`G1`).
fn xy(line: &str) -> Option<(f64, f64)> {
    if !(line.starts_with("G0 ") || line.starts_with("G1 ")) {
        return None;
    }
    let word = |c: char| line.split_whitespace().find_map(|w| w.strip_prefix(c)).and_then(|v| v.parse::<f64>().ok());
    Some((word('X')?, word('Y')?))
}

#[test]
fn the_p12_sample_exports_true_size_flipped_and_beam_off_at_both_ends() {
    let mut s = Session::new();
    let g = p12_sample(&mut s);
    let dir = tmp("p12");
    let path = dir.join("p12.gcode");
    let r = ex(&mut s, "modifier.stencilGcode", json!({"id": g, "path": path.to_string_lossy(), "kerfMm": 0.1, "label": false}));
    assert_eq!(r["sheets"], 1, "{r}");
    let f = &r["files"][0];
    assert!(close(&f["wMm"], 230.1) && close(&f["hMm"], 150.1), "the sheet plus the kerf: {f}");
    let b = &f["boundsMm"];
    assert!(close(&b["x0"], 0.0) && close(&b["y0"], 0.0) && close(&b["x1"], 230.1) && close(&b["y1"], 150.1), "{b}");
    assert_eq!(f["name"], "p12.gcode");
    assert!(f["lines"].as_u64().unwrap() > 10 && f["estimatedSec"].as_f64().unwrap() > 0.0, "{f}");
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with(';')).collect();
    // Beam off before the first motion and after the last.
    let first_motion = lines.iter().position(|l| xy(l).is_some()).unwrap();
    assert!(lines[..first_motion].contains(&"M5"), "{lines:?}");
    assert!(lines[first_motion].starts_with("G0"), "the first motion is a travel (beam off in laser mode): {}", lines[first_motion]);
    let last_motion = lines.iter().rposition(|l| xy(l).is_some()).unwrap();
    assert!(lines[last_motion].starts_with("G0"), "the last motion is the park, a travel: {}", lines[last_motion]);
    assert_eq!(lines.last(), Some(&"M5"));
    let last_burn = lines.iter().rposition(|l| l.starts_with("G1")).unwrap();
    assert!(lines[last_burn..last_motion].contains(&"M5"), "beam off before the park");
    // Y is flipped: the white hole is in the design's top half (sheet y 15..75), so in machine
    // coordinates (y up from the sheet's bottom-left) its edges are near Y 75 and 135.
    let cut = &lines[lines.iter().position(|l| *l == "M4 S1000").unwrap()..];
    let hole: Vec<(f64, f64)> = cut.iter().filter_map(|l| xy(l)).filter(|&(x, y)| x > 110.0 && x < 220.0 && y > 5.0 && y < 145.0).collect();
    assert!(!hole.is_empty());
    assert!(hole.iter().all(|&(_, y)| y > 74.0), "{hole:?}");
    assert!(hole.iter().any(|&(_, y)| (y - 135.0).abs() < 0.2) && hole.iter().any(|&(_, y)| (y - 75.1).abs() < 0.2), "{hole:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn data_dry_run_and_dialog() {
    let mut s = Session::new();
    let g = stencil(&mut s, json!({}));
    let r = ex(&mut s, "modifier.stencilGcode", json!({"id": g, "data": true}));
    let f = &r["files"][0];
    assert_eq!(f["name"], "Stencil laser.gcode");
    let text = decode(f);
    assert!(text.starts_with(';') && text.contains("\nG21\nG90\nM5\n") && text.ends_with("M5\n"), "{text}");
    assert_eq!(r["material"], "paper");
    assert_eq!(r["gcode"]["sMax"], 1000.0, "the profile's S max: {r}");
    assert_eq!(r["params"]["bedWidthMm"], 584.2, "the machine's bed: {r}");
    assert!(s.prefs.picabia_machine["materials"]["paper"]["cut"].is_object(), "remembered: {}", s.prefs.picabia_machine);
    // A dry run describes and remembers nothing.
    let mut t = Session::new();
    let g2 = stencil(&mut t, json!({}));
    let r = ex(&mut t, "modifier.stencilGcode", json!({"id": g2, "dryRun": true, "cut": {"passes": 5}}));
    assert!(r["files"][0].get("data").is_none() && r["files"][0]["lines"].as_u64().unwrap() > 0, "{r}");
    assert_eq!(r["gcode"]["cut"]["passes"], 5);
    assert_eq!(t.prefs.picabia_machine, Value::Null);
    // Without path, data or dryRun: the dialog, with the format.
    t.ui_requests.clear();
    assert_eq!(ex(&mut t, "modifier.stencilGcode", json!({})), json!({"dialog": true}));
    let u = t.ui_requests.iter().find(|u| u.kind == "dialog.stencilExport").unwrap();
    assert_eq!(u.data["format"], "gcode");
    assert_eq!(id_of(&u.data["id"]), Some(g2));
    assert_eq!(u.data["material"], "paper");
    assert!(u.data["gcode"]["cut"]["speedMmMin"].is_number() && u.data["params"]["kerfMm"] == 0.1, "{}", u.data);
    assert_eq!(t.prefs.picabia_machine, Value::Null);
}

#[test]
fn several_sheets_are_numbered_files() {
    let mut s = Session::new();
    let g = stencil(&mut s, json!({}));
    let dir = tmp("split");
    let path = dir.join("big.gcode");
    let r = ex(&mut s, "modifier.stencilGcode", json!({"id": g, "path": path.to_string_lossy(), "bedWidthMm": 200, "bedHeightMm": 200}));
    let n = r["sheets"].as_u64().unwrap() as usize;
    assert!(n >= 2, "{r}");
    for (k, f) in r["files"].as_array().unwrap().iter().enumerate() {
        let want = dir.join(format!("big {}.gcode", k + 1));
        assert_eq!(f["path"], json!(want.to_string_lossy()));
        assert_eq!(f["sheet"], format!("{} of {n}", k + 1));
        assert!(f["boundsMm"]["x1"].as_f64().unwrap() <= 200.0 + 1e-6, "{f}");
        assert!(std::fs::read_to_string(&want).unwrap().contains("G21"));
    }
    assert!(!path.exists());
    let r = ex(&mut s, "modifier.stencilGcode", json!({"id": g, "data": true, "bedWidthMm": 200, "bedHeightMm": 200}));
    assert_eq!(r["files"][1]["name"], "Stencil laser 2.gcode");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn op_settings_are_remembered_per_material() {
    let mut s = Session::new();
    let g = stencil(&mut s, json!({"material": "film"}));
    let r = ex(&mut s, "modifier.stencilGcode", json!({"id": g, "data": true}));
    assert_eq!(r["material"], "film");
    assert_eq!(r["gcode"]["cut"]["speedMmMin"], 200.0, "film's starting cut: {r}");
    ex(&mut s, "modifier.stencilGcode", json!({"id": g, "data": true, "cut": {"speedMmMin": 250, "passes": 4}, "air": true}));
    let r = ex(&mut s, "modifier.stencilGcode", json!({"id": g, "data": true}));
    assert_eq!(r["gcode"]["cut"]["speedMmMin"], 250.0);
    assert_eq!(r["gcode"]["cut"]["passes"], 4);
    assert_eq!(r["gcode"]["air"], false, "air is the profile's, not remembered per export");
    assert!(decode(&r["files"][0]).contains("F250"));
    // Paper's are its own.
    let r = ex(&mut s, "modifier.stencilGcode", json!({"id": g, "data": true, "material": "paper"}));
    assert_eq!(r["gcode"]["cut"]["speedMmMin"], 300.0);
    assert_eq!(s.prefs.picabia_machine["materials"]["film"]["cut"]["speedMmMin"], 250.0);
    // The profile's S max and air reach the G-code.
    ex(&mut s, "laser.profile", json!({"set": {"sMax": 255, "air": true}}));
    let r = ex(&mut s, "modifier.stencilGcode", json!({"id": g, "data": true}));
    let text = decode(&r["files"][0]);
    assert!(text.contains("M4 S255") && text.contains("\nM8\n") && text.contains("\nM9\n"), "{text}");
    // Prefs round trip.
    let back: Prefs = serde_json::from_str(&serde_json::to_string(&s.prefs).unwrap()).unwrap();
    assert_eq!(back.picabia_machine, s.prefs.picabia_machine);
}

#[test]
fn errors_not_panics() {
    let mut s = Session::new();
    let plain = rect(&mut s, 0.0, 0.0, 50.0, 50.0, BLACK);
    assert!(matches!(s.execute("modifier.stencilGcode", &json!({"id": plain, "data": true})), Err(EngineError::Disabled(..))));
    let g = stencil(&mut s, json!({}));
    for p in [
        json!({"id": g, "path": 5}),
        json!({"id": g, "data": "yes"}),
        json!({"id": g, "data": true, "material": "wood"}),
        json!({"id": g, "data": true, "cutColour": "#ff0000"}),
    ] {
        assert!(matches!(s.execute("modifier.stencilGcode", &p), Err(EngineError::BadParams { .. })), "{p}");
    }
    // Everything off: refused with the writer's reason.
    let e = s
        .execute(
            "modifier.stencilGcode",
            &json!({"id": g, "data": true, "label": false, "cut": {"enabled": false}, "score": {"enabled": false}, "engrave": {"enabled": false}}),
        )
        .unwrap_err();
    assert!(matches!(e, EngineError::Disabled(..)), "{e}");
    assert_eq!(s.prefs.picabia_machine, Value::Null, "a refused export remembers nothing");
    // Junk settings are clamped, not refused.
    let r = ex(
        &mut s,
        "modifier.stencilGcode",
        json!({"id": g, "data": true, "sMax": 1e308, "originXMm": -1e9, "cut": {"powerPct": 900, "passes": -3, "speedMmMin": "x"}}),
    );
    assert_eq!(r["gcode"]["sMax"], 100000.0);
    assert_eq!(r["gcode"]["originXMm"], -5000.0);
    assert_eq!(r["gcode"]["cut"]["powerPct"], 100.0);
}

#[test]
fn the_test_card_and_the_power_ladder_as_gcode() {
    let mut s = Session::new();
    let r = ex(&mut s, "modifier.stencilTestCard", json!({"format": "gcode", "material": "card", "data": true}));
    assert_eq!(r["kind"], "card");
    let f = &r["files"][0];
    assert_eq!(f["name"], "Test card card.gcode");
    assert!((f["wMm"].as_f64().unwrap() - figcraft_stencil::CARD_W_MM).abs() < 1.0, "{f}");
    let text = decode(f);
    assert!(text.contains("M4 S1000") && text.ends_with("M5\n"), "card's cut at 100%");
    assert_eq!(r["gcode"]["cut"]["speedMmMin"], 150.0, "card's starting cut");
    assert!(s.prefs.picabia_machine["materials"]["card"].is_object(), "the card's settings are remembered");
}

#[test]
fn the_power_ladder_as_gcode() {
    let mut s = Session::new();
    // The ladder: G-code by default, each square at its own power.
    let r = ex(
        &mut s,
        "modifier.stencilTestCard",
        json!({"kind": "powerLadder", "data": true, "powersPct": [10, 50], "speedsMmMin": [100, 1000], "passes": 2}),
    );
    assert_eq!(r["kind"], "powerLadder");
    let f = &r["files"][0];
    assert_eq!(f["name"], "Power ladder paper.gcode");
    assert_eq!(f["sheet"], "power ladder");
    let text = decode(f);
    for want in ["M4 S100", "M4 S500", "F100", "F1000"] {
        assert!(text.contains(want), "{want}: {text}");
    }
    assert_eq!(r["ladder"]["powersPct"], json!([10.0, 50.0]));
    // The ladder can't be an SVG.
    let e = s.execute("modifier.stencilTestCard", &json!({"kind": "powerLadder", "format": "svg", "data": true})).unwrap_err();
    assert!(matches!(e, EngineError::Disabled(..)) && e.to_string().contains("G-code"), "{e}");
    for p in [json!({"kind": "ladder", "data": true}), json!({"format": "pdf", "data": true}), json!({"format": 1, "data": true})] {
        assert!(matches!(s.execute("modifier.stencilTestCard", &p), Err(EngineError::BadParams { .. })), "{p}");
    }
    // A custom material's ladder needs no stencil (its card does).
    s.set_selection(vec![]);
    assert!(s.execute("modifier.stencilTestCard", &json!({"kind": "powerLadder", "material": "custom", "data": true})).is_ok());
    assert!(s.execute("modifier.stencilTestCard", &json!({"format": "gcode", "material": "custom", "data": true})).is_err());
    // The dialog carries the format and kind.
    s.ui_requests.clear();
    assert_eq!(ex(&mut s, "modifier.stencilTestCard", json!({"kind": "powerLadder"})), json!({"dialog": true}));
    let u = s.ui_requests.iter().find(|u| u.kind == "dialog.stencilTestCard").unwrap();
    assert_eq!((u.data["format"].as_str(), u.data["kind"].as_str()), (Some("gcode"), Some("powerLadder")));
    assert!(u.data["gcode"]["engrave"].is_object());
    // The plain card's dialog is as before.
    s.ui_requests.clear();
    ex(&mut s, "modifier.stencilTestCard", json!({}));
    let u = s.ui_requests.iter().find(|u| u.kind == "dialog.stencilTestCard").unwrap();
    assert!(u.data.get("format").is_none(), "{}", u.data);
    // To a file.
    let dir = tmp("card");
    let path = dir.join("ladder.nc");
    let r = ex(&mut s, "modifier.stencilTestCard", json!({"kind": "powerLadder", "path": path.to_string_lossy()}));
    assert_eq!(r["files"][0]["name"], "ladder.nc");
    assert!(std::fs::read_to_string(&path).unwrap().contains("power ladder"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_machine_profile() {
    let mut s = Session::new();
    let r = ex(&mut s, "laser.profile", json!({}));
    let p = &r["profile"];
    assert_eq!((p["bedWidthMm"].as_f64(), p["bedHeightMm"].as_f64()), (Some(584.2), Some(1219.2)));
    assert_eq!(
        (p["sMax"].as_f64(), p["rapidMmMin"].as_f64(), p["air"].as_bool(), p["checked"].as_bool()),
        (Some(1000.0), Some(3000.0), Some(false), Some(false))
    );
    assert_eq!(r["sMaxUsed"], 1000.0);
    assert_eq!(r["sMaxMachine"], Value::Null);
    assert_eq!(r["materialDefaults"]["film"]["cut"]["passes"], 3);
    assert_eq!(s.prefs.picabia_machine, Value::Null, "reading writes nothing");
    let r = ex(
        &mut s,
        "laser.profile",
        json!({"set": {"name": "Shop laser", "bedWidthMm": 1e9, "bedHeightMm": "x", "rapidMmMin": 6000, "materials": {"paper": {"cut": {"passes": 3}}, "wood": {"cut": {}}}}}),
    );
    let p = &r["profile"];
    assert_eq!(p["name"], "Shop laser");
    assert_eq!(p["bedWidthMm"], 5000.0);
    assert_eq!(p["bedHeightMm"], 1219.2);
    assert_eq!(p["materials"]["paper"]["cut"]["passes"], 3);
    assert_eq!(p["materials"]["paper"]["cut"]["speedMmMin"], 300.0, "overlaid on paper's starting values");
    assert!(p["materials"].get("wood").is_none());
    assert_eq!(s.prefs.picabia_machine["rapidMmMin"], 6000.0);
    assert_eq!(r["bedMm"], json!({"w": 5000.0, "h": 1219.2}));
    for bad in [json!({"set": 5}), json!({"set": "x"}), json!({"sett": {}})] {
        assert!(matches!(s.execute("laser.profile", &bad), Err(EngineError::BadParams { .. })), "{bad}");
    }
    assert_eq!(MachineProfile::from_json(&json!([1, 2])), MachineProfile::default());
    assert_eq!(MachineProfile::from_json(&MachineProfile::default().to_json()), MachineProfile::default());
    let none = Prefs::default();
    assert!(!serde_json::to_string(&none).unwrap().contains("picabia_machine"), "null isn't written");
    assert_eq!(serde_json::from_str::<Prefs>("{}").unwrap().picabia_machine, Value::Null);
}

#[test]
fn gcode_file_bounds() {
    let b = gcode_bounds("G21 G90\nG0 X10 Y5\nG1 X20 Y30 F100\nG0 X0 Y0\n").unwrap();
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (0.0, 0.0, 20.0, 30.0));
    // Relative moves and inches.
    let b = gcode_bounds("G0 X10 Y10\nG91\nG1 X5 Y-20\n").unwrap();
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (10.0, -10.0, 15.0, 10.0), "where the head starts isn't the file's");
    let b = gcode_bounds("G20\nG0 X1 Y2\n").unwrap();
    assert_eq!((b.x1, b.y1), (25.4, 50.8));
    // Arcs count as their whole circle; comments and $ lines are skipped.
    let b = gcode_bounds("G0 X10 Y10 ; start\n(a comment X999)\n$H\nG2 X20 Y10 I5 J0\n").unwrap();
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (10.0, 5.0, 20.0, 15.0));
    for (file, why) in [
        ("G53 G0 X0 Y0", "G53"),
        ("G92 X0 Y0", "G92"),
        ("G28", "G28"),
        ("G55\nG0 X1 Y1", "G55"),
        ("G10 L20 P1 X0", "G10"),
        ("M5\nG21", "no moves"),
        ("G0 X1..2 Y0", "line 1"),
        ("G0 X1 Y\u{e9}", "line 1"),
        ("G0 X10000000 Y0", "out of range"),
        ("G91\nG1 X5 Y5", "relative"),
        ("G0 X5\nG91 G1 Y5", "relative"),
    ] {
        let e = gcode_bounds(file).unwrap_err();
        assert!(e.contains(why), "{file}: {e}");
    }
    // Junk never panics.
    let mut seed = 7u64;
    for _ in 0..2000 {
        let line: String = (0..40)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                b"GXYIJR0123456789.-+ ;()$%\n\x7f"[(seed >> 33) as usize % 27] as char
            })
            .collect();
        let _ = gcode_bounds(&line);
    }
}

/// The lines between `M4 S{s}` and the next `M5`: one operation's moves.
fn op_moves(text: &str, s: u32) -> Vec<(f64, f64)> {
    let start = format!("M4 S{s}");
    text.lines().skip_while(|l| *l != start).skip(1).take_while(|l| *l != "M5").filter_map(xy).collect()
}

#[test]
fn any_layers_export_as_gcode() {
    let mut s = Session::new();
    let f = id_of(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 192, "height": 96}))["id"]).unwrap();
    let r = id_of(&ex(&mut s, "shape.rectangle", json!({"x": 24, "y": 24, "width": 48, "height": 48, "parent": f}))["id"]).unwrap();
    let inner = id_of(&ex(&mut s, "shape.rectangle", json!({"x": 36, "y": 36, "width": 24, "height": 24, "parent": f}))["id"]).unwrap();
    let line = id_of(&ex(&mut s, "shape.line", json!({"x": 96, "y": 48, "width": 72, "parent": f}))["id"]).unwrap();
    ex(&mut s, "node.set", json!({"ids": [line], "props": {"strokes": [{"type": "solid", "color": "#000000"}], "strokeWeight": 1}}));
    let r1 = ex(&mut s, "laser.gcode", json!({"ids": [f], "data": true}));
    let file = &r1["files"][0];
    assert_eq!(file["name"], "Frame 1 laser.gcode".replace("Frame 1", &s.doc().unwrap().get(f).unwrap().name));
    // 192 × 96 units at a CSS pixel each: 50.8 × 25.4 mm; the frame itself (the page) isn't cut.
    assert!(close(&file["wMm"], 50.8) && close(&file["hMm"], 25.4), "{file}");
    assert!((r1["mmPerUnit"].as_f64().unwrap() - 25.4 / 96.0).abs() < 1e-12);
    let text = decode(file);
    // The cut: the inner square first, then the outer one (inner contours first); Y up.
    let cut = op_moves(&text, 1000);
    let inner_first = cut.iter().position(|&(x, _)| (x - 9.525).abs() < 0.01 || (x - 15.875).abs() < 0.01).unwrap();
    let outer_first = cut.iter().position(|&(x, _)| (x - 6.35).abs() < 0.01).unwrap();
    assert!(inner_first < outer_first, "{cut:?}");
    assert!(cut.iter().all(|&(x, y)| x > 6.0 && x < 19.1 && y > 6.0 && y < 19.1), "no frame outline: {cut:?}");
    // The line: open, with the score settings (paper: 30% of 1000), at half height.
    let score = op_moves(&text, 300);
    assert!(score.len() >= 2 && score.iter().all(|&(_, y)| (y - 12.7).abs() < 0.01), "{score:?}");
    let xs: Vec<f64> = score.iter().map(|p| p.0).collect();
    assert!(xs.iter().any(|x| (x - 25.4).abs() < 0.01) && xs.iter().any(|x| (x - 44.45).abs() < 0.01), "{xs:?}");
    // Scaled to a width; the rectangles alone (no frame: their own bounds).
    let r2 = ex(&mut s, "laser.gcode", json!({"ids": [r, inner], "dryRun": true, "widthMm": 100}));
    assert!(close(&r2["files"][0]["wMm"], 100.0) && close(&r2["files"][0]["hMm"], 100.0), "{r2}");
    assert_eq!(r2["files"][0]["name"], "Design laser.gcode");
    // Text is cut as glyph outlines.
    let t = id_of(&ex(&mut s, "text.create", json!({"text": "Hi", "x": 300, "y": 0, "props": {"fontSize": 40}}))["id"]).unwrap();
    // Without fonts (CI without craft-fonts) the text lays out no glyphs: nothing to cut.
    let glyphs = figcraft_text::layout_node(s.doc().unwrap().node(t).unwrap()).map_or(0, |l| l.0.glyphs.len());
    match s.execute("laser.gcode", &json!({"ids": [t], "data": true})) {
        Ok(r3) => assert!(glyphs > 0 && op_moves(&decode(&r3["files"][0]), 1000).len() > 8, "{r3}"),
        Err(e) => assert!(glyphs == 0 && matches!(e, EngineError::Disabled(..)), "{e}"),
    }
    // A stencil goes the stencil's way; stencil-only params are refused for a design.
    let g = stencil(&mut s, json!({}));
    let r4 = ex(&mut s, "laser.gcode", json!({"ids": [g], "dryRun": true, "kerfMm": 0.2}));
    assert!(close(&r4["files"][0]["wMm"], 330.2), "{r4}");
    assert!(matches!(s.execute("laser.gcode", &json!({"ids": [r], "data": true, "kerfMm": 0.2})), Err(EngineError::BadParams { .. })));
    // The dialog; nothing selected; junk scales.
    s.ui_requests.clear();
    assert_eq!(ex(&mut s, "laser.gcode", json!({"ids": [r]})), json!({"dialog": true}));
    assert!(s.ui_requests.iter().any(|u| u.kind == "dialog.laserGcode" && u.data["ids"] == json!([r])));
    s.set_selection(vec![]);
    assert!(matches!(s.execute("laser.gcode", &json!({"data": true})), Err(EngineError::Disabled(..))));
    for p in [
        json!({"ids": [r], "data": true, "mmPerUnit": 0}),
        json!({"ids": [r], "data": true, "widthMm": "wide"}),
        json!({"ids": [r], "data": true, "mmPerUnit": 1e9}),
    ] {
        assert!(matches!(s.execute("laser.gcode", &p), Err(EngineError::BadParams { .. })), "{p}");
    }
    // A hidden layer isn't cut.
    ex(&mut s, "node.set", json!({"ids": [inner], "props": {"visible": false}}));
    let r5 = ex(&mut s, "laser.gcode", json!({"ids": [f], "data": true}));
    assert!(op_moves(&decode(&r5["files"][0]), 1000).iter().all(|&(x, _)| (x - 9.525).abs() > 0.01));
}
