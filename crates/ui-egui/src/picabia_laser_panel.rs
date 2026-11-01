//! PICABIA: the Laser window: connect to a GRBL laser (or the simulator), see its state, home,
//! unlock and jog it, set the job's origin, frame the job, choose what to cut and how, start it
//! through the safety checklist, follow its progress, pause, resume and stop it. A floating
//! window, opened by `laser.panel` (File ▸ Laser…, the Actions palette); not part of the modifier
//! UI. Every action is a `laser.*` command; the window only reads `laser.status` (each frame while
//! open or connected, repainting about ten times a second while connected).
//!
//! Safety (see [`run`]): Start opens the checklist and only its Start arms the laser; a large red
//! Stop is always on screen while connected (in the window, or in a bar at the bottom of the app
//! when the window is closed) and Shift+Esc stops from anywhere. Esc never stops a job and doesn't
//! close this window (it isn't a dialog); closing it with a job running leaves the bar and its Stop.

mod job;
pub(crate) mod ops;
mod profile;
mod run;
mod status;

use egui::{Context, CornerRadius, Id, Sense, Ui, vec2};
use serde_json::{Value, json};

pub use job::{JobState, Source};
pub use run::{Checklist, Confirm, stop_shortcut};
pub use status::Status;
pub(crate) use status::{clock, readable};

use crate::FigApp;
use crate::i18n::{self, tr, trf};
use crate::icons::{self, Icon};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets::{self, FieldEdit, Lead};

/// The label column of the window's rows.
pub(crate) const LABEL_W: f32 = 72.0;
/// Jog steps, mm.
pub const STEPS: [f64; 4] = [0.1, 1.0, 10.0, 100.0];
/// A column's width.
const COLUMN: f32 = 360.0;
/// Between the columns.
const GUTTER: f32 = 24.0;
/// The window's content width.
const WIDTH: f32 = COLUMN * 2.0 + GUTTER + 1.0;

/// The Laser window's state (UI state, not the document).
#[derive(Clone, Debug)]
pub struct LaserPanel {
    pub open: bool,
    /// `laser.ports`: (path, name).
    pub ports: Vec<(String, String)>,
    ports_loaded: bool,
    /// The port picked (a path from `ports`).
    pub port: Option<String>,
    /// Jog step, mm.
    pub step: f64,
    /// Jog feed, mm/min (`None`: the profile's travel speed).
    pub feed: Option<f64>,
    pub job: JobState,
    pub checklist: Option<Checklist>,
    pub show_profile: bool,
    pub show_log: bool,
    /// What the last action has to say (unlocking without homing, a frame's placement).
    pub notice: Option<String>,
}

impl Default for LaserPanel {
    fn default() -> Self {
        LaserPanel {
            open: false,
            ports: Vec::new(),
            ports_loaded: false,
            port: None,
            step: 10.0,
            feed: None,
            job: JobState::default(),
            checklist: None,
            show_profile: false,
            show_log: false,
            notice: None,
        }
    }
}

/// What the window asks for, run once it has drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    RefreshPorts,
    Connect(String),
    Disconnect,
    Home,
    Unlock,
    Jog(f64, f64),
    JogCancel,
    SetOrigin,
    Frame(Value),
    /// Open the checklist for these job params.
    Start(Value),
    Pause,
    /// Open the short confirm for a paused job.
    Resume,
    Stop,
    Profile(Value),
    /// The selection's G-code export dialog.
    Export,
}

/// Open the window (the `dialog.laserPanel` request; seam in `handle_ui_requests`).
pub fn open(app: &mut FigApp) {
    let p = &mut app.ui.picabia_laser_panel;
    p.open = true;
    p.ports_loaded = false;
}

/// File ▸ Export G-code for laser… and Laser… come after this File menu item.
pub fn file_menu_after(id: &str) -> bool {
    id == "file.exportSvg"
}

