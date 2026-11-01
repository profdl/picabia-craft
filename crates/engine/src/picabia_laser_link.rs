//! PICABIA: the laser link (`laser.*` commands): connect to a GRBL laser over a serial port (or
//! the simulator), read its state, home, jog, frame, and stream a job with pause, resume and stop.
//!
//! # Threads
//! Natively a worker thread owns the port and a [`figcraft_grbl::Controller`]; commands send it
//! messages and read a shared copy of its state, so no command waits on the machine for more than
//! about a second. The worker pumps as soon as bytes arrive (a serial read waits at most 20 ms),
//! never on a fixed sleep. In the browser build there is no serial port: `laser.status` reports
//! disconnected, `laser.ports` lists nothing and the rest are refused.
//!
//! # Safety
//! - Only a person in the desktop app can turn the beam on: [`Session::laser_arm`] is a plain
//!   method, not a command (no command, param, control channel, MCP, CLI or plugin can arm). It arms
//!   one `laser.start`, beam frame or resume of a paused job within [`ARM_SECONDS`]; the attempt
//!   uses it up, whether it starts or not.
//! - A job starts only when the machine is ready and Idle, `$32 = 1`, and the job stays on the bed
//!   (the profile's, or less where `$130`/`$131` say so).
//! - `laser.stop` and `laser.pause` are always allowed. A running job is stopped (hold, reset,
//!   `M5`) on `laser.disconnect`, on a panicking command ([`Session::laser_emergency_stop`]), on
//!   quit ([`Session::laser_shutdown`]) and when the link is dropped; a lost port fails the job at
//!   the line it reached.
// The browser build has no laser link: the job builders and status helpers go unused there.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use figcraft_geom::kurbo::Rect;
use serde_json::{Value, json};
use web_time::{Duration, Instant};

use crate::params::{get, num};
use crate::picabia_laser_gcode::{LaserRun, MachineProfile, bed_mm, build_run, rect_json};
use crate::{CommandSpec, EngineError, Result, Session, bad, spec};

/// How long an arm lasts.
pub const ARM_SECONDS: u64 = 60;
/// Why the browser build refuses.
pub const DESKTOP_ONLY: &str = "laser control needs the desktop app";
/// A job may reach this far past the bed's edges (mm).
pub const BED_TOLERANCE_MM: f64 = 0.01;
/// Log lines `laser.status` returns.
pub const STATUS_LOG_LINES: usize = 50;
/// The simulator's port path.
pub const SIM_PORT: &str = "sim";
/// How long `laser.disconnect` waits for the stop sequence before it resets outright (ms).
const DISCONNECT_BUDGET_MS: u64 = 900;
/// How long quitting (and dropping the link) waits (ms).
const SHUTDOWN_BUDGET_MS: u64 = 450;
/// The most a jog may move or reach (mm).
const MAX_JOG_MM: f64 = 5000.0;

/// What `laser.status` tells about the running job besides the controller's progress.
#[derive(Clone, Debug, Default)]
struct JobInfo {
    source: &'static str,
    sheet: Option<String>,
    layer: Option<figcraft_doc::NodeId>,
    estimated_sec: Option<f64>,
    bounds: Option<Rect>,
    warnings: Vec<String>,
}

/// The session's laser link. See the module docs.
#[derive(Default)]
pub struct LaserLink {
    armed_at: Option<Instant>,
    job: Option<JobInfo>,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<native::Worker>,
}

impl std::fmt::Debug for LaserLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaserLink").field("armed", &self.armed_at.is_some()).field("open", &self.is_open()).finish()
    }
}

impl Session {
    /// PICABIA: arm the laser for one `laser.start` (or beam frame, or resume of a paused job)
    /// within [`ARM_SECONDS`]. Only the desktop UI calls this, after the person has gone through
    /// the safety checklist; it is deliberately not a command.
    pub fn laser_arm(&mut self) {
        self.picabia_laser.armed_at = Some(Instant::now());
    }

    /// PICABIA: something went wrong (a command panicked, the UI caught a panic): disarm and stop
    /// a running job (hold, reset, `M5`). Never blocks.
    pub fn laser_emergency_stop(&mut self) {
        self.picabia_laser.armed_at = None;
        self.picabia_laser.stop();
    }

    /// PICABIA: the app is quitting: disarm, stop a running job, give the stop sequence at most
    /// about 500 ms to go out, close the port and end the worker.
    pub fn laser_shutdown(&mut self) {
        self.picabia_laser.armed_at = None;
        self.picabia_laser.close(SHUTDOWN_BUDGET_MS);
    }
}

impl LaserLink {
    /// Use up the arm: whether it was armed within [`ARM_SECONDS`].
    fn take_arm(&mut self) -> bool {
        self.armed_at.take().is_some_and(|t| t.elapsed() <= Duration::from_secs(ARM_SECONDS))
    }

    /// Armed, and for how many more seconds.
    fn armed_left(&self) -> Option<f64> {
        let t = self.armed_at?;
        let left = Duration::from_secs(ARM_SECONDS).checked_sub(t.elapsed())?;
        Some((left.as_secs_f64() * 10.0).round() / 10.0)
    }

    /// Age the arm (tests: an arm older than [`ARM_SECONDS`]).
    #[cfg(test)]
    pub(crate) fn age_arm(&mut self, secs: u64) {
        self.armed_at = self.armed_at.and_then(|t| t.checked_sub(Duration::from_secs(secs)));
    }

    #[cfg(test)]
    pub(crate) fn is_armed(&self) -> bool {
        self.armed_left().is_some()
    }
}

#[cfg(target_arch = "wasm32")]
impl LaserLink {
    pub fn is_open(&self) -> bool {
        false
    }
    fn stop(&mut self) {}
    fn close(&mut self, _budget_ms: u64) {}
    pub(crate) fn machine_s_max(&self) -> Option<f64> {
        None
    }
    pub(crate) fn machine_travel(&self) -> (Option<f64>, Option<f64>) {
        (None, None)
    }
    pub(crate) fn machine_frame(&self) -> MachineFrame {
        MachineFrame::default()
    }
}

/// A plain `Disabled` for `cmd`.
fn disabled(cmd: &str, why: impl Into<String>) -> EngineError {
    EngineError::Disabled(cmd.into(), why.into())
}

/// A number for G-code: 3 decimals, no exponent, no `-0`.
fn gnum(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" || s.is_empty() { "0".into() } else { s }
}

