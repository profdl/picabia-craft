//! PICABIA: the laser export and test card dialogs of a stencil (`dialog.stencilExport`,
//! `dialog.stencilTestCard`, requested by `modifier.stencilExport` and `modifier.stencilTestCard`
//! when they're run without a path), and the two buttons under a stencil row that ask for them.
//!
//! The dialog holds the settings only: kerf, bed size (or no limit), sheet overlap, the three
//! operation colours and the label (the test card: material, kerf and colours). Its summary comes
//! from the command itself, run with `data: true` on open and a moment after each change: sheets,
//! their sizes in mm and what the job warns of. Export runs the command again with the path the
//! save dialog picked (the browser: with `data: true`, each sheet a download).

use std::ffi::OsStr;

use egui::{Context, Id, Ui, vec2};
use figcraft_doc::{Document, NodeId};
use figcraft_modifiers::ModifierKind;
use serde_json::{Map, Value, json};

use crate::FigApp;
use crate::i18n::{tr, trf, trn};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets::{self, FieldEdit, Lead};

/// The export command (`{id?, path?, data?, kerfMm?, ...}`).
pub const EXPORT: &str = "modifier.stencilExport";
/// The test card command (`{material?, kerfMm?, path?, data?, colours}`).
pub const TEST_CARD: &str = "modifier.stencilTestCard";

/// Seconds the summary waits after a change before it asks the command again.
const DEBOUNCE: f64 = 0.3;

/// Test card materials: (value written in `material`, label), as the stencil's Material field.
pub(crate) use crate::right::modifiers::MATERIALS;

/// Which dialog is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Export for laser: the stencil's cut as one SVG per sheet.
    Export,
    /// Laser test card: a card of bridges, holes, slits, corners and a kerf gauge.
    TestCard,
}

impl Kind {
    /// The command the dialog runs.
    pub fn command(self) -> &'static str {
        match self {
            Kind::Export => EXPORT,
            Kind::TestCard => TEST_CARD,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Kind::Export => tr("Export for laser"),
            Kind::TestCard => tr("Laser test card"),
        }
    }
}

/// One sheet of the summary.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetLine {
    pub name: String,
    pub w_mm: f64,
    pub h_mm: f64,
    pub contours: u64,
    pub loose: u64,
}

/// What the last dry run came to.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Summary {
    /// Not asked yet (or asked again after a change).
    #[default]
    Pending,
    /// The sheets and the job's warnings.
    Ready { sheets: Vec<SheetLine>, warnings: Vec<String> },
    /// The command refused (not a stencil, nothing to cut, too many sheets).
    Failed(String),
}

/// The open dialog's settings and summary (UI state, not the document).
#[derive(Clone, Debug)]
pub struct LaserDialog {
    pub kind: Kind,
    /// The stencil layer (export).
    pub id: Option<NodeId>,
    /// Kerf in mm; sent only once typed (`kerf_set`), else the material's (shown from the summary).
    pub kerf_mm: f64,
    pub kerf_set: bool,
    pub bed_w_mm: f64,
    pub bed_h_mm: f64,
    /// The bed has no size limit (sent as 0 × 0).
    pub no_limit: bool,
    pub overlap_mm: f64,
    pub cut: [u8; 3],
    pub engrave: [u8; 3],
    pub score: [u8; 3],
    pub label: bool,
    /// Test card material (`paper`, `film`, `card`, `custom`).
    pub material: String,
    pub summary: Summary,
    /// The params of the last dry run.
    ran: Option<Value>,
    /// When (egui time) a setting last changed.
    changed_at: f64,
}

impl LaserDialog {
    /// The dialog with the defaults of `figcraft-stencil`'s `LaserParams`.
    pub fn new(kind: Kind) -> Self {
        LaserDialog {
            kind,
            id: None,
            kerf_mm: 0.1,
            kerf_set: false,
            bed_w_mm: 600.0,
            bed_h_mm: 400.0,
            no_limit: false,
            overlap_mm: 10.0,
            cut: [0xff, 0, 0],
            engrave: [0, 0, 0],
            score: [0, 0, 0xff],
            label: true,
            material: "paper".into(),
            summary: Summary::Pending,
            ran: None,
            changed_at: 0.0,
        }
    }