/// The File menu's laser items (seam in `menus::menu_level`, after [`file_menu_after`]'s item):
/// whether one ran.
pub fn file_menu(app: &mut FigApp, ui: &mut Ui, t: &Tokens) -> bool {
    let mut picked = None;
    if crate::menus::item(ui, &i18n::menu_item("laser.gcode", "Export G-code for laser…"), "", true, false, t) {
        picked = Some("laser.gcode");
    }
    crate::menus::separator(ui);
    if crate::menus::item(ui, &i18n::menu_item("laser.panel", "Laser…"), "", true, false, t) {
        picked = Some("laser.panel");
    }
    match picked {
        Some(id) => {
            crate::menus::invoke(app, id);
            true
        }
        None => false,
    }
}

/// The window, the checklist, and the bar while the window is closed (seam in `dialogs::show`).
pub fn show(app: &mut FigApp, ctx: &Context) {
    let link_open = app.session.picabia_laser.is_open();
    if !app.ui.picabia_laser_panel.open && !link_open {
        return;
    }
    let st = Status::from_json(&figcraft_engine::picabia_laser_link::status_json(&app.session));
    if st.open() {
        // About ten times a second while connected: the position and the progress move.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    run::checklist(app, ctx, &st);
    if !app.ui.picabia_laser_panel.open {
        if st.open() {
            run::bar(app, ctx, &st);
        }
        return;
    }
    let mut panel = std::mem::take(&mut app.ui.picabia_laser_panel);
    if !panel.ports_loaded {
        load_ports(app, &mut panel);
    }
    let mut actions = Vec::new();
    let mut open = true;
    let t = Tokens::get(ctx);
    crate::dialogs::floating(tr("Laser"), ctx)
        .id(Id::unique("picabia_laser_panel"))
        // Near the top, over the right panel's side: clear of the canvas toolbar at the bottom.
        .default_pos(ctx.content_rect().right_top() + vec2(-WIDTH - 72.0, 64.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            if cfg!(target_arch = "wasm32") {
                job::wrapped(
                    ui,
                    tr("Laser control needs the desktop app. G-code export works here: File ▸ Export G-code for laser…"),
                    t.text_secondary,
                );
                ui.add_space(8.0);
                if widgets::button(ui, tr("Export G-code…"), true, true).clicked() {
                    actions.push(Action::Export);
                }
                return;
            }
            body(app, ui, &mut panel, &st, &mut actions);
        });
    // Laser… asked for again while the window drew keeps it open.
    let asked = std::mem::take(&mut app.ui.picabia_laser_panel).open;
    panel.open = open || asked;
    app.ui.picabia_laser_panel = panel;
    for a in actions {
        act(app, a, &st);
    }
}

/// The window's sections.
fn body(app: &mut FigApp, ui: &mut Ui, panel: &mut LaserPanel, st: &Status, actions: &mut Vec<Action>) {
    connection(ui, panel, st, actions);
    if st.open() {
        ui.add_space(8.0);
        if run::stop_button(ui, 44.0) {
            actions.push(Action::Stop);
        }
    }
    ui.add_space(4.0);
    // Two columns: the machine on the left, the job on the right.
    ui.horizontal_top(|ui| {
        column(ui, |ui| machine_column(ui, panel, st, actions));
        ui.add_space(GUTTER / 2.0);
        let (r, _) = ui.allocate_exact_size(vec2(1.0, ui.available_height().max(1.0)), Sense::hover());
        ui.painter().vline(r.center().x, r.y_range(), egui::Stroke::new(1.0, Tokens::get(ui.ctx()).border));
        ui.add_space(GUTTER / 2.0);
        column(ui, |ui| job_column(app, ui, panel, st, actions));
    });
}

/// One column of the window.
fn column(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(COLUMN, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(COLUMN);
        add(ui);
    });
}

