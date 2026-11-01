//! PICABIA: modifier stack commands, and the stack (plugin data) surviving save/reopen,
//! copy/paste, duplicate, Alt-drag, collaboration and branch merges.

use figcraft_collab::{Mode, Replica, diff};
use figcraft_doc::NodeId;
use figcraft_modifiers::{ModifierKind, PLUGIN_KEY};
use serde_json::{Value, json};

use crate::params::id_of;
use crate::tools::{PointerEvent, PointerKind};
use crate::{EngineError, Session};

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn err(s: &mut Session, cmd: &str, p: Value) -> String {
    match s.execute(cmd, &p) {
        Err(e) => e.to_string(),
        Ok(v) => panic!("{cmd} {p} ran: {v}"),
    }
}

fn rect(s: &mut Session, x: f64) -> NodeId {
    id_of(&ex(s, "shape.rectangle", json!({"x": x, "y": 0, "width": 100, "height": 100}))["id"]).unwrap()
}

fn data(s: &Session, id: NodeId) -> Option<String> {
    s.doc().unwrap().node(id).unwrap().plugin_data.get(PLUGIN_KEY).cloned()
}

fn kinds(s: &mut Session, id: NodeId) -> Vec<String> {
    let l = ex(s, "modifier.list", json!({"id": id}));
    l["modifiers"].as_array().unwrap().iter().map(|m| m["kind"].as_str().unwrap().to_string()).collect()
}

/// A rectangle with a 5-copy linear array.
fn arrayed() -> (Session, NodeId) {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    let out = ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "linearArray", "props": {"count": 5, "offsetX": 120}}));
    assert_eq!(out["modifier"], "m1");
    (s, r)
}

#[test]
fn add_set_toggle_move_remove_with_undo() {
    let (mut s, r) = arrayed();
    let l = ex(&mut s, "modifier.list", json!({"id": r}));
    assert_eq!(l["instances"], 5);
    assert_eq!(l["modifiers"][0]["count"], 5);
    assert_eq!(l["modifiers"][0]["label"], "Linear array");
    assert_eq!(s.undo_labels(1), ["Add modifier"]);
    // The selection is the default target.
    ex(&mut s, "select.set", json!({"ids": [r]}));
    ex(&mut s, "modifier.add", json!({"kind": "mirror", "index": 0}));
    assert_eq!(kinds(&mut s, r), ["mirror", "linearArray"]);
    ex(&mut s, "modifier.move", json!({"from": 0, "to": 1}));
    assert_eq!(kinds(&mut s, r), ["linearArray", "mirror"]);
    ex(&mut s, "modifier.set", json!({"modifier": "m1", "props": {"count": 8, "offsetY": "10"}}));
    // 8 copies, then the mirror (keepOriginal) adds the reflected block: 16.
    assert_eq!(ex(&mut s, "modifier.list", json!({}))["instances"], 16);
    assert_eq!(s.undo_labels(1), ["Set modifier"]);
    ex(&mut s, "modifier.toggle", json!({"index": 0}));
    let l = ex(&mut s, "modifier.list", json!({}));
    assert_eq!((l["modifiers"][0]["enabled"].clone(), l["instances"].as_u64() == Some(16)), (json!(false), false), "{l}");
    ex(&mut s, "modifier.toggle", json!({"index": 0, "enabled": true}));
    ex(&mut s, "modifier.remove", json!({"modifier": "m2"}));
    assert_eq!(kinds(&mut s, r), ["linearArray"]);
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(kinds(&mut s, r), ["linearArray", "mirror"]);
    ex(&mut s, "modifier.remove_all", json!({}));
    assert_eq!(data(&s, r), None, "an empty stack leaves no data");
    ex(&mut s, "edit.undo", json!({}));
    assert!(data(&s, r).is_some());
}

