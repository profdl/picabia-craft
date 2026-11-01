//! PICABIA: the stencil laser dialogs: the request data and the dialog's fields map to the
//! command's params (kerf only once typed, no limit as 0 × 0, the test card's material), junk keeps
//! the defaults, the UI request arms open the window, Escape closes it, the summary reads the
//! command's result and asks again a moment after a change, the stencil row's buttons and the
//! Modifiers submenu ask for the dialogs.

use egui::{Event, Modifiers, PointerButton, Pos2};
use figcraft_doc::NodeId;
use figcraft_engine::{Session, UiRequest};
use serde_json::{Value, json};

use super::{EXPORT, Kind, LaserDialog, Summary, TEST_CARD, hex, parse_hex, request, saved_text, summary_of, written};
use crate::layers_drag_tests::Ui;
use crate::live_tests::{flat, texts};

fn id(v: &Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

/// A session with a rectangle carrying a stencil row, selected.
fn stencil() -> (Ui, NodeId) {
    let mut s = Session::new();
    let r = id(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 40, "height": 40})).unwrap());
    s.execute("modifier.add", &json!({"ids": [r], "kind": "stencil"})).unwrap();
    s.execute("select.set", &json!({"ids": [r]})).unwrap();
    let mut u = Ui::new(s);
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    (u, r)
}

fn drawn(u: &mut Ui) -> Vec<String> {
    // A window's first frame only measures it.
    u.frame(vec![]);
    texts(&flat(&u.frame_output(vec![]).shapes)).into_iter().map(|(t, _, _)| t).collect()
}

fn ask(u: &mut Ui, kind: &str, data: Value) {
    u.app.session.ui_requests.push(UiRequest { kind: kind.into(), data });
    u.app.handle_ui_requests();
}

#[test]
fn the_request_and_the_fields_map_to_the_commands_params() {
    let data = json!({"id": 7, "params": {
        "kerfMm": 0.15, "bedWidthMm": 900, "bedHeightMm": 600, "overlapMm": 12,
        "cutColour": "#FF0000", "engraveColour": "#112233", "scoreColour": "#0000ff", "label": false
    }});
    let mut d = LaserDialog::from_request(Kind::Export, &data);
    assert_eq!(d.id, Some(NodeId(7)));
    assert_eq!((d.kerf_mm, d.bed_w_mm, d.bed_h_mm, d.overlap_mm, d.label, d.no_limit), (0.15, 900.0, 600.0, 12.0, false, false));
    // The kerf the command chose isn't sent back until one is typed (the material's follows).
    let p = d.params();
    assert_eq!(
        p,
        json!({"id": 7, "bedWidthMm": 900.0, "bedHeightMm": 600.0, "overlapMm": 12.0, "label": false,
            "cutColour": "#ff0000", "engraveColour": "#112233", "scoreColour": "#0000ff"})
    );
    d.kerf_mm = 0.2;
    d.kerf_set = true;
    d.no_limit = true;
    let p = d.params();
    assert_eq!((p["kerfMm"].clone(), p["bedWidthMm"].clone(), p["bedHeightMm"].clone()), (json!(0.2), json!(0.0), json!(0.0)));
    // A bed of 0 × 0 is no limit.
    let d = LaserDialog::from_request(Kind::Export, &json!({"params": {"bedWidthMm": 0, "bedHeightMm": 0}}));
    assert!(d.no_limit && d.id.is_none() && d.params().get("id").is_none());
    // The test card: material and colours, no bed, overlap, label or layer.
    let mut d = LaserDialog::from_request(Kind::TestCard, &json!({"id": 3, "material": "film", "kerfMm": 0.05}));
    assert_eq!((d.material.as_str(), d.kerf_mm), ("film", 0.05));
    let p = d.params();
    assert_eq!(p, json!({"material": "film", "cutColour": "#ff0000", "engraveColour": "#000000", "scoreColour": "#0000ff"}));
    d.kerf_set = true;
    assert_eq!(d.params()["kerfMm"], json!(0.05));
}