/// The machine: its state, jogging, the profile and the log.
fn machine_column(ui: &mut Ui, panel: &mut LaserPanel, st: &Status, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    section(ui, tr("Machine"));
    machine(ui, st, actions);
    if let Some(n) = &panel.notice {
        job::wrapped(ui, n, status::readable(t.warning, &t));
    }
    if st.connected {
        section(ui, tr("Position"));
        jog(ui, panel, st, actions);
    }
    ui.add_space(8.0);
    widgets::separator(ui);
    if widgets::disclosure(ui, tr("Machine profile"), panel.show_profile) {
        panel.show_profile = !panel.show_profile;
    }
    if panel.show_profile
        && let Some(set) = profile::rows(ui, st)
    {
        actions.push(Action::Profile(set));
    }
    if widgets::disclosure(ui, tr("Log"), panel.show_log) {
        panel.show_log = !panel.show_log;
    }
    if panel.show_log {
        log(ui, st);
    }
}

/// The job: what is cut and how, Frame and Start, and its progress.
fn job_column(app: &mut FigApp, ui: &mut Ui, panel: &mut LaserPanel, st: &Status, actions: &mut Vec<Action>) {
    section(ui, tr("Job"));
    let now = ui.ctx().input(|i| i.time);
    if job::settings(app, ui, &mut panel.job, st.job_active()) {
        panel.job.changed(now);
    }
    job::refresh(app, ui.ctx(), &mut panel.job);
    ui.add_space(4.0);
    job::summary(ui, &panel.job);
    ui.add_space(8.0);
    start_row(app, ui, panel, st, actions);
    if st.job.is_some() {
        section(ui, tr("Progress"));
        if let Some(a) = run::progress(ui, st) {
            actions.push(a);
        }
    }
}

/// A section's heading.
fn section(ui: &mut Ui, title: &str) {
    ui.add_space(10.0);
    widgets::separator(ui);
    ui.add_space(6.0);
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(title).font(semibold(11.0)).color(t.text));
    ui.add_space(2.0);
}

/// A row's label.
pub(crate) fn lead(ui: &mut Ui, label: &str) {
    let t = Tokens::get(ui.ctx());
    ui.allocate_ui(vec2(LABEL_W, widgets::FIELD_H), |ui| {
        ui.set_min_width(LABEL_W);
        ui.label(egui::RichText::new(label).font(regular(11.0)).color(t.text));
    });
}

fn load_ports(app: &mut FigApp, panel: &mut LaserPanel) {
    panel.ports_loaded = true;
    let r = app.session.execute("laser.ports", &json!({})).unwrap_or(Value::Null);
    panel.ports = r
        .get("ports")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(64)
        .filter_map(|p| {
            let path = p.get("path")?.as_str()?.to_string();
            let name = p.get("name").and_then(Value::as_str).unwrap_or(&path).to_string();
            Some((path, name))
        })
        .collect();
    if panel.port.as_ref().is_none_or(|p| !panel.ports.iter().any(|(path, _)| path == p)) {
        panel.port = panel.ports.first().map(|(p, _)| p.clone());
    }
}

/// The port picker and Connect / Disconnect.
fn connection(ui: &mut Ui, panel: &mut LaserPanel, st: &Status, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        lead(ui, tr("Port"));
        if st.open() {
            let name = st.port_name.clone().or_else(|| st.port.clone()).unwrap_or_default();
            ui.allocate_ui(vec2(196.0, widgets::FIELD_H), |ui| {
                ui.set_min_width(196.0);
                widgets::label(ui, &widgets::elide(ui, &name, 190.0), t.text);
            });
            if widgets::button(ui, tr("Disconnect"), false, !st.job_active()).clicked() {
                actions.push(Action::Disconnect);
            }
            return;
        }
        let shown = panel
            .port
            .as_ref()
            .and_then(|p| panel.ports.iter().find(|(path, _)| path == p))
            .map_or_else(|| tr("No port found").to_string(), |(_, n)| n.clone());
        let resp = widgets::dropdown(ui, &shown, 196.0, None);
        crate::a11y::combo(&resp, tr("Port"), &shown, crate::a11y::menu_open(&resp));
        crate::right::menu_popup(&resp).show(|ui| {
            for (path, name) in &panel.ports {
                if crate::right::menu_item(ui, name, "") {
                    panel.port = Some(path.clone());
                    ui.close();
                }
            }
            crate::menus::separator(ui);
            if crate::right::menu_item(ui, tr("Look for ports again"), "") {
                actions.push(Action::RefreshPorts);
                ui.close();
            }
        });
        if widgets::button(ui, tr("Connect"), true, panel.port.is_some()).clicked()
            && let Some(p) = &panel.port
        {
            actions.push(Action::Connect(p.clone()));
        }
    });
    if !st.open() && st.phase == "lost" {
        job::wrapped(ui, tr("The connection was lost: check the cable and connect again."), t.danger);
    }
}