/// Whether `bounds` lies on a `bed` (w, h) bed, within [`BED_TOLERANCE_MM`]; the reason when not.
pub fn check_bed(bounds: Rect, bed: (f64, f64)) -> std::result::Result<(), String> {
    let t = BED_TOLERANCE_MM;
    let finite = [bounds.x0, bounds.y0, bounds.x1, bounds.y1].iter().all(|v| v.is_finite());
    if finite && bounds.x0 >= -t && bounds.y0 >= -t && bounds.x1 <= bed.0 + t && bounds.y1 <= bed.1 + t {
        return Ok(());
    }
    Err(format!(
        "the job reaches X {} to {}, Y {} to {} mm, off the bed (0 to {} × 0 to {} mm): move its origin or use a smaller sheet",
        gnum(bounds.x0),
        gnum(bounds.x1),
        gnum(bounds.y0),
        gnum(bounds.y1),
        gnum(bed.0),
        gnum(bed.1)
    ))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!("laser.panel", "Laser…", "{} (asks the desktop app for its Laser window: dialog.laserPanel) → {dialog: true}", false, panel),
        spec!(
            "laser.gcode",
            "Export G-code for laser…",
            "{ids? (the layers, default the selection: frames, groups, shapes, vectors, text; a single stencil layer is exported as modifier.stencilGcode), kerfMm?, overlapMm?, label?, bedWidthMm?, bedHeightMm? (a single stencil layer's only, as modifier.stencilGcode), path? (the .gcode to write), data? (true: return it as base64), dryRun? (true: only describe it, nothing written or remembered; neither path, data nor dryRun: only asks the UI for its dialog, dialog.laserGcode {ids, material, gcode}, and returns {dialog: true}), material? (paper|film|card|custom: whose operation settings; default paper), mmPerUnit? (mm per FigCraft unit, 0.001-100; default 25.4/96, a CSS pixel, as SVG readers take FigCraft's SVG), widthMm? (scale the layers' bounds to this width instead), sMax?, originXMm?, originYMm?, air?, cut?, score?, engrave? (as modifier.stencilGcode)} (the layers' visible outlines in page space, mm, the page their bounds with the origin top-left: closed outlines cut (inner contours first, nearest neighbour, the largest outermost last), open paths cut along their line with the score settings, text as glyph outlines, modifier copies included; no kerf compensation, no sheet outline; a selected frame is the page, its own rectangle isn't cut) → {files: [{name, path | data, sheet (\"design\"), wMm, hMm, lines, estimatedSec, burnMm, travelMm, boundsMm, warnings}], sheets (1), warnings, material, mmPerUnit, gcode {sMax, originXMm, originYMm, air, cut, score, engrave} (the op settings are remembered per material)}",
            false,
            crate::picabia_laser_design::gcode
        ),
        spec!(
            "laser.ports",
            "List laser ports",
            "{} → {ports: [{path (what laser.connect takes), name (USB product and maker when known), usb, sim?}]} (USB first; on macOS only /dev/cu.*; the simulator {path: \"sim\", name: \"Simulated GRBL\", sim: true} in debug builds or with PICABIA_LASER_SIM=1; empty in the browser)",
            false,
            ports
        ),
        spec!(
            "laser.connect",
            "Connect to laser",
            "{port (a path from laser.ports, or \"sim\"), baud? (default 115200), sim? (testing aid, port \"sim\" only: {laserMode (bool, $32), sMax ($30), maxTravelMm [x, y, z] ($130-$132), speedFactor (moves this many times faster, 0.01-1e6), timeScale (the simulated clock runs this many times faster, 1-1000), alarmAtLine, errorAtLine (raise ALARM:1 / answer error:20 at that G-code line), disconnectAtLine (the port fails once the machine has read that many G-code lines), homing ($22), startInAlarm, bannerOnOpen, rapidMmMin}; default the owner's X-Carve: laser mode on, $30 = 1000)} (opens the port and starts the link; it then waits for the banner and reads $I and $$: poll laser.status until phase is ready; refused while a job runs) → laser.status",
            false,
            connect
        ),
        spec!(
            "laser.disconnect",
            "Disconnect laser",
            "{} (a running job is stopped first: hold, reset, M5; the port is closed) → laser.status",
            false,
            disconnect
        ),
        spec!(
            "laser.status",
            "Laser status",
            "{} → {connected, phase (closed|connecting|ready|lost), port, portName, sim, state (Idle|Run|Hold|Jog|Alarm|Door|Check|Home|Sleep|Unknown, null before the first report), pos {x, y, z} (work, mm) | null, mpos {x, y, z} | null, feed, power (S) | null, armed, armedSecLeft | null, stopping, job {name, state (running|paused|done|stopped|failed), line (lines answered), lines, sent, percent, elapsedSec, etaSec | null, estimatedSec | null, why?, failedLine?, source (stencil|file|testCard|powerLadder|frame), sheet | null, layer | null, boundsMm {x0, y0, x1, y1} | null, warnings} | null, alarm {code, text, advice} | null, settings {sMax ($30) | null, laserMode ($32) | null, maxTravel [x, y, z] | null, homing | null, softLimits | null, version | null, build | null, rxSize}, profile {name, bedWidthMm, bedHeightMm, sMax, rapidMmMin, air, checked, origin (head|machine)}, sMaxUsed (the machine's $30 when known, else the profile's), bedMm {w, h} (jobs must stay on it: the profile's bed, less where $130/$131 are smaller), canStart (everything but the arming and the job's size allows a start), startBlockedBy [plain reasons], lastError | null, warnings [plain], log [the last 50 lines in (\"< \") and out (\"> \")], simStats {burnMm, beamOnTravelMm, beamOnStillMs, movedMm, gcodeLines, overflows, maxRxUsed, resets} | null}",
            false,
            status
        ),
        spec!("laser.home", "Home laser", "{} (sends $H: the homing cycle) → laser.status + {sent}", false, home),
        spec!(
            "laser.unlock",
            "Unlock laser",
            "{} (sends $X: clears the alarm lock without homing) → laser.status + {sent, warning (the machine no longer knows where it is: home it or check the head's position before cutting)}",
            false,
            unlock
        ),
        spec!(
            "laser.jog",
            "Jog laser",
            "{dx?, dy? (mm, relative: $J=G91) | x?, y? (mm, work coordinates: $J=G90), feed? (mm/min, 1-20000; default the profile's rapid)} (beam off; refused while a job runs) → laser.status + {sent}",
            false,
            jog
        ),
        spec!("laser.jogCancel", "Cancel jog", "{} (0x85: stops a jog at once) → laser.status", false, jog_cancel),
        spec!(
            "laser.setOrigin",
            "Set laser origin",
            "{} (G10 L20 P1 X0 Y0: the head's position becomes the work origin, where a job's sheet has its bottom-left corner) → laser.status + {sent}",
            false,
            set_origin
        ),
        spec!(
            "laser.frame",
            "Frame laser job",
            "{ids?, sheet?, path?, testCard?, material?, mmPerUnit?, widthMm?, kerfMm?, overlapMm?, label?, sMax?, originXMm?, originYMm?, air?, cut?, score?, engrave?, powersPct?, speedsMmMin?, squareMm?, gapMm?, passes? (the job, as laser.start), beam? (false, the default: trace the job's bounding box with the beam off, as jogs at the profile's rapid speed; true: with the beam at 1% (M4 S = 1% of $30), which needs the laser armed in the desktop app)} (the machine must be Idle and the box on the bed, as laser.start checks it) → laser.status + {boundsMm, beam, sent, warnings}",
            false,
            frame
        ),
        spec!(
            "laser.start",
            "Start laser job",
            "{ids? (the layers to cut, default the selection: any layers, cut along their outlines as laser.gcode; a single stencil layer: its sheets), sheet? (a stencil's sheet, 1-based; needed when it has several), path? (a .gcode/.nc file instead), testCard? (card|powerLadder instead), material? (paper|film|card|custom; default the stencil's, else paper), mmPerUnit?, widthMm? (a design's scale, as laser.gcode), kerfMm?, overlapMm?, label? (a stencil's), sMax?, originXMm?, originYMm?, air?, cut?, score?, engrave? ({enabled, speedMmMin, powerPct, passes}: as modifier.stencilGcode; sMax is always the connected machine's $30 when it reports one), powersPct?, speedsMmMin?, squareMm?, gapMm?, passes? (the power ladder's)} (refused unless the laser was armed in the desktop app within 60 s (the attempt uses the arm up), the machine is ready and Idle, $32 = 1, and the job stays on the bed: with the profile's origin at the head (the default), the work offset plus the job's bounds must lie in the machine's travel (its sign convention taken from where the head is; when it can't be told, only what can be is checked, with a warning); with the origin at the machine's, the bounds in 0..bed) → laser.status + {started: {name, lines, estimatedSec, boundsMm, warnings}}",
            false,
            start
        ),
        spec!("laser.pause", "Pause laser", "{} (feed hold, !: always allowed; nothing happens when not connected) → laser.status", false, pause),
        spec!(
            "laser.resume",
            "Resume laser",
            "{} (~ after a hold; resuming a paused job turns the beam back on, so it needs the laser armed in the desktop app, as laser.start) → laser.status",
            false,
            resume
        ),
        spec!(
            "laser.stop",
            "Stop laser",
            "{} (always allowed: feed hold, then a soft reset once the machine has stopped (at most 0.5 s), then M5; a running job ends stopped; nothing happens when not connected) → laser.status",
            false,
            stop
        ),
        spec!(
            "laser.profile",
            "Laser machine profile",
            "{set? {name?, bedWidthMm?, bedHeightMm? (10-5000), sMax? (1-100000), rapidMmMin? (10-20000), air?, checked? (the values were checked on the machine), origin? (head: work 0,0 is where the person pressed Set origin, the sheet's bottom-left (default) | machine: work coordinates are the bed's), materials? {paper|film|card|custom: {cut?, score?, engrave?: {enabled?, speedMmMin?, powerPct?, passes?}}}} (merged into the saved profile)} → {profile {name, bedWidthMm, bedHeightMm, sMax, rapidMmMin, air, checked, origin, materials}, bedMm {w, h}, sMaxUsed, sMaxMachine | null, materialDefaults {paper|film|card|custom: {cut, score, engrave}} (the settings each material's export starts from)} (defaults: the owner's X-Carve, 584.2 × 1219.2 mm, $30 1000, rapid 3000 mm/min, no air: guesses until checked)",
            false,
            crate::picabia_laser_gcode::profile
        ),
    ]
}

