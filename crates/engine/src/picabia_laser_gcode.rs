//! PICABIA: G-code for the laser: `modifier.stencilGcode` (a stencil layer's sheets as GRBL 1.1
//! G-code), the test card and the power ladder as G-code (`modifier.stencilTestCard {format:
//! "gcode"}`), the machine profile (`Prefs::picabia_machine`, `laser.profile`) and the jobs
//! `laser.start` and `laser.frame` run ([`build_run`]).
//!
//! Settings are layered: [`GcodeParams::for_material`], then the material's remembered operation
//! settings (`picabia_machine.materials`), then the machine's `S` max (`$30` of the connected
//! machine, else the profile's) and air assist, then the command's params. The laser settings
//! (kerf, bed, overlap, label) as for the SVG export, except that the bed is the machine's.
// The browser build has no laser link: the job builders and status helpers go unused there.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use base64::Engine as _;
use figcraft_doc::NodeId;
use figcraft_geom::kurbo::Rect;
use figcraft_modifiers::StencilMaterial;
use figcraft_stencil::{Gcode, GcodeParams, LadderParams, LaserParams, LaserSheet};
use serde_json::{Map, Value, json};

use crate::params::{get, string};
use crate::picabia_laser::{CardSetup, Out, card_setup, card_sheet, finite, sheet_path, stencil_layer, stencil_material, stencil_sheets};
use crate::{EngineError, Result, Session, bad};

pub const GCODE: &str = "modifier.stencilGcode";
pub const PROFILE: &str = "laser.profile";

/// The materials whose operation settings are remembered.
pub const MATERIALS: [&str; 4] = ["paper", "film", "card", "custom"];
/// The biggest G-code file `laser.start {path}` reads.
pub const MAX_GCODE_FILE_BYTES: u64 = 64 << 20;
/// File extensions `laser.start {path}` takes.
const GCODE_EXTENSIONS: [&str; 6] = ["gcode", "nc", "ngc", "gc", "tap", "txt"];

/// The laser machine: its bed, power scale, travel speed, air assist and the operation settings
/// remembered per material. Defaults: the owner's X-Carve (23 × 48 in, a 2.8 W J Tech diode),
/// **guesses until checked on the machine** (`checked: false`).
#[derive(Clone, Debug, PartialEq)]
pub struct MachineProfile {
    pub name: String,
    /// The bed, mm (X across, Y front to back).
    pub bed_w_mm: f64,
    pub bed_h_mm: f64,
    /// `S` at full power (GRBL `$30`); the connected machine's own `$30` wins for jobs.
    pub s_max: f64,
    /// Travel (beam off) speed for estimates and framing, mm/min.
    pub rapid_mm_min: f64,
    /// Air assist (`M8`/`M9`).
    pub air: bool,
    /// The values were checked on the machine (P13.4); false: guesses.
    pub checked: bool,
    /// Where a job's origin is: `"head"` (the default: the person jogs to the sheet's bottom-left
    /// and presses Set origin, so work 0,0 is there) or `"machine"` (work coordinates are the
    /// bed's).
    pub origin: String,
    /// Per material (`paper`, `film`, `card`, `custom`): `{cut, score, engrave}` op settings.
    pub materials: Map<String, Value>,
}

impl Default for MachineProfile {
    fn default() -> Self {
        MachineProfile {
            name: "X-Carve 23x48 + J Tech 2.8W".into(),
            bed_w_mm: 584.2,
            bed_h_mm: 1219.2,
            s_max: 1000.0,
            rapid_mm_min: 3000.0,
            air: false,
            checked: false,
            origin: "head".into(),
            materials: Map::new(),
        }
    }
}

impl MachineProfile {
    pub const MIN_BED_MM: f64 = 10.0;
    pub const MAX_BED_MM: f64 = 5000.0;
    pub const MIN_RAPID: f64 = 10.0;
    pub const MAX_RAPID: f64 = 20_000.0;
    const MAX_NAME: usize = 80;

    /// The session's profile (`Prefs::picabia_machine`; null: the defaults).
    pub fn of(s: &Session) -> MachineProfile {
        Self::from_json(&s.prefs.picabia_machine)
    }

    /// A profile from JSON, leniently (see [`apply_json`](Self::apply_json)).
    pub fn from_json(v: &Value) -> MachineProfile {
        let mut p = MachineProfile::default();
        p.apply_json(v);
        p
    }