#[test]
fn junk_request_data_keeps_the_defaults() {
    for junk in [
        json!(null),
        json!(5),
        json!("x"),
        json!([1, 2]),
        json!({"params": 3}),
        json!({"id": "a", "params": {
            "kerfMm": "wide", "bedWidthMm": -1e300, "overlapMm": f64::MAX, "cutColour": "red", "engraveColour": "#12345",
            "scoreColour": "#gg0000", "label": 1, "material": "steel"
        }}),
    ] {
        let d = LaserDialog::from_request(Kind::TestCard, &junk);
        assert_eq!((d.kerf_mm, d.material.as_str(), d.label), (0.1, "paper", true), "{junk}");
        assert_eq!((hex(d.cut), hex(d.engrave), hex(d.score)), ("#ff0000".into(), "#000000".into(), "#0000ff".into()), "{junk}");
        assert!(d.overlap_mm <= 200.0 && d.bed_w_mm >= 10.0, "{junk}");
    }
    assert_eq!(parse_hex(" #A0b1C2 "), Some([0xa0, 0xb1, 0xc2]));
    assert_eq!(parse_hex("a0b1c2"), Some([0xa0, 0xb1, 0xc2]));
    for bad in ["", "#", "#12345", "#1234567", "#éé0000", "#zz0000"] {
        assert_eq!(parse_hex(bad), None, "{bad}");
    }
}

#[test]
fn the_summary_reads_the_commands_result() {
    let r = json!({"files": [
        {"name": "Rose 1.svg", "path": "/tmp/x/Rose 1.svg", "sheet": "1 of 2", "wMm": 600, "hMm": 400, "contours": 132, "loosePieces": 0},
        {"name": "Rose 2.svg", "path": "/tmp/x/Rose 2.svg", "sheet": "2 of 2", "wMm": 610.5, "hMm": 400, "contours": 3, "loosePieces": 4}
    ], "sheets": 2, "warnings": ["sheet 2 of 2: 4 pieces would fall out; move the split or add ties"], "params": {"kerfMm": 0.12}});
    let Summary::Ready { sheets, warnings } = summary_of(&r) else { panic!("ready") };
    assert_eq!(sheets.len(), 2);
    assert_eq!((sheets[1].name.as_str(), sheets[1].w_mm, sheets[1].contours, sheets[1].loose), ("2 of 2", 610.5, 3, 4));
    assert_eq!(warnings.len(), 1);
    assert_eq!(written(&r), vec!["Rose 1.svg".to_string(), "Rose 2.svg".to_string()]);
    assert_eq!(saved_text(&written(&r)), "Saved 2 sheets: Rose 1.svg, Rose 2.svg");
    assert_eq!(saved_text(&["a.svg".into()]), "Saved a.svg");
    // The kerf the command used shows until one is typed.
    let mut d = LaserDialog::new(Kind::Export);
    d.take_result(Ok(r.clone()));
    assert_eq!(d.kerf_mm, 0.12);
    d.kerf_mm = 0.3;
    d.kerf_set = true;
    d.take_result(Ok(r));
    assert_eq!(d.kerf_mm, 0.3);
    d.take_result(Err("nothing to cut".into()));
    assert_eq!(d.summary, Summary::Failed("nothing to cut".into()));
    assert!(matches!(summary_of(&json!(null)), Summary::Ready { sheets, warnings } if sheets.is_empty() && warnings.is_empty()));
}