/// The status plus `extra`'s fields.
fn status_with(s: &Session, extra: Value) -> Value {
    let mut v = status_json(s);
    if let (Some(o), Value::Object(e)) = (v.as_object_mut(), extra) {
        o.extend(e);
    }
    v
}

fn panel(s: &mut Session, _p: &Value) -> Result<Value> {
    s.request_ui("dialog.laserPanel", json!({}));
    Ok(json!({"dialog": true}))
}

fn status(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(status_json(s))
}

fn xyz(v: Option<[f64; 3]>) -> Value {
    v.map_or(Value::Null, |p| json!({"x": p[0], "y": p[1], "z": p[2]}))
}

/// `laser.status` (see its params doc).
pub fn status_json(s: &Session) -> Value {
    let link = &s.picabia_laser;
    let prof = MachineProfile::of(s);
    let (bw, bh) = bed_mm(s);
    let mut v = json!({
        "connected": false,
        "phase": "closed",
        "port": Value::Null,
        "portName": Value::Null,
        "sim": false,
        "state": Value::Null,
        "pos": Value::Null,
        "mpos": Value::Null,
        "feed": Value::Null,
        "power": Value::Null,
        "armed": link.armed_left().is_some(),
        "armedSecLeft": link.armed_left(),
        "stopping": false,
        "job": Value::Null,
        "alarm": Value::Null,
        "settings": {"sMax": Value::Null, "laserMode": Value::Null, "maxTravel": Value::Null, "homing": Value::Null, "softLimits": Value::Null, "version": Value::Null, "build": Value::Null, "rxSize": figcraft_grbl::RX_BUFFER},
        "profile": {"name": prof.name, "bedWidthMm": prof.bed_w_mm, "bedHeightMm": prof.bed_h_mm, "sMax": prof.s_max, "rapidMmMin": prof.rapid_mm_min, "air": prof.air, "checked": prof.checked, "origin": prof.origin},
        "sMaxUsed": link.machine_s_max().unwrap_or(prof.s_max),
        "bedMm": {"w": bw, "h": bh},
        "canStart": false,
        "startBlockedBy": [],
        "lastError": Value::Null,
        "warnings": [],
        "log": [],
        "simStats": Value::Null,
    });
    #[cfg(target_arch = "wasm32")]
    {
        v["startBlockedBy"] = json!([DESKTOP_ONLY]);
        v["warnings"] = json!([DESKTOP_ONLY]);
    }
    #[cfg(not(target_arch = "wasm32"))]
    native::fill_status(link, &prof, &mut v);
    v
}

/// The link, open and ready, or why not.
#[cfg(not(target_arch = "wasm32"))]
fn ready(cmd: &str, s: &Session) -> Result<()> {
    match s.picabia_laser.blocked_by_link() {
        None => Ok(()),
        Some(why) => Err(disabled(cmd, why)),
    }
}

fn ports(_s: &mut Session, _p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        Ok(json!({"ports": []}))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut list: Vec<Value> =
            figcraft_grbl::serial::list_ports().into_iter().map(|p| json!({"path": p.path, "name": p.name, "usb": p.usb})).collect();
        if native::sim_allowed() {
            list.push(json!({"path": SIM_PORT, "name": "Simulated GRBL", "usb": false, "sim": true}));
        }
        Ok(json!({"ports": list}))
    }
}

fn connect(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "laser.connect";
    let port = match get(p, "port") {
        Some(Value::String(port)) if !port.trim().is_empty() && port.len() <= 256 && !port.chars().any(char::is_control) => port.trim().to_string(),
        _ => return Err(bad(CMD, "`port` must be a port path from laser.ports")),
    };
    let baud = match get(p, "baud") {
        None => 0,
        Some(v) => match crate::params::f64_of(v) {
            Some(b) if b.fract() == 0.0 && (1200.0..=4_000_000.0).contains(&b) => b as u32,
            _ => return Err(bad(CMD, "`baud` must be a baud rate (1200-4000000)")),
        },
    };
    let sim = get(p, "sim");
    if sim.is_some() && port != SIM_PORT {
        return Err(bad(CMD, "`sim` options go with port \"sim\" only"));
    }
    if sim.is_some_and(|v| !v.is_object()) {
        return Err(bad(CMD, "`sim` must be an object of simulator options"));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, baud);
        Err(disabled(CMD, DESKTOP_ONLY))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if s.picabia_laser.job_active() {
            return Err(disabled(CMD, "a job is running: stop it before connecting again"));
        }
        s.picabia_laser.close(SHUTDOWN_BUDGET_MS);
        s.picabia_laser.job = None;
        s.picabia_laser.open(CMD, &port, baud, sim)?;
        Ok(status_json(s))
    }
}

fn disconnect(s: &mut Session, _p: &Value) -> Result<Value> {
    s.picabia_laser.close(DISCONNECT_BUDGET_MS);
    Ok(status_json(s))
}

/// Send one-off lines (refused while a job runs).
fn send_lines(cmd: &str, s: &mut Session, lines: &[String]) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, lines);
        Err(disabled(cmd, DESKTOP_ONLY))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ready(cmd, s)?;
        s.picabia_laser.command(lines.to_vec()).map_err(|why| disabled(cmd, why))?;
        Ok(json!({"sent": lines}))
    }
}

