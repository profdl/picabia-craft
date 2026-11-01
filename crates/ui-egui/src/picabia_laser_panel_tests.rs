//! PICABIA: the Laser window: it opens from `laser.panel` (File ▸ Laser…, the Actions palette)
//! and isn't hidden; Esc neither closes it nor stops a job; `laser.status` reads leniently; the job
//! section's params for each source; the checklist arms only once every box is ticked and only for
//! a person (control-channel clicks arm the simulator only); `laser_arm` is called from one file of
//! the app; a whole job against the simulated GRBL from the window's buttons; Shift+Esc and a
//! panicking frame stop a running job; the bar keeps Stop on screen with the window closed.

use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use figcraft_doc::NodeId;
use figcraft_engine::{Session, UiRequest};
use serde_json::{Value, json};

use super::job::{JobState, Source, Summary};
use super::ops::{Ops, material_label};
use super::run::{Checklist, Confirm, Refused, confirm};
use super::status::{self, Job, Status};
use crate::layers_drag_tests::Ui;
use crate::live_tests::{flat, texts};

fn id(v: &Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

/// A session with a 40 × 40 rectangle, selected.
fn design() -> (Session, NodeId) {
    let mut s = Session::new();
    let r = id(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 40, "height": 40})).unwrap());
    s.execute("select.set", &json!({"ids": [r]})).unwrap();
    (s, r)
}

fn drawn(u: &mut Ui) -> Vec<(String, Pos2, egui::Vec2)> {
    texts(&flat(&u.frame_output(vec![]).shapes))
}

fn shows(u: &mut Ui, text: &str) -> bool {
    drawn(u).iter().any(|(t, _, _)| t == text)
}