    /// Overwrite what `v` sets validly (`name`, `bedWidthMm`, `bedHeightMm`, `sMax`, `rapidMmMin`,
    /// `air`, `checked`, `materials: {paper|film|card|custom: {cut?, score?, engrave?}}`, op objects
    /// overlaid field by field); wrong types are ignored, numbers clamped.
    pub fn apply_json(&mut self, v: &Value) {
        let Some(o) = v.as_object() else { return };
        let num = |k: &str| o.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        if let Some(n) = o.get("name").and_then(Value::as_str) {
            let n: String = n.chars().filter(|c| !c.is_control()).take(Self::MAX_NAME).collect();
            if !n.trim().is_empty() {
                self.name = n.trim().to_string();
            }
        }
        if let Some(x) = num("bedWidthMm") {
            self.bed_w_mm = x.clamp(Self::MIN_BED_MM, Self::MAX_BED_MM);
        }
        if let Some(x) = num("bedHeightMm") {
            self.bed_h_mm = x.clamp(Self::MIN_BED_MM, Self::MAX_BED_MM);
        }
        if let Some(x) = num("sMax") {
            self.s_max = x.clamp(1.0, GcodeParams::MAX_S_MAX);
        }
        if let Some(x) = num("rapidMmMin") {
            self.rapid_mm_min = x.clamp(Self::MIN_RAPID, Self::MAX_RAPID);
        }
        if let Some(b) = o.get("air").and_then(Value::as_bool) {
            self.air = b;
        }
        if let Some(b) = o.get("checked").and_then(Value::as_bool) {
            self.checked = b;
        }
        if let Some(v) = o.get("origin").and_then(Value::as_str).map(str::trim) {
            if v.eq_ignore_ascii_case("head") {
                self.origin = "head".into();
            } else if v.eq_ignore_ascii_case("machine") {
                self.origin = "machine".into();
            }
        }
        if let Some(m) = o.get("materials").and_then(Value::as_object) {
            for id in MATERIALS {
                if let Some(ops) = m.get(id).filter(|v| v.is_object()) {
                    let mut gp = self.material_ops(id);
                    gp.apply_json(ops);
                    self.materials.insert(id.into(), ops_json(&gp));
                }
            }
        }
    }

    /// The profile as JSON (the [`apply_json`](Self::apply_json) names).
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "bedWidthMm": self.bed_w_mm,
            "bedHeightMm": self.bed_h_mm,
            "sMax": self.s_max,
            "rapidMmMin": self.rapid_mm_min,
            "air": self.air,
            "checked": self.checked,
            "origin": self.origin,
            "materials": Value::Object(self.materials.clone()),
        })
    }

    /// Material `id`'s starting settings with its remembered operations (the profile's `S` max
    /// and air not applied yet).
    fn material_ops(&self, id: &str) -> GcodeParams {
        let mut gp = GcodeParams::for_material(id);
        if let Some(ops) = self.materials.get(id) {
            gp.apply_json(ops);
        }
        gp
    }

    /// Jobs start at the head's work origin (`origin: "head"`).
    pub fn origin_at_head(&self) -> bool {
        self.origin != "machine"
    }

    /// Remember `gp`'s operations for material `id`.
    pub fn remember(&mut self, id: &str, gp: &GcodeParams) {
        if MATERIALS.contains(&id) {
            self.materials.insert(id.into(), ops_json(gp));
        }
    }
}

/// `{cut, score, engrave}` of `gp`.
fn ops_json(gp: &GcodeParams) -> Value {
    let all = gp.to_json();
    json!({"cut": all["cut"], "score": all["score"], "engrave": all["engrave"]})
}

/// The `material` param (paper|film|card|custom), `None` when not given.
pub(crate) fn material_param(cmd: &str, p: &Value) -> Result<Option<StencilMaterial>> {
    match get(p, "material") {
        None => Ok(None),
        Some(_) => {
            let m = string(p, "material").unwrap_or_default();
            StencilMaterial::ALL
                .into_iter()
                .find(|x| x.id().eq_ignore_ascii_case(m.trim()))
                .map(Some)
                .ok_or_else(|| bad(cmd, "`material` must be paper, film, card or custom"))
        }
    }
}