fn home(s: &mut Session, _p: &Value) -> Result<Value> {
    let r = send_lines("laser.home", s, &["$H".into()])?;
    Ok(status_with(s, r))
}

fn unlock(s: &mut Session, _p: &Value) -> Result<Value> {
    let mut r = send_lines("laser.unlock", s, &["$X".into()])?;
    r["warning"] = json!("Unlocked without homing: the machine no longer knows where it is. Home it, or check the head's position, before cutting.");
    Ok(status_with(s, r))
}

fn set_origin(s: &mut Session, _p: &Value) -> Result<Value> {
    let r = send_lines("laser.setOrigin", s, &["G10 L20 P1 X0 Y0".into()])?;
    Ok(status_with(s, r))
}

fn jog(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "laser.jog";
    let axis = |k: &str| -> Result<Option<f64>> {
        match get(p, k) {
            None => Ok(None),
            Some(_) => match num(p, k) {
                Some(v) if v.abs() <= MAX_JOG_MM => Ok(Some(v)),
                _ => Err(bad(CMD, format!("`{k}` must be a distance in mm (at most {MAX_JOG_MM})"))),
            },
        }
    };
    let (dx, dy, x, y) = (axis("dx")?, axis("dy")?, axis("x")?, axis("y")?);
    let relative = dx.is_some() || dy.is_some();
    let absolute = x.is_some() || y.is_some();
    if relative == absolute {
        return Err(bad(CMD, "give dx/dy (a relative jog) or x/y (to a work position), not both and not neither"));
    }
    let feed = match get(p, "feed") {
        None => MachineProfile::of(s).rapid_mm_min,
        Some(_) => match num(p, "feed") {
            Some(f) if (1.0..=20_000.0).contains(&f) => f,
            _ => return Err(bad(CMD, "`feed` must be a speed in mm/min (1-20000)")),
        },
    };
    let (mode, ax, ay) = if relative { ("G91", dx, dy) } else { ("G90", x, y) };
    let mut line = format!("$J={mode} G21");
    if let Some(v) = ax {
        line.push_str(&format!(" X{}", gnum(v)));
    }
    if let Some(v) = ay {
        line.push_str(&format!(" Y{}", gnum(v)));
    }
    line.push_str(&format!(" F{}", gnum(feed)));
    let r = send_lines(CMD, s, &[line])?;
    Ok(status_with(s, r))
}

fn jog_cancel(s: &mut Session, _p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
        Err(disabled("laser.jogCancel", DESKTOP_ONLY))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ready("laser.jogCancel", s)?;
        let _ = s.picabia_laser.act(native::Act::JogCancel);
        Ok(status_json(s))
    }
}

fn pause(s: &mut Session, _p: &Value) -> Result<Value> {
    // Not connected: nothing to pause, and that's fine.
    #[cfg(not(target_arch = "wasm32"))]
    let _ = s.picabia_laser.act(native::Act::Pause);
    Ok(status_json(s))
}

fn stop(s: &mut Session, _p: &Value) -> Result<Value> {
    // Not connected: nothing to stop, and that's fine.
    #[cfg(not(target_arch = "wasm32"))]
    let _ = s.picabia_laser.act(native::Act::Stop);
    Ok(status_json(s))
}

fn resume(s: &mut Session, _p: &Value) -> Result<Value> {
    const CMD: &str = "laser.resume";
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
        Err(disabled(CMD, DESKTOP_ONLY))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ready(CMD, s)?;
        // A paused job lights the beam again: that takes the person's arm, as a start does.
        if s.picabia_laser.job_active() && !s.picabia_laser.take_arm() {
            return Err(disabled(CMD, NOT_ARMED));
        }
        s.picabia_laser.act(native::Act::Resume).map_err(|why| disabled(CMD, why))?;
        Ok(status_json(s))
    }
}

/// Why a beam command is refused without the arm.
pub const NOT_ARMED: &str = "the laser isn't armed: in the desktop app's Laser panel, go through the safety checklist and press Start (arming lasts 60 s and one start; scripts and agents can't arm)";

/// The machine must be Idle, laser mode on, and `bounds` on the bed.
#[cfg(not(target_arch = "wasm32"))]
fn check_machine(cmd: &str, s: &Session, bounds: Rect, need_laser_mode: bool) -> Result<Vec<String>> {
    ready(cmd, s)?;
    if let Some(why) = s.picabia_laser.blocked_by_machine(need_laser_mode) {
        return Err(disabled(cmd, why));
    }
    let prof = MachineProfile::of(s);
    let machine = s.picabia_laser.machine_frame();
    check_placement(bounds, bed_mm(s), prof.origin_at_head(), &machine).map_err(|why| disabled(cmd, why))
}

/// What the machine says about where work coordinates sit: per axis (X, Y) the work offset
/// (machine = work + offset), the machine position now, the travel (`$130`/`$131`), and whether
/// homing is on (without it machine coordinates start wherever the head was at power-up).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MachineFrame {
    pub wco: Option<[f64; 2]>,
    pub mpos: Option<[f64; 2]>,
    pub travel: [Option<f64>; 2],
    pub homing: bool,
}

/// Whether a job whose G-code reaches `bounds` (work coordinates, mm) fits: never bigger than the
/// `bed`; with the origin at the head (`head`), placed in machine coordinates (work offset plus
/// bounds) within the travel, taking the machine's sign convention from where the head is (GRBL's
/// homed space is often negative: `-travel..0`); when that can't be told, the job must fit one of
/// the two conventions and a warning says only that much was checked; without homing, the work
/// offset or the travel, only the size is checked (with a warning). With the origin at the
/// machine's (`!head`), work coordinates are taken as the bed's: `bounds` within `0..bed`.
/// Returns the warnings, or why not.
pub fn check_placement(bounds: Rect, bed: (f64, f64), head: bool, m: &MachineFrame) -> std::result::Result<Vec<String>, String> {
    let t = BED_TOLERANCE_MM;
    let finite = [bounds.x0, bounds.y0, bounds.x1, bounds.y1].iter().all(|v| v.is_finite());
    if !finite {
        return Err("the job has no usable size".into());
    }
    let (w, h) = (bounds.width(), bounds.height());
    if w > bed.0 + t || h > bed.1 + t {
        return Err(format!(
            "the job is {} × {} mm, bigger than the bed ({} × {} mm): use a smaller sheet",
            gnum(w),
            gnum(h),
            gnum(bed.0),
            gnum(bed.1)
        ));
    }
    if !head {
        return check_bed(bounds, bed).map(|()| Vec::new());
    }
    let (Some(wco), true, [Some(tx), Some(ty)]) = (m.wco, m.homing, m.travel) else {
        return Ok(vec![
            "Picabia can't tell where the job sits on the bed (it needs homing, the work offset and the travel $130/$131): only its size was checked. Frame it before starting.".into(),
        ]);
    };
    let mut warnings = Vec::new();
    let axes = [("X", wco[0], tx, bounds.x0, bounds.x1, m.mpos.map(|p| p[0])), ("Y", wco[1], ty, bounds.y0, bounds.y1, m.mpos.map(|p| p[1]))];
    for (axis, off, travel, b0, b1, pos) in axes {
        let (lo, hi) = (off + b0, off + b1);
        let fits_pos = lo >= -t && hi <= travel + t;
        let fits_neg = lo >= -travel - t && hi <= t;
        let sign = pos.filter(|p| p.abs() > 0.5).map(|p| p > 0.0);
        let ok = match sign {
            Some(true) => fits_pos,
            Some(false) => fits_neg,
            None => {
                if fits_pos != fits_neg {
                    warnings.push(format!(
                        "The head is at machine {axis} 0, so Picabia can't tell which way the machine counts {axis}: the job fits one way. Frame it before starting."
                    ));
                }
                fits_pos || fits_neg
            }
        };
        if !ok {
            let range = match sign {
                Some(false) => format!("{} to 0", gnum(-travel)),
                _ => format!("0 to {}", gnum(travel)),
            };
            return Err(format!(
                "the job would reach machine {axis} {} to {}, off the machine's travel ({range} mm): jog to the sheet's bottom-left corner and press Set origin, or use a smaller sheet",
                gnum(lo),
                gnum(hi)
            ));
        }
    }
    Ok(warnings)
}

