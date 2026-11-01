//! PICABIA: laser export of stencil layers (`modifier.stencilExport`) and the laser test card
//! (`modifier.stencilTestCard`).
//!
//! The geometry is `figcraft_stencil`'s ([`figcraft_stencil::laser_job`],
//! [`figcraft_stencil::test_card`], [`figcraft_stencil::laser_svg`]); here the settings are
//! layered (the remembered ones in [`crate::Prefs::picabia_laser`], then the material's kerf, then
//! the command's params), the stencil is brought up to date, and the sheets are written to files
//! (or returned as base64). Without `path` or `data` a command only asks the UI for its dialog.

use base64::Engine as _;
use figcraft_doc::{Node, NodeId};
use figcraft_modifiers::{MaterialLimits, Stencil, StencilMaterial};
use figcraft_render::picabia::stencil as store;
use figcraft_stencil::{LaserInput, LaserParams, LaserSheet};
use serde_json::{Value, json};

use crate::params::{get, string, targets};
use crate::{EngineError, Result, Session, bad};

const EXPORT: &str = "modifier.stencilExport";
const TEST_CARD: &str = "modifier.stencilTestCard";

/// Where the sheets go: a file path, base64 data, only their description (a dry run: the
/// dialog's summary), or (none of those) the dialog.
#[derive(PartialEq)]
enum Out {
    Path(String),
    Data,
    Summary,
    Dialog,
}

/// `dryRun: true` wins, then `path` (a non-empty string), then `data: true`; none: the dialog.
fn out_of(cmd: &str, p: &Value) -> Result<Out> {
    match get(p, "dryRun") {
        None | Some(Value::Bool(false)) => {}
        Some(Value::Bool(true)) => return Ok(Out::Summary),
        Some(_) => return Err(bad(cmd, "`dryRun` must be true or false")),
    }
    match get(p, "path") {
        None => {}
        Some(Value::String(s)) if !s.trim().is_empty() => return Ok(Out::Path(s.clone())),
        Some(_) => return Err(bad(cmd, "`path` must be a file path")),
    }
    match get(p, "data") {
        None | Some(Value::Bool(false)) => Ok(Out::Dialog),
        Some(Value::Bool(true)) => Ok(Out::Data),
        Some(_) => Err(bad(cmd, "`data` must be true or false")),
    }
}

/// The settings a command uses: defaults, then the remembered ones, then `kerf` (the material's;
/// `None` keeps the remembered kerf), then the command's own params. Clamped.
fn settings(s: &Session, kerf: Option<f64>, p: &Value) -> LaserParams {
    let mut lp = LaserParams::default();
    lp.apply_json(&s.prefs.picabia_laser);
    if let Some(k) = kerf {
        lp.apply_json(&json!({"kerfMm": k}));
    }
    lp.apply_json(p);
    lp
}

/// The stencil layer a command acts on: `id`/`ids` or the selection, its first layer.
fn stencil_layer(cmd: &str, s: &Session, p: &Value) -> Result<NodeId> {
    let id = targets(s, p).first().copied().ok_or_else(|| EngineError::Disabled(cmd.into(), "select a stencil layer".into()))?;
    let n = s.doc()?.get(id)?;
    if !store::is_stencil(n) {
        return Err(EngineError::Disabled(cmd.into(), format!("layer {id} isn't a stencil (its modifiers don't end in a stencil row)")));
    }
    Ok(id)
}

/// The stencil params of layer `n` (`None`: not a stencil with a usable size).
fn stencil_params(s: &Session, n: &Node) -> Option<Stencil> {
    store::frame(s.doc().ok()?, n).map(|f| f.params)
}

/// A material's name as it is engraved and shown: "Paper", "Film", "Card", "Custom".
fn material_name(m: StencilMaterial) -> &'static str {
    match m {
        StencilMaterial::Paper => "Paper",
        StencilMaterial::Film => "Film",
        StencilMaterial::Card => "Card",
        StencilMaterial::Custom => "Custom",
    }
}

/// A length in mm for a label: one decimal at most, without a trailing `.0`.
fn mm(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r:.1}") }
}

