//! PICABIA: running a laser job from the Laser window: the safety checklist that Start opens (and
//! the short confirm a paused job's Resume opens), the one place the desktop app arms the laser,
//! the progress of the job, and the emergency stop (the big Stop button, the bar that stays on
//! screen while the window is closed, and Shift+Esc anywhere in the app).
//!
//! Arming: [`figcraft_engine::Session::laser_arm`] is called from [`confirm`] only, after every box
//! of the checklist is ticked (a test checks that no other file in the app calls it). Clicks the
//! control channel injects (`ui.input`) arm only the simulator: on a real machine they are refused,
//! so scripts and agents can't fire the laser through the window either.

use egui::{Align2, Color32, Context, CornerRadius, Id, Key, Modifiers, Sense, Ui, vec2};
use serde_json::Value;

use super::Action;
use super::status::{self, Status};
use crate::FigApp;
use crate::i18n::tr;
use crate::theme::{Tokens, regular, semibold};
use crate::widgets;

/// What the checklist confirms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Start the job.
    Start,
    /// Resume a paused job (the beam comes back on).
    Resume,
}

impl Confirm {
    /// The boxes to tick, in order.
    pub fn checks(self) -> Vec<&'static str> {
        match self {
            Confirm::Start => vec![
                tr("The laser shielding is in place"),
                tr("Everyone near the machine wears laser safety glasses"),
                tr("The laser is focused for this material"),
                tr("Extraction or ventilation is on"),
                tr("The material lies flat and is held down"),
                tr("Someone stays at the machine until the job ends"),
            ],
            Confirm::Resume => vec![tr("The shielding is in place and safety glasses are on"), tr("Nothing has moved and someone is at the machine")],
        }
    }

    fn title(self) -> &'static str {
        match self {
            Confirm::Start => tr("Before the laser starts"),
            Confirm::Resume => tr("Before the job resumes"),
        }
    }

    fn button(self) -> &'static str {
        match self {
            Confirm::Start => tr("Start"),
            Confirm::Resume => tr("Resume"),
        }
    }
}

/// The open checklist: what it confirms, the job's params (for Start) and the boxes ticked.
#[derive(Clone, Debug)]
pub struct Checklist {
    pub kind: Confirm,
    pub params: Value,
    pub ticks: Vec<bool>,
}

impl Checklist {
    pub fn new(kind: Confirm, params: Value) -> Self {
        Checklist { kind, params, ticks: vec![false; kind.checks().len()] }
    }

    /// Every box is ticked.
    pub fn complete(&self) -> bool {
        !self.ticks.is_empty() && self.ticks.iter().all(|t| *t)
    }
}

/// Why [`confirm`] didn't run the job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Not every box is ticked.
    Unticked,
    /// The click came from the control channel and the machine is a real one.
    NotAPerson,
}

/// The checklist's Start (or Resume) was pressed: arm the laser and run the job. The one place
/// the app arms the laser (see the module docs). Returns whether the command was run (its errors
/// show as a toast, as any command's).
pub fn confirm(app: &mut FigApp, list: &Checklist, st: &Status) -> Result<(), Refused> {
    if !list.complete() {
        return Err(Refused::Unticked);
    }
    if app.control_synthetic && !st.sim {
        app.toast(tr("Only a person at this computer can start the laser: scripts and agents can't."));
        return Err(Refused::NotAPerson);
    }
    app.session.laser_arm();
    let (cmd, params) = match list.kind {
        Confirm::Start => ("laser.start", list.params.clone()),
        Confirm::Resume => ("laser.resume", serde_json::json!({})),
    };
    app.run(cmd, params);
    Ok(())
}

/// Draw the open checklist (a modal window over the app); closes it on Start, Cancel or Escape.
pub fn checklist(app: &mut FigApp, ctx: &Context, st: &Status) {
    let Some(mut list) = app.ui.picabia_laser_panel.checklist.take() else { return };
    // The job ended or the link closed under it: nothing to confirm.
    if !st.connected || (list.kind == Confirm::Resume && !st.paused()) {
        return;
    }
    let t = Tokens::get(ctx);
    let (mut go, mut cancel) = (false, false);
    let mut open = true;
    crate::dialogs::window(list.kind.title(), ctx).id(Id::unique("picabia_laser_checklist")).open(&mut open).show(ctx, |ui| {
        ui.set_width(340.0);
        let intro = match list.kind {
            Confirm::Start => tr("The laser can burn the material, the machine and eyes. Tick each box once it's true."),
            Confirm::Resume => tr("Resuming turns the beam back on."),
        };
        super::job::wrapped(ui, intro, t.text_secondary);
        ui.add_space(8.0);
        for (i, text) in list.kind.checks().into_iter().enumerate() {
            let on = list.ticks.get(i).copied().unwrap_or(false);
            if widgets::checkbox(ui, on, text)
                && let Some(t) = list.ticks.get_mut(i)
            {
                *t = !on;
            }
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            go = widgets::button(ui, list.kind.button(), true, list.complete()).clicked();
            cancel = widgets::button(ui, tr("Cancel"), false, true).clicked();
        });
    });
    let escape = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
    if !open || cancel || escape {
        return;
    }
    if go && list.complete() {
        let _ = confirm(app, &list, st);
        return;
    }
    app.ui.picabia_laser_panel.checklist = Some(list);
}

