//! The layout oracle: a game's layout of a design held to Picabia's own. The game prints, case by
//! case, the values it laid a screen out with and what it drew; the oracle gives the design the
//! same values through Picabia's commands (texts filled, flagged instances shown or hidden, the
//! chosen pressables in their `Selected` variant, the screen resized to the viewport), lets Picabia
//! lay it out, and compares every fill's rectangle and every text's start and baseline.
//!
//! The game's lines, in order:
//! - `case <screen> <width> <height>`: a screen (kebab case) laid out at that size in design units;
//! - `text <slot> <value…>`, `flag <flag> on|off`, `selected <action>`: the values it used;
//! - `rect <x> <y> <w> <h>` and `run <x> <baseline> <text…>`: what it drew, in order.
//!
//! Other lines are ignored. Fails on any disagreement beyond `TOLERANCE` pixels.

use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::convert::{ACTION_PROP, SCREENS_PAGE, SHOWN_PROP, STATE_PROP, constructor};

/// How far, in design pixels, a position or size may differ (F32 arithmetic against F64).
pub const TOLERANCE: f64 = 0.05;

#[derive(Clone, Debug, PartialEq)]
pub enum Drawn {
    Rect { x: f64, y: f64, w: f64, h: f64 },
    Run { x: f64, baseline: f64, text: String },
}

#[derive(Debug, Default)]
pub struct Case {
    pub screen: String,
    pub width: f64,
    pub height: f64,
    pub texts: Vec<(String, String)>,
    pub flags: Vec<(String, bool)>,
    pub selected: Vec<String>,
    pub drawn: Vec<Drawn>,
}

fn num(s: Option<&str>) -> Result<f64, String> {
    s.ok_or("a number is missing")?.parse::<f64>().map_err(|e| e.to_string())
}

fn last(cases: &mut [Case], line: usize) -> Result<&mut Case, String> {
    cases.last_mut().ok_or_else(|| format!("line {}: before any case", line + 1))
}

/// The game's printed cases.
pub fn parse(input: &str) -> Result<Vec<Case>, String> {
    let mut cases: Vec<Case> = Vec::new();
    for (i, line) in input.lines().enumerate() {
        let mut w = line.split(' ');
        let at = |e: String| format!("line {}: {e}", i + 1);
        match w.next() {
            Some("case") => cases.push(Case {
                screen: w.next().ok_or_else(|| at("no screen".into()))?.to_string(),
                width: num(w.next()).map_err(at)?,
                height: num(w.next()).map_err(at)?,
                ..Case::default()
            }),
            Some("text") => {
                let slot = w.next().ok_or_else(|| at("no slot".into()))?.to_string();
                last(&mut cases, i)?.texts.push((slot, w.collect::<Vec<_>>().join(" ")));
            }
            Some("flag") => {
                let flag = w.next().ok_or_else(|| at("no flag".into()))?.to_string();
                let on = w.next() == Some("on");
                last(&mut cases, i)?.flags.push((flag, on));
            }
            Some("selected") => {
                let a = w.next().ok_or_else(|| at("no action".into()))?.to_string();
                last(&mut cases, i)?.selected.push(a);
            }
            Some("rect") => {
                let r = Drawn::Rect {
                    x: num(w.next()).map_err(at)?,
                    y: num(w.next()).map_err(at)?,
                    w: num(w.next()).map_err(at)?,
                    h: num(w.next()).map_err(at)?,
                };
                last(&mut cases, i)?.drawn.push(r);
            }
            Some("run") => {
                let (x, b) = (num(w.next()).map_err(at)?, num(w.next()).map_err(at)?);
                last(&mut cases, i)?.drawn.push(Drawn::Run { x, baseline: b, text: w.collect::<Vec<_>>().join(" ") });
            }
            _ => {}
        }
    }
    Ok(cases)
}

fn same_name(a: &str, b: &str) -> bool {
    constructor(a).is_some_and(|c| constructor(b).as_ref() == Some(&c))
}

fn prop(doc: &Document, id: NodeId, name: &str) -> Option<String> {
    match doc.node(id).map(|n| &n.kind) {
        Some(NodeKind::Instance { props, .. }) => match props.get(name) {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            _ => None,
        },
        _ => None,
    }
}

fn doc_of(s: &Session) -> Result<&Document, String> {
    s.doc().map_err(|e| e.to_string())
}

fn ex(s: &mut Session, cmd: &str, p: Value) -> Result<(), String> {
    s.execute(cmd, &p).map(|_| ()).map_err(|e| format!("{cmd}: {e}"))
}