/// The bed jobs must stay on: the profile's, or less where the connected machine's `$130`/`$131`
/// travel is smaller.
pub(crate) fn bed_mm(s: &Session) -> (f64, f64) {
    let prof = MachineProfile::of(s);
    let (tx, ty) = s.picabia_laser.machine_travel();
    let pick = |bed: f64, travel: Option<f64>| match travel {
        Some(t) if t.is_finite() && t > 0.0 => bed.min(t),
        _ => bed,
    };
    (pick(prof.bed_w_mm, tx), pick(prof.bed_h_mm, ty))
}

/// The G-code settings for `material`: its starting values, the remembered operations, the
/// machine's `S` max (the connected machine's `$30`, else the profile's) and air, then `p`.
pub(crate) fn job_params(s: &Session, material: StencilMaterial, p: &Value) -> GcodeParams {
    let prof = MachineProfile::of(s);
    let mut gp = prof.material_ops(material.id());
    gp.s_max = s.picabia_laser.machine_s_max().unwrap_or(prof.s_max);
    gp.air = prof.air;
    gp.apply_json(p);
    gp.header = vec![format!("machine: {}", ascii(&prof.name)), format!("material: {}", material.id())];
    gp
}

/// The laser settings for G-code: as the SVG export's, with the bed the machine's.
pub(crate) fn job_laser_params(s: &Session, kerf: Option<f64>, p: &Value) -> LaserParams {
    let (w, h) = bed_mm(s);
    let mut lp = LaserParams::default();
    lp.apply_json(&s.prefs.picabia_laser);
    lp.apply_json(&json!({"bedWidthMm": w, "bedHeightMm": h}));
    if let Some(k) = kerf {
        lp.apply_json(&json!({"kerfMm": k}));
    }
    lp.apply_json(p);
    lp
}

fn ascii(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c } else { '?' }).collect()
}

/// `r` as `{x0, y0, x1, y1}` (finite).
pub(crate) fn rect_json(r: Rect) -> Value {
    json!({"x0": finite(r.x0), "y0": finite(r.y0), "x1": finite(r.x1), "y1": finite(r.y1)})
}

/// Seconds, rounded to tenths.
fn secs(v: f64) -> f64 {
    (finite(v) * 10.0).round() / 10.0
}

/// One sheet's G-code, refused (empty text) as an error.
fn sheet_gcode(cmd: &str, sheet: &LaserSheet, gp: &GcodeParams) -> Result<Gcode> {
    let g = figcraft_stencil::laser_gcode(sheet, gp);
    if g.text.is_empty() {
        let why = if g.warnings.is_empty() { "the G-code was refused".to_string() } else { g.warnings.join("; ") };
        return Err(EngineError::Disabled(cmd.into(), format!("sheet {}: {why}", sheet.name)));
    }
    Ok(g)
}

/// Write (or encode) each file and describe it.
fn emit(cmd: &str, sheets: &[(&LaserSheet, Gcode)], out: &Out, base: &str, rapid: f64) -> Result<Vec<Value>> {
    let n = sheets.len();
    let mut files = Vec::with_capacity(n);
    for (i, (sheet, g)) in sheets.iter().enumerate() {
        let mut f = json!({
            "sheet": sheet.name,
            "wMm": finite(sheet.w_mm),
            "hMm": finite(sheet.h_mm),
            "lines": g.lines,
            "estimatedSec": secs(figcraft_stencil::estimate_sec(g, rapid)),
            "burnMm": finite(g.burn_mm),
            "travelMm": finite(g.travel_mm),
            "boundsMm": rect_json(g.bounds_mm),
            "warnings": g.warnings,
        });
        let name = sheet_path(&format!("{base}.gcode"), i + 1, n);
        match out {
            Out::Path(path) => {
                let path = sheet_path(path, i + 1, n);
                crate::file_access::write(&path, g.text.as_bytes()).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))?;
                let file = std::path::Path::new(&path).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(&path).to_string();
                f["name"] = json!(file);
                f["path"] = json!(path);
            }
            Out::Data | Out::Dialog => {
                f["name"] = json!(name);
                f["data"] = json!(base64::engine::general_purpose::STANDARD.encode(g.text.as_bytes()));
            }
            Out::Summary => f["name"] = json!(name),
        }
        files.push(f);
    }
    Ok(files)
}

/// One sheet's G-code written (or encoded) and described, as [`emit`].
pub(crate) fn emit_one(cmd: &str, sheet: &LaserSheet, gp: &GcodeParams, out: &Out, base: &str, rapid: f64) -> Result<Vec<Value>> {
    let g = sheet_gcode(cmd, sheet, gp)?;
    emit(cmd, &[(sheet, g)], out, base, rapid)
}