/// `name` as a file name: path separators and characters file systems refuse become `-`.
fn file_safe(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .take(120)
        .map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '-' } else { c })
        .collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "Stencil".into() } else { s }
}

/// `path` for sheet `k` (1-based) of `n`: `path` itself for one sheet, else " k" before the
/// extension (`Rose.svg` → `Rose 1.svg`; without an extension, at the end).
pub fn sheet_path(path: &str, k: usize, n: usize) -> String {
    if n <= 1 {
        return path.to_string();
    }
    let p = std::path::Path::new(path);
    match (p.extension().and_then(std::ffi::OsStr::to_str), p.file_stem().and_then(std::ffi::OsStr::to_str)) {
        (Some(ext), Some(_)) if path.len() > ext.len() => {
            let cut = path.len() - ext.len() - 1;
            match path.get(..cut) {
                Some(base) => format!("{base} {k}.{ext}"),
                None => format!("{path} {k}"),
            }
        }
        _ => format!("{path} {k}"),
    }
}

/// Write (or encode) each sheet as an SVG, and describe it.
fn emit(cmd: &str, sheets: &[LaserSheet], lp: &LaserParams, out: &Out, base: &str) -> Result<Vec<Value>> {
    let n = sheets.len();
    let mut files = Vec::with_capacity(n);
    for (i, sheet) in sheets.iter().enumerate() {
        let mut f = json!({
            "sheet": sheet.name,
            "wMm": finite(sheet.w_mm),
            "hMm": finite(sheet.h_mm),
            "contours": sheet.cut.len(),
            "loosePieces": sheet.loose_pieces,
        });
        match out {
            Out::Path(path) => {
                let svg = figcraft_stencil::laser_svg(sheet, lp);
                let path = sheet_path(path, i + 1, n);
                crate::file_access::write(&path, svg.as_bytes()).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))?;
                let name = std::path::Path::new(&path).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(&path).to_string();
                f["name"] = json!(name);
                f["path"] = json!(path);
            }
            Out::Data | Out::Dialog => {
                let svg = figcraft_stencil::laser_svg(sheet, lp);
                f["name"] = json!(sheet_path(&format!("{base}.svg"), i + 1, n));
                f["data"] = json!(base64::engine::general_purpose::STANDARD.encode(svg.as_bytes()));
            }
            Out::Summary => f["name"] = json!(sheet_path(&format!("{base}.svg"), i + 1, n)),
        }
        files.push(f);
    }
    Ok(files)
}

fn finite(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

/// `modifier.stencilExport`: a stencil layer's drawn cut (the tile with tile output) as
/// kerf-compensated laser sheets.
pub fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let out = out_of(EXPORT, p)?;
    let id = stencil_layer(EXPORT, s, p)?;
    let doc = s.doc()?;
    let n = doc.get(id)?;
    let frame = store::frame(doc, n).ok_or_else(|| EngineError::Disabled(EXPORT.into(), format!("layer {id} has no size to cut")))?;
    let lp = settings(s, frame.params.material.limits().map(|l| l.kerf_mm), p);
    if matches!(out, Out::Dialog) {
        s.request_ui("dialog.stencilExport", json!({"id": id, "params": lp.to_json()}));
        return Ok(json!({"dialog": true}));
    }
    // The fresh result when there is one, else computed now (and kept for the canvas).
    let fresh = match (crate::picabia_stencil::info(doc, n), store::lookup(doc, n)) {
        (Some((info, true)), Some((shape, true))) => Some((info, shape.cut.clone(), shape.sheet)),
        _ => None,
    };
    let (info, cut, sheet) = fresh
        .or_else(|| crate::picabia_stencil::compute_cut_now(doc, id))
        .ok_or_else(|| EngineError::Other(format!("{EXPORT}: layer {id}'s stencil couldn't be computed")))?;
    if cut.elements().is_empty() {
        let why = info.warnings.first().map(|w| format!(" ({w})")).unwrap_or_default();
        return Err(EngineError::Disabled(EXPORT.into(), format!("layer {id}'s stencil has nothing to cut{why}")));
    }
    let size = match info.tile_mm {
        Some(_) => "Repeat tile".to_string(),
        None => format!("Sheet {} × {} mm", mm(info.sheet_mm.0), mm(info.sheet_mm.1)),
    };
    let label_lines: Vec<String> =
        [n.name.trim().to_string(), size, material_name(frame.params.material).to_string()].into_iter().filter(|l| !l.is_empty()).collect();
    let input = LaserInput {
        cut,
        sheet,
        mm_per_unit: frame.mm_per_unit,
        hoshi: info.hoshi.clone(),
        hoshi_d_mm: frame.params.hoshi_diameter_mm,
        label_lines,
        repeat_cell: info.cell,
    };
    let job = figcraft_stencil::laser_job(&input, &lp);
    if job.sheets.is_empty() {
        let why = if job.warnings.is_empty() { "the laser job was refused".to_string() } else { job.warnings.join("; ") };
        return Err(EngineError::Disabled(EXPORT.into(), why));
    }
    let base = format!("{} laser", file_safe(&n.name));
    let files = emit(EXPORT, &job.sheets, &lp, &out, &base)?;
    let params = lp.to_json();
    // Remembered for the next export; a dry run (the dialog's summary) changes nothing.
    if out != Out::Summary {
        s.prefs.picabia_laser = params.clone();
    }
    Ok(json!({"id": id, "files": files, "sheets": job.sheets.len(), "warnings": job.warnings, "params": params}))
}