    /// The dialog a UI request asks for: `data` is `{id?, params?}` (the settings the command
    /// would use: the remembered ones, the stencil's kerf), or the settings themselves.
    pub fn from_request(kind: Kind, data: &Value) -> Self {
        let mut d = LaserDialog::new(kind);
        d.id = data.get("id").and_then(figcraft_engine::params::id_of);
        let p = data.get("params").filter(|p| p.is_object()).unwrap_or(data);
        d.read(p);
        d
    }

    /// Take the settings `p` (camelCase, as the command reports them in `params`), keeping the
    /// current value of what's missing or junk.
    fn read(&mut self, p: &Value) {
        let num = |k: &str, lo: f64, hi: f64| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).map(|v| v.clamp(lo, hi));
        if let Some(k) = num("kerfMm", 0.0, 2.0) {
            self.kerf_mm = k;
        }
        match (num("bedWidthMm", 0.0, 5000.0), num("bedHeightMm", 0.0, 5000.0)) {
            (Some(w), Some(h)) if w <= 0.0 || h <= 0.0 => self.no_limit = true,
            (w, h) => {
                if let Some(w) = w {
                    self.bed_w_mm = w.max(10.0);
                    self.no_limit = false;
                }
                if let Some(h) = h {
                    self.bed_h_mm = h.max(10.0);
                    self.no_limit = false;
                }
            }
        }
        if let Some(o) = num("overlapMm", 0.0, 200.0) {
            self.overlap_mm = o;
        }
        for (k, c) in [("cutColour", &mut self.cut), ("engraveColour", &mut self.engrave), ("scoreColour", &mut self.score)] {
            if let Some(rgb) = p.get(k).and_then(Value::as_str).and_then(parse_hex) {
                *c = rgb;
            }
        }
        if let Some(l) = p.get("label").and_then(Value::as_bool) {
            self.label = l;
        }
        if let Some(m) = p.get("material").and_then(Value::as_str).filter(|m| MATERIALS.iter().any(|(id, _)| id == m)) {
            self.material = m.to_string();
        }
    }

    /// The command's params for these settings (without `path` or `data`).
    pub fn params(&self) -> Value {
        let mut p = Map::new();
        if let Some(id) = self.id.filter(|_| self.kind == Kind::Export) {
            p.insert("id".into(), json!(id));
        }
        if self.kind == Kind::TestCard {
            p.insert("material".into(), json!(self.material));
        }
        if self.kerf_set {
            p.insert("kerfMm".into(), json!(round3(self.kerf_mm)));
        }
        if self.kind == Kind::Export {
            let (w, h) = if self.no_limit { (0.0, 0.0) } else { (self.bed_w_mm, self.bed_h_mm) };
            p.insert("bedWidthMm".into(), json!(round3(w)));
            p.insert("bedHeightMm".into(), json!(round3(h)));
            p.insert("overlapMm".into(), json!(round3(self.overlap_mm)));
            p.insert("label".into(), json!(self.label));
        }
        p.insert("cutColour".into(), json!(hex(self.cut)));
        p.insert("engraveColour".into(), json!(hex(self.engrave)));
        p.insert("scoreColour".into(), json!(hex(self.score)));
        Value::Object(p)
    }

    /// Read a dry run's result into the summary; the kerf the command used shows until one is
    /// typed.
    pub fn take_result(&mut self, r: Result<Value, String>) {
        self.summary = match r {
            Ok(v) => {
                if !self.kerf_set
                    && let Some(k) = v.get("params").and_then(|p| p.get("kerfMm")).and_then(Value::as_f64).filter(|k| k.is_finite())
                {
                    self.kerf_mm = k.clamp(0.0, 2.0);
                }
                summary_of(&v)
            }
            Err(e) => Summary::Failed(e),
        };
    }

    /// The suggested file name: the layer's name, or the card's material.
    fn file_name(&self, doc: Option<&Document>) -> String {
        match self.kind {
            Kind::Export => {
                let name = self.id.and_then(|id| doc?.node(id)).map(|n| n.name.trim().to_string()).filter(|n| !n.is_empty());
                format!("{} laser.svg", name.unwrap_or_else(|| "Stencil".into()))
            }
            Kind::TestCard => format!("Test card {}.svg", self.material),
        }
    }

    fn changed(&mut self, now: f64) {
        self.changed_at = now;
    }
}