/// The machine's state badge, position, Home / Unlock, its alarm and errors.
fn machine(ui: &mut Ui, st: &Status, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let (text, colour) = status::badge(st, &t);
        let galley = ui.painter().layout_no_wrap(text, semibold(11.0), status::readable(colour, &t));
        let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 26.0, 22.0), Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(11), colour.gamma_multiply(0.16));
        ui.painter().circle_filled(rect.left_center() + vec2(10.0, 0.0), 4.0, colour);
        ui.painter().galley(rect.left_center() + vec2(18.0, -galley.size().y / 2.0), galley, status::readable(colour, &t));
        if let Some(p) = st.pos {
            let pos = format!("X {}  Y {}  mm", widgets::fmt_num(p[0]), widgets::fmt_num(p[1]));
            ui.label(egui::RichText::new(pos).font(egui::FontId::monospace(11.0)).color(t.text));
        }
    });
    if st.connected {
        let mut facts = Vec::new();
        if let Some(v) = &st.version {
            facts.push(format!("GRBL {v}"));
        }
        if let Some(s) = st.s_max_machine {
            facts.push(format!("$30 = {}", widgets::fmt_num(s)));
        }
        match st.laser_mode {
            Some(true) => facts.push(tr("laser mode on").to_string()),
            Some(false) => facts.push(tr("laser mode off ($32 = 0)").to_string()),
            None => {}
        }
        if st.sim {
            facts.push(tr("simulated").to_string());
        }
        if !facts.is_empty() {
            widgets::label(ui, &facts.join(" · "), t.text_secondary);
        }
        ui.horizontal(|ui| {
            let free = !st.job_active();
            if widgets::button(ui, tr("Home"), false, free).clicked() {
                actions.push(Action::Home);
            }
            if widgets::button(ui, tr("Unlock"), false, free).clicked() {
                actions.push(Action::Unlock);
            }
        });
    }
    if let Some(a) = &st.alarm {
        ui.add_space(4.0);
        egui::Frame::NONE.fill(t.danger.gamma_multiply(0.12)).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::same(8)).show(
            ui,
            |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    egui::RichText::new(trf("Alarm {code}: {text}", &[("code", &a.code), ("text", &a.text)])).font(semibold(11.0)).color(t.danger),
                );
                if !a.advice.is_empty() {
                    job::wrapped(ui, &a.advice, t.text);
                }
            },
        );
    }
    if let Some(e) = &st.last_error {
        job::wrapped(ui, e, t.danger);
    }
    for w in &st.warnings {
        job::wrapped(ui, w, status::readable(t.warning, &t));
    }
}