/// Click the middle of the drawn text `label`: in the checklist when it's open, else in the Laser
/// window (the app has its own "Frame" and "Start"), else anywhere.
fn click(u: &mut Ui, label: &str) {
    let found: Vec<Pos2> = drawn(u).into_iter().filter(|(t, _, _)| t == label).map(|(_, pos, size)| pos + size / 2.0).collect();
    let areas: Vec<egui::Rect> =
        ["picabia_laser_checklist", "picabia_laser_panel"].iter().filter_map(|id| u.ctx.memory(|m| m.area_rect(egui::Id::unique(*id)))).collect();
    let at = areas
        .iter()
        .find_map(|r| found.iter().copied().find(|p| r.contains(*p)))
        .or_else(|| found.first().copied())
        .unwrap_or_else(|| panic!("no {label:?} drawn"));
    u.frame(vec![Event::PointerMoved(at)]);
    u.frame(vec![
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    u.frame(vec![]);
}

fn ask(u: &mut Ui, kind: &str) {
    u.app.session.ui_requests.push(UiRequest { kind: kind.into(), data: json!({}) });
    u.app.handle_ui_requests();
}

fn status(u: &Ui) -> Status {
    Status::from_json(&figcraft_engine::picabia_laser_link::status_json(&u.app.session))
}

/// Draw frames until `done` (panics after `secs`).
fn until(u: &mut Ui, secs: f64, what: &str, done: impl Fn(&Status) -> bool) {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    loop {
        u.frame(vec![]);
        let st = status(u);
        if done(&st) {
            return;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}: {st:#?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The window open over `s`, connected to the simulator (`sim` options), ready and Idle.
fn connected(s: Session, sim: Value) -> Ui {
    let mut u = Ui::new(s);
    ask(&mut u, "dialog.laserPanel");
    u.app.session.execute("laser.connect", &json!({"port": "sim", "sim": sim})).unwrap();
    until(&mut u, 10.0, "ready", |st| st.connected && st.state.as_deref() == Some("Idle"));
    u
}

#[test]
fn laser_panel_opens_the_window_from_the_file_menu_and_the_palette() {
    let (s, _) = design();
    let mut u = Ui::new(s);
    assert!(!u.app.ui.picabia_laser_panel.open);
    assert!(!shows(&mut u, "Laser"));
    // The command asks for the window.
    u.app.run("laser.panel", json!({}));
    assert!(u.app.ui.picabia_laser_panel.open);
    u.frame(vec![]);
    let t = drawn(&mut u);
    for label in ["Laser", "Connect", "Job", "Selection", "Test card", "Power ladder", "G-code file", "Machine profile", "Log"] {
        assert!(t.iter().any(|(x, _, _)| x == label), "{label} missing: {:?}", t.iter().map(|x| &x.0).collect::<Vec<_>>());
    }
    // The simulator is offered in debug builds.
    assert!(u.app.ui.picabia_laser_panel.ports.iter().any(|(p, n)| p == "sim" && n == "Simulated GRBL"));
    // Not connected: no Stop and no jog pad.
    assert!(!t.iter().any(|(x, _, _)| x == "Shift+Esc anywhere" || x == "Set origin here"));
    // Picabia's hiding leaves it alone, and the palette finds it.
    crate::picabia_hide::hide(&mut u.app);
    for id in ["laser.panel", "laser.gcode", "laser.stop"] {
        assert!(!crate::picabia_hide::hides(&u.app, id), "{id}");
    }
    let rows = crate::menus::palette_sections(&u.app, "laser", crate::menus::PaletteTab::All);
    assert!(rows.iter().flat_map(|(_, r)| r).any(|r| r.key == "command:laser.panel"), "the palette lists Laser…");
    // File ▸ Laser… sits after Export selection as SVG…; it runs laser.panel.
    assert!(super::file_menu_after("file.exportSvg") && !super::file_menu_after("file.export"));
    u.app.ui.picabia_laser_panel.open = false;
    crate::menus::invoke(&mut u.app, "laser.panel");
    assert!(u.app.ui.picabia_laser_panel.open);
}

#[test]
fn escape_neither_closes_the_window_nor_stops_a_job() {
    let (s, _) = design();
    let mut u = Ui::new(s);
    ask(&mut u, "dialog.laserPanel");
    u.frame(vec![]);
    u.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    assert!(u.app.ui.picabia_laser_panel.open, "Esc leaves the Laser window open");
    // The window's close button closes it.
    u.app.ui.picabia_laser_panel.open = false;
    u.frame(vec![]);
    assert!(!shows(&mut u, "Connect"));
}

#[test]
fn status_reads_leniently() {
    for junk in [json!(null), json!(3), json!("x"), json!({"connected": "yes", "pos": {"x": "a"}, "job": 4, "alarm": [], "log": [1, 2]})] {
        let st = Status::from_json(&junk);
        assert!(!st.connected && st.pos.is_none() && st.job.is_none() && st.alarm.is_none() && st.log.is_empty(), "{junk}");
        assert!(!st.stoppable());
    }
    let v = json!({"connected": true, "phase": "ready", "sim": true, "state": "Run", "pos": {"x": 1.5, "y": -2, "z": 0},
        "job": {"name": "Rect laser", "state": "running", "line": 120, "lines": 480, "percent": 25.0, "elapsedSec": 42, "etaSec": 126},
        "alarm": {"code": 1, "text": "Hard limit", "advice": "Home it"}, "canStart": false, "startBlockedBy": ["a job is running"],
        "settings": {"sMax": 1000, "laserMode": true, "version": "1.1h"}, "bedMm": {"w": 584.2, "h": 1219.2}, "log": ["> ?", "< ok"]});
    let st = Status::from_json(&v);
    assert!(st.connected && st.sim && st.job_active() && st.moving() && st.stoppable() && !st.paused());
    assert_eq!(st.pos, Some([1.5, -2.0, 0.0]));
    assert_eq!((st.s_max_machine, st.laser_mode, st.version.as_deref(), st.bed), (Some(1000.0), Some(true), Some("1.1h"), Some((584.2, 1219.2))));
    assert_eq!(st.alarm.as_ref().map(|a| a.code), Some(1));
    let j = st.job.clone().unwrap();
    assert_eq!(status::progress_line(&j), "Line 120 of 480 · 0:42 elapsed · about 2:06 left");
    assert_eq!(status::job_outcome(&j), None);
    let failed = Job { state: "failed".into(), why: Some("ALARM:1 hard limit".into()), failed_line: Some(7), ..j.clone() };
    assert_eq!(status::job_outcome(&failed).unwrap(), "Failed at line 7: ALARM:1 hard limit");
    assert!(!failed.active());
    let done = Job { state: "done".into(), elapsed: 3725.0, ..j };
    assert_eq!(status::job_outcome(&done).unwrap(), "Done in 1:02:05.");
    assert!(!status::progress_line(&done).contains("left"), "a finished job has nothing left");
    assert_eq!((status::clock(f64::NAN), status::clock(-5.0), status::clock(59.6)), ("0:00".into(), "0:00".into(), "1:00".into()));
    // The badge's colour follows the state.
    let t = crate::theme::Tokens::light();
    for (state, colour) in [("Idle", t.success), ("Alarm", t.danger), ("Hold", t.warning), ("Run", t.accent)] {
        let st = Status { connected: true, phase: "ready".into(), state: Some(state.into()), ..Status::default() };
        assert_eq!(status::badge(&st, &t).1, colour, "{state}");
    }
    assert_eq!(status::badge(&Status::default(), &t).0, "Not connected");
}

#[test]
fn ops_read_and_write_the_commands_fields() {
    let mut ops = Ops::default();
    ops.read(&json!({"cut": {"enabled": false, "speedMmMin": 250, "powerPct": 90, "passes": 3}, "score": {"powerPct": 500}, "engrave": "x"}));
    assert_eq!((ops.0[0].enabled, ops.0[0].speed, ops.0[0].power, ops.0[0].passes), (false, 250.0, 90.0, 3));
    assert_eq!(ops.0[1].power, 100.0, "clamped");
    assert_eq!(ops.0[2], Ops::default().0[2], "junk keeps the defaults");
    let v = ops.to_json();
    assert_eq!(v["cut"], json!({"enabled": false, "speedMmMin": 250.0, "powerPct": 90.0, "passes": 3}));
    let mut back = Ops::default();
    back.read(&v);
    assert_eq!(back, ops);
    ops.read(&json!({"cut": {"speedMmMin": f64::MAX, "passes": -4, "powerPct": null}}));
    assert_eq!((ops.0[0].speed, ops.0[0].passes, ops.0[0].power), (20_000.0, 1, 90.0));
    assert_eq!(material_label("film"), "Film");
    assert_eq!(material_label("steel"), "steel");
}

#[test]
fn the_job_section_sends_what_is_cut() {
    let sel = [NodeId(4), NodeId(9)];
    let mut j = JobState::default();
    // The selection: nothing selected is nothing to cut.
    assert_eq!(j.params(&[]), None);
    assert_eq!(j.dry_run(&[]), None);
    assert_eq!(j.params(&sel), Some(json!({"ids": [4, 9]})));
    assert_eq!(j.dry_run(&sel), Some(("laser.gcode", json!({"ids": [4, 9], "dryRun": true}))));
    // A material picked and the table edited go with it.
    j.material = Some("film".into());
    j.ops_set = true;
    let p = j.params(&sel).unwrap();
    assert_eq!(p["material"], "film");
    assert_eq!(p["cut"]["passes"], 2);
    // The test card and the ladder.
    j.source = Source::TestCard;
    assert_eq!(j.params(&[]).unwrap()["testCard"], "card");
    assert_eq!(j.dry_run(&[]).unwrap().0, "modifier.stencilTestCard");
    assert_eq!(j.dry_run(&[]).unwrap().1["format"], "gcode");
    j.source = Source::PowerLadder;
    assert_eq!(j.params(&[]).unwrap()["testCard"], "powerLadder");
    assert_eq!(j.dry_run(&[]).unwrap().1["kind"], "powerLadder");
    // A file: its path only, nothing to work out first.
    j.source = Source::File("/tmp/x.gcode".into());
    assert_eq!(j.params(&[]), Some(json!({"path": "/tmp/x.gcode"})));
    assert_eq!(j.dry_run(&sel), None);
    // A stencil's sheets: the one picked.
    let mut j = JobState::default();
    j.take_result(Ok(json!({"files": [
        {"sheet": "1 of 2", "wMm": 600, "hMm": 400, "lines": 900, "estimatedSec": 120, "warnings": ["w1"]},
        {"sheet": "2 of 2", "wMm": 300, "hMm": 400, "lines": 300, "estimatedSec": 40}
    ], "warnings": ["w0"], "material": "card", "gcode": {"cut": {"passes": 5}}})));
    assert_eq!((j.sheets(), j.material_shown(), j.ops.0[0].passes), (2, "card", 5));
    let Summary::Ready { warnings, .. } = &j.summary else { panic!("ready") };
    assert_eq!(warnings, &vec!["w0".to_string(), "w1".to_string()]);
    j.sheet = 2;
    assert_eq!(j.params(&sel).unwrap()["sheet"], 2);
    assert_eq!(j.chosen_sheet().unwrap().lines, 300);
    // The table follows the command until it's edited.
    j.ops_set = true;
    j.take_result(Ok(json!({"files": [], "gcode": {"cut": {"passes": 9}}})));
    assert_eq!(j.ops.0[0].passes, 5);
    assert_eq!(j.sheet, 1, "a sheet that's gone falls back to the first");
    j.take_result(Err("nothing to cut".into()));
    assert_eq!(j.summary, Summary::Failed("nothing to cut".into()));
}

#[test]
fn the_job_summary_comes_from_a_dry_run_and_says_when_nothing_is_selected() {
    let (s, _) = design();
    let mut u = Ui::new(s);
    ask(&mut u, "dialog.laserPanel");
    u.frame(vec![]);
    let t = drawn(&mut u);
    assert!(t.iter().any(|(x, _, _)| x.starts_with("Cuts Rectangle")), "{:?}", t.iter().map(|x| &x.0).collect::<Vec<_>>());
    let Summary::Ready { sheets, .. } = &u.app.ui.picabia_laser_panel.job.summary else { panic!("{:?}", u.app.ui.picabia_laser_panel.job.summary) };
    // 40 px at 25.4/96 mm: 10.583 mm square.
    assert_eq!(sheets.len(), 1);
    assert!((sheets[0].w_mm - 10.583).abs() < 0.01, "{sheets:?}");
    assert!(t.iter().any(|(x, _, _)| x.starts_with("10.58 × 10.58 mm")), "{:?}", t.iter().map(|x| &x.0).collect::<Vec<_>>());
    u.app.session.execute("select.set", &json!({"ids": []})).unwrap();
    u.frame(vec![]);
    let t = drawn(&mut u);
    assert!(t.iter().any(|(x, _, _)| x.starts_with("Nothing selected")));
    assert_eq!(u.app.ui.picabia_laser_panel.job.summary, Summary::NothingSelected);
}

#[test]
fn laser_arm_is_called_from_one_file_of_the_app() {
    // Only the checklist's handler arms: no other UI file, nor the control channel, MCP, CLI,
    // plugins or the apps. (The engine defines it and its tests call it.)
    let needle = concat!("laser_", "arm(");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut hits = Vec::new();
    let mut dirs = vec![root.join("apps")];
    for e in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        if e.file_name() != "engine" {
            dirs.push(e.path());
        }
    }
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                if !p.file_name().is_some_and(|n| n == "target" || n.to_string_lossy().starts_with('.')) {
                    dirs.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                let n = text.matches(needle).count();
                if n > 0 {
                    hits.push((p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().into_owned(), n));
                }
            }
        }
    }
    assert_eq!(hits, vec![("crates/ui-egui/src/picabia_laser_panel/run.rs".to_string(), 1)]);
}

#[test]
fn the_checklist_arms_only_when_every_box_is_ticked_and_only_for_a_person() {
    let (s, r) = design();
    let mut u = connected(s, json!({"speedFactor": 0.2, "timeScale": 1}));
    let mut list = Checklist::new(Confirm::Start, json!({"ids": [r]}));
    assert_eq!(list.ticks.len(), 6);
    list.ticks[..5].fill(true);
    let st = status(&u);
    assert_eq!(confirm(&mut u.app, &list, &st), Err(Refused::Unticked));
    assert!(!status(&u).job_active());
    // A control-channel click on a real machine (a status that isn't the simulator's) arms nothing.
    list.ticks.fill(true);
    u.app.control_synthetic = true;
    let real = Status { sim: false, ..st.clone() };
    assert_eq!(confirm(&mut u.app, &list, &real), Err(Refused::NotAPerson));
    let v = figcraft_engine::picabia_laser_link::status_json(&u.app.session);
    assert_eq!((v["armed"].as_bool(), v["job"].is_null()), (Some(false), true), "{v}");
    // The simulator may be driven through the window by the control channel.
    assert_eq!(confirm(&mut u.app, &list, &st), Ok(()));
    u.app.control_synthetic = false;
    assert!(status(&u).job_active(), "{:?}", status(&u));
    u.app.run("laser.stop", json!({}));
    until(&mut u, 5.0, "stopped", |st| st.job.as_ref().is_some_and(|j| j.state == "stopped") && !st.stopping);
}

#[test]
fn a_whole_job_from_the_windows_buttons() {
    let (s, _) = design();
    let mut u = connected(s, json!({"speedFactor": 0.5, "timeScale": 1}));
    let t = drawn(&mut u);
    for label in ["Stop", "Idle", "Position", "Home", "Unlock", "Set origin here", "Frame", "Start…"] {
        assert!(t.iter().any(|(x, _, _)| x == label), "{label} missing: {:?}", t.iter().map(|x| &x.0).collect::<Vec<_>>());
    }
    // Set origin, frame (beam off).
    click(&mut u, "Set origin here");
    click(&mut u, "Frame");
    until(&mut u, 5.0, "framing", |st| st.state.as_deref() == Some("Jog"));
    until(&mut u, 10.0, "framed", |st| st.state.as_deref() == Some("Idle"));
    // Start opens the checklist; its Start runs only once every box is ticked.
    click(&mut u, "Start…");
    let t = drawn(&mut u);
    assert!(u.app.ui.picabia_laser_panel.checklist.is_some(), "{:?} {:?}", status(&u), t.iter().map(|x| &x.0).collect::<Vec<_>>());
    click(&mut u, "Start");
    assert!(u.app.ui.picabia_laser_panel.checklist.is_some() && !status(&u).job_active(), "Start is disabled until every box is ticked");
    for check in Confirm::Start.checks() {
        click(&mut u, check);
    }
    assert!(u.app.ui.picabia_laser_panel.checklist.as_ref().is_some_and(Checklist::complete));
    click(&mut u, "Start");
    assert!(u.app.ui.picabia_laser_panel.checklist.is_none());
    until(&mut u, 5.0, "running", |st| st.job.as_ref().is_some_and(|j| j.state == "running"));
    assert!(shows(&mut u, "Pause"));
    // Pause; Resume needs the short confirm.
    click(&mut u, "Pause");
    until(&mut u, 5.0, "paused", |st| st.paused());
    click(&mut u, "Resume…");
    let list = u.app.ui.picabia_laser_panel.checklist.clone().expect("the resume confirm");
    assert_eq!(list.kind, Confirm::Resume);
    for check in Confirm::Resume.checks() {
        click(&mut u, check);
    }
    click(&mut u, "Resume");
    until(&mut u, 5.0, "resumed", |st| st.job.as_ref().is_some_and(|j| j.state == "running"));
    // The big Stop.
    click(&mut u, "Stop");
    until(&mut u, 5.0, "stopped", |st| st.job.as_ref().is_some_and(|j| j.state == "stopped") && !st.stopping);
    assert!(shows(&mut u, "Stopped at line") || drawn(&mut u).iter().any(|(x, _, _)| x.starts_with("Stopped at line")));
}

#[test]
fn shift_escape_stops_a_running_job_from_anywhere_and_escape_doesnt() {
    let (s, r) = design();
    let mut u = connected(s, json!({"speedFactor": 0.2, "timeScale": 1}));
    let mut list = Checklist::new(Confirm::Start, json!({"ids": [r]}));
    list.ticks.fill(true);
    let st = status(&u);
    confirm(&mut u.app, &list, &st).unwrap();
    until(&mut u, 5.0, "running", |st| st.job.as_ref().is_some_and(|j| j.state == "running"));
    // The window closed: the bar keeps the state and Stop on screen.
    u.app.ui.picabia_laser_panel.open = false;
    u.frame(vec![]);
    let t = drawn(&mut u);
    assert!(t.iter().any(|(x, _, _)| x == "Stop") && t.iter().any(|(x, _, _)| x == "Show"), "{:?}", t.iter().map(|x| &x.0).collect::<Vec<_>>());
    let esc = |m: Modifiers| Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: m };
    u.frame_with(vec![esc(Modifiers::NONE)], Modifiers::NONE);
    u.frame(vec![]);
    assert!(status(&u).job_active(), "Esc alone never stops a job");
    u.frame_with(vec![esc(Modifiers::SHIFT)], Modifiers::SHIFT);
    until(&mut u, 5.0, "stopped", |st| st.job.as_ref().is_some_and(|j| j.state == "stopped") && !st.stopping);
}

#[test]
fn a_panicking_frame_stops_a_running_job() {
    let (s, r) = design();
    let mut u = connected(s, json!({"speedFactor": 0.2, "timeScale": 1}));
    let mut list = Checklist::new(Confirm::Start, json!({"ids": [r]}));
    list.ticks.fill(true);
    let st = status(&u);
    confirm(&mut u.app, &list, &st).unwrap();
    until(&mut u, 5.0, "running", |st| st.job.as_ref().is_some_and(|j| j.state == "running"));
    let ctx = u.ctx.clone();
    #[allow(clippy::panic)]
    crate::windows::guard(&mut u.app, &ctx, |_| panic!("a broken frame"));
    until(&mut u, 5.0, "stopped", |st| st.job.as_ref().is_some_and(|j| j.state == "stopped") && !st.stopping);
}