/// The summary a command result `{files: [{name, wMm, hMm, contours, loosePieces}], warnings}`
/// shows.
pub fn summary_of(v: &Value) -> Summary {
    let sheets = v
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(64)
        .map(|f| SheetLine {
            name: f.get("sheet").or_else(|| f.get("name")).and_then(Value::as_str).unwrap_or_default().to_string(),
            w_mm: f.get("wMm").and_then(Value::as_f64).unwrap_or(0.0),
            h_mm: f.get("hMm").and_then(Value::as_f64).unwrap_or(0.0),
            contours: f.get("contours").and_then(Value::as_u64).unwrap_or(0),
            loose: f.get("loosePieces").and_then(Value::as_u64).unwrap_or(0),
        })
        .collect();
    let warnings = v.get("warnings").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).take(32).map(str::to_string).collect();
    Summary::Ready { sheets, warnings }
}

/// `#rrggbb` (or `rrggbb`) as bytes.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let byte = |i: usize| s.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// `c` as `#rrggbb`, lowercase.
pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn mm(v: f64) -> String {
    widgets::fmt_num(v)
}

/// The stencil layer among `sel` (the first with an enabled stencil row).
pub fn stencil_layer(doc: &Document, sel: &[NodeId]) -> Option<NodeId> {
    sel.iter().copied().find(|id| {
        doc.node(*id)
            .and_then(figcraft_modifiers::node_stack)
            .is_some_and(|s| s.is_ok_and(|s| s.modifiers.iter().any(|m| m.enabled && matches!(m.kind, ModifierKind::Stencil(_)))))
    })
}

/// What a button or menu item asks for: the command and its params (no path: the command asks
/// for the dialog).
pub fn request(kind: Kind, node: Option<NodeId>) -> (&'static str, Value) {
    match (kind, node) {
        (Kind::Export, Some(id)) => (EXPORT, json!({"id": id})),
        (Kind::Export, None) => (EXPORT, json!({})),
        (Kind::TestCard, _) => (TEST_CARD, json!({})),
    }
}

/// Open the dialog a UI request asks for (seam arms in `handle_ui_requests`).
pub fn open(app: &mut FigApp, kind: Kind, data: &Value) {
    app.ui.picabia_laser = Some(LaserDialog::from_request(kind, data));
}

/// The two buttons under a stencil row: Export for laser… and Test card….
pub(crate) fn panel_buttons(app: &mut FigApp, ui: &mut Ui, node: NodeId) {
    let (mut export, mut card) = (false, false);
    crate::right::row(ui, 32.0, |ui| {
        export = widgets::button(ui, tr("Export for laser…"), false, true).clicked();
        card = widgets::button(ui, tr("Test card…"), false, true).clicked();
    });
    for (clicked, kind) in [(export, Kind::Export), (card, Kind::TestCard)] {
        if clicked {
            let (id, params) = request(kind, Some(node));
            app.run(id, params);
        }
    }
}

/// Ask the command what these settings come to, when they changed a moment ago.
fn refresh(app: &mut FigApp, ctx: &Context, d: &mut LaserDialog) {
    let params = d.params();
    if d.ran.as_ref() == Some(&params) {
        return;
    }
    let now = ctx.input(|i| i.time);
    let wait = d.changed_at + DEBOUNCE - now;
    if d.ran.is_some() && wait > 0.0 {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        return;
    }
    let mut dry = params.clone();
    dry["data"] = json!(true);
    // Read-only and not undoable: run directly so a refusal shows in the dialog, not as a toast.
    let r = app.session.execute(d.kind.command(), &dry).map_err(|e| e.to_string());
    d.take_result(r);
    d.ran = Some(params);
}