#[test]
fn bad_params_are_refused_and_roll_back() {
    let (mut s, r) = arrayed();
    let before = data(&s, r);
    for (cmd, p, want) in [
        ("modifier.add", json!({"ids": [r], "kind": "spiralArray"}), "unknown kind"),
        ("modifier.add", json!({"ids": [r]}), "`kind` required"),
        ("modifier.add", json!({"ids": [r], "kind": "linearArray", "props": {"offsetX": "NaN"}}), "finite"),
        ("modifier.add", json!({"ids": [r], "kind": "linearArray", "props": {"offsetX": "-inf"}}), "finite"),
        ("modifier.add", json!({"ids": [r], "kind": "linearArray", "props": {"nope": 1}}), "no param `nope`"),
        ("modifier.add", json!({"ids": [r], "kind": "linearArray", "props": 5}), "object"),
        ("modifier.set", json!({"ids": [r], "modifier": "m1", "props": {"count": "Infinity"}}), "finite"),
        ("modifier.set", json!({"ids": [r], "modifier": "m9", "props": {}}), "no modifier `m9`"),
        ("modifier.set", json!({"ids": [r], "index": 3, "props": {}}), "no modifier at index 3"),
        ("modifier.set", json!({"ids": [r], "modifier": "m1"}), "`props` required"),
        ("modifier.remove", json!({"ids": [r]}), "required"),
        ("modifier.remove", json!({"ids": [r], "modifier": "m1", "index": 0}), "not both"),
        ("modifier.move", json!({"ids": [r], "from": 0, "to": 4}), "1 modifiers"),
        ("modifier.toggle", json!({"ids": [r], "index": 0, "enabled": "maybe"}), "true or false"),
        ("modifier.add", json!({"ids": [r], "kind": "linearArray", "bogus": 1}), "unknown parameter `bogus`"),
    ] {
        let e = err(&mut s, cmd, p.clone());
        assert!(e.contains(want), "{cmd} {p}: {e}");
        assert_eq!(data(&s, r), before, "{cmd} {p} changed nothing");
    }
    let page = s.ds().unwrap().page;
    assert!(err(&mut s, "modifier.add", json!({"ids": [page], "kind": "mirror"})).contains("page"));
}

#[test]
fn damaged_data_is_kept_until_cleared() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"pluginData": {PLUGIN_KEY: "{broken"}}}));
    let l = ex(&mut s, "modifier.list", json!({"id": r}));
    assert!(l["error"].as_str().unwrap().contains("JSON"), "{l}");
    assert!(err(&mut s, "modifier.add", json!({"ids": [r], "kind": "mirror"})).contains("remove_all"));
    assert_eq!(data(&s, r).as_deref(), Some("{broken"));
    ex(&mut s, "modifier.remove_all", json!({"ids": [r]}));
    assert_eq!(data(&s, r), None);
}

#[test]
fn the_stack_is_capped_at_64() {
    let mut s = Session::new();
    let r = rect(&mut s, 0.0);
    let steps: Vec<Value> = (0..64).map(|_| json!({"command": "modifier.add", "params": {"ids": [r], "kind": "mirror"}})).collect();
    ex(&mut s, "command.batch", json!({"commands": steps}));
    assert!(err(&mut s, "modifier.add", json!({"ids": [r], "kind": "mirror"})).contains("at most 64"));
}

#[test]
fn copy_and_paste_modifiers() {
    let (mut s, r) = arrayed();
    let other = rect(&mut s, 500.0);
    ex(&mut s, "modifier.add", json!({"ids": [other], "kind": "randomize"}));
    assert!(matches!(s.execute("modifier.paste", &json!({"ids": [other]})), Err(EngineError::Disabled(..))));
    assert_eq!(ex(&mut s, "modifier.copy", json!({"id": r}))["copied"], 1);
    assert_eq!(ex(&mut s, "modifier.paste", json!({"ids": [other]}))["pasted"], 1);
    let l = ex(&mut s, "modifier.list", json!({"id": other}));
    assert_eq!(kinds(&mut s, other), ["randomize", "linearArray"]);
    assert_eq!(l["modifiers"][1]["id"], "m2", "a fresh id where m1 was taken");
    ex(&mut s, "modifier.paste", json!({"ids": [other], "replace": true}));
    assert_eq!(kinds(&mut s, other), ["linearArray"]);
    // One row by id; copying layers afterwards keeps the copied rows.
    ex(&mut s, "modifier.copy", json!({"id": other, "modifier": "m1"}));
    ex(&mut s, "edit.copy", json!({"ids": [r]}));
    assert!(s.execute("modifier.paste", &json!({"ids": [r]})).is_ok());
}

#[test]
fn the_stack_survives_layer_copy_paste_duplicate_and_alt_drag() {
    let (mut s, r) = arrayed();
    let want = data(&s, r).unwrap();
    ex(&mut s, "edit.copy", json!({"ids": [r]}));
    let pasted = id_of(&ex(&mut s, "edit.paste", json!({}))["ids"][0]).unwrap();
    assert_ne!(pasted, r);
    assert_eq!(data(&s, pasted).as_deref(), Some(want.as_str()), "copy/paste");
    let dup = id_of(&ex(&mut s, "edit.duplicate", json!({"ids": [r]}))["ids"][0]).unwrap();
    assert_eq!(data(&s, dup).as_deref(), Some(want.as_str()), "duplicate");
    // Alt-drag the original (at 0,0 100×100) to make a copy.
    ex(&mut s, "select.set", json!({"ids": [r]}));
    let ev = |kind, x: f64| PointerEvent { alt: true, ..PointerEvent::at(kind, x, 50.0, 1.0) };
    s.pointer(&ev(PointerKind::Down, 50.0)).unwrap();
    s.pointer(&ev(PointerKind::Move, 150.0)).unwrap();
    s.pointer(&ev(PointerKind::Move, 300.0)).unwrap();
    s.pointer(&ev(PointerKind::Up, 300.0)).unwrap();
    let copy = s.selection()[0];
    assert_ne!(copy, r);
    assert_eq!(data(&s, copy).as_deref(), Some(want.as_str()), "Alt-drag");
}

