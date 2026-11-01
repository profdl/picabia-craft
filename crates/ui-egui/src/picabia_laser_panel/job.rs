//! PICABIA: the Laser window's job: what is cut (the selection, the test card, the power ladder or
//! a G-code file), the sheet of a stencil with several, the material and the operation table, and
//! what the job comes to (size, lines, time, warnings) from a dry run of the export command,
//! asked again a moment after a change.

use std::path::PathBuf;

use egui::{Context, Ui};
use figcraft_doc::NodeId;
use serde_json::{Value, json};

use super::ops::{self, Ops};
use crate::FigApp;
use crate::i18n::{tr, trf, trn};
use crate::theme::Tokens;
use crate::widgets;

/// Seconds the summary waits after a change before it asks again.
const DEBOUNCE: f64 = 0.3;
/// Seconds after which an unchanged job is asked again (the design may have been edited).
const STALE: f64 = 3.0;

/// What the job cuts.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Source {
    /// The selected layers (a single stencil layer: its sheets).
    #[default]
    Selection,
    /// The material's test card.
    TestCard,
    /// The power ladder.
    PowerLadder,
    /// A G-code file.
    File(PathBuf),
}

/// One sheet of the job.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub w_mm: f64,
    pub h_mm: f64,
    pub lines: u64,
    pub est_sec: Option<f64>,
}

/// What the job comes to.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Summary {
    /// Not asked yet (or a change is waiting).
    #[default]
    Pending,
    /// Nothing to ask: no layers selected.
    NothingSelected,
    /// A G-code file: nothing to work out before it runs.
    File,
    Ready {
        sheets: Vec<Sheet>,
        warnings: Vec<String>,
    },
    /// The command refused (the reason).
    Failed(String),
}

/// The job's settings and summary (UI state).
#[derive(Clone, Debug, Default)]
pub struct JobState {
    pub source: Source,
    /// The material picked (`None`: the command's choice, a stencil's own, else paper).
    pub material: Option<String>,
    /// The material the last dry run used.
    pub material_used: Option<String>,
    pub ops: Ops,
    /// The person edited the table: it's sent (else the material's remembered settings apply).
    pub ops_set: bool,
    /// The stencil sheet to cut, 1-based.
    pub sheet: usize,
    /// The last dry run cut a stencil layer (its sheets), not a design's outlines.
    pub stencil: bool,
    pub summary: Summary,
    /// The dry run's params and the design's state it ran on.
    ran: Option<(Value, (usize, usize))>,
    ran_at: f64,
    changed_at: f64,
}

impl JobState {
    /// The material shown: the one picked, else the one the job used, else paper.
    pub fn material_shown(&self) -> &str {
        self.material.as_deref().or(self.material_used.as_deref()).unwrap_or("paper")
    }

    /// The job's params for `laser.start` and `laser.frame` (`sel` the selection), or `None` when
    /// there's nothing to cut.
    pub fn params(&self, sel: &[NodeId]) -> Option<Value> {
        let mut p = match &self.source {
            Source::File(path) => return Some(json!({"path": path.to_string_lossy()})),
            Source::Selection if sel.is_empty() => return None,
            Source::Selection => {
                let mut p = json!({"ids": sel});
                if self.sheets() > 1 {
                    p["sheet"] = json!(self.sheet.max(1));
                }
                p
            }
            Source::TestCard => json!({"testCard": "card"}),
            Source::PowerLadder => json!({"testCard": "powerLadder"}),
        };
        self.put_settings(&mut p);
        Some(p)
    }

    /// The dry run: its command and params, or `None` when there's nothing to ask.
    pub fn dry_run(&self, sel: &[NodeId]) -> Option<(&'static str, Value)> {
        let (cmd, mut p) = match &self.source {
            Source::File(_) => return None,
            Source::Selection if sel.is_empty() => return None,
            Source::Selection => ("laser.gcode", json!({"ids": sel})),
            Source::TestCard => ("modifier.stencilTestCard", json!({"format": "gcode", "kind": "card"})),
            Source::PowerLadder => ("modifier.stencilTestCard", json!({"format": "gcode", "kind": "powerLadder"})),
        };
        self.put_settings(&mut p);
        p["dryRun"] = json!(true);
        Some((cmd, p))
    }

    fn put_settings(&self, p: &mut Value) {
        if let Some(m) = &self.material {
            p["material"] = json!(m);
        }
        if self.ops_set {
            self.ops.put(p);
        }
    }

    /// How many sheets the last dry run found.
    pub fn sheets(&self) -> usize {
        match &self.summary {
            Summary::Ready { sheets, .. } => sheets.len(),
            _ => 0,
        }
    }

    /// The sheet to cut (of the last dry run).
    pub fn chosen_sheet(&self) -> Option<&Sheet> {
        match &self.summary {
            Summary::Ready { sheets, .. } => sheets.get(self.sheet.max(1) - 1).or(sheets.first()),
            _ => None,
        }
    }