/// `modifier.stencilGcode`: a stencil layer's sheets as GRBL G-code, one file per sheet.
pub fn stencil_gcode(s: &mut Session, p: &Value) -> Result<Value> {
    let out = crate::picabia_laser::out_of(GCODE, p)?;
    let id = stencil_layer(GCODE, s, p)?;
    let (own, kerf) = stencil_material(GCODE, s, id)?;
    let material = material_param(GCODE, p)?.unwrap_or(own);
    let lp = job_laser_params(s, kerf, p);
    let gp = job_params(s, material, p);
    if matches!(out, Out::Dialog) {
        s.request_ui(
            "dialog.stencilExport",
            json!({"id": id, "format": "gcode", "material": material.id(), "params": lp.to_json(), "gcode": gp.to_json()}),
        );
        return Ok(json!({"dialog": true}));
    }
    let job = stencil_sheets(GCODE, s, id, &lp)?;
    let mut parts = Vec::with_capacity(job.sheets.len());
    for sheet in &job.sheets {
        parts.push((sheet, sheet_gcode(GCODE, sheet, &gp)?));
    }
    let rapid = MachineProfile::of(s).rapid_mm_min;
    let files = emit(GCODE, &parts, &out, &format!("{} laser", job.base), rapid)?;
    // Remembered per material for the next export; a dry run changes nothing.
    if out != Out::Summary {
        let mut prof = MachineProfile::of(s);
        prof.remember(material.id(), &gp);
        s.prefs.picabia_machine = prof.to_json();
    }
    Ok(json!({
        "id": id,
        "files": files,
        "sheets": job.sheets.len(),
        "warnings": job.warnings,
        "material": material.id(),
        "params": lp.to_json(),
        "gcode": gp.to_json(),
    }))
}

/// The G-code settings of the test card or ladder for `setup`.
pub(crate) fn card_gcode_params(s: &Session, setup: &CardSetup, p: &Value) -> GcodeParams {
    job_params(s, setup.material, p)
}

/// The test card's or the ladder's G-code and a name for it.
fn card_job(cmd: &str, s: &Session, setup: &CardSetup, p: &Value, ladder: bool) -> Result<(LaserSheet, Gcode, GcodeParams, String)> {
    let gp = card_gcode_params(s, setup, p);
    if ladder {
        let l = figcraft_stencil::power_ladder(&LadderParams::from_json(p));
        let g = figcraft_stencil::ladder_gcode(&l, &gp);
        if g.text.is_empty() {
            return Err(EngineError::Disabled(cmd.into(), format!("the power ladder's G-code was refused: {}", g.warnings.join("; "))));
        }
        return Ok((l.sheet, g, gp, format!("Power ladder {}", setup.material.id())));
    }
    let sheet = card_sheet(setup).ok_or_else(|| EngineError::Disabled(cmd.into(), "the card needs the material's limits".into()))?;
    let g = sheet_gcode(cmd, &sheet, &gp)?;
    Ok((sheet, g, gp, format!("Test card {}", setup.material.id())))
}

/// `modifier.stencilTestCard {format: "gcode"}`: the test card or (`ladder`) the power ladder as
/// G-code.
pub(crate) fn card_gcode(s: &mut Session, p: &Value, out: &Out, setup: &CardSetup, ladder: bool) -> Result<Value> {
    let cmd = crate::picabia_laser::TEST_CARD;
    let (sheet, g, gp, base) = card_job(cmd, s, setup, p, ladder)?;
    let rapid = MachineProfile::of(s).rapid_mm_min;
    let warnings = g.warnings.clone();
    let files = emit(cmd, &[(&sheet, g)], out, &base, rapid)?;
    // The card's settings are remembered as the export's are; the ladder's per-square settings
    // aren't settings to remember.
    if *out != Out::Summary && !ladder {
        s.prefs.picabia_laser = setup.lp.to_json();
        let mut prof = MachineProfile::of(s);
        prof.remember(setup.material.id(), &gp);
        s.prefs.picabia_machine = prof.to_json();
    }
    let mut r = json!({
        "files": files,
        "sheets": 1,
        "warnings": warnings,
        "material": setup.material.id(),
        "kind": if ladder { "powerLadder" } else { "card" },
        "params": setup.lp.to_json(),
        "gcode": gp.to_json(),
    });
    if ladder {
        r["ladder"] = LadderParams::from_json(p).to_json();
    }
    Ok(r)
}