/// Draw the open dialog (seam in `dialogs::show`).
pub fn show(app: &mut FigApp, ctx: &Context) {
    let Some(mut d) = app.ui.picabia_laser.take() else { return };
    if app.session.doc().is_err() {
        return;
    }
    refresh(app, ctx, &mut d);
    let t = Tokens::get(ctx);
    let now = ctx.input(|i| i.time);
    let mut open = true;
    let (mut cancel, mut export) = (false, false);
    crate::dialogs::window(d.kind.title(), ctx).id(Id::unique("picabia_laser")).open(&mut open).show(ctx, |ui| {
        ui.set_width(360.0);
        if let Some(name) = d.id.and_then(|id| app.session.doc().ok()?.node(id)).map(|n| n.name.clone()) {
            widgets::label(ui, &name, t.text_secondary);
            ui.add_space(4.0);
        }
        if fields(ui, &mut d) {
            d.changed(now);
        }
        ui.add_space(8.0);
        widgets::separator(ui);
        ui.add_space(4.0);
        summary(ui, &d.summary, &t);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let ok = !matches!(d.summary, Summary::Failed(_));
            export = widgets::button(ui, tr("Export…"), true, ok).clicked();
            cancel = widgets::button(ui, tr("Cancel"), false, true).clicked();
        });
    });
    if !open || cancel {
        return;
    }
    let params = d.params();
    let name = d.file_name(app.session.doc().ok());
    let kind = d.kind;
    app.ui.picabia_laser = Some(d);
    if export {
        save(app, kind, params, &name);
    }
}

/// The label column of the dialog's rows.
const LABEL_W: f32 = 84.0;
const FIELD_W: f32 = 84.0;

fn lead(ui: &mut Ui, label: &str) {
    let t = Tokens::get(ui.ctx());
    ui.allocate_ui(vec2(LABEL_W, widgets::FIELD_H), |ui| {
        ui.set_min_width(LABEL_W);
        ui.label(egui::RichText::new(label).font(regular(11.0)).color(t.text));
    });
}

/// A mm field: the new value (clamped) when one was typed or scrubbed.
fn mm_field(ui: &mut Ui, key: &str, name: &str, v: f64, lo: f64, hi: f64) -> Option<f64> {
    let e = widgets::number_field(ui, Id::unique(("picabia_laser", key)), Lead::Named(name), Some(v), "mm", FIELD_W, 0.1)?;
    let v = match e {
        FieldEdit::Set(v) | FieldEdit::Scrub(v, _) => v,
    };
    v.is_finite().then(|| v.clamp(lo, hi))
}

/// The settings rows; whether one changed.
fn fields(ui: &mut Ui, d: &mut LaserDialog) -> bool {
    let mut changed = false;
    if d.kind == Kind::TestCard {
        ui.horizontal(|ui| {
            lead(ui, tr("Material"));
            let shown = MATERIALS.iter().find(|(id, _)| *id == d.material).map_or("", |(_, l)| tr(l));
            let resp = widgets::dropdown(ui, shown, FIELD_W * 2.0 + 16.0, None);
            crate::a11y::combo(&resp, tr("Material"), shown, crate::a11y::menu_open(&resp));
            crate::right::menu_popup(&resp).show(|ui| {
                for (id, label) in MATERIALS {
                    if crate::right::menu_item(ui, tr(label), "") {
                        if d.material != *id {
                            d.material = (*id).to_string();
                            // The card's kerf follows the material until one is typed.
                            d.kerf_set = false;
                            changed = true;
                        }
                        ui.close();
                    }
                }
            });
        });
    }
    ui.horizontal(|ui| {
        lead(ui, tr("Kerf"));
        if let Some(v) = mm_field(ui, "kerf", tr("Kerf"), d.kerf_mm, 0.0, 2.0) {
            d.kerf_mm = v;
            d.kerf_set = true;
            changed = true;
        }
    });
    if d.kind == Kind::Export {
        ui.horizontal(|ui| {
            lead(ui, tr("Bed"));
            if d.no_limit {
                widgets::label(ui, tr("No limit"), Tokens::get(ui.ctx()).text_secondary);
            } else {
                if let Some(v) = mm_field(ui, "bed_w", tr("Bed width"), d.bed_w_mm, 10.0, 5000.0) {
                    d.bed_w_mm = v;
                    changed = true;
                }
                widgets::label(ui, "×", Tokens::get(ui.ctx()).text_secondary);
                if let Some(v) = mm_field(ui, "bed_h", tr("Bed height"), d.bed_h_mm, 10.0, 5000.0) {
                    d.bed_h_mm = v;
                    changed = true;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
            if widgets::checkbox(ui, d.no_limit, tr("No limit")) {
                d.no_limit = !d.no_limit;
                changed = true;
            }
        });
        if !d.no_limit {
            ui.horizontal(|ui| {
                lead(ui, tr("Overlap"));
                if let Some(v) = mm_field(ui, "overlap", tr("Overlap"), d.overlap_mm, 0.0, 200.0) {
                    d.overlap_mm = v;
                    changed = true;
                }
            });
        }
    }
    ui.add_space(4.0);
    for (key, label, c) in [("cut", tr("Cut"), &mut d.cut), ("engrave", tr("Engrave"), &mut d.engrave), ("score", tr("Score"), &mut d.score)] {
        ui.horizontal(|ui| {
            lead(ui, label);
            if egui::color_picker::color_edit_button_srgb(ui, c).changed() {
                changed = true;
            }
            if let Some(rgb) =
                widgets::text_field_named(ui, Id::unique(("picabia_laser", key, "hex")), &hex(*c), FIELD_W, label).as_deref().and_then(parse_hex)
                && rgb != *c
            {
                *c = rgb;
                changed = true;
            }
        });
    }
    if d.kind == Kind::Export {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
            if widgets::checkbox(ui, d.label, tr("Engrave a label")) {
                d.label = !d.label;
                changed = true;
            }
        });
    }
    changed
}

