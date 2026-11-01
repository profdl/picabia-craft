//! PICABIA: `laser.status` as the Laser window reads it (leniently: junk reads as nothing), and
//! the words and colours it shows for it.

use egui::Color32;
use serde_json::Value;

use crate::i18n::{tr, trf};
use crate::theme::Tokens;

/// The running (or last) job.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Job {
    pub name: String,
    /// running | paused | done | stopped | failed.
    pub state: String,
    pub line: u64,
    pub lines: u64,
    pub percent: f64,
    pub elapsed: f64,
    pub eta: Option<f64>,
    pub why: Option<String>,
    pub failed_line: Option<u64>,
    pub source: String,
}

impl Job {
    /// Running or paused: the machine is the job's.
    pub fn active(&self) -> bool {
        matches!(self.state.as_str(), "running" | "paused")
    }
}

/// An alarm: its code, what it means and what to do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Alarm {
    pub code: u64,
    pub text: String,
    pub advice: String,
}

/// `laser.status`, read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub connected: bool,
    /// closed | connecting | ready | lost.
    pub phase: String,
    pub port: Option<String>,
    pub port_name: Option<String>,
    pub sim: bool,
    /// Idle, Run, Hold, Jog, Alarm, Door, Check, Home, Sleep, Unknown (`None` before the first
    /// report).
    pub state: Option<String>,
    /// Work position, mm.
    pub pos: Option<[f64; 3]>,
    pub stopping: bool,
    pub job: Option<Job>,
    pub alarm: Option<Alarm>,
    pub can_start: bool,
    pub blocked_by: Vec<String>,
    pub last_error: Option<String>,
    pub warnings: Vec<String>,
    pub log: Vec<String>,
    /// The machine's `$30`, `$32` and firmware version, when it reported them.
    pub s_max_machine: Option<f64>,
    pub laser_mode: Option<bool>,
    pub version: Option<String>,
    /// The machine profile (`laser.profile`'s fields).
    pub profile: Value,
    /// The bed jobs must stay on, mm.
    pub bed: Option<(f64, f64)>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite())
}

fn strings(v: &Value, k: &str, cap: usize) -> Vec<String> {
    v.get(k).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).take(cap).map(str::to_string).collect()
}

impl Status {
    /// Read `laser.status`'s JSON.
    pub fn from_json(v: &Value) -> Status {
        let pos = v.get("pos").and_then(|p| Some([f(p, "x")?, f(p, "y")?, f(p, "z").unwrap_or(0.0)]));
        let job = v.get("job").filter(|j| j.is_object()).map(|j| Job {
            name: s(j, "name").unwrap_or_default(),
            state: s(j, "state").unwrap_or_default(),
            line: j.get("line").and_then(Value::as_u64).unwrap_or(0),
            lines: j.get("lines").and_then(Value::as_u64).unwrap_or(0),
            percent: f(j, "percent").unwrap_or(0.0).clamp(0.0, 100.0),
            elapsed: f(j, "elapsedSec").unwrap_or(0.0).max(0.0),
            eta: f(j, "etaSec").map(|e| e.max(0.0)),
            why: s(j, "why"),
            failed_line: j.get("failedLine").and_then(Value::as_u64),
            source: s(j, "source").unwrap_or_default(),
        });
        let alarm = v.get("alarm").filter(|a| a.is_object()).map(|a| Alarm {
            code: a.get("code").and_then(Value::as_u64).unwrap_or(0),
            text: s(a, "text").unwrap_or_default(),
            advice: s(a, "advice").unwrap_or_default(),
        });
        let settings = v.get("settings").cloned().unwrap_or(Value::Null);
        let bed = v.get("bedMm").and_then(|b| Some((f(b, "w")?, f(b, "h")?)));
        Status {
            connected: v.get("connected").and_then(Value::as_bool).unwrap_or(false),
            phase: s(v, "phase").unwrap_or_else(|| "closed".into()),
            port: s(v, "port"),
            port_name: s(v, "portName"),
            sim: v.get("sim").and_then(Value::as_bool).unwrap_or(false),
            state: s(v, "state"),
            pos,
            stopping: v.get("stopping").and_then(Value::as_bool).unwrap_or(false),
            job,
            alarm,
            can_start: v.get("canStart").and_then(Value::as_bool).unwrap_or(false),
            blocked_by: strings(v, "startBlockedBy", 16),
            last_error: s(v, "lastError"),
            warnings: strings(v, "warnings", 16),
            log: strings(v, "log", 200),
            s_max_machine: f(&settings, "sMax"),
            laser_mode: settings.get("laserMode").and_then(Value::as_bool),
            version: s(&settings, "version"),
            profile: v.get("profile").cloned().unwrap_or(Value::Null),
            bed,
        }
    }