/// `laser.profile`: read the machine profile, or merge `set` into it.
pub fn profile(s: &mut Session, p: &Value) -> Result<Value> {
    let mut prof = MachineProfile::of(s);
    match get(p, "set") {
        None => {}
        Some(v @ Value::Object(_)) => {
            prof.apply_json(v);
            s.prefs.picabia_machine = prof.to_json();
        }
        Some(_) => return Err(bad(PROFILE, "`set` must be an object of profile fields")),
    }
    let (w, h) = bed_mm(s);
    let machine = s.picabia_laser.machine_s_max();
    Ok(json!({
        "profile": prof.to_json(),
        "bedMm": {"w": w, "h": h},
        "sMaxUsed": machine.unwrap_or(prof.s_max),
        "sMaxMachine": machine,
        "materialDefaults": MATERIALS.iter().map(|m| (m.to_string(), ops_json(&prof.material_ops(m)))).collect::<Map<String, Value>>(),
    }))
}

/// A job for `laser.start`/`laser.frame`: its G-code and where it goes.
pub(crate) struct LaserRun {
    pub name: String,
    pub text: String,
    /// Work coordinates of every motion target, mm.
    pub bounds: Rect,
    pub estimated_sec: Option<f64>,
    pub warnings: Vec<String>,
    /// `stencil`, `file`, `testCard`, `powerLadder`.
    pub source: &'static str,
    /// "1 of 2" for a stencil's sheet.
    pub sheet: Option<String>,
    pub layer: Option<NodeId>,
}

/// The job `p` names: `path` (a G-code file), `testCard` (card|powerLadder) or a stencil layer
/// (`id`, else the selection) and its `sheet` (1-based).
pub(crate) fn build_run(cmd: &str, s: &Session, p: &Value) -> Result<LaserRun> {
    let rapid = MachineProfile::of(s).rapid_mm_min;
    let machine_s_max = s.picabia_laser.machine_s_max();
    let mut warnings = Vec::new();
    let mut check_s_max = |gp: &mut GcodeParams| {
        if let Some(m) = machine_s_max
            && (gp.s_max - m).abs() > 1e-9
        {
            warnings.push(format!("sMax {} was replaced by the machine's $30 = {m}", gp.s_max));
            gp.s_max = m;
        }
    };
    if let Some(v) = get(p, "path") {
        let Value::String(path) = v else { return Err(bad(cmd, "`path` must be a G-code file path")) };
        if [get(p, "testCard"), get(p, "sheet")].iter().any(Option::is_some) {
            return Err(bad(cmd, "`path` can't go with `testCard` or `sheet`"));
        }
        return file_run(cmd, path);
    }
    if let Some(v) = get(p, "testCard") {
        let ladder = match v {
            Value::String(k) if k.trim().eq_ignore_ascii_case("card") => false,
            Value::String(k) if k.trim().eq_ignore_ascii_case("powerLadder") => true,
            _ => return Err(bad(cmd, "`testCard` must be card or powerLadder")),
        };
        let setup = card_setup(cmd, s, p, !ladder)?;
        let mut gp = card_gcode_params(s, &setup, p);
        check_s_max(&mut gp);
        let (_, g, _, name) = card_job(cmd, s, &setup, &json_with(p, &gp), ladder)?;
        warnings.extend(g.warnings.iter().cloned());
        return Ok(LaserRun {
            name,
            estimated_sec: Some(secs(figcraft_stencil::estimate_sec(&g, rapid))),
            bounds: g.bounds_mm,
            text: g.text,
            warnings,
            source: if ladder { "powerLadder" } else { "testCard" },
            sheet: None,
            layer: None,
        });
    }
    let Some(id) = crate::picabia_laser_design::single_stencil(s, p) else {
        // Any other layers: their outlines.
        let material = material_param(cmd, p)?.unwrap_or(StencilMaterial::Paper);
        let mut gp = job_params(s, material, p);
        check_s_max(&mut gp);
        let d = crate::picabia_laser_design::design_sheet(cmd, s, p)?;
        let g = sheet_gcode(cmd, &d.sheet, &gp)?;
        warnings.extend(d.warnings);
        warnings.extend(g.warnings.iter().cloned());
        return Ok(LaserRun {
            name: format!("{} laser", d.base),
            estimated_sec: Some(secs(figcraft_stencil::estimate_sec(&g, rapid))),
            bounds: g.bounds_mm,
            text: g.text,
            warnings,
            source: "design",
            sheet: None,
            layer: crate::params::targets(s, p).first().copied(),
        });
    };
    let (own, kerf) = stencil_material(cmd, s, id)?;
    let material = material_param(cmd, p)?.unwrap_or(own);
    let lp = job_laser_params(s, kerf, p);
    let mut gp = job_params(s, material, p);
    check_s_max(&mut gp);
    let job = stencil_sheets(cmd, s, id, &lp)?;
    let n = job.sheets.len();
    let k = match get(p, "sheet") {
        None if n == 1 => 1,
        None => return Err(EngineError::Disabled(cmd.into(), format!("the stencil has {n} sheets: choose one with `sheet` (1-{n})"))),
        Some(v) => match crate::params::f64_of(v) {
            Some(k) if k.fract() == 0.0 && k >= 1.0 && k <= n as f64 => k as usize,
            _ => return Err(bad(cmd, format!("`sheet` must be a sheet number, 1-{n}"))),
        },
    };
    let sheet = job.sheets.get(k - 1).ok_or_else(|| bad(cmd, format!("`sheet` must be a sheet number, 1-{n}")))?;
    let g = sheet_gcode(cmd, sheet, &gp)?;
    warnings.extend(job.warnings);
    warnings.extend(g.warnings.iter().cloned());
    Ok(LaserRun {
        name: if n == 1 { format!("{} laser", job.base) } else { format!("{} laser {k}", job.base) },
        estimated_sec: Some(secs(figcraft_stencil::estimate_sec(&g, rapid))),
        bounds: g.bounds_mm,
        text: g.text,
        warnings,
        source: "stencil",
        sheet: Some(sheet.name.clone()),
        layer: Some(id),
    })
}

