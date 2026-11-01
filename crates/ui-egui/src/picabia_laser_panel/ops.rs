//! PICABIA: the laser's operation table (cut, score, engrave: on, speed, power, passes) and its
//! materials, shared by the Laser window and the G-code export dialog. The values are the
//! commands' (`cut`, `score`, `engrave` params of `laser.gcode`, `laser.start`,
//! `modifier.stencilGcode`, `modifier.stencilTestCard`): the table shows what the command reported
//! until the person edits it, then sends all three.

use egui::{CornerRadius, Id, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::i18n::{tr, tr_noop};
use crate::icons::{self, Icon};
use crate::theme::{Tokens, regular};
use crate::widgets::{self, FieldEdit, Lead};

/// Laser materials: (value written in `material`, label).
pub const MATERIALS: &[(&str, &str)] =
    &[("paper", tr_noop("Paper")), ("film", tr_noop("Film")), ("card", tr_noop("Card")), ("custom", tr_noop("Custom"))];

/// The label of material `id` (the id itself when unknown).
pub fn material_label(id: &str) -> &str {
    MATERIALS.iter().find(|(m, _)| *m == id).map_or(id, |(_, l)| tr(l))
}

/// One operation's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Op {
    pub enabled: bool,
    /// mm/min, 1-20000.
    pub speed: f64,
    /// %, 0-100.
    pub power: f64,
    /// 1-20.
    pub passes: u32,
}

impl Op {
    const fn new(speed: f64, power: f64, passes: u32) -> Self {
        Op { enabled: true, speed, power, passes }
    }

    /// Take what `v` (`{enabled, speedMmMin, powerPct, passes}`) sets validly.
    fn read(&mut self, v: &Value) {
        let num = |k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        if let Some(b) = v.get("enabled").and_then(Value::as_bool) {
            self.enabled = b;
        }
        if let Some(s) = num("speedMmMin") {
            self.speed = s.clamp(1.0, 20_000.0);
        }
        if let Some(p) = num("powerPct") {
            self.power = p.clamp(0.0, 100.0);
        }
        if let Some(n) = num("passes") {
            self.passes = n.round().clamp(1.0, 20.0) as u32;
        }
    }

    fn to_json(self) -> Value {
        json!({"enabled": self.enabled, "speedMmMin": round1(self.speed), "powerPct": round1(self.power), "passes": self.passes})
    }
}

/// The three operations, in the table's order: cut, score, engrave.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ops(pub [Op; 3]);

/// The operations' param names and labels, in [`Ops`]' order.
pub const OP_KEYS: [(&str, &str); 3] = [("cut", tr_noop("Cut")), ("score", tr_noop("Score")), ("engrave", tr_noop("Engrave"))];

impl Default for Ops {
    /// Paper's starting settings (as `figcraft-stencil`'s `GcodeParams::default`): shown only
    /// until the command reports the material's own.
    fn default() -> Self {
        Ops([Op::new(300.0, 100.0, 2), Op::new(1500.0, 30.0, 1), Op::new(2000.0, 20.0, 1)])
    }
}

impl Ops {
    /// Take the operations `v` (`{cut?, score?, engrave?}`, as a command's `gcode` result) sets
    /// validly; junk keeps what's there.
    pub fn read(&mut self, v: &Value) {
        for ((key, _), op) in OP_KEYS.iter().zip(self.0.iter_mut()) {
            if let Some(o) = v.get(*key).filter(|o| o.is_object()) {
                op.read(o);
            }
        }
    }

    /// `{cut, score, engrave}` for a command's params.
    pub fn to_json(self) -> Value {
        let mut m = serde_json::Map::new();
        for ((key, _), op) in OP_KEYS.iter().zip(self.0.iter()) {
            m.insert((*key).to_string(), op.to_json());
        }
        Value::Object(m)
    }