fn frame(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "laser.frame";
    let beam = match get(p, "beam") {
        None | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        Some(_) => return Err(bad(CMD, "`beam` must be true or false")),
    };
    // A beam frame uses the arm up whatever happens next.
    let armed = beam && s.picabia_laser.take_arm();
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p, armed);
        Err(disabled(CMD, DESKTOP_ONLY))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if beam && !armed {
            return Err(disabled(CMD, NOT_ARMED));
        }
        ready(CMD, s)?;
        let run = build_run(CMD, s, p)?;
        let b = run.bounds;
        let placement = check_machine(CMD, s, b, beam)?;
        let rapid = gnum(MachineProfile::of(s).rapid_mm_min);
        let corners = [(b.x0, b.y0), (b.x1, b.y0), (b.x1, b.y1), (b.x0, b.y1), (b.x0, b.y0)];
        let sent: Vec<String> = if beam {
            let s_max = s.picabia_laser.machine_s_max().unwrap_or(MachineProfile::of(s).s_max);
            let low = (s_max * 0.01).round().max(1.0);
            let mut lines =
                vec!["G21".to_string(), "G90".into(), "M5".into(), format!("G0 X{} Y{}", gnum(b.x0), gnum(b.y0)), format!("M4 S{}", gnum(low))];
            for (i, (x, y)) in corners.iter().enumerate().skip(1) {
                let f = if i == 1 { format!(" F{rapid}") } else { String::new() };
                lines.push(format!("G1 X{} Y{}{f}", gnum(*x), gnum(*y)));
            }
            lines.push("M5".into());
            s.picabia_laser.start_job(format!("Frame of {} (beam at 1%)", run.name), lines.clone()).map_err(|why| disabled(CMD, why))?;
            s.picabia_laser.job = Some(JobInfo {
                source: "frame",
                sheet: run.sheet.clone(),
                layer: run.layer,
                estimated_sec: None,
                bounds: Some(b),
                warnings: Vec::new(),
            });
            lines
        } else {
            let mut lines = vec!["M5".to_string()];
            lines.extend(corners.iter().map(|(x, y)| format!("$J=G90 G21 X{} Y{} F{rapid}", gnum(*x), gnum(*y))));
            s.picabia_laser.command(lines.clone()).map_err(|why| disabled(CMD, why))?;
            lines
        };
        Ok(status_with(s, json!({"boundsMm": rect_json(b), "beam": beam, "sent": sent, "warnings": placement})))
    }
}