/// `p` with `gp`'s `sMax` (the rest of `p` as given).
fn json_with(p: &Value, gp: &GcodeParams) -> Value {
    let mut v = p.clone();
    if let Some(o) = v.as_object_mut() {
        o.insert("sMax".into(), json!(gp.s_max));
    }
    v
}

/// A G-code file as a job: read (at most [`MAX_GCODE_FILE_BYTES`], ASCII text) and its bounds
/// worked out ([`gcode_bounds`]).
fn file_run(cmd: &str, path: &str) -> Result<LaserRun> {
    let p = std::path::Path::new(path);
    let ext = p.extension().and_then(std::ffi::OsStr::to_str).map(str::to_ascii_lowercase).unwrap_or_default();
    if path.trim().is_empty() || !GCODE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(bad(cmd, "`path` must be a G-code file (.gcode, .nc, .ngc, .gc, .tap or .txt)"));
    }
    let bytes = crate::file_access::read_capped(p, MAX_GCODE_FILE_BYTES).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))?;
    let text = String::from_utf8(bytes).map_err(|_| EngineError::Disabled(cmd.into(), "the file isn't plain text G-code".into()))?;
    // The head starts at the work origin.
    let bounds = gcode_bounds(&text).map_err(|why| EngineError::Disabled(cmd.into(), why))?.union_pt(figcraft_geom::kurbo::Point::ZERO);
    let name = p.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("G-code").to_string();
    Ok(LaserRun { name, text, bounds, estimated_sec: None, warnings: Vec::new(), source: "file", sheet: None, layer: None })
}