/// The job's progress, how it ended, and Pause / Resume (what was pressed).
pub fn progress(ui: &mut Ui, st: &Status) -> Option<Action> {
    let t = Tokens::get(ui.ctx());
    let job = st.job.clone()?;
    ui.label(egui::RichText::new(&job.name).font(semibold(11.0)).color(t.text));
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 8.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(4), t.field);
    let done = rect.with_max_x(rect.min.x + rect.width() * (job.percent / 100.0) as f32);
    let fill = match job.state.as_str() {
        "failed" | "stopped" => t.danger,
        "paused" => t.warning,
        "done" => t.success,
        _ => t.accent,
    };
    ui.painter().rect_filled(done, CornerRadius::same(4), fill);
    widgets::label(ui, &status::progress_line(&job), t.text_secondary);
    if let Some(outcome) = status::job_outcome(&job) {
        let c = match job.state.as_str() {
            "failed" => t.danger,
            "done" => t.text,
            _ => t.text_secondary,
        };
        super::job::wrapped(ui, &outcome, c);
    }
    let mut out = None;
    if job.active() {
        ui.horizontal(|ui| {
            if job.state == "paused" {
                if widgets::button(ui, tr("Resume…"), true, !st.stopping).clicked() {
                    out = Some(Action::Resume);
                }
            } else if widgets::button(ui, tr("Pause"), false, !st.stopping).clicked() {
                out = Some(Action::Pause);
            }
        });
    }
    out
}

/// The big red Stop: always there while connected. Whether it was pressed.
pub fn stop_button(ui: &mut Ui, h: f32) -> bool {
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let label = tr("Stop");
    crate::a11y::button(&resp, label);
    let fill = if resp.hovered() { t.danger.gamma_multiply(0.85) } else { t.danger };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    let size = if h >= 36.0 { 15.0 } else { 12.0 };
    ui.painter().text(rect.center() - vec2(0.0, if h >= 36.0 { 5.0 } else { 0.0 }), Align2::CENTER_CENTER, label, semibold(size), Color32::WHITE);
    if h >= 36.0 {
        ui.painter().text(rect.center() + vec2(0.0, 10.0), Align2::CENTER_CENTER, tr("Shift+Esc anywhere"), regular(10.0), Color32::WHITE);
    }
    resp.clicked()
}

/// While connected with the window closed: a bar at the bottom of the window with the machine's
/// state, the job's progress, Stop and a way back to the window.
pub fn bar(app: &mut FigApp, ctx: &Context, st: &Status) {
    let t = Tokens::get(ctx);
    let (badge, colour) = status::badge(st, &t);
    let (mut stop, mut show) = (false, false);
    egui::Area::new(Id::unique("picabia_laser_bar")).order(egui::Order::Foreground).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -16.0)).show(ctx, |ui| {
        egui::Frame::window(&ctx.global_style()).fill(t.panel).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tr("Laser")).font(semibold(11.0)).color(t.text));
                ui.label(egui::RichText::new(&badge).font(semibold(11.0)).color(colour));
                if let Some(j) = st.job.as_ref().filter(|j| j.active()) {
                    widgets::label(ui, &format!("{} · {}%", j.name, widgets::fmt_num(j.percent)), t.text_secondary);
                }
                ui.allocate_ui(vec2(72.0, widgets::FIELD_H), |ui| stop = stop_button(ui, widgets::FIELD_H));
                show = widgets::button(ui, tr("Show"), false, true).clicked();
            });
        });
    });
    if stop {
        app.run("laser.stop", serde_json::json!({}));
    }
    if show {
        app.ui.picabia_laser_panel.open = true;
    }
}

/// Shift+Esc: the emergency stop, anywhere in the app (a field, a menu, another window, the
/// Laser window closed), whenever a job runs or the machine moves. Runs before the shortcuts and
/// dialogs see the key (seam in `app.rs`). Esc alone never stops a job.
pub fn stop_shortcut(app: &mut FigApp, ctx: &Context) {
    if !app.session.picabia_laser.is_open() {
        return;
    }
    let st = Status::from_json(&figcraft_engine::picabia_laser_link::status_json(&app.session));
    if st.stoppable() && ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Escape)) {
        app.run("laser.stop", serde_json::json!({}));
        app.toast(tr("Laser stopped"));
    }
}