#[test]
fn the_stack_survives_save_and_reopen() {
    let (mut s, r) = arrayed();
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "circularArray", "props": {"radius": 40}}));
    // Fields and kinds this build doesn't know are kept too.
    let mut v: Value = serde_json::from_str(&data(&s, r).unwrap()).unwrap();
    v["stack"].as_array_mut().unwrap().push(json!({"id": "m9", "enabled": true, "kind": "futureKind", "x": [1]}));
    v["stack"][0]["wobble"] = json!(3);
    let src = v.to_string();
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"pluginData": {PLUGIN_KEY: src}}}));
    let dir = std::env::temp_dir().join(format!("picabia-modifiers-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("stack.figcraft");
    ex(&mut s, "file.saveAs", json!({"path": path}));
    let mut t = Session::new();
    ex(&mut t, "file.open", json!({"path": path}));
    let back = data(&t, r).expect("the stack reopens");
    assert_eq!(figcraft_modifiers::parse(&back).unwrap(), figcraft_modifiers::parse(&src).unwrap());
    let l = ex(&mut t, "modifier.list", json!({"id": r}));
    assert_eq!(l["modifiers"][2]["kind"], "futureKind");
    assert_eq!(l["modifiers"][0]["wobble"], 3);
    assert!(l["warnings"].as_array().unwrap().iter().any(|w| w["modifier"] == "m9"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_stack_syncs_through_collab_changes() {
    let mut s = Session::with_document(crate::collab_tests::base());
    let replica = Replica::new(s.doc().unwrap().clone());
    let r = rect(&mut s, 900.0);
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "linearArray", "props": {"count": 4}}));
    let d = diff(&replica, s.doc().unwrap()).unwrap();
    let mut other = replica.clone();
    other.apply(&d.changes, Mode::Strict).unwrap();
    assert_eq!(other.document().node(r).unwrap().plugin_data.get(PLUGIN_KEY), data(&s, r).as_ref());
    // A later edit of the stack alone travels as a plugin-data change.
    let replica = d.replica;
    ex(&mut s, "modifier.set", json!({"ids": [r], "index": 0, "props": {"count": 6}}));
    let d = diff(&replica, s.doc().unwrap()).unwrap();
    let mut other = replica.clone();
    other.apply(&d.changes, Mode::Strict).unwrap();
    let synced = other.document().node(r).unwrap().plugin_data.get(PLUGIN_KEY).cloned().unwrap();
    let stack = figcraft_modifiers::parse(&synced).unwrap();
    assert!(matches!(&stack.modifiers[0].kind, ModifierKind::LinearArray(p) if p.count == 6));
}

#[test]
fn the_stack_merges_from_a_branch() {
    let mut s = Session::with_document(crate::collab_tests::base());
    let r = rect(&mut s, 900.0);
    let b = ex(&mut s, "branch.create", json!({"name": "Arrays"}));
    let branch = b["id"].as_str().unwrap().to_string();
    ex(&mut s, "modifier.add", json!({"ids": [r], "kind": "linearArray", "props": {"count": 7}}));
    let want = data(&s, r);
    ex(&mut s, "doc.switch", json!({"index": 0}));
    assert_eq!(data(&s, r), None, "main has no stack yet");
    let merged = ex(&mut s, "branch.merge", json!({"id": branch}));
    assert_eq!(merged["merged"], true);
    assert_eq!(data(&s, r), want);
}

/// Modifier-specific junk on every `modifier.*` command (the general sweep in `tests.rs` covers
/// them with its own junk too): nothing panics.
#[test]
fn modifier_commands_survive_junk() {
    let junk = [
        json!({"kind": "linearArray", "props": {"count": 1e308, "offsetX": 1e308, "rotateEach": -1e308, "scaleEach": 1e308}}),
        json!({"kind": "linearArray", "props": {"count": "NaN", "offsetX": "inf"}}),
        json!({"kind": [1], "props": [1], "index": -1, "modifier": 5}),
        json!({"kind": "\u{0}", "props": {"\u{0}": null}, "index": 1e308, "modifier": "\u{0}"}),
        json!({"from": -1, "to": 1e308, "enabled": [1], "replace": {"x": 1}}),
        json!({"from": "x", "to": null, "enabled": null, "modifier": null, "index": null}),
        json!({"ids": [{"x": 1}], "kind": "mirror", "props": {"axis": 7, "keepOriginal": "x"}}),
        json!({"kind": "gridArray", "props": {"rows": -5, "cols": 1e9, "gapX": "1e400"}}),
        json!({"kind": "randomize", "props": {"seed": -1, "scale": "x".repeat(10_000)}}),
        json!({"kind": "copyToShape", "props": {"target": "\u{0}", "mode": 7, "count": 1e308, "seed": -1, "scale": "inf"}}),
        json!({"kind": "copyToShape", "props": {"target": 1e308, "mode": "populate", "count": 1000}}),
        json!({"kind": "copyToShape", "props": {"target": "x".repeat(10_000), "align": "x"}, "modifier": "m1", "index": 0}),
        json!({"kind": "copyToShape", "props": {"target": -3, "mode": "points"}, "id": -1}),
        json!({"kind": "stencil", "props": {"threshold": 1e308, "widthMm": -1e308, "marginMm": "inf", "toleranceMm": "NaN", "minCutAreaMm2": 1e308}}),
        json!({"kind": "stencil", "props": {"colourMode": 7, "preview": [1], "paperColour": "\u{0}", "cutColours": ["#zzz", 5, null, "x".repeat(10_000)]}}),
        json!({"kind": "stencil", "props": {"cutColours": vec!["#abcdef"; 5_000], "invert": "x", "clothColour": {"a": 1}}, "index": 0}),
        json!({"kind": "imageTrace", "props": {"preset": [1], "colors": 1e308, "threshold": -1e308, "noise": "NaN", "ignoreColor": 5, "mode": {"x": 1}}}),
        json!({"kind": "imageTrace", "props": {"preset": "outline", "fills": false, "strokes": false, "strokeWidth": 1e308, "palette": "documentLibrary"}}),
        json!({"kind": "pathfinder", "props": {"op": 7, "precision": -1e308, "outlineWidth": "inf"}, "upTo": 1e308}),
        json!({"kind": "pathfinder", "props": {"op": "outline", "precision": 1e-308}, "index": 0, "upTo": 0}),
        // Laser export and test card settings (data mode: nothing is written).
        json!({"data": true, "kerfMm": 1e308, "bedWidthMm": -1e308, "bedHeightMm": "NaN", "overlapMm": 1e308, "cutColour": 5, "label": "x", "material": [1]}),
        json!({"data": true, "bedWidthMm": 10, "bedHeightMm": 10, "overlapMm": 200, "material": "custom", "scoreColour": "\u{0}"}),
        json!({"data": "x", "path": {"x": 1}, "kerfMm": "inf", "engraveColour": "#zzzzzz", "material": "\u{0}"}),
    ];
    let ids: Vec<&str> = crate::all_commands().iter().map(|c| c.id).filter(|i| i.starts_with("modifier.")).collect();
    assert_eq!(ids.len(), 18, "{ids:?}");
    for cmd in ids {
        for j in &junk {
            for damaged in [false, true] {
                let (mut s, r) = arrayed();
                if damaged {
                    let _ = s.execute("node.set", &json!({"ids": [r], "props": {"pluginData": {PLUGIN_KEY: "{\"stack\": [1]}"}}}));
                }
                let _ = s.execute("modifier.copy", &json!({"id": r}));
                let mut p = j.clone();
                if p.get("ids").is_none() {
                    p["ids"] = json!([r]);
                }
                if let Err(EngineError::Internal { cmd, msg }) = s.execute(cmd, &p) {
                    panic!("{cmd} panicked on {p}: {msg}");
                }
            }
        }
    }
}

#[test]
fn every_modifier_command_documents_its_params() {
    for (id, names) in [
        ("modifier.add", vec!["ids", "kind", "props", "index"]),
        ("modifier.remove", vec!["ids", "modifier", "index"]),
        ("modifier.move", vec!["ids", "from", "to"]),
        ("modifier.set", vec!["ids", "modifier", "index", "props"]),
        ("modifier.toggle", vec!["ids", "modifier", "index", "enabled"]),
        ("modifier.copy", vec!["id", "modifier", "index"]),
        ("modifier.paste", vec!["ids", "replace"]),
        ("modifier.list", vec!["id"]),
        ("modifier.remove_all", vec!["ids"]),
        ("modifier.apply", vec!["ids", "upTo"]),
        ("modifier.duplicate", vec!["ids", "modifier", "index"]),
        ("modifier.targets", vec!["id", "modifier", "index"]),
    ] {
        assert_eq!(crate::cmd::param_names(id).unwrap(), names.as_slice(), "{id}");
    }
}