fn start(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "laser.start";
    // The attempt uses the arm up, whether it starts or not.
    let armed = s.picabia_laser.take_arm();
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p, armed);
        Err(disabled(CMD, DESKTOP_ONLY))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !armed {
            return Err(disabled(CMD, NOT_ARMED));
        }
        ready(CMD, s)?;
        let run: LaserRun = build_run(CMD, s, p)?;
        let placement = check_machine(CMD, s, run.bounds, true)?;
        let mut run = run;
        run.warnings.extend(placement);
        let lines: Vec<String> = run.text.lines().map(str::to_string).collect();
        let count = lines.len();
        s.picabia_laser.start_job(run.name.clone(), lines).map_err(|why| disabled(CMD, why))?;
        let started = json!({"name": run.name.clone(), "lines": count, "estimatedSec": run.estimated_sec, "boundsMm": rect_json(run.bounds), "warnings": run.warnings});
        s.picabia_laser.job = Some(JobInfo {
            source: run.source,
            sheet: run.sheet,
            layer: run.layer,
            estimated_sec: run.estimated_sec,
            bounds: Some(run.bounds),
            warnings: run.warnings,
        });
        Ok(status_with(s, json!({"started": started})))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod native {
    //! The worker thread and the port it owns.

    use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use figcraft_grbl::sim::{SimGrbl, SimOptions, SimStats, SimTransport};
    use figcraft_grbl::{Controller, HOLD, JobState, MachineState, Phase, RESET, Snapshot, Transport};
    use serde_json::{Value, json};

    use super::{LaserLink, MachineProfile, SHUTDOWN_BUDGET_MS, SIM_PORT, STATUS_LOG_LINES, xyz};
    use crate::{EngineError, Result};

    /// What the worker publishes at most this often while nothing changes (ms).
    const PUBLISH_MS: u64 = 10;
    /// How long a command waits for the worker's answer.
    const REPLY_TIMEOUT: Duration = Duration::from_millis(1000);

    /// The simulator is a port in debug builds, or with `PICABIA_LASER_SIM=1`.
    pub fn sim_allowed() -> bool {
        cfg!(debug_assertions) || std::env::var("PICABIA_LASER_SIM").is_ok_and(|v| v.trim() == "1")
    }

    /// The worker's answer: done, or refused (the reason).
    type Reply = Sender<std::result::Result<(), String>>;

    /// A realtime action.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Act {
        Pause,
        Resume,
        Stop,
        JogCancel,
    }

    /// A message to the worker. Answers go out after the new state is published, so a command
    /// returns the state its message made.
    pub enum Msg {
        /// One-off lines.
        Command(Vec<String>, Reply),
        Start {
            name: String,
            lines: Vec<String>,
            reply: Reply,
        },
        /// A realtime action, answered when `ack` is given.
        Act(Act, Option<Reply>),
        /// Stop a running job (waiting at most `budget_ms` for the stop sequence), close the port,
        /// end the worker.
        Close {
            budget_ms: u64,
        },
    }

    /// The worker's state as the commands see it.
    #[derive(Default)]
    pub struct Shared {
        pub snap: Snapshot,
        /// The worker's clock (ms since the port opened): the job's times are on it.
        pub now_ms: u64,
        pub sim: Option<SimStats>,
        /// The worker is running.
        pub running: bool,
        /// Why the port went away.
        pub io_error: Option<String>,
    }

    /// The port the worker owns.
    pub trait LinkPort: Transport {
        fn sim_stats(&self) -> Option<SimStats> {
            None
        }
    }

    impl LinkPort for figcraft_grbl::serial::SerialTransport {}

    /// The simulator as a port: a real clock (sped up `time_scale` times), a short sleep when
    /// nothing arrived (a serial read waits for bytes; the simulator answers at once), and an
    /// optional pulled cable.
    struct SimPort {
        inner: SimTransport,
        disconnect_at_line: Option<usize>,
        time_scale: u64,
    }

    impl Transport for SimPort {
        fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
            self.cable()?;
            self.inner.write(data)
        }

        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.cable()?;
            let n = self.inner.read(buf)?;
            if n == 0 {
                let sim = self.inner.sim();
                let busy = sim.state() != MachineState::Idle || sim.rx_used() > 0 || sim.planner_len() > 0;
                // A sped-up clock answers within microseconds: yield rather than sleep (a short
                // sleep lasts about a millisecond on some systems). At real time, wait one
                // simulated millisecond.
                if busy && self.time_scale >= 10 {
                    std::thread::yield_now();
                } else {
                    std::thread::sleep(Duration::from_micros(if busy { 1000 / self.time_scale.max(1) } else { 2000 }));
                }
            }
            Ok(n)
        }

        fn name(&self) -> &str {
            self.inner.name()
        }
    }

    impl SimPort {
        fn cable(&self) -> std::io::Result<()> {
            match self.disconnect_at_line {
                Some(n) if self.inner.sim().stats().gcode_lines >= n => {
                    Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the simulated cable was pulled"))
                }
                _ => Ok(()),
            }
        }
    }

    impl LinkPort for SimPort {
        fn sim_stats(&self) -> Option<SimStats> {
            Some(self.inner.sim().stats().clone())
        }
    }

    /// The simulator's options from `laser.connect {sim}` (lenient: wrong types are ignored).
    fn sim_options(v: Option<&Value>) -> (SimOptions, Option<usize>, u64) {
        let mut o = SimOptions::default();
        let Some(m) = v.and_then(Value::as_object) else { return (o, None, 1) };
        let num = |k: &str| m.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        let line = |k: &str| num(k).filter(|x| *x >= 1.0 && *x <= 1e9).map(|x| x as usize);
        let flag = |k: &str| m.get(k).and_then(Value::as_bool);
        if let Some(b) = flag("laserMode") {
            o.laser_mode = b;
        }
        if let Some(x) = num("sMax") {
            o.s_max = x.clamp(1.0, 100_000.0);
        }
        if let Some(a) = m.get("maxTravelMm").and_then(Value::as_array) {
            for (i, t) in a.iter().take(3).enumerate() {
                if let (Some(t), Some(slot)) = (t.as_f64().filter(|x| x.is_finite()), o.max_travel.get_mut(i)) {
                    *slot = t.clamp(1.0, 10_000.0);
                }
            }
        }
        if let Some(x) = num("speedFactor") {
            o.speed_factor = x.clamp(0.01, 1e6);
        }
        if let Some(x) = num("rapidMmMin") {
            o.rapid_mm_min = x.clamp(1.0, 100_000.0);
        }
        o.alarm_at_line = line("alarmAtLine");
        o.error_at_line = line("errorAtLine");
        if let Some(b) = flag("homing") {
            o.homing = b;
        }
        if let Some(b) = flag("startInAlarm") {
            o.start_in_alarm = b;
        }
        if let Some(b) = flag("bannerOnOpen") {
            o.banner_on_open = b;
        }
        let scale = num("timeScale").map_or(1, |x| x.clamp(1.0, 1000.0) as u64);
        (o, line("disconnectAtLine"), scale)
    }

    /// The running worker.
    pub struct Worker {
        tx: Sender<Msg>,
        shared: Arc<Mutex<Shared>>,
        handle: Option<JoinHandle<()>>,
        pub port: String,
        pub port_name: String,
        pub sim: bool,
    }

    fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
        m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    impl Drop for LaserLink {
        fn drop(&mut self) {
            self.close(SHUTDOWN_BUDGET_MS);
        }
    }

    impl LaserLink {
        /// Open `port` and start the worker.
        pub(super) fn open(&mut self, cmd: &str, port: &str, baud: u32, sim: Option<&Value>) -> Result<()> {
            let (link, name, is_sim): (Box<dyn LinkPort>, String, bool) = if port == SIM_PORT {
                if !sim_allowed() {
                    return Err(EngineError::Disabled(cmd.into(), "the simulator is available in debug builds or with PICABIA_LASER_SIM=1".into()));
                }
                let (opt, disconnect_at_line, time_scale) = sim_options(sim);
                let t0 = Instant::now();
                let clock = Box::new(move || u64::try_from(t0.elapsed().as_micros()).unwrap_or(u64::MAX).saturating_mul(time_scale) / 1000);
                let inner = SimTransport::new(SimGrbl::new(opt), clock);
                (Box::new(SimPort { inner, disconnect_at_line, time_scale }), "Simulated GRBL".into(), true)
            } else {
                let t = figcraft_grbl::serial::open(port, baud).map_err(|e| EngineError::Other(format!("{cmd}: couldn't open {port}: {e}")))?;
                let name = figcraft_grbl::serial::list_ports().into_iter().find(|p| p.path == port).map_or_else(|| port.to_string(), |p| p.name);
                (Box::new(t), name, false)
            };
            let shared = Arc::new(Mutex::new(Shared { running: true, ..Shared::default() }));
            let (tx, rx) = channel();
            let worker_shared = shared.clone();
            let handle = std::thread::Builder::new()
                .name("picabia-laser".into())
                .spawn(move || run(link, rx, worker_shared))
                .map_err(|e| EngineError::Other(format!("{cmd}: couldn't start the laser link: {e}")))?;
            self.worker = Some(Worker { tx, shared, handle: Some(handle), port: port.to_string(), port_name: name, sim: is_sim });
            Ok(())
        }

        /// The worker is running.
        pub fn is_open(&self) -> bool {
            self.worker.as_ref().is_some_and(|w| lock(&w.shared).running)
        }

        /// Read the shared state.
        fn with<R>(&self, f: impl FnOnce(&Shared) -> R) -> Option<R> {
            self.worker.as_ref().map(|w| f(&lock(&w.shared)))
        }

        /// Send `msg` to a running worker (false: there is none).
        pub(super) fn post(&self, msg: Msg) -> bool {
            self.is_open() && self.worker.as_ref().is_some_and(|w| w.tx.send(msg).is_ok())
        }

        fn ask(&self, msg: impl FnOnce(Reply) -> Msg) -> std::result::Result<(), String> {
            let (tx, rx) = channel();
            if !self.post(msg(tx)) {
                return Err("the laser isn't connected".into());
            }
            rx.recv_timeout(REPLY_TIMEOUT).unwrap_or_else(|_| Err("the laser link didn't answer".into()))
        }

        /// Queue one-off lines.
        pub(super) fn command(&self, lines: Vec<String>) -> std::result::Result<(), String> {
            self.ask(|reply| Msg::Command(lines, reply))
        }

        /// Start streaming a job.
        pub(super) fn start_job(&self, name: String, lines: Vec<String>) -> std::result::Result<(), String> {
            self.ask(|reply| Msg::Start { name, lines, reply })
        }

        /// Do `a` and wait (at most a second) until it is done.
        pub(super) fn act(&self, a: Act) -> std::result::Result<(), String> {
            self.ask(|reply| Msg::Act(a, Some(reply)))
        }

        /// Stop a running job or motion (never blocks).
        pub(super) fn stop(&mut self) {
            self.post(Msg::Act(Act::Stop, None));
        }

        /// Stop what runs (waiting at most about `budget_ms`), close the port, end the worker.
        pub(super) fn close(&mut self, budget_ms: u64) {
            let Some(mut w) = self.worker.take() else { return };
            let _ = w.tx.send(Msg::Close { budget_ms });
            let deadline = Instant::now() + Duration::from_millis(budget_ms + 150);
            if let Some(h) = w.handle.take() {
                while !h.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
                if h.is_finished() {
                    let _ = h.join();
                }
            }
            // Keep the last state for laser.status (phase, the job's end, the log).
            lock(&w.shared).running = false;
            self.worker = Some(Worker { handle: None, ..w });
        }

        /// A job is running or paused.
        pub(super) fn job_active(&self) -> bool {
            self.is_open() && self.with(|sh| sh.snap.job.as_ref().is_some_and(|j| j.state.is_active())).unwrap_or(false)
        }

        /// The connected machine's `$30`.
        pub(crate) fn machine_s_max(&self) -> Option<f64> {
            if !self.is_open() {
                return None;
            }
            self.with(|sh| sh.snap.settings.s_max().filter(|v| v.is_finite() && *v > 0.0)).flatten()
        }

        /// The connected machine's `$130`, `$131`.
        pub(crate) fn machine_travel(&self) -> (Option<f64>, Option<f64>) {
            if !self.is_open() {
                return (None, None);
            }
            self.with(|sh| (sh.snap.settings.get(130), sh.snap.settings.get(131))).unwrap_or((None, None))
        }

        /// Where work coordinates sit on the machine (see [`super::MachineFrame`]).
        pub(crate) fn machine_frame(&self) -> super::MachineFrame {
            if !self.is_open() {
                return super::MachineFrame::default();
            }
            self.with(|sh| {
                let st = sh.snap.status.as_ref();
                let mpos = st.and_then(|s| s.mpos);
                let wpos = st.and_then(|s| s.wpos);
                let wco = st.and_then(|s| s.wco).or_else(|| mpos.zip(wpos).map(|(m, w)| [m[0] - w[0], m[1] - w[1], m[2] - w[2]]));
                let travel = |n: u16| sh.snap.settings.get(n).filter(|t| t.is_finite() && *t > 0.0);
                super::MachineFrame {
                    wco: wco.map(|w| [w[0], w[1]]),
                    mpos: mpos.map(|m| [m[0], m[1]]),
                    travel: [travel(130), travel(131)],
                    homing: sh.snap.settings.homing() == Some(true),
                }
            })
            .unwrap_or_default()
        }

        /// Why nothing can be sent now (not connected, still connecting, stopping).
        pub(super) fn blocked_by_link(&self) -> Option<String> {
            if !self.is_open() {
                return Some(match self.with(|sh| sh.io_error.clone()).flatten() {
                    Some(e) => format!("the laser isn't connected ({e})"),
                    None => "the laser isn't connected".into(),
                });
            }
            self.with(|sh| {
                if sh.snap.phase != Phase::Ready {
                    Some("the laser is still connecting (reading its settings)".to_string())
                } else if sh.snap.stopping {
                    Some("the laser is stopping; wait until it is back".to_string())
                } else {
                    None
                }
            })
            .flatten()
        }

        /// Why a job or frame can't start now on the machine's side.
        pub(super) fn blocked_by_machine(&self, need_laser_mode: bool) -> Option<String> {
            self.with(|sh| machine_blocks(&sh.snap, need_laser_mode).into_iter().next()).flatten()
        }
    }

    /// Everything on the machine's side that keeps a job from starting.
    fn machine_blocks(snap: &Snapshot, need_laser_mode: bool) -> Vec<String> {
        let mut why = Vec::new();
        if snap.job.as_ref().is_some_and(|j| j.state.is_active()) {
            why.push("a job is running: pause or stop it first".to_string());
        }
        let state = snap.state();
        if state != MachineState::Idle {
            why.push(match state {
                MachineState::Alarm => "the machine is in alarm: home it (or unlock it) first".to_string(),
                MachineState::Unknown => "the machine hasn't reported its state yet".to_string(),
                s => format!("the machine must be Idle to start (it is {})", s.as_str()),
            });
        }
        if need_laser_mode {
            match snap.settings.laser_mode() {
                Some(true) => {}
                Some(false) => why.push(figcraft_grbl::Refusal::LaserModeOff.to_string()),
                None => why.push(figcraft_grbl::Refusal::LaserModeUnknown.to_string()),
            }
        }
        why
    }

    /// Fill `laser.status` from the worker's state.
    pub(super) fn fill_status(link: &LaserLink, prof: &MachineProfile, v: &mut Value) {
        let Some(w) = link.worker.as_ref() else {
            v["startBlockedBy"] = json!(["the laser isn't connected"]);
            return;
        };
        let sh = lock(&w.shared);
        let snap = &sh.snap;
        let open = sh.running;
        let phase = if open || snap.phase == Phase::Lost { snap.phase.as_str() } else { "closed" };
        v["connected"] = json!(open && matches!(snap.phase, Phase::Connecting | Phase::Ready));
        v["phase"] = json!(phase);
        v["port"] = json!(w.port);
        v["portName"] = json!(w.port_name);
        v["sim"] = json!(w.sim);
        if let Some(st) = &snap.status {
            v["state"] = json!(st.state.as_str());
            v["pos"] = xyz(st.wpos);
            v["mpos"] = xyz(st.mpos);
            v["feed"] = json!(st.feed);
            v["power"] = json!(st.spindle);
        }
        v["stopping"] = json!(snap.stopping);
        if let Some(a) = snap.alarm {
            v["alarm"] = json!({"code": a.code, "text": a.text, "advice": a.advice});
        }
        let set = &snap.settings;
        v["settings"] = json!({
            "sMax": set.s_max(),
            "laserMode": set.laser_mode(),
            "maxTravel": set.max_travel(),
            "homing": set.homing(),
            "softLimits": set.soft_limits(),
            "version": snap.version,
            "build": snap.build,
            "rxSize": snap.rx_size,
        });
        if let Some(j) = &snap.job {
            let info = link.job.clone().unwrap_or_default();
            let end = j.ended_ms.unwrap_or(sh.now_ms);
            let elapsed = end.saturating_sub(j.started_ms) as f64 / 1000.0;
            let percent = if j.lines == 0 { 0.0 } else { j.acked as f64 * 100.0 / j.lines as f64 };
            let percent = if j.state == JobState::Done { 100.0 } else { (percent * 10.0).round() / 10.0 };
            let eta = if !j.state.is_active() {
                Some(0.0)
            } else if percent >= 5.0 {
                Some(elapsed * (100.0 - percent) / percent)
            } else {
                info.estimated_sec.map(|e| (e - elapsed).max(0.0))
            };
            let mut job = json!({
                "name": j.name,
                "state": j.state.as_str(),
                "line": j.acked,
                "lines": j.lines,
                "sent": j.sent,
                "percent": percent,
                "elapsedSec": (elapsed * 10.0).round() / 10.0,
                "etaSec": eta.map(|e| (e * 10.0).round() / 10.0),
                "estimatedSec": info.estimated_sec,
                "source": if info.source.is_empty() { "file" } else { info.source },
                "sheet": info.sheet,
                "layer": info.layer.map(|id| json!(id)),
                "boundsMm": info.bounds.map_or(Value::Null, super::rect_json),
                "warnings": info.warnings,
            });
            if let JobState::Failed { line, why } = &j.state {
                job["why"] = json!(why);
                job["failedLine"] = json!(line);
            }
            v["job"] = job;
        }
        v["lastError"] = json!(snap.last_error.clone().or_else(|| sh.io_error.clone()));
        let skip = snap.log.len().saturating_sub(STATUS_LOG_LINES);
        v["log"] = json!(snap.log.iter().skip(skip).collect::<Vec<_>>());
        if let Some(st) = &sh.sim {
            v["simStats"] = json!({
                "burnMm": st.burn_mm,
                "beamOnTravelMm": st.beam_on_travel_mm,
                "beamOnStillMs": st.beam_on_still_ms,
                "movedMm": st.moved_mm,
                "gcodeLines": st.gcode_lines,
                "overflows": st.overflows,
                "maxRxUsed": st.max_rx_used,
                "resets": st.resets,
            });
        }
        // What blocks a start, and what to know.
        let mut blocked = Vec::new();
        if !open {
            blocked.push("the laser isn't connected".to_string());
        } else if snap.phase != Phase::Ready {
            blocked.push("the laser is still connecting (reading its settings)".to_string());
        } else {
            if snap.stopping {
                blocked.push("the laser is stopping".to_string());
            }
            blocked.extend(machine_blocks(snap, true));
        }
        v["canStart"] = json!(blocked.is_empty());
        v["startBlockedBy"] = json!(blocked);
        let mut warnings = Vec::new();
        if open && snap.phase == Phase::Ready {
            match set.laser_mode() {
                Some(false) => warnings.push(figcraft_grbl::Refusal::LaserModeOff.to_string()),
                None => warnings.push(figcraft_grbl::Refusal::LaserModeUnknown.to_string()),
                Some(true) => {}
            }
            if let Some(m) = set.s_max()
                && (m - prof.s_max).abs() > 1e-9
            {
                warnings.push(format!("The machine's $30 is {m}; the profile says {}: jobs use the machine's.", prof.s_max));
            }
            for (axis, n, bed) in [("X", 130, prof.bed_w_mm), ("Y", 131, prof.bed_h_mm)] {
                if let Some(t) = set.get(n)
                    && t > 0.0
                    && t + super::BED_TOLERANCE_MM < bed
                {
                    warnings.push(format!(
                        "The machine's {axis} travel (${n} = {t} mm) is less than the profile's bed ({bed} mm): jobs must fit the travel."
                    ));
                }
            }
        }
        if !prof.checked {
            warnings.push("The machine profile's values are guesses until checked on the machine.".to_string());
        }
        v["warnings"] = json!(warnings);
    }

    /// The worker: pump the port as bytes arrive, handle messages, publish the state.
    fn run(mut port: Box<dyn LinkPort>, rx: Receiver<Msg>, shared: Arc<Mutex<Shared>>) {
        let t0 = Instant::now();
        let now = move || u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
        let mut ctrl = Controller::new();
        ctrl.on_connected(now());
        let mut last_key = None;
        let mut last_pub = 0u64;
        let mut replies: Vec<(Reply, std::result::Result<(), String>)> = Vec::new();
        let mut buf = vec![0u8; 4096];
        loop {
            let mut changed = false;
            loop {
                match rx.try_recv() {
                    Ok(Msg::Close { budget_ms }) => {
                        close(&mut ctrl, port.as_mut(), budget_ms, &now);
                        publish(&ctrl, port.as_ref(), &shared, now(), false, None);
                        return;
                    }
                    Ok(m) => {
                        replies.extend(handle(&mut ctrl, m));
                        changed = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        close(&mut ctrl, port.as_mut(), SHUTDOWN_BUDGET_MS, &now);
                        publish(&ctrl, port.as_ref(), &shared, now(), false, None);
                        return;
                    }
                }
            }
            if changed {
                publish(&ctrl, port.as_ref(), &shared, now(), true, None);
                for (reply, r) in replies.drain(..) {
                    let _ = reply.send(r);
                }
            }
            if let Err(e) = step(&mut ctrl, port.as_mut(), &mut buf, now()) {
                ctrl.on_disconnect();
                publish(&ctrl, port.as_ref(), &shared, now(), false, Some(format!("the port failed: {e}")));
                return;
            }
            if ctrl.phase() == Phase::Lost {
                publish(&ctrl, port.as_ref(), &shared, now(), false, None);
                return;
            }
            let t = now();
            let key = (ctrl.phase(), ctrl.job().map(|j| j.state.clone()));
            if last_key.as_ref() != Some(&key) || t >= last_pub + PUBLISH_MS {
                publish(&ctrl, port.as_ref(), &shared, t, true, None);
                last_key = Some(key);
                last_pub = t;
            }
        }
    }

    fn publish(ctrl: &Controller, port: &dyn LinkPort, shared: &Mutex<Shared>, now_ms: u64, running: bool, io_error: Option<String>) {
        let snap = ctrl.snapshot();
        let sim = port.sim_stats();
        let mut sh = lock(shared);
        sh.snap = snap;
        sh.now_ms = now_ms;
        sh.sim = sim;
        sh.running = running;
        if io_error.is_some() {
            sh.io_error = io_error;
        }
    }

    /// One I/O step: one read (a serial read returns as soon as bytes arrive, else after its 20 ms
    /// timeout), then write what the controller wants at once. (Reading until a read comes back
    /// empty would wait out the timeout after every answer before refilling the machine's buffer.)
    fn step(ctrl: &mut Controller, port: &mut dyn LinkPort, buf: &mut [u8], now_ms: u64) -> std::io::Result<()> {
        let n = port.read(buf)?;
        if let Some(chunk) = buf.get(..n).filter(|c| !c.is_empty()) {
            ctrl.on_bytes(chunk, now_ms);
        }
        let out = ctrl.poll(now_ms);
        if !out.is_empty() {
            port.write(&out)?;
        }
        Ok(())
    }

    /// Do what `msg` asks; the answer to send once the state is published.
    fn handle(ctrl: &mut Controller, msg: Msg) -> Option<(Reply, std::result::Result<(), String>)> {
        match msg {
            Msg::Command(lines, reply) => {
                let mut r = Ok(());
                for line in &lines {
                    if let Err(e) = ctrl.command(line) {
                        r = Err(e.to_string());
                        break;
                    }
                }
                Some((reply, r))
            }
            Msg::Start { name, lines, reply } => Some((reply, ctrl.start_job(&name, lines).map_err(|e| e.to_string()))),
            Msg::Act(a, ack) => {
                match a {
                    Act::Pause => ctrl.pause(),
                    Act::Resume => ctrl.resume(),
                    Act::Stop => ctrl.stop(),
                    Act::JogCancel => ctrl.jog_cancel(),
                }
                ack.map(|r| (r, Ok(())))
            }
            // Handled by the loop.
            Msg::Close { .. } => None,
        }
    }

    /// Stop what the machine is doing before the port closes: the stop sequence, given at most
    /// `budget_ms`; if it hasn't finished, hold and reset outright (the reset turns the laser off).
    fn close(ctrl: &mut Controller, port: &mut dyn LinkPort, budget_ms: u64, now: &dyn Fn() -> u64) {
        let snap = ctrl.snapshot();
        let moving = snap.job.as_ref().is_some_and(|j| j.state.is_active())
            || snap.stopping
            || matches!(snap.state(), MachineState::Run | MachineState::Jog | MachineState::Hold | MachineState::Home);
        if !moving || !matches!(ctrl.phase(), Phase::Ready | Phase::Connecting) {
            return;
        }
        ctrl.stop();
        let mut buf = [0u8; 1024];
        let end = now().saturating_add(budget_ms);
        let mut done = false;
        while now() < end {
            if step(ctrl, port, &mut buf, now()).is_err() {
                break;
            }
            if !ctrl.snapshot().stopping {
                // The banner is back: send the M5 that follows it.
                let _ = step(ctrl, port, &mut buf, now());
                done = true;
                break;
            }
        }
        if !done {
            let _ = port.write(&[HOLD, RESET]);
        }
    }
}