    /// Put the operations into a command's params `p` (an object).
    pub fn put(&self, p: &mut Value) {
        if let (Some(o), Value::Object(ops)) = (p.as_object_mut(), self.to_json()) {
            o.extend(ops);
        }
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Column widths: the operation's name, its switch, speed, power, passes.
const NAME_W: f32 = 64.0;
const ON_W: f32 = 22.0;
const SPEED_W: f32 = 108.0;
const POWER_W: f32 = 70.0;
const PASSES_W: f32 = 56.0;

/// The operation table; whether the person changed something.
pub fn table(ui: &mut Ui, salt: &str, ops: &mut Ops) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        for (text, w) in [("", NAME_W + ON_W), (tr("Speed"), SPEED_W), (tr("Power"), POWER_W), (tr("Passes"), PASSES_W)] {
            ui.allocate_ui(vec2(w, 16.0), |ui| {
                ui.set_min_width(w);
                ui.label(egui::RichText::new(text).font(regular(10.0)).color(t.text_secondary));
            });
        }
    });
    for ((key, label), op) in OP_KEYS.iter().zip(ops.0.iter_mut()) {
        let label = tr(label);
        ui.horizontal(|ui| {
            ui.allocate_ui(vec2(NAME_W, widgets::FIELD_H), |ui| {
                ui.set_min_width(NAME_W);
                let c = if op.enabled { t.text } else { t.text_tertiary };
                ui.label(egui::RichText::new(label).font(regular(11.0)).color(c));
            });
            if tick(ui, op.enabled, label) {
                op.enabled = !op.enabled;
                changed = true;
            }
            let id = |k: &str| Id::unique(("picabia_laser_ops", salt, *key, k));
            let field = |ui: &mut Ui, k: &str, name: &str, v: f64, suffix: &str, w: f32, step: f64| -> Option<f64> {
                let e = widgets::number_field(ui, id(k), Lead::Named(name), Some(v), suffix, w, step)?;
                let (FieldEdit::Set(v) | FieldEdit::Scrub(v, _)) = e;
                v.is_finite().then_some(v)
            };
            if let Some(v) = field(ui, "speed", tr("Speed"), op.speed, "mm/min", SPEED_W - 6.0, 10.0) {
                op.speed = v.clamp(1.0, 20_000.0);
                changed = true;
            }
            if let Some(v) = field(ui, "power", tr("Power"), op.power, "%", POWER_W - 6.0, 1.0) {
                op.power = v.clamp(0.0, 100.0);
                changed = true;
            }
            if let Some(v) = field(ui, "passes", tr("Passes"), f64::from(op.passes), "×", PASSES_W - 6.0, 1.0) {
                op.passes = v.round().clamp(1.0, 20.0) as u32;
                changed = true;
            }
        });
    }
    changed
}

/// A checkbox without a label of its own (the row names it for screen readers); whether it was
/// clicked.
pub(crate) fn tick(ui: &mut Ui, on: bool, name: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ON_W, widgets::FIELD_H), Sense::click());
    crate::a11y::checkbox(&resp, name, on);
    let b = egui::Rect::from_min_size(pos2(rect.min.x, rect.center().y - 7.0), vec2(14.0, 14.0));
    if on {
        ui.painter().rect_filled(b, CornerRadius::same(3), t.accent);
        icons::paint(ui.painter(), b.shrink(1.5), Icon::Check, t.accent_on);
    } else {
        ui.painter().rect_stroke(b, CornerRadius::same(3), Stroke::new(1.0, t.border_strong), StrokeKind::Inside);
    }
    resp.clicked()
}

/// The material drop-down (`chosen`, shown as its label); the material picked, when one was.
pub fn material_dropdown(ui: &mut Ui, chosen: &str, width: f32) -> Option<String> {
    let shown = material_label(chosen).to_string();
    let resp = widgets::dropdown(ui, &shown, width, None);
    crate::a11y::combo(&resp, tr("Material"), &shown, crate::a11y::menu_open(&resp));
    let mut picked = None;
    crate::right::menu_popup(&resp).show(|ui| {
        for (id, label) in MATERIALS {
            if crate::right::menu_item(ui, tr(label), "") {
                picked = Some((*id).to_string());
                ui.close();
            }
        }
    });
    picked.filter(|m| m != chosen)
}