/// The work-coordinate box every move of `text` reaches (mm), arcs as their whole circle. Refused
/// (the reason) when the file moves in ways that can't be checked against the bed: machine
/// coordinates (`G53`), returns to stored positions (`G28`, `G30`), coordinate offsets (`G92`,
/// `G10`), other work coordinate systems (`G54`–`G59` other than `G54`), relative moves (`G91`)
/// from a position the file never set, or nothing that moves.
pub fn gcode_bounds(text: &str) -> std::result::Result<Rect, String> {
    let mut pos = (0.0f64, 0.0f64);
    let mut abs = true;
    let mut scale = 1.0;
    let mut motion = 0u8;
    let mut b: Option<Rect> = None;
    let mut known = (false, false);
    let grow = |b: &mut Option<Rect>, x: f64, y: f64| {
        *b = Some(match *b {
            Some(r) => Rect::new(r.x0.min(x), r.y0.min(y), r.x1.max(x), r.y1.max(y)),
            None => Rect::new(x, y, x, y),
        });
    };
    for (i, raw) in text.lines().enumerate() {
        let line = strip_comments(raw).to_ascii_uppercase();
        let words = words(&line).map_err(|w| format!("line {}: {w}", i + 1))?;
        let (mut x, mut y, mut ci, mut cj, mut r) = (None, None, None, None, None);
        for (letter, value) in words {
            match letter {
                'G' => {
                    let g = (value * 10.0).round() as i64;
                    match g {
                        0 | 10 | 20 | 30 => motion = (g / 10) as u8,
                        900 => abs = true,
                        910 => abs = false,
                        200 => scale = 25.4,
                        210 => scale = 1.0,
                        530 | 280 | 281 | 300 | 301 | 920 | 921 | 922 | 923 | 100 => {
                            return Err(format!(
                                "line {}: G{} moves or offsets in a way Picabia can't check against the bed; export the G-code from Picabia instead",
                                i + 1,
                                trim_num(value)
                            ));
                        }
                        550..=593 => {
                            return Err(format!(
                                "line {}: G{} picks another work coordinate system, which Picabia can't check against the bed",
                                i + 1,
                                trim_num(value)
                            ));
                        }
                        _ => {}
                    }
                }
                'X' => x = Some(value * scale),
                'Y' => y = Some(value * scale),
                'I' => ci = Some(value * scale),
                'J' => cj = Some(value * scale),
                'R' => r = Some(value * scale),
                _ => {}
            }
        }
        if x.is_none() && y.is_none() {
            continue;
        }
        // A relative move from a position the file never set can't be checked.
        if !abs && ((x.is_some() && !known.0) || (y.is_some() && !known.1)) {
            return Err(format!(
                "line {}: a relative move (G91) from a position the file never set, which Picabia can't check against the bed",
                i + 1
            ));
        }
        if abs {
            known = (known.0 || x.is_some(), known.1 || y.is_some());
        }
        let start = pos;
        let to = |cur: f64, v: Option<f64>| match v {
            Some(v) if abs => v,
            Some(v) => cur + v,
            None => cur,
        };
        pos = (to(pos.0, x), to(pos.1, y));
        if !(pos.0.is_finite() && pos.1.is_finite()) || pos.0.abs() > 1e6 || pos.1.abs() > 1e6 {
            return Err(format!("line {}: a coordinate out of range", i + 1));
        }
        // Where the head starts isn't the file's: only where it moves to (arcs below).
        grow(&mut b, pos.0, pos.1);
        if motion == 2 || motion == 3 {
            // The whole circle: an arc can bulge past its end points.
            let rad = match (ci, cj, r) {
                (_, _, Some(r)) => r.abs(),
                (i, j, None) => i.unwrap_or(0.0).hypot(j.unwrap_or(0.0)),
            };
            let (cx, cy) = match r {
                Some(_) => (start.0, start.1),
                None => (start.0 + ci.unwrap_or(0.0), start.1 + cj.unwrap_or(0.0)),
            };
            let rad = if r.is_some() { rad * 2.0 } else { rad };
            if rad.is_finite() {
                grow(&mut b, cx - rad, cy - rad);
                grow(&mut b, cx + rad, cy + rad);
            }
        }
    }
    b.ok_or_else(|| "the file has no moves".to_string())
}

fn trim_num(v: f64) -> String {
    let s = format!("{v:.1}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `line` without `;…` and `(…)` comments.
fn strip_comments(line: &str) -> String {
    let line = line.split(';').next().unwrap_or("");
    let mut out = String::with_capacity(line.len());
    let mut depth = 0u32;
    for c in line.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// The `letter number` words of a comment-free, upper-case line (`$` lines and `%` have none).
fn words(line: &str) -> std::result::Result<Vec<(char, f64)>, String> {
    let line: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    if line.starts_with('$') || line.starts_with('%') || line.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if !c.is_ascii_alphabetic() {
            return Err(format!("`{c}` where a letter was expected"));
        }
        let start = at + c.len_utf8();
        let mut end = start;
        while let Some(&(j, d)) = chars.peek() {
            if d.is_ascii_digit() || matches!(d, '.' | '-' | '+') {
                end = j + d.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let v = line.get(start..end).and_then(|t| t.parse::<f64>().ok()).filter(|v| v.is_finite());
        let Some(v) = v else { return Err(format!("`{c}` without a number")) };
        out.push((c, v));
    }
    Ok(out)
}