/// The design laid out by Picabia for `case`: what it draws, in the game's order.
pub fn expected(doc: &Document, case: &Case) -> Result<Vec<Drawn>, String> {
    let mut s = Session::with_document(doc.clone());
    let page = doc.pages.iter().copied().find(|p| doc.node(*p).is_some_and(|n| n.name == SCREENS_PAGE)).ok_or("no Screens page")?;
    let root = doc.node(page).and_then(|p| p.children.iter().copied().find(|c| doc.node(*c).is_some_and(|n| same_name(&n.name, &case.screen))));
    let root = root.ok_or_else(|| format!("no screen {}", case.screen))?;
    let subtree = doc.subtree(root);
    for id in &subtree {
        let d = doc_of(&s)?;
        if let Some(NodeKind::Text(t)) = d.node(*id).map(|n| &n.kind) {
            let chars = t.characters.trim();
            if let Some((_, v)) =
                case.texts.iter().find(|(slot, _)| chars.strip_prefix('{').and_then(|c| c.strip_suffix('}')).is_some_and(|c| same_name(c, slot)))
            {
                ex(&mut s, "node.set", json!({"ids": [id], "props": {"characters": v}}))?;
            }
        }
        if let Some(flag) = prop(doc_of(&s)?, *id, SHOWN_PROP) {
            let on = case.flags.iter().any(|(f, on)| *on && same_name(f, &flag));
            ex(&mut s, "node.set", json!({"ids": [id], "props": {"visible": on}}))?;
        }
        if let Some(action) = prop(doc_of(&s)?, *id, ACTION_PROP)
            && case.selected.iter().any(|a| same_name(a, &action))
        {
            ex(&mut s, "instance.setProperties", json!({"ids": [id], "properties": {STATE_PROP: "Selected"}}))?;
        }
    }
    ex(&mut s, "node.set", json!({"ids": [root], "props": {"width": case.width, "height": case.height}}))?;
    let d = doc_of(&s)?;
    let mut out = Vec::new();
    walk(d, root, (0.0, 0.0), true, &mut out);
    Ok(out)
}

fn walk(doc: &Document, id: NodeId, origin: (f64, f64), is_root: bool, out: &mut Vec<Drawn>) {
    let Some(n) = doc.node(id) else { return };
    if !n.visible {
        return;
    }
    let c = n.transform.as_coeffs();
    let (x, y) = if is_root { origin } else { (origin.0 + c[4], origin.1 + c[5]) };
    match &n.kind {
        NodeKind::Text(t) => {
            let laid = figcraft_text::layout_cached(t, None, None);
            let natural = laid.width.ceil();
            let line = laid.lines.first();
            let baseline = line.map(|l| l.baseline - l.top).unwrap_or(0.0);
            let h_line = line.map(|l| l.height).unwrap_or(n.height);
            let dx = match t.align_h {
                figcraft_doc::TextAlignH::Center => (n.width - natural) / 2.0,
                figcraft_doc::TextAlignH::Right => n.width - natural,
                _ => 0.0,
            };
            let dy = match t.align_v {
                figcraft_doc::TextAlignV::Middle => (n.height - h_line) / 2.0,
                figcraft_doc::TextAlignV::Bottom => n.height - h_line,
                figcraft_doc::TextAlignV::Top => 0.0,
            };
            out.push(Drawn::Run { x: x + dx, baseline: y + dy + baseline, text: t.characters.clone() });
        }
        _ => {
            for p in n.fills.iter().filter(|p| p.visible) {
                if matches!(p.kind, figcraft_color::PaintKind::Solid { .. }) {
                    out.push(Drawn::Rect { x, y, w: n.width, h: n.height });
                }
            }
            for k in &n.children {
                walk(doc, *k, (x, y), false, out);
            }
        }
    }
}

fn agrees(a: &Drawn, b: &Drawn) -> bool {
    let near = |p: f64, q: f64| (p - q).abs() <= TOLERANCE;
    match (a, b) {
        (Drawn::Rect { x, y, w, h }, Drawn::Rect { x: x2, y: y2, w: w2, h: h2 }) => near(*x, *x2) && near(*y, *y2) && near(*w, *w2) && near(*h, *h2),
        (Drawn::Run { x, baseline, text }, Drawn::Run { x: x2, baseline: b2, text: t2 }) => near(*x, *x2) && near(*baseline, *b2) && text == t2,
        _ => false,
    }
}

/// Every case compared: the disagreements, each named by its case and place.
pub fn compare(doc: &Document, cases: &[Case]) -> Result<Vec<String>, String> {
    let mut bad = Vec::new();
    for case in cases {
        let want = expected(doc, case)?;
        let name = format!("{} at {}x{}", case.screen, case.width, case.height);
        if want.len() != case.drawn.len() {
            bad.push(format!("{name}: Picabia draws {} things, the game {}", want.len(), case.drawn.len()));
        }
        for (i, (w, g)) in want.iter().zip(&case.drawn).enumerate() {
            if !agrees(w, g) {
                bad.push(format!("{name}, #{i}: Picabia {w:?}, the game {g:?}"));
            }
        }
    }
    Ok(bad)
}