/// The summary: the sheets, their sizes and what the job warns of.
fn summary(ui: &mut Ui, s: &Summary, t: &Tokens) {
    match s {
        Summary::Pending => widgets::label(ui, tr("Working out the sheets…"), t.text_secondary),
        Summary::Failed(e) => wrapped(ui, e, t.danger),
        Summary::Ready { sheets, warnings } => {
            ui.label(egui::RichText::new(trn("{n} sheets", sheets.len(), &[])).font(semibold(11.0)).color(t.text));
            for s in sheets {
                let (w, h) = (mm(s.w_mm), mm(s.h_mm));
                let size = trf("{w} × {h} mm", &[("w", &w), ("h", &h)]);
                let cuts = trn("{n} cuts", s.contours as usize, &[]);
                let line = if s.name.is_empty() { format!("{size}, {cuts}") } else { format!("{}: {size}, {cuts}", s.name) };
                widgets::label(ui, &line, t.text_secondary);
                if s.loose > 0 {
                    wrapped(ui, &trn("{n} pieces would fall out", s.loose as usize, &[]), t.warning);
                }
            }
            for w in warnings {
                wrapped(ui, w, t.warning);
            }
        }
    }
}

/// A line of the summary that wraps to the dialog's width.
fn wrapped(ui: &mut Ui, text: &str, color: egui::Color32) {
    ui.add(egui::Label::new(egui::RichText::new(text).font(regular(11.0)).color(color)).wrap());
}

/// Export: a save dialog for the first sheet's file (more sheets take " 1", " 2"... beside it);
/// in the browser each sheet downloads.
fn save(app: &mut FigApp, kind: Kind, params: Value, name: &str) {
    let cmd = kind.command();
    if app.services.downloads() {
        let mut p = params;
        p["data"] = json!(true);
        let Some(r) = app.run(cmd, p) else { return };
        let mut names = Vec::new();
        for f in r.get("files").and_then(Value::as_array).into_iter().flatten() {
            let n = f.get("name").and_then(Value::as_str).unwrap_or("stencil.svg").to_string();
            if app.download(&n, f) {
                names.push(n);
            }
        }
        if !names.is_empty() {
            app.toast(saved_text(&names));
            app.ui.picabia_laser = None;
        }
        return;
    }
    app.pick_save(kind.title(), name, &["svg"], move |app, mut path| {
        if path.extension().is_none_or(OsStr::is_empty) {
            path.set_extension("svg");
        }
        let mut p = params;
        p["path"] = json!(path.to_string_lossy());
        if let Some(r) = app.run(cmd, p) {
            app.toast(saved_text(&written(&r)));
            app.ui.picabia_laser = None;
        }
    });
}

/// The file names a command result wrote.
pub fn written(r: &Value) -> Vec<String> {
    r.get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let p = f.get("path").or_else(|| f.get("name")).and_then(Value::as_str)?;
            Some(std::path::Path::new(p).file_name().map_or(p.to_string(), |n| n.to_string_lossy().into_owned()))
        })
        .collect()
}

/// "Saved Rose laser.svg" or "Saved 3 sheets: Rose laser 1.svg, ...".
pub fn saved_text(names: &[String]) -> String {
    match names {
        [one] => trf("Saved {name}", &[("name", one)]),
        _ => trn("Saved {n} sheets: {names}", names.len(), &[("names", &names.join(", "))]),
    }
}

#[cfg(test)]
#[path = "picabia_laser_tests.rs"]
mod tests;