#[test]
fn the_ui_requests_open_the_dialog_and_escape_closes_it() {
    let (mut u, r) = stencil();
    ask(&mut u, "dialog.stencilExport", json!({"id": r, "params": {"kerfMm": 0.1, "bedWidthMm": 600, "bedHeightMm": 400}}));
    let d = u.app.ui.picabia_laser.as_ref().expect("the export dialog is open");
    assert_eq!((d.kind, d.id), (Kind::Export, Some(r)));
    let shown = drawn(&mut u);
    for label in ["Export for laser", "Kerf", "Bed", "Overlap", "Cut", "Engrave", "Score", "Engrave a label", "Export…", "Cancel"] {
        assert!(shown.iter().any(|t| t == label), "{label:?} is drawn: {shown:?}");
    }
    // The summary asked the command on open.
    assert_ne!(u.app.ui.picabia_laser.as_ref().unwrap().summary, Summary::Pending);
    u.frame(vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    assert!(u.app.ui.picabia_laser.is_none(), "Escape closes it");
    assert_eq!(u.app.session.selection(), vec![r], "and leaves the selection alone");

    ask(&mut u, "dialog.stencilTestCard", json!({"params": {"material": "card"}}));
    let d = u.app.ui.picabia_laser.as_ref().expect("the test card dialog is open");
    assert_eq!((d.kind, d.material.as_str()), (Kind::TestCard, "card"));
    let shown = drawn(&mut u);
    for label in ["Laser test card", "Material", "Cut from card", "Kerf"] {
        assert!(shown.iter().any(|t| t == label), "{label:?} is drawn: {shown:?}");
    }
    assert!(!shown.iter().any(|t| t == "Overlap" || t == "Engrave a label"), "no bed settings on the card");
}

#[test]
fn the_summary_asks_again_a_moment_after_a_change() {
    let (mut u, r) = stencil();
    ask(&mut u, "dialog.stencilExport", json!({"id": r}));
    u.frame(vec![]);
    let first = u.app.ui.picabia_laser.as_ref().unwrap().ran.clone();
    assert_eq!(first, Some(u.app.ui.picabia_laser.as_ref().unwrap().params()));
    let now = u.ctx.input(|i| i.time);
    let d = u.app.ui.picabia_laser.as_mut().unwrap();
    d.overlap_mm = 20.0;
    d.changed(now);
    u.frame(vec![]);
    assert_eq!(u.app.ui.picabia_laser.as_ref().unwrap().ran, first, "not at once");
    for _ in 0..30 {
        u.frame(vec![]);
    }
    let d = u.app.ui.picabia_laser.as_ref().unwrap();
    assert_eq!(d.ran.as_ref().map(|p| p["overlapMm"].clone()), Some(json!(20.0)), "then with the new settings");
}

/// Scroll the Design panel until the button labelled `label` is in view; its centre.
fn button(u: &mut Ui, label: &str) -> Pos2 {
    for _ in 0..30 {
        let out = u.frame_output(vec![]);
        let at =
            out.platform_output.accesskit_update.unwrap().nodes.into_iter().find(|(_, n)| n.label() == Some(label)).and_then(|(_, n)| n.bounds());
        let b = at.unwrap_or_else(|| panic!("{label:?} is shown"));
        let p = Pos2::new(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
        if p.y > 60.0 && p.y < 860.0 {
            return p;
        }
        let wheel = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            source: egui::MouseWheelSource::Unknown,
            delta: egui::vec2(0.0, if p.y >= 860.0 { -100.0 } else { 100.0 }),
            phase: egui::TouchPhase::Move,
            modifiers: Default::default(),
        };
        u.frame(vec![Event::PointerMoved(Pos2::new(1300.0, 500.0)), wheel]);
    }
    panic!("{label:?} never came into view");
}

fn click(u: &mut Ui, p: Pos2) {
    for _ in 0..3 {
        u.frame(vec![Event::PointerMoved(p)]);
    }
    for pressed in [true, false] {
        u.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    }
    u.frame(vec![]);
}

#[test]
fn the_stencil_rows_buttons_ask_for_the_dialogs() {
    assert_eq!(request(Kind::Export, Some(NodeId(4))), (EXPORT, json!({"id": 4})));
    assert_eq!(request(Kind::TestCard, Some(NodeId(4))), (TEST_CARD, json!({})));
    for (label, kind, cmd) in [("Export for laser…", Kind::Export, EXPORT), ("Test card…", Kind::TestCard, TEST_CARD)] {
        let (mut u, r) = stencil();
        let p = button(&mut u, label);
        click(&mut u, p);
        if figcraft_engine::find_command(cmd).is_some() {
            let d = u.app.ui.picabia_laser.as_ref().unwrap_or_else(|| panic!("{label} opens the dialog"));
            assert_eq!(d.kind, kind);
            if kind == Kind::Export {
                assert_eq!(d.id, Some(r));
            }
        } else {
            // Before the engine has the command: the click ran it (and it was refused).
            let e = u.app.ui.last_error.clone().unwrap_or_default();
            assert!(e.contains(cmd), "{label} ran {cmd}: {e:?}");
        }
    }
}

#[test]
fn the_modifiers_submenu_offers_laser_items_on_a_stencil() {
    let (mut u, r) = stencil();
    let items = crate::picabia_ui::items(&u.app);
    let laser: Vec<_> = items.iter().filter(|i| i.1 == EXPORT || i.1 == TEST_CARD).map(|i| (i.1, i.2.clone(), i.3)).collect();
    assert_eq!(laser, vec![(EXPORT, json!({"id": r}), true), (TEST_CARD, json!({}), true)]);
    // A layer without a stencil row doesn't offer them.
    let other = id(&u.app.session.execute("shape.rectangle", &json!({"x": 100, "y": 0, "width": 40, "height": 40})).unwrap());
    u.app.session.execute("select.set", &json!({"ids": [other]})).unwrap();
    assert!(!crate::picabia_ui::items(&u.app).iter().any(|i| i.1 == EXPORT || i.1 == TEST_CARD));
    // Nor does a disabled stencil.
    u.app.session.execute("select.set", &json!({"ids": [r]})).unwrap();
    u.app.session.execute("modifier.toggle", &json!({"index": 0, "enabled": false})).unwrap();
    assert!(!crate::picabia_ui::items(&u.app).iter().any(|i| i.1 == EXPORT));
}