/// The jog pad (arrows, step, feed) and Set origin.
fn jog(ui: &mut Ui, panel: &mut LaserPanel, st: &Status, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    let free = st.connected && !st.job_active() && st.state.as_deref() != Some("Alarm");
    ui.horizontal(|ui| {
        // The pad: up, left / right, down.
        const B: f32 = 30.0;
        let (rect, _) = ui.allocate_exact_size(vec2(B * 3.0 + 8.0, B * 3.0 + 8.0), Sense::hover());
        let cell = |cx: f32, cy: f32| egui::Rect::from_min_size(rect.min + vec2(cx * (B + 4.0), cy * (B + 4.0)), vec2(B, B));
        let s = panel.step;
        for (r, icon, (dx, dy), name) in [
            (cell(1.0, 0.0), Icon::ChevronUp, (0.0, s), tr("Jog up (Y+)")),
            (cell(0.0, 1.0), Icon::ChevronLeft, (-s, 0.0), tr("Jog left (X−)")),
            (cell(2.0, 1.0), Icon::ChevronRight, (s, 0.0), tr("Jog right (X+)")),
            (cell(1.0, 2.0), Icon::Chevron, (0.0, -s), tr("Jog down (Y−)")),
        ] {
            let resp =
                ui.interact(r, Id::unique(("picabia_laser_jog", name)), if free { Sense::click() } else { Sense::hover() }).on_hover_text(name);
            crate::a11y::button(&resp, name);
            let fill = if free && resp.hovered() { t.hover } else { t.field };
            ui.painter().rect_filled(r, CornerRadius::same(5), fill);
            icons::paint(ui.painter(), egui::Rect::from_center_size(r.center(), vec2(14.0, 14.0)), icon, if free { t.icon } else { t.text_tertiary });
            if free && resp.clicked() {
                actions.push(Action::Jog(dx, dy));
            }
        }
        // The middle: stop a long jog.
        let mid = cell(1.0, 1.0);
        let resp = ui.interact(mid, Id::unique("picabia_laser_jog_cancel"), if st.connected { Sense::click() } else { Sense::hover() });
        crate::a11y::button(&resp, tr("Stop the jog"));
        let resp = resp.on_hover_text(tr("Stop the jog"));
        ui.painter().rect_stroke(mid.shrink(6.0), CornerRadius::same(3), egui::Stroke::new(1.5, t.icon_secondary), egui::StrokeKind::Inside);
        if st.connected && resp.clicked() {
            actions.push(Action::JogCancel);
        }
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                widgets::label(ui, tr("Step"), t.text);
                let labels = ["0.1", "1", "10", "100"];
                let active = STEPS.iter().position(|v| (*v - panel.step).abs() < 1e-9).unwrap_or(2);
                ui.allocate_ui(vec2(150.0, widgets::FIELD_H), |ui| {
                    if let Some(i) = widgets::segmented_text(ui, &labels, active) {
                        panel.step = STEPS.get(i).copied().unwrap_or(10.0);
                    }
                });
                widgets::label(ui, "mm", t.text_secondary);
            });
            ui.horizontal(|ui| {
                widgets::label(ui, tr("Feed"), t.text);
                let rapid = st.profile.get("rapidMmMin").and_then(Value::as_f64).unwrap_or(3000.0);
                if let Some(FieldEdit::Set(v) | FieldEdit::Scrub(v, _)) = widgets::number_field(
                    ui,
                    Id::unique("picabia_laser_feed"),
                    Lead::Named(tr("Feed")),
                    Some(panel.feed.unwrap_or(rapid)),
                    "mm/min",
                    120.0,
                    10.0,
                ) && v.is_finite()
                {
                    panel.feed = Some(v.clamp(1.0, 20_000.0));
                }
            });
            ui.add_space(4.0);
            if widgets::button(ui, tr("Set origin here"), false, free)
                .on_hover_text(tr("The head's position becomes the sheet's bottom-left corner"))
                .clicked()
            {
                actions.push(Action::SetOrigin);
            }
        });
    });
    widgets::label(ui, tr("Jog to the sheet's bottom-left corner, Set origin, Frame, then Start."), t.text_secondary);
}

/// Frame, Start…, and why Start can't run when it can't.
fn start_row(app: &mut FigApp, ui: &mut Ui, panel: &mut LaserPanel, st: &Status, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    let sel = app.session.selection();
    let params = panel.job.params(&sel);
    let mut blocked: Vec<String> = Vec::new();
    match &panel.job.summary {
        job::Summary::Failed(_) => blocked.push(tr("the job can't be cut as it is (see above)").to_string()),
        job::Summary::Pending => blocked.push(tr("the job is still being worked out").to_string()),
        _ => {}
    }
    if params.is_none() {
        blocked.push(match panel.job.source {
            Source::File(_) => tr("no G-code file chosen").to_string(),
            _ => tr("nothing selected").to_string(),
        });
    }
    if st.job_active() {
        blocked.push(tr("a job is running").to_string());
    } else {
        blocked.extend(st.blocked_by.iter().cloned());
    }
    let can_frame = params.is_some() && st.connected && !st.job_active() && st.state.as_deref() == Some("Idle");
    let can_start = blocked.is_empty() && st.can_start;
    ui.horizontal(|ui| {
        if widgets::button(ui, tr("Frame"), false, can_frame).on_hover_text(tr("Trace the job's outline with the beam off")).clicked()
            && let Some(p) = &params
        {
            actions.push(Action::Frame(p.clone()));
        }
        if widgets::button(ui, tr("Start…"), true, can_start).clicked()
            && let Some(p) = &params
        {
            actions.push(Action::Start(p.clone()));
        }
        if widgets::button(ui, tr("Export G-code…"), false, matches!(panel.job.source, Source::Selection) && !sel.is_empty()).clicked() {
            actions.push(Action::Export);
        }
    });
    if !can_start && !blocked.is_empty() {
        job::wrapped(ui, &trf("Start needs: {reasons}", &[("reasons", &blocked.join("; "))]), t.text_secondary);
    }
}