    /// Read a dry run's result.
    pub fn take_result(&mut self, r: Result<Value, String>) {
        self.summary = match r {
            Ok(v) => {
                if !self.ops_set
                    && let Some(g) = v.get("gcode")
                {
                    self.ops.read(g);
                }
                self.stencil = v.get("id").is_some_and(|id| !id.is_null());
                self.material_used = v.get("material").and_then(Value::as_str).map(str::to_string);
                let sheets: Vec<Sheet> = v
                    .get("files")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .take(64)
                    .map(|f| Sheet {
                        name: f.get("sheet").or_else(|| f.get("name")).and_then(Value::as_str).unwrap_or_default().to_string(),
                        w_mm: f.get("wMm").and_then(Value::as_f64).unwrap_or(0.0),
                        h_mm: f.get("hMm").and_then(Value::as_f64).unwrap_or(0.0),
                        lines: f.get("lines").and_then(Value::as_u64).unwrap_or(0),
                        est_sec: f.get("estimatedSec").and_then(Value::as_f64).filter(|e| e.is_finite()),
                    })
                    .collect();
                let mut warnings: Vec<String> = v
                    .get("warnings")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .take(16)
                    .map(str::to_string)
                    .collect();
                for f in v.get("files").and_then(Value::as_array).into_iter().flatten().take(64) {
                    for w in f.get("warnings").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).take(8) {
                        if !warnings.iter().any(|x| x == w) {
                            warnings.push(w.to_string());
                        }
                    }
                }
                if self.sheet > sheets.len() || self.sheet == 0 {
                    self.sheet = 1;
                }
                Summary::Ready { sheets, warnings }
            }
            Err(e) => Summary::Failed(e),
        };
    }

    /// A setting changed at `now` (egui time): ask again a moment later.
    pub fn changed(&mut self, now: f64) {
        self.changed_at = now;
    }
}

/// Ask the export command what the job comes to, when it changed a moment ago (or a while has
/// passed: the design may have been edited).
pub fn refresh(app: &mut FigApp, ctx: &Context, j: &mut JobState) {
    let sel = app.session.selection();
    let Some((cmd, params)) = j.dry_run(&sel) else {
        j.summary = match j.source {
            Source::File(_) => Summary::File,
            _ => Summary::NothingSelected,
        };
        j.ran = None;
        return;
    };
    let now = ctx.input(|i| i.time);
    let depth = app.session.undo_depth();
    let same = j.ran.as_ref().is_some_and(|(p, d)| *p == params && *d == depth);
    if same && now - j.ran_at < STALE {
        return;
    }
    let wait = j.changed_at + DEBOUNCE - now;
    if j.ran.is_some() && !same && wait > 0.0 {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        return;
    }
    // Read-only and not undoable: run directly so a refusal shows in the window, not as a toast.
    let r = app.session.execute(cmd, &params).map_err(|e| e.to_string());
    j.take_result(r);
    j.ran = Some((params, depth));
    j.ran_at = now;
}

/// "Rectangle", "Rectangle and 2 more", or that nothing is selected.
pub fn selection_name(app: &FigApp) -> Option<String> {
    let sel = app.session.selection();
    let first = sel.first().and_then(|id| app.session.doc().ok()?.node(*id)).map(|n| n.name.clone())?;
    Some(match sel.len() {
        1 => first,
        n => trn("{name} and {n} more", n - 1, &[("name", &first)]),
    })
}