/// The material `p` names (`material`), else the selected stencil's, else paper.
fn card_material(p: &Value, stencil: Option<&Stencil>) -> Result<StencilMaterial> {
    match get(p, "material") {
        None => Ok(stencil.map_or(StencilMaterial::Paper, |st| st.material)),
        Some(_) => {
            let m = string(p, "material").unwrap_or_default();
            StencilMaterial::ALL
                .into_iter()
                .find(|x| x.id().eq_ignore_ascii_case(m.trim()))
                .ok_or_else(|| bad(TEST_CARD, "`material` must be paper, film, card or custom"))
        }
    }
}

/// `modifier.stencilTestCard`: the card that measures a material's kerf and cutting limits.
pub fn test_card(s: &mut Session, p: &Value) -> Result<Value> {
    let out = out_of(TEST_CARD, p)?;
    // The selected (or named) stencil layer, when there is one: its material and limits.
    let picked = targets(s, p).first().copied();
    let stencil = match picked {
        Some(id) => {
            let n = s.doc()?.get(id)?;
            if store::is_stencil(n) { stencil_params(s, n).map(|st| (id, st)) } else { None }
        }
        None => None,
    };
    let material = card_material(p, stencil.as_ref().map(|(_, st)| st))?;
    let lp = settings(s, material.limits().map(|l| l.kerf_mm), p);
    let limits = match material.limits() {
        Some(l) => MaterialLimits { kerf_mm: lp.kerf_mm, ..l },
        None => {
            let Some((_, st)) = &stencil else {
                return Err(EngineError::Disabled(TEST_CARD.into(), "a custom material's card needs a selected stencil layer (its limits)".into()));
            };
            MaterialLimits {
                min_cut_width_mm: st.min_cut_width_mm,
                min_bridge_mm: st.min_bridge_mm,
                tie_width_mm: st.tie_width_mm,
                min_corner_angle: st.min_corner_angle,
                kerf_mm: lp.kerf_mm,
            }
        }
    };
    if matches!(out, Out::Dialog) {
        let id = stencil.as_ref().map_or(Value::Null, |(id, _)| json!(id));
        s.request_ui("dialog.stencilTestCard", json!({"id": id, "material": material.id(), "params": lp.to_json()}));
        return Ok(json!({"dialog": true}));
    }
    let card = figcraft_stencil::test_card(&limits, material_name(material), lp.kerf_mm);
    let base = format!("Test card {}", material.id());
    let files = emit(TEST_CARD, std::slice::from_ref(&card), &lp, &out, &base)?;
    let params = lp.to_json();
    // Remembered for the next export; a dry run (the dialog's summary) changes nothing.
    if out != Out::Summary {
        s.prefs.picabia_laser = params.clone();
    }
    Ok(json!({"files": files, "sheets": 1, "warnings": [], "material": material.id(), "params": params}))
}