    /// The link is open (connecting or ready).
    pub fn open(&self) -> bool {
        self.connected || self.phase == "connecting"
    }

    /// A job is running or paused.
    pub fn job_active(&self) -> bool {
        self.job.as_ref().is_some_and(Job::active)
    }

    /// The machine is moving or holding a move (a job, a jog, homing, a frame).
    pub fn moving(&self) -> bool {
        matches!(self.state.as_deref(), Some("Run" | "Jog" | "Home" | "Hold"))
    }

    /// The emergency stop (Shift+Esc) has something to stop.
    pub fn stoppable(&self) -> bool {
        self.open() && (self.job_active() || self.moving() || self.stopping)
    }

    /// A paused job (resuming it needs the short confirm).
    pub fn paused(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.state == "paused")
    }
}

/// `c` as text: the theme's warning yellow is too light to read on a light panel and its success
/// green too dark on a dark one, so text in them moves part way to the text colour there.
pub fn readable(c: Color32, t: &Tokens) -> Color32 {
    if (!t.dark && c == t.warning) || (t.dark && c == t.success) { c.lerp_to_gamma(t.text, 0.55) } else { c }
}

/// What the state badge says, and its colour.
pub fn badge(st: &Status, t: &Tokens) -> (String, Color32) {
    if !st.open() {
        let text = if st.phase == "lost" { tr("Connection lost") } else { tr("Not connected") };
        return (text.to_string(), if st.phase == "lost" { t.danger } else { t.text_secondary });
    }
    if st.phase == "connecting" {
        return (tr("Connecting…").to_string(), t.text_secondary);
    }
    if st.stopping {
        return (tr("Stopping…").to_string(), t.warning);
    }
    let Some(state) = st.state.as_deref() else { return (tr("Waiting for the machine…").to_string(), t.text_secondary) };
    let (label, c) = match state {
        "Idle" => (tr("Idle"), t.success),
        "Run" => (tr("Running"), t.accent),
        "Jog" => (tr("Jogging"), t.accent),
        "Home" => (tr("Homing"), t.accent),
        "Hold" => (tr("Paused"), t.warning),
        "Door" => (tr("Door open"), t.warning),
        "Alarm" => (tr("Alarm"), t.danger),
        "Check" => (tr("Check mode"), t.warning),
        "Sleep" => (tr("Asleep"), t.text_secondary),
        _ => (tr("Unknown state"), t.text_secondary),
    };
    (label.to_string(), c)
}

/// Seconds as `m:ss` (`h:mm:ss` from an hour).
pub fn clock(secs: f64) -> String {
    let s = if secs.is_finite() { secs.clamp(0.0, 1e7).round() as u64 } else { 0 };
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// The progress line: "Line 120 of 480 · 0:42 elapsed · about 2:06 left".
pub fn progress_line(j: &Job) -> String {
    let mut out = trf("Line {line} of {lines}", &[("line", &j.line), ("lines", &j.lines)]);
    out.push_str(" · ");
    out.push_str(&trf("{time} elapsed", &[("time", &clock(j.elapsed))]));
    if j.active()
        && let Some(eta) = j.eta
    {
        out.push_str(" · ");
        out.push_str(&trf("about {time} left", &[("time", &clock(eta))]));
    }
    out
}

/// How the job ended (or that it's paused), in plain words.
pub fn job_outcome(j: &Job) -> Option<String> {
    match j.state.as_str() {
        "paused" => Some(tr("Paused: the beam is off and the head holds still. Resume or Stop.").to_string()),
        "done" => Some(trf("Done in {time}.", &[("time", &clock(j.elapsed))])),
        "stopped" => Some(trf("Stopped at line {line} of {lines}.", &[("line", &j.line), ("lines", &j.lines)])),
        "failed" => {
            let why = j.why.clone().unwrap_or_default();
            Some(match j.failed_line {
                Some(l) => trf("Failed at line {line}: {why}", &[("line", &l), ("why", &why)]),
                None => trf("Failed: {why}", &[("why", &why)]),
            })
        }
        _ => None,
    }
}