/// The source picker, the selection or file, the sheet, the material and the operation table;
/// whether a setting changed. `locked`: a job is running (nothing changes under it).
pub fn settings(app: &mut FigApp, ui: &mut Ui, j: &mut JobState, locked: bool) -> bool {
    let stencil = j.stencil && app.session.selection().len() == 1;
    let t = Tokens::get(ui.ctx());
    let mut changed = false;
    let labels = [tr("Selection"), tr("Test card"), tr("Power ladder"), tr("G-code file")];
    let active = match j.source {
        Source::Selection => 0,
        Source::TestCard => 1,
        Source::PowerLadder => 2,
        Source::File(_) => 3,
    };
    ui.add_enabled_ui(!locked, |ui| {
        if let Some(i) = widgets::segmented_text(ui, &labels, active)
            && i != active
        {
            j.source = match i {
                0 => Source::Selection,
                1 => Source::TestCard,
                2 => Source::PowerLadder,
                _ => Source::File(PathBuf::new()),
            };
            j.summary = Summary::Pending;
            j.ran = None;
            changed = true;
        }
    });
    ui.add_space(4.0);
    match &j.source {
        Source::Selection => match selection_name(app) {
            Some(name) if stencil => {
                widgets::label(ui, &trf("Cuts the stencil {name}: its sheet's cuts, scores and label.", &[("name", &name)]), t.text_secondary)
            }
            Some(name) => widgets::label(ui, &trf("Cuts {name}: closed outlines are cut, open paths scored.", &[("name", &name)]), t.text_secondary),
            None => wrapped(
                ui,
                tr("Nothing selected: select the layers to cut, or choose the test card, the power ladder or a G-code file."),
                super::status::readable(t.warning, &t),
            ),
        },
        Source::TestCard => widgets::label(ui, tr("The material's test card: bridges, holes, slits and a kerf gauge."), t.text_secondary),
        Source::PowerLadder => widgets::label(ui, tr("Squares at each power and speed, to find the material's settings."), t.text_secondary),
        Source::File(path) => {
            let named = path.file_name().map(|n| n.to_string_lossy().into_owned());
            let mut open = false;
            ui.horizontal(|ui| {
                widgets::label(ui, named.as_deref().unwrap_or(tr("No file chosen")), if named.is_some() { t.text } else { t.text_secondary });
                open = widgets::button(ui, tr("Open…"), false, !locked).clicked();
            });
            if open {
                app.pick_open(tr("Open G-code"), &["gcode", "nc", "ngc", "gc", "tap", "txt"], |app, path| {
                    app.ui.picabia_laser_panel.job.source = Source::File(path);
                });
            }
        }
    }
    if matches!(j.source, Source::File(_)) {
        return changed;
    }
    // A stencil with several sheets: which one.
    if j.sheets() > 1 {
        ui.horizontal(|ui| {
            super::lead(ui, tr("Sheet"));
            let shown = j.chosen_sheet().map(sheet_label).unwrap_or_default();
            ui.add_enabled_ui(!locked, |ui| {
                let resp = widgets::dropdown(ui, &shown, 220.0, None);
                crate::a11y::combo(&resp, tr("Sheet"), &shown, crate::a11y::menu_open(&resp));
                let labels: Vec<String> = match &j.summary {
                    Summary::Ready { sheets, .. } => sheets.iter().map(sheet_label).collect(),
                    _ => Vec::new(),
                };
                crate::right::menu_popup(&resp).show(|ui| {
                    for (i, l) in labels.iter().enumerate() {
                        if crate::right::menu_item(ui, l, "") {
                            j.sheet = i + 1;
                            ui.close();
                        }
                    }
                });
            });
        });
    }
    ui.horizontal(|ui| {
        super::lead(ui, tr("Material"));
        ui.add_enabled_ui(!locked, |ui| {
            if let Some(m) = ops::material_dropdown(ui, j.material_shown(), 140.0) {
                j.material = Some(m);
                // The table follows the material's remembered settings until edited again.
                j.ops_set = false;
                changed = true;
            }
        });
    });
    ui.add_space(2.0);
    ui.add_enabled_ui(!locked, |ui| {
        if ops::table(ui, "panel", &mut j.ops) {
            j.ops_set = true;
            changed = true;
        }
    });
    changed
}

/// "1 of 2 · 600 × 400 mm".
fn sheet_label(s: &Sheet) -> String {
    let size = trf("{w} × {h} mm", &[("w", &widgets::fmt_num(s.w_mm)), ("h", &widgets::fmt_num(s.h_mm))]);
    if s.name.is_empty() { size } else { format!("{} · {size}", s.name) }
}

/// What the job comes to: its size, lines and time, and what it warns of.
pub fn summary(ui: &mut Ui, j: &JobState) {
    let t = Tokens::get(ui.ctx());
    match &j.summary {
        Summary::Pending => widgets::label(ui, tr("Working out the job…"), t.text_secondary),
        Summary::NothingSelected | Summary::File => {}
        Summary::Failed(e) => wrapped(ui, e, t.danger),
        Summary::Ready { warnings, .. } => {
            if let Some(s) = j.chosen_sheet() {
                let mut line = trf("{w} × {h} mm", &[("w", &widgets::fmt_num(s.w_mm)), ("h", &widgets::fmt_num(s.h_mm))]);
                line.push_str(" · ");
                line.push_str(&trn("{n} lines", s.lines as usize, &[]));
                if let Some(e) = s.est_sec {
                    line.push_str(" · ");
                    line.push_str(&trf("about {time}", &[("time", &super::status::clock(e))]));
                }
                ui.label(egui::RichText::new(line).font(crate::theme::semibold(11.0)).color(t.text));
            }
            for w in warnings {
                wrapped(ui, w, super::status::readable(t.warning, &t));
            }
        }
    }
}

/// A line that wraps to the window's width.
pub fn wrapped(ui: &mut Ui, text: &str, color: egui::Color32) {
    ui.add(egui::Label::new(egui::RichText::new(text).font(crate::theme::regular(11.0)).color(color)).wrap());
}
