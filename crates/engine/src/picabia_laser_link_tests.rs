//! PICABIA: the laser link (`laser.*`) against the simulated GRBL: connecting, every command,
//! jobs to done with the beam never on while travelling, pause and resume, stop, alarms, a pulled
//! cable, the emergency stop, quitting, and every refusal (not armed, an expired or used-up arm,
//! `$32 = 0`, not Idle, off the bed).

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::picabia_laser_gcode_tests::stencil;
use crate::{EngineError, Session};

/// Fast enough to finish a stencil job in well under a second.
fn fast() -> Value {
    json!({"speedFactor": 1000, "timeScale": 100})
}

/// Slow enough that a job is still running when the test pauses or stops it.
fn slow() -> Value {
    json!({"speedFactor": 5, "timeScale": 1})
}

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn st(s: &mut Session) -> Value {
    ex(s, "laser.status", json!({}))
}

/// Poll `laser.status` until `done` (panics with the last status after `secs`).
fn wait(s: &mut Session, secs: f64, what: &str, done: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    loop {
        let v = st(s);
        if done(&v) {
            return v;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}: {v:#}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Connected to the simulator with `sim` options, ready and Idle.
fn connected(sim: Value) -> Session {
    let mut s = Session::new();
    let r = ex(&mut s, "laser.connect", json!({"port": "sim", "sim": sim}));
    assert_eq!(r["port"], "sim");
    assert_eq!(r["sim"], true);
    wait(&mut s, 10.0, "ready", |v| v["phase"] == "ready" && v["state"] == "Idle");
    s
}

fn job_state(v: &Value) -> &str {
    v["job"]["state"].as_str().unwrap_or("")
}

fn disabled(r: crate::Result<Value>) -> String {
    match r {
        Err(EngineError::Disabled(_, why)) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Start the selected stencil (or `p`'s job), armed.
fn start(s: &mut Session, p: Value) -> Value {
    s.laser_arm();
    ex(s, "laser.start", p)
}

#[test]
fn connect_reads_the_machine_and_lists_the_simulator() {
    let mut s = Session::new();
    let ports = ex(&mut s, "laser.ports", json!({}));
    assert!(ports["ports"].as_array().unwrap().iter().any(|p| p["path"] == "sim" && p["name"] == "Simulated GRBL"), "{ports}");
    let v = st(&mut s);
    assert_eq!((v["connected"].as_bool(), v["phase"].as_str()), (Some(false), Some("closed")));
    assert_eq!(v["canStart"], false);
    assert_eq!(v["job"], Value::Null);
    let mut s = connected(json!({"sMax": 1200}));
    let v = st(&mut s);
    assert_eq!(v["connected"], true);
    assert_eq!(v["settings"]["laserMode"], true);
    assert_eq!(v["settings"]["sMax"], 1200.0);
    assert_eq!(v["sMaxUsed"], 1200.0, "the machine's $30 wins");
    assert_eq!(v["profile"]["sMax"], 1000.0);
    assert!(v["settings"]["version"].as_str().unwrap().starts_with("1.1"), "{v}");
    assert_eq!(v["settings"]["maxTravel"], json!([584.2, 1219.2, 50.0]));
    assert_eq!(v["bedMm"], json!({"w": 584.2, "h": 1219.2}));
    assert_eq!(v["canStart"], true, "{v}");
    assert_eq!(v["startBlockedBy"], json!([]));
    assert_eq!(v["armed"], false);
    assert!(v["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("$30 is 1200")), "{v}");
    assert!(v["log"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().contains("$$")), "{v}");
    assert!(v["pos"]["x"].is_number() && v["simStats"]["overflows"] == 0, "{v}");
    // The G-code export uses the connected machine's $30 too.
    let g = stencil(&mut s, json!({}));
    let r = ex(&mut s, "modifier.stencilGcode", json!({"id": g, "dryRun": true}));
    assert_eq!(r["gcode"]["sMax"], 1200.0);
    // A shorter travel than the profile's bed narrows the bed.
    let mut t = connected(json!({"maxTravelMm": [300, 200, 50]}));
    assert_eq!(st(&mut t)["bedMm"], json!({"w": 300.0, "h": 200.0}));
    // Disconnect.
    let v = ex(&mut s, "laser.disconnect", json!({}));
    assert_eq!((v["connected"].as_bool(), v["phase"].as_str()), (Some(false), Some("closed")));
    assert!(s.execute("laser.home", &json!({})).is_err());
    // Connecting again works.
    ex(&mut s, "laser.connect", json!({"port": "sim", "sim": fast()}));
    wait(&mut s, 10.0, "ready again", |v| v["phase"] == "ready");
}

#[test]
fn the_machine_commands() {
    let mut s = connected(json!({"speedFactor": 50, "homing": true, "timeScale": 10}));
    let v = ex(&mut s, "laser.jog", json!({"dx": 10, "dy": 5, "feed": 3000}));
    assert_eq!(v["sent"], json!(["$J=G91 G21 X10 Y5 F3000"]));
    wait(&mut s, 5.0, "the jog", |v| v["state"] == "Idle" && v["pos"]["x"] == 10.0 && v["pos"]["y"] == 5.0);
    ex(&mut s, "laser.jog", json!({"x": 40.5, "y": 30}));
    wait(&mut s, 5.0, "the absolute jog", |v| v["state"] == "Idle" && v["pos"]["x"] == 40.5 && v["pos"]["y"] == 30.0);
    let v = ex(&mut s, "laser.setOrigin", json!({}));
    assert_eq!(v["sent"], json!(["G10 L20 P1 X0 Y0"]));
    wait(&mut s, 5.0, "the new origin", |v| v["pos"]["x"] == 0.0 && v["pos"]["y"] == 0.0 && v["mpos"]["x"] == 40.5);
    // Jog cancel stops a long jog.
    ex(&mut s, "laser.jog", json!({"dx": 400, "feed": 100}));
    wait(&mut s, 5.0, "jogging", |v| v["state"] == "Jog");
    ex(&mut s, "laser.jogCancel", json!({}));
    let v = wait(&mut s, 5.0, "the cancelled jog", |v| v["state"] == "Idle");
    assert!(v["pos"]["x"].as_f64().unwrap() < 400.0, "{v}");
    // Home and unlock.
    let v = ex(&mut s, "laser.home", json!({}));
    assert_eq!(v["sent"], json!(["$H"]));
    wait(&mut s, 10.0, "homing", |v| v["state"] == "Idle" && v["log"].as_array().unwrap().iter().any(|l| l == "> $H"));
    let v = ex(&mut s, "laser.unlock", json!({}));
    assert_eq!(v["sent"], json!(["$X"]));
    assert!(v["warning"].as_str().unwrap().contains("Home it"), "{v}");
    // Frame (beam off): the box of the job, the beam never on.
    let g = stencil(&mut s, json!({}));
    let v = ex(&mut s, "laser.frame", json!({"id": g}));
    assert_eq!(v["beam"], false);
    assert_eq!(v["boundsMm"], json!({"x0": 0.0, "y0": 0.0, "x1": 330.1, "y1": 180.1}));
    assert_eq!(v["sent"][0], "M5");
    assert_eq!(v["sent"][3], "$J=G90 G21 X330.1 Y180.1 F3000");
    let v = wait(&mut s, 10.0, "the frame", |v| v["state"] == "Idle" && v["pos"]["x"] == 0.0 && v["simStats"]["movedMm"].as_f64().unwrap() > 1000.0);
    assert_eq!(v["simStats"]["burnMm"], 0.0, "{v}");
    assert_eq!(v["simStats"]["beamOnTravelMm"], 0.0);
    // Pause, stop and resume are harmless with no job.
    for cmd in ["laser.pause", "laser.resume", "laser.stop"] {
        ex(&mut s, cmd, json!({}));
    }
    wait(&mut s, 5.0, "back after the stop", |v| v["phase"] == "ready" && !v["stopping"].as_bool().unwrap() && v["state"] == "Idle");
    // Junk.
    for (cmd, p) in [
        ("laser.jog", json!({"dx": 1, "x": 1})),
        ("laser.jog", json!({})),
        ("laser.jog", json!({"dx": "far"})),
        ("laser.jog", json!({"dx": 1e9})),
        ("laser.jog", json!({"dx": 1, "feed": 0})),
        ("laser.jog", json!({"dx": 1, "feed": "fast"})),
        ("laser.frame", json!({"beam": "yes"})),
        ("laser.connect", json!({"port": 5})),
        ("laser.connect", json!({"port": ""})),
        ("laser.connect", json!({"port": "sim", "baud": 7})),
        ("laser.connect", json!({"port": "/dev/x", "sim": {}})),
        ("laser.connect", json!({"port": "sim", "sim": 3})),
    ] {
        assert!(matches!(s.execute(cmd, &p), Err(EngineError::BadParams { .. })), "{cmd} {p}");
    }
}

#[test]
fn a_job_runs_to_done_with_the_beam_off_while_travelling() {
    let mut s = connected(fast());
    let g = stencil(&mut s, json!({}));
    let v = start(&mut s, json!({"id": g}));
    let started = &v["started"];
    assert_eq!(started["name"], "Stencil laser");
    assert!(started["lines"].as_u64().unwrap() > 20 && started["estimatedSec"].as_f64().unwrap() > 0.0, "{v}");
    assert_eq!(started["boundsMm"], json!({"x0": 0.0, "y0": 0.0, "x1": 330.1, "y1": 180.1}));
    assert_eq!(v["armed"], false, "the start used the arm up");
    let v = wait(&mut s, 30.0, "the job to finish", |v| job_state(v) == "done");
    let job = &v["job"];
    assert_eq!(job["percent"], 100.0);
    assert_eq!(job["line"], job["lines"]);
    assert_eq!(job["source"], "stencil");
    assert_eq!(job["sheet"], "1 of 1");
    assert_eq!(job["etaSec"], 0.0);
    let sim = &v["simStats"];
    assert!(sim["burnMm"].as_f64().unwrap() > 400.0, "{sim}");
    assert_eq!(sim["beamOnTravelMm"], 0.0, "{sim}");
    assert_eq!(sim["beamOnStillMs"], 0.0, "{sim}");
    assert_eq!(sim["overflows"], 0);
    assert!(sim["maxRxUsed"].as_u64().unwrap() <= 128);
    assert_eq!(v["state"], "Idle");
    assert_eq!(v["canStart"], true, "{v}");
    // The test card and the power ladder from the panel.
    start(&mut s, json!({"testCard": "card", "material": "film"}));
    let v = wait(&mut s, 30.0, "the card", |v| job_state(v) == "done");
    assert_eq!(v["job"]["source"], "testCard");
    assert_eq!(v["job"]["name"], "Test card film");
    start(&mut s, json!({"testCard": "powerLadder", "powersPct": [20, 40], "speedsMmMin": [600]}));
    let v = wait(&mut s, 30.0, "the ladder", |v| job_state(v) == "done");
    assert_eq!(v["job"]["source"], "powerLadder");
    assert_eq!(v["simStats"]["beamOnTravelMm"], 0.0);
}

#[test]
fn a_gcode_file_runs_and_one_that_leaves_the_bed_is_refused() {
    let mut s = connected(fast());
    let dir = std::env::temp_dir().join(format!("picabia-link-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ok = dir.join("square.gcode");
    std::fs::write(&ok, "; a square\nG21 G90 M5\nG0 X10 Y10\nM4 S300\nG1 X60 Y10 F1200\nG1 X60 Y60\nG1 X10 Y60\nG1 X10 Y10\nM5\nG0 X0 Y0\nM5\n")
        .unwrap();
    let v = start(&mut s, json!({"path": ok.to_string_lossy()}));
    assert_eq!(v["started"]["name"], "square.gcode");
    assert_eq!(v["started"]["boundsMm"], json!({"x0": 0.0, "y0": 0.0, "x1": 60.0, "y1": 60.0}));
    let v = wait(&mut s, 20.0, "the file", |v| job_state(v) == "done");
    assert!((v["simStats"]["burnMm"].as_f64().unwrap() - 200.0).abs() < 1e-6, "{v}");
    assert_eq!(v["job"]["source"], "file");
    // Off the bed (Y 2000 on a 1219.2 mm bed).
    let far = dir.join("far.nc");
    std::fs::write(&far, "G21 G90\nG0 X10 Y2000\nM5\n").unwrap();
    s.laser_arm();
    let why = disabled(s.execute("laser.start", &json!({"path": far.to_string_lossy()})));
    assert!(why.contains("bigger than the bed"), "{why}");
    // Files Picabia can't check, and junk.
    let g92 = dir.join("g92.gcode");
    std::fs::write(&g92, "G92 X0 Y0\nG0 X1 Y1\n").unwrap();
    s.laser_arm();
    assert!(disabled(s.execute("laser.start", &json!({"path": g92.to_string_lossy()}))).contains("G92"));
    s.laser_arm();
    assert!(matches!(s.execute("laser.start", &json!({"path": dir.join("x.svg").to_string_lossy()})), Err(EngineError::BadParams { .. })));
    s.laser_arm();
    assert!(s.execute("laser.start", &json!({"path": dir.join("missing.gcode").to_string_lossy()})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pause_and_resume() {
    let mut s = connected(slow());
    let g = stencil(&mut s, json!({}));
    start(&mut s, json!({"id": g}));
    wait(&mut s, 10.0, "burning", |v| v["simStats"]["burnMm"].as_f64().unwrap() > 1.0);
    let v = ex(&mut s, "laser.pause", json!({}));
    assert_eq!(job_state(&v), "paused");
    let v = wait(&mut s, 5.0, "the hold", |v| v["state"] == "Hold");
    std::thread::sleep(Duration::from_millis(150));
    let held = wait(&mut s, 5.0, "the hold", |v| v["state"] == "Hold");
    let moved = held["simStats"]["movedMm"].as_f64().unwrap();
    std::thread::sleep(Duration::from_millis(150));
    let later = st(&mut s);
    assert_eq!(later["simStats"]["movedMm"].as_f64().unwrap(), moved, "nothing moves in a hold: {v}");
    assert_eq!(later["simStats"]["beamOnStillMs"], 0.0, "the beam is off in a hold");
    // Resuming a paused job lights the beam again: it needs the arm.
    let why = disabled(s.execute("laser.resume", &json!({})));
    assert!(why.contains("armed"), "{why}");
    assert_eq!(job_state(&st(&mut s)), "paused");
    s.laser_arm();
    let v = ex(&mut s, "laser.resume", json!({}));
    assert_eq!(job_state(&v), "running");
    assert_eq!(v["armed"], false);
    wait(&mut s, 5.0, "moving again", |v| v["simStats"]["movedMm"].as_f64().unwrap() > moved + 1.0);
    ex(&mut s, "laser.stop", json!({}));
}

#[test]
fn stop_mid_job() {
    let mut s = connected(slow());
    let g = stencil(&mut s, json!({}));
    start(&mut s, json!({"id": g}));
    wait(&mut s, 10.0, "burning", |v| v["simStats"]["burnMm"].as_f64().unwrap() > 1.0);
    let v = ex(&mut s, "laser.stop", json!({}));
    assert_eq!(job_state(&v), "stopped");
    let v = wait(&mut s, 5.0, "the stop sequence", |v| !v["stopping"].as_bool().unwrap() && v["state"] == "Idle");
    assert!(v["simStats"]["resets"].as_u64().unwrap() >= 1, "{v}");
    assert_eq!(v["alarm"], Value::Null, "stopped before the reset: no alarm 3");
    assert_eq!(v["power"], 0.0, "{v}");
    assert!(v["log"].as_array().unwrap().iter().any(|l| l == "> M5"), "M5 after the reset: {v}");
    let burnt = v["simStats"]["burnMm"].as_f64().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(st(&mut s)["simStats"]["burnMm"].as_f64().unwrap(), burnt, "nothing burns after the stop");
    assert_eq!(st(&mut s)["simStats"]["beamOnTravelMm"], 0.0);
    // The next job may start.
    start(&mut s, json!({"id": g}));
    assert_eq!(job_state(&st(&mut s)), "running");
    ex(&mut s, "laser.stop", json!({}));
}

#[test]
fn an_alarm_mid_job_fails_it_at_its_line() {
    let mut s = connected(json!({"speedFactor": 1000, "timeScale": 100, "alarmAtLine": 30}));
    let g = stencil(&mut s, json!({}));
    start(&mut s, json!({"id": g}));
    let v = wait(&mut s, 20.0, "the alarm", |v| job_state(v) == "failed" && !v["stopping"].as_bool().unwrap() && v["state"] == "Alarm");
    assert!(v["job"]["why"].as_str().unwrap().contains("alarm 1"), "{v}");
    assert!(v["job"]["failedLine"].as_u64().unwrap() > 0);
    assert_eq!(v["alarm"]["code"], 1, "{v}");
    assert!(v["alarm"]["advice"].as_str().unwrap().len() > 10);
    assert_eq!(v["state"], "Alarm");
    assert_eq!(v["canStart"], false);
    assert!(v["startBlockedBy"][0].as_str().unwrap().contains("alarm"), "{v}");
    assert_eq!(v["simStats"]["beamOnTravelMm"], 0.0);
    // An error at a line fails it too.
    let mut s = connected(json!({"speedFactor": 1000, "timeScale": 100, "errorAtLine": 12}));
    let g = stencil(&mut s, json!({}));
    start(&mut s, json!({"id": g}));
    let v = wait(&mut s, 20.0, "the error", |v| job_state(v) == "failed");
    assert!(v["job"]["why"].as_str().unwrap().contains("error 20"), "{v}");
    assert!(v["lastError"].as_str().unwrap().contains("The job stopped"), "{v}");
}

#[test]
fn a_pulled_cable_fails_the_job_and_loses_the_link() {
    let mut s = connected(json!({"speedFactor": 1000, "timeScale": 100, "disconnectAtLine": 25}));
    let g = stencil(&mut s, json!({}));
    start(&mut s, json!({"id": g}));
    let v = wait(&mut s, 20.0, "the lost port", |v| v["phase"] == "lost");
    assert_eq!(v["connected"], false);
    assert_eq!(job_state(&v), "failed");
    assert!(v["job"]["why"].as_str().unwrap().contains("lost"), "{v}");
    assert!(v["lastError"].as_str().unwrap().contains("cable"), "{v}");
    assert!(disabled(s.execute("laser.home", &json!({}))).contains("isn't connected"));
    // Stop and pause stay harmless; connecting again works.
    ex(&mut s, "laser.stop", json!({}));
    ex(&mut s, "laser.pause", json!({}));
    ex(&mut s, "laser.connect", json!({"port": "sim", "sim": fast()}));
    let v = wait(&mut s, 10.0, "ready again", |v| v["phase"] == "ready");
    assert_eq!(v["job"], Value::Null, "a new connection starts without a job");
}

#[test]
fn disconnect_mid_job_stops_it_first() {
    let mut s = connected(slow());
    let g = stencil(&mut s, json!({}));
    start(&mut s, json!({"id": g}));
    wait(&mut s, 10.0, "burning", |v| v["simStats"]["burnMm"].as_f64().unwrap() > 1.0);
    let t = Instant::now();
    let v = ex(&mut s, "laser.disconnect", json!({}));
    assert!(t.elapsed() < Duration::from_millis(1200), "{:?}", t.elapsed());
    assert_eq!(v["connected"], false);
    assert_eq!(job_state(&v), "stopped");
    assert!(v["simStats"]["resets"].as_u64().unwrap() >= 1, "the machine was reset (beam off) before the port closed: {v}");
    assert!(v["log"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().contains("stop")), "{v}");
}

#[test]
fn the_emergency_stop_and_a_panicking_command_stop_a_job() {
    let mut s = connected(slow());
    let g = stencil(&mut s, json!({}));
    start(&mut s, json!({"id": g}));
    wait(&mut s, 10.0, "burning", |v| v["simStats"]["burnMm"].as_f64().unwrap() > 1.0);
    s.laser_arm();
    s.laser_emergency_stop();
    let v = wait(&mut s, 5.0, "the stop", |v| job_state(v) == "stopped" && !v["stopping"].as_bool().unwrap() && v["state"] == "Idle");
    assert_eq!(v["armed"], false, "the emergency stop disarms");
    assert!(v["simStats"]["resets"].as_u64().unwrap() >= 1);
    // A command that panics (here: normalising after an edit) stops the job through exec.rs.
    start(&mut s, json!({"id": g}));
    wait(&mut s, 10.0, "running", |v| job_state(v) == "running");
    crate::derived::PANIC.with(|p| p.set(true));
    let r = s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 5, "height": 5}));
    crate::derived::PANIC.with(|p| p.set(false));
    assert!(matches!(r, Err(EngineError::Internal { .. })), "{r:?}");
    let v = wait(&mut s, 5.0, "the stop", |v| job_state(v) == "stopped");
    assert_eq!(v["connected"], true, "the link stays open");
}

#[test]
fn quitting_and_dropping_stop_a_job_quickly() {
    let mut s = connected(slow());
    let g = stencil(&mut s, json!({}));
    start(&mut s, json!({"id": g}));
    wait(&mut s, 10.0, "burning", |v| v["simStats"]["burnMm"].as_f64().unwrap() > 1.0);
    let t = Instant::now();
    s.laser_shutdown();
    let took = t.elapsed();
    assert!(took < Duration::from_millis(700), "{took:?}");
    let v = st(&mut s);
    assert_eq!(v["connected"], false);
    assert_eq!(job_state(&v), "stopped");
    assert!(v["simStats"]["resets"].as_u64().unwrap() >= 1, "{v}");
    // Dropping the session does the same, as fast.
    let mut s = connected(slow());
    let g = stencil(&mut s, json!({}));
    start(&mut s, json!({"id": g}));
    wait(&mut s, 10.0, "running", |v| job_state(v) == "running");
    let t = Instant::now();
    drop(s);
    assert!(t.elapsed() < Duration::from_millis(700), "{:?}", t.elapsed());
}

#[test]
fn refusals() {
    let mut s = connected(fast());
    let g = stencil(&mut s, json!({}));
    // Not armed.
    let why = disabled(s.execute("laser.start", &json!({"id": g})));
    assert!(why.contains("isn't armed") && why.contains("can't arm"), "{why}");
    // An expired arm.
    s.laser_arm();
    s.picabia_laser.age_arm(61);
    assert!(disabled(s.execute("laser.start", &json!({"id": g}))).contains("isn't armed"));
    // A refused start uses the arm up too.
    s.laser_arm();
    assert!(s.picabia_laser.is_armed());
    assert!(s.execute("laser.start", &json!({"id": g, "sheet": 9})).is_err());
    assert!(!s.picabia_laser.is_armed(), "the attempt used the arm up");
    assert!(disabled(s.execute("laser.start", &json!({"id": g}))).contains("isn't armed"));
    // Off the bed: a 330 mm sheet 500 mm along a 584.2 mm bed, work coordinates taken as the bed's.
    ex(&mut s, "laser.profile", json!({"set": {"origin": "machine"}}));
    s.laser_arm();
    let why = disabled(s.execute("laser.start", &json!({"id": g, "originXMm": 500})));
    assert!(why.contains("off the bed") && why.contains("830.1"), "{why}");
    // One arm, one start.
    let v = start(&mut s, json!({"id": g}));
    assert_eq!(job_state(&v), "running");
    wait(&mut s, 20.0, "done", |v| job_state(v) == "done");
    assert!(disabled(s.execute("laser.start", &json!({"id": g}))).contains("isn't armed"));
    // A beam frame needs the arm; a plain one doesn't.
    assert!(disabled(s.execute("laser.frame", &json!({"id": g, "beam": true}))).contains("isn't armed"));
    s.laser_arm();
    let v = ex(&mut s, "laser.frame", json!({"id": g, "beam": true}));
    assert!(v["sent"].as_array().unwrap().iter().any(|l| l == "M4 S10"), "1% of $30 = 1000: {v}");
    let v = wait(&mut s, 20.0, "the beam frame", |v| job_state(v) == "done");
    assert_eq!(v["job"]["source"], "frame");
    assert_eq!(v["simStats"]["beamOnTravelMm"], 0.0);
    // A sheet the stencil doesn't have.
    s.laser_arm();
    let e = s.execute("laser.start", &json!({"id": g, "sheet": 9})).unwrap_err();
    assert!(matches!(&e, EngineError::BadParams { msg, .. } if msg.contains("sheet")), "{e}");
    // Not a stencil, nothing selected.
    s.set_selection(vec![]);
    s.laser_arm();
    assert!(s.execute("laser.start", &json!({})).is_err());
    // Not connected.
    let mut t = Session::new();
    t.laser_arm();
    assert!(disabled(t.execute("laser.start", &json!({"testCard": "card"}))).contains("isn't connected"));
    assert!(disabled(t.execute("laser.frame", &json!({"testCard": "card"}))).contains("isn't connected"));
}

#[test]
fn laser_mode_off_and_not_idle_are_refused() {
    let mut s = connected(json!({"laserMode": false, "speedFactor": 1000, "timeScale": 100}));
    let v = st(&mut s);
    assert_eq!(v["settings"]["laserMode"], false);
    assert_eq!(v["canStart"], false);
    assert!(v["startBlockedBy"][0].as_str().unwrap().contains("$32=1"), "{v}");
    s.laser_arm();
    let why = disabled(s.execute("laser.start", &json!({"testCard": "card"})));
    assert!(why.contains("Laser mode is off") && why.contains("$32=1"), "{why}");
    // A beam frame too; a plain frame is fine (the beam stays off).
    s.laser_arm();
    assert!(disabled(s.execute("laser.frame", &json!({"testCard": "card", "beam": true}))).contains("$32=1"));
    ex(&mut s, "laser.frame", json!({"testCard": "card"}));
    let v = wait(&mut s, 10.0, "the frame", |v| v["state"] == "Idle" && v["simStats"]["movedMm"].as_f64().unwrap() > 100.0);
    assert_eq!(v["simStats"]["burnMm"], 0.0);
    // In alarm.
    let mut s = Session::new();
    ex(&mut s, "laser.connect", json!({"port": "sim", "sim": {"startInAlarm": true, "homing": true, "speedFactor": 1000, "timeScale": 100}}));
    let v = wait(&mut s, 10.0, "ready in alarm", |v| v["phase"] == "ready" && v["state"] == "Alarm");
    assert_eq!(v["canStart"], false);
    s.laser_arm();
    assert!(disabled(s.execute("laser.start", &json!({"testCard": "card"}))).contains("alarm"));
    // Home, then it may start.
    ex(&mut s, "laser.home", json!({}));
    wait(&mut s, 10.0, "homed", |v| v["state"] == "Idle");
    start(&mut s, json!({"testCard": "card"}));
    wait(&mut s, 30.0, "the card", |v| job_state(v) == "done");
    // A job running: no second start, no jog, no reconnect.
    let mut s = connected(slow());
    start(&mut s, json!({"testCard": "card"}));
    s.laser_arm();
    assert!(disabled(s.execute("laser.start", &json!({"testCard": "card"}))).contains("job is running"));
    assert!(disabled(s.execute("laser.jog", &json!({"dx": 1}))).contains("job is running"));
    assert!(disabled(s.execute("laser.connect", &json!({"port": "sim"}))).contains("job is running"));
    ex(&mut s, "laser.stop", json!({}));
}

#[test]
fn junk_params_on_every_laser_command() {
    let junk = [
        json!({}),
        json!({"port": "sim", "sim": {"laserMode": "x", "sMax": -1e308, "speedFactor": 1e308, "timeScale": -5, "alarmAtLine": 1e300, "maxTravelMm": [1e308, "x", -1]}}),
        json!({"port": "/dev/picabia-no-such-port", "baud": 1e308}),
        json!({"dx": 1e308, "dy": "NaN", "feed": -1}),
        json!({"x": [1], "y": {"a": 1}, "feed": 1e-308}),
        json!({"id": 999999, "sheet": -1, "beam": 1, "testCard": 5}),
        json!({"testCard": "card", "material": [1], "sMax": "x", "cut": 5, "engrave": {"passes": 1e308}, "originXMm": 1e308}),
        json!({"testCard": "powerLadder", "powersPct": [1e308, -1, "x"], "speedsMmMin": [], "squareMm": -1, "gapMm": 1e308, "passes": 1e9}),
        json!({"path": 5}),
        json!({"path": "\u{0}.gcode"}),
        json!({"set": {"name": "\u{0}", "bedWidthMm": -1e308, "sMax": "x", "materials": {"paper": 5, "film": {"cut": {"passes": -1}}}}}),
        json!({"set": [1]}),
    ];
    let ids: Vec<&str> = crate::all_commands().iter().map(|c| c.id).filter(|i| i.starts_with("laser.")).collect();
    assert_eq!(ids.len(), 17, "{ids:?}");
    for connect in [false, true] {
        let mut s = if connect { connected(fast()) } else { Session::new() };
        let _ = stencil(&mut s, json!({}));
        for cmd in &ids {
            for j in &junk {
                let mut taken = j.clone();
                if let (Some(names), Some(o)) = (crate::cmd::param_names(cmd), taken.as_object_mut()) {
                    o.retain(|k, _| matches!(k.as_str(), "ids" | "id") || names.contains(&k.as_str()));
                }
                for armed in [false, true] {
                    if armed {
                        s.laser_arm();
                    }
                    if let Err(EngineError::Internal { cmd, msg }) = s.execute(cmd, &taken) {
                        panic!("{cmd} panicked on {taken}: {msg}");
                    }
                }
            }
        }
        let _ = s.execute("laser.stop", &json!({}));
    }
}

/// A long job through `laser.start` against the simulator at a high speed: wall time, lines per
/// second, and how long `laser.status` takes meanwhile.
#[test]
fn a_100k_line_job_streams_and_status_stays_fast() {
    let mut s = connected(json!({"speedFactor": 100000, "timeScale": 1000}));
    let dir = std::env::temp_dir().join(format!("picabia-long-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.gcode");
    let mut text = String::from("G21\nG90\nM5\nM4 S100\n");
    for i in 0..33_332 {
        let y = (i % 100) as f64;
        text.push_str(&format!("G0 X10 Y{y}\nG1 X20 Y{y} F3000\nG1 X10 Y{y}\n"));
    }
    text.push_str("M5\n");
    std::fs::write(&path, &text).unwrap();
    let t = Instant::now();
    let v = start(&mut s, json!({"path": path.to_string_lossy()}));
    let lines = v["started"]["lines"].as_u64().unwrap();
    assert!(lines > 100_000, "{lines}");
    let mut slowest = Duration::ZERO;
    let mut polls = 0u32;
    loop {
        let q = Instant::now();
        let v = st(&mut s);
        slowest = slowest.max(q.elapsed());
        polls += 1;
        if job_state(&v) == "done" {
            assert_eq!(v["simStats"]["beamOnTravelMm"], 0.0);
            assert_eq!(v["simStats"]["overflows"], 0);
            break;
        }
        assert!(job_state(&v) == "running", "{v}");
        assert!(t.elapsed() < Duration::from_secs(120), "{v}");
        std::thread::sleep(Duration::from_millis(5));
    }
    let wall = t.elapsed().as_secs_f64();
    eprintln!(
        "TIMING laser.start 100k: {lines} lines in {wall:.2} s wall = {:.0} lines/s; laser.status slowest {:.3} ms over {polls} polls",
        lines as f64 / wall,
        slowest.as_secs_f64() * 1000.0
    );
    assert!(slowest < Duration::from_millis(20), "laser.status took {slowest:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn placement_with_the_origin_at_the_head() {
    use crate::picabia_laser_link::{MachineFrame, check_placement};
    use figcraft_geom::kurbo::Rect;
    let job = Rect::new(0.0, 0.0, 330.1, 180.1);
    let bed = (584.2, 1219.2);
    // Nothing known: only the size, with a warning.
    let w = check_placement(job, bed, true, &MachineFrame::default()).unwrap();
    assert!(w[0].contains("only its size"), "{w:?}");
    assert!(check_placement(Rect::new(0.0, 0.0, 600.0, 10.0), bed, true, &MachineFrame::default()).unwrap_err().contains("bigger than the bed"));
    // Homed, positive space (the head at +100, +50), origin set at machine 100, 50: fits.
    let pos = MachineFrame { wco: Some([100.0, 50.0]), mpos: Some([100.0, 50.0]), travel: [Some(584.2), Some(1219.2)], homing: true };
    assert_eq!(check_placement(job, bed, true, &pos).unwrap(), Vec::<String>::new());
    // Origin at machine X 300: the sheet would reach 630.1.
    let far = MachineFrame { wco: Some([300.0, 0.0]), mpos: Some([300.0, 10.0]), ..pos };
    let e = check_placement(job, bed, true, &far).unwrap_err();
    assert!(e.contains("machine X 300 to 630.1") && e.contains("0 to 584.2") && e.contains("Set origin"), "{e}");
    // Negative space (GRBL homed to the far corner): the origin at -500, -1000 fits; at -100 it doesn't.
    let neg = MachineFrame { wco: Some([-500.0, -1000.0]), mpos: Some([-500.0, -1000.0]), ..pos };
    assert!(check_placement(job, bed, true, &neg).unwrap().is_empty());
    let neg_far = MachineFrame { wco: Some([-100.0, -1000.0]), mpos: Some([-100.0, -1000.0]), ..pos };
    assert!(check_placement(job, bed, true, &neg_far).unwrap_err().contains("-584.2 to 0"));
    // The head at 0: either way, so a warning when it fits only one; refused when it fits neither.
    let zero = MachineFrame { wco: Some([0.0, 0.0]), mpos: Some([0.0, 0.0]), ..pos };
    assert!(check_placement(job, bed, true, &zero).unwrap()[0].contains("can't tell which way"));
    let across = MachineFrame { wco: Some([-100.0, 0.0]), mpos: Some([0.0, 0.0]), ..pos };
    assert!(check_placement(job, bed, true, &across).is_err());
    // Without homing, machine coordinates mean nothing: size only.
    let unhomed = MachineFrame { homing: false, ..far };
    assert!(check_placement(job, bed, true, &unhomed).unwrap()[0].contains("only its size"));
    // The origin at the machine's: work coordinates within 0..bed.
    assert!(check_placement(job, bed, false, &MachineFrame::default()).unwrap().is_empty());
    assert!(check_placement(Rect::new(500.0, 0.0, 830.1, 180.1), bed, false, &MachineFrame::default()).unwrap_err().contains("off the bed"));
    assert!(check_placement(Rect::new(f64::NAN, 0.0, 1.0, 1.0), bed, true, &MachineFrame::default()).is_err());
}

#[test]
fn a_homed_machine_checks_the_job_in_machine_coordinates() {
    let mut s = connected(json!({"homing": true, "speedFactor": 1000, "timeScale": 100, "maxTravelMm": [584.2, 1219.2, 50]}));
    ex(&mut s, "laser.home", json!({}));
    wait(&mut s, 10.0, "homed", |v| v["state"] == "Idle" && v["log"].as_array().unwrap().iter().any(|l| l == "> $H"));
    // Jog to X 400 and set the origin there: a 330 mm sheet would run off the 584.2 mm travel.
    ex(&mut s, "laser.jog", json!({"x": 400, "y": 20}));
    wait(&mut s, 10.0, "the jog", |v| v["state"] == "Idle" && v["pos"]["x"] == 400.0);
    ex(&mut s, "laser.setOrigin", json!({}));
    wait(&mut s, 10.0, "the origin", |v| v["pos"]["x"] == 0.0 && v["mpos"]["x"] == 400.0);
    let g = stencil(&mut s, json!({}));
    s.laser_arm();
    let why = disabled(s.execute("laser.start", &json!({"id": g})));
    assert!(why.contains("machine X 400 to 730.1") && why.contains("Set origin"), "{why}");
    assert!(disabled(s.execute("laser.frame", &json!({"id": g}))).contains("machine X 400"));
    // Back to X 100: it fits, no warning.
    ex(&mut s, "laser.jog", json!({"x": -300, "y": 0}));
    wait(&mut s, 10.0, "the jog", |v| v["state"] == "Idle" && v["mpos"]["x"] == 100.0);
    ex(&mut s, "laser.setOrigin", json!({}));
    wait(&mut s, 10.0, "the origin", |v| v["pos"]["x"] == 0.0 && v["mpos"]["x"] == 100.0);
    let v = start(&mut s, json!({"id": g}));
    assert!(v["started"]["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap().contains("can't tell")), "{v}");
    let v = wait(&mut s, 30.0, "done", |v| job_state(v) == "done");
    assert_eq!(v["simStats"]["beamOnTravelMm"], 0.0);
}

#[test]
fn any_layers_can_be_cut_and_the_panel_opens() {
    let mut s = connected(fast());
    let r = crate::params::id_of(&ex(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 96, "height": 48}))["id"]).unwrap();
    let v = start(&mut s, json!({"ids": [r]}));
    assert_eq!(v["started"]["boundsMm"], json!({"x0": 0.0, "y0": 0.0, "x1": 25.4, "y1": 12.7}), "a CSS pixel a unit: {v}");
    let v = wait(&mut s, 20.0, "done", |v| job_state(v) == "done");
    assert_eq!(v["job"]["source"], "design");
    assert!((v["simStats"]["burnMm"].as_f64().unwrap() - 2.0 * 2.0 * (25.4 + 12.7)).abs() < 0.01, "paper's cut: 2 passes: {v}");
    assert_eq!(v["simStats"]["beamOnTravelMm"], 0.0);
    ex(&mut s, "laser.frame", json!({"ids": [r], "widthMm": 100}));
    let v = wait(&mut s, 20.0, "the frame", |v| v["state"] == "Idle" && v["pos"]["x"] == 0.0 && v["simStats"]["movedMm"].as_f64().unwrap() > 400.0);
    assert!(v["pos"]["y"] == 0.0, "{v}");
    s.ui_requests.clear();
    assert_eq!(ex(&mut s, "laser.panel", json!({})), json!({"dialog": true}));
    assert!(s.ui_requests.iter().any(|u| u.kind == "dialog.laserPanel"));
    assert!(matches!(s.execute("laser.panel", &json!({"x": 1})), Err(EngineError::BadParams { .. })));
}
