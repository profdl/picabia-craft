//! PICABIA: the Laser window's machine profile: the bed, where a job's origin is, the power scale
//! (`S` max, with the connected machine's `$30` one click away), the travel speed and air assist.
//! Shown from `laser.status`'s `profile`, written with `laser.profile {set}` and saved with the
//! preferences.

use egui::{Id, Ui};
use serde_json::{Value, json};

use super::status::Status;
use crate::FigApp;
use crate::i18n::{tr, trf};
use crate::theme::Tokens;
use crate::widgets::{self, FieldEdit, Lead};

/// A profile number.
fn num(p: &Value, k: &str, default: f64) -> f64 {
    p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(default)
}

/// A number field in the profile; the new value when one was committed.
fn field(ui: &mut Ui, key: &str, name: &str, v: f64, suffix: &str, w: f32, step: f64) -> Option<f64> {
    match widgets::number_field(ui, Id::unique(("picabia_laser_profile", key)), Lead::Named(name), Some(v), suffix, w, step)? {
        FieldEdit::Set(v) => v.is_finite().then_some(v),
        // Scrubbing would write the preferences on every step: only the end counts.
        FieldEdit::Scrub(v, true) => v.is_finite().then_some(v),
        FieldEdit::Scrub(_, false) => None,
    }
}

/// The profile's rows; the fields to set, when one changed.
pub fn rows(ui: &mut Ui, st: &Status) -> Option<Value> {
    let t = Tokens::get(ui.ctx());
    let p = &st.profile;
    let mut set = None;
    if let Some(name) = p.get("name").and_then(Value::as_str) {
        widgets::label(ui, name, t.text_secondary);
    }
    ui.horizontal(|ui| {
        super::lead(ui, tr("Bed"));
        if let Some(v) = field(ui, "bed_w", tr("Bed width"), num(p, "bedWidthMm", 584.2), "mm", 84.0, 1.0) {
            set = Some(json!({"bedWidthMm": v}));
        }
        widgets::label(ui, "×", t.text_secondary);
        if let Some(v) = field(ui, "bed_h", tr("Bed height"), num(p, "bedHeightMm", 1219.2), "mm", 84.0, 1.0) {
            set = Some(json!({"bedHeightMm": v}));
        }
    });
    ui.horizontal(|ui| {
        super::lead(ui, tr("Origin"));
        let head = p.get("origin").and_then(Value::as_str) != Some("machine");
        let labels = [tr("Where the head is"), tr("Machine zero")];
        ui.allocate_ui(egui::vec2(220.0, widgets::FIELD_H), |ui| {
            if let Some(i) = widgets::segmented_text(ui, &labels, usize::from(!head))
                && (i == 0) != head
            {
                set = Some(json!({"origin": if i == 0 { "head" } else { "machine" }}));
            }
        });
    });
    ui.horizontal(|ui| {
        super::lead(ui, tr("S max"));
        let s_max = num(p, "sMax", 1000.0);
        if let Some(v) = field(ui, "s_max", tr("S max"), s_max, "", 84.0, 1.0) {
            set = Some(json!({"sMax": v}));
        }
        if let Some(m) = st.s_max_machine
            && (m - s_max).abs() > 1e-9
            && widgets::button(ui, &trf("Use machine value ($30 = {n})", &[("n", &widgets::fmt_num(m))]), false, true).clicked()
        {
            set = Some(json!({"sMax": m}));
        }
    });
    ui.horizontal(|ui| {
        super::lead(ui, tr("Travel speed"));
        if let Some(v) = field(ui, "rapid", tr("Travel speed"), num(p, "rapidMmMin", 3000.0), "mm/min", 120.0, 10.0) {
            set = Some(json!({"rapidMmMin": v}));
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(super::LABEL_W + ui.spacing().item_spacing.x);
        let air = p.get("air").and_then(Value::as_bool).unwrap_or(false);
        if widgets::checkbox(ui, air, tr("Air assist (M8/M9)")) {
            set = Some(json!({"air": !air}));
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(super::LABEL_W + ui.spacing().item_spacing.x);
        let checked = p.get("checked").and_then(Value::as_bool).unwrap_or(false);
        if widgets::checkbox(ui, checked, tr("Checked on the machine")) {
            set = Some(json!({"checked": !checked}));
        }
    });
    if p.get("checked").and_then(Value::as_bool) != Some(true) {
        super::job::wrapped(ui, tr("These values are guesses until checked on the machine."), t.warning);
    }
    set
}

/// Write `set` into the profile and keep it.
pub fn write(app: &mut FigApp, set: Value) {
    if app.run("laser.profile", json!({"set": set})).is_some() {
        app.services.save_prefs(&app.session.prefs);
    }
}