/// The last lines to and from the machine.
fn log(ui: &mut Ui, st: &Status) {
    let t = Tokens::get(ui.ctx());
    egui::Frame::NONE.fill(t.field).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().id_salt("picabia_laser_log").max_height(140.0).stick_to_bottom(true).show(ui, |ui| {
            if st.log.is_empty() {
                widgets::label(ui, tr("Nothing yet"), t.text_secondary);
            }
            for l in &st.log {
                ui.label(egui::RichText::new(l).font(egui::FontId::monospace(10.0)).color(t.text_secondary));
            }
        });
    });
}

/// Run what the window asked for.
fn act(app: &mut FigApp, a: Action, st: &Status) {
    let empty = json!({});
    match a {
        Action::RefreshPorts => app.ui.picabia_laser_panel.ports_loaded = false,
        Action::Connect(port) => {
            app.ui.picabia_laser_panel.notice = None;
            app.run("laser.connect", json!({"port": port}));
        }
        Action::Disconnect => {
            app.run("laser.disconnect", empty);
        }
        Action::Home => {
            app.ui.picabia_laser_panel.notice = None;
            app.run("laser.home", empty);
        }
        Action::Unlock => {
            let warning = app.run("laser.unlock", empty).and_then(|r| r.get("warning").and_then(Value::as_str).map(str::to_string));
            app.ui.picabia_laser_panel.notice = warning;
        }
        Action::Jog(dx, dy) => {
            let mut p = json!({});
            if dx != 0.0 {
                p["dx"] = json!(dx);
            }
            if dy != 0.0 {
                p["dy"] = json!(dy);
            }
            if let Some(f) = app.ui.picabia_laser_panel.feed {
                p["feed"] = json!(f);
            }
            app.run("laser.jog", p);
        }
        Action::JogCancel => {
            app.run("laser.jogCancel", empty);
        }
        Action::SetOrigin => {
            app.ui.picabia_laser_panel.notice = None;
            app.run("laser.setOrigin", empty);
        }
        Action::Frame(p) => {
            let r = app.run("laser.frame", p);
            let warnings: Vec<String> = r
                .as_ref()
                .and_then(|r| r.get("warnings"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            app.ui.picabia_laser_panel.notice = (!warnings.is_empty()).then(|| warnings.join(" "));
        }
        Action::Start(p) => app.ui.picabia_laser_panel.checklist = Some(Checklist::new(Confirm::Start, p)),
        Action::Pause => {
            app.run("laser.pause", empty);
        }
        Action::Resume => {
            if st.paused() {
                app.ui.picabia_laser_panel.checklist = Some(Checklist::new(Confirm::Resume, Value::Null));
            } else {
                // A hold that isn't a job's (a jog): resuming it lights nothing.
                app.run("laser.resume", empty);
            }
        }
        Action::Stop => {
            app.run("laser.stop", empty);
        }
        Action::Profile(set) => profile::write(app, set),
        Action::Export => {
            let ids = app.session.selection();
            app.run("laser.gcode", json!({"ids": ids}));
        }
    }
}

#[cfg(test)]
#[path = "picabia_laser_panel_tests.rs"]
mod tests;
