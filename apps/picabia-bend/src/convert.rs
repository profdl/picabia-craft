//! A Picabia document's screens converted to the game's interface tree. Every layer the game's
//! vocabulary cannot draw faithfully is refused by name, and every refusal is reported at once
//! (a designer fixes a file, not one error at a time).
//!
//! The design's conventions:
//! - the page named `Screens` holds one top-level frame per screen, named for the game's screen
//!   in kebab case (`hud-play`);
//! - a text whose characters are `{name}` shows the game's text slot `name`;
//! - an instance with a text property `@action` is pressable: a click gives that action, and its
//!   component set's `State` variants (`Idle`, and optionally `Hovered`, `Pressed`, `Selected`)
//!   are what it shows in each state (a state without a variant shows `Idle`);
//! - an instance with a text property `@shown` is shown while the game's flag of that name holds.
//!
//! A property left empty binds nothing (an instance takes its component's defaults, so a
//! component offering `@shown` gives it an empty default).

use std::collections::BTreeMap;

use figcraft_doc::{CrossAlign as PCross, HConstraint, LayoutMode, MainAlign as PMain, Sizing as PSizing, StrokeAlign, VConstraint};
use figcraft_doc::{Document, Node, NodeId, NodeKind};
use figcraft_doc::{TextAlignH, TextAlignV, TextAutoResize, TextCase, TextDecoration};
use figcraft_engine::Session;
use figcraft_engine::measure::TextMeasure;
use figcraft_layout::Measure;
use serde_json::{Value, json};

use crate::model::{
    AlignH, AlignV, Arrange, Bounds, Constructor, CrossAlign, Design, FontId, HPin, MainAlign, Metrics, Paint, Sizing, Stroke, Surface, TextStyle,
    UiNode, VPin, Words,
};

/// The page whose top-level frames are the game's screens.
pub const SCREENS_PAGE: &str = "Screens";
/// The component property naming the action a click on an instance gives.
pub const ACTION_PROP: &str = "@action";
/// The component property naming the flag an instance is shown while.
pub const SHOWN_PROP: &str = "@shown";
/// The variant property whose values are a pressable's states.
pub const STATE_PROP: &str = "State";
/// The states a pressable shows, in the order the vocabulary's `Pressable` takes them.
pub const STATES: [&str; 4] = ["Idle", "Hovered", "Pressed", "Selected"];
/// The characters a bound text may show, measured into each font's metrics: printable ASCII.
pub const BOUND_CHARS: std::ops::RangeInclusive<u32> = 32..=126;

/// A layer the generator cannot carry into the game, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub layer: String,
    pub why: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.layer, self.why)
    }
}

/// A slot, screen, action or flag name in kebab or snake case as its Bend constructor.
pub fn constructor(name: &str) -> Option<Constructor> {
    let mut out = String::new();
    for part in name.trim().split(['-', '_', ' ']) {
        let mut cs = part.chars();
        let first = cs.next()?;
        if !first.is_ascii_alphabetic() || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        out.push(first.to_ascii_uppercase());
        out.extend(cs);
    }
    (!out.is_empty()).then_some(Constructor(out))
}

struct Ctx {
    refusals: Vec<Refusal>,
    fonts: Vec<Metrics>,
    font_ids: BTreeMap<(String, String), FontId>,
}

impl Ctx {
    fn refuse(&mut self, layer: &str, why: impl Into<String>) {
        self.refusals.push(Refusal { layer: layer.to_string(), why: why.into() });
    }

    fn font(&mut self, layer: &str, family: &str, style: &str) -> Option<FontId> {
        let key = (family.to_string(), style.to_string());
        if let Some(id) = self.font_ids.get(&key) {
            return Some(*id);
        }
        let face = figcraft_text::fontdb::db().write().ok()?.find(family, style);
        let Some(face) = face else {
            self.refuse(layer, format!("the font {family} {style} is not loaded (pass its file with --font)"));
            return None;
        };
        let advances = BOUND_CHARS.map(|c| (c, char::from_u32(c).map(|ch| face.advance(face.glyph_for(ch))).unwrap_or(0.0))).collect();
        let id = self.fonts.len() as FontId;
        self.fonts.push(Metrics { family: family.into(), style: style.into(), upem: face.upem, missing: face.advance(0), advances });
        self.font_ids.insert(key, id);
        Some(id)
    }
}

fn sizing(s: PSizing) -> Sizing {
    match s {
        PSizing::Fixed => Sizing::Fixed,
        PSizing::Hug => Sizing::Hug,
        PSizing::Fill => Sizing::Fill,
    }
}

fn thousandths(v: f64) -> u32 {
    (v.clamp(0.0, 1.0) * 1000.0).round() as u32
}

fn packed(c: &figcraft_color::Color) -> u32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(c.r) << 16) | (byte(c.g) << 8) | byte(c.b)
}

fn plain_blend(b: figcraft_color::BlendMode) -> bool {
    matches!(b, figcraft_color::BlendMode::Normal | figcraft_color::BlendMode::PassThrough)
}

fn solid(ctx: &mut Ctx, layer: &str, p: &figcraft_color::Paint, what: &str) -> Option<Paint> {
    if !plain_blend(p.blend) {
        ctx.refuse(layer, format!("a {what} with a blend mode other than normal is not drawn yet"));
        return None;
    }
    match &p.kind {
        figcraft_color::PaintKind::Solid { color } => {
            Some(Paint { rgb: packed(color), alpha: thousandths(f64::from(p.opacity) * f64::from(color.a)) })
        }
        _ => {
            ctx.refuse(layer, format!("only solid {what}s are drawn yet (gradients, images and patterns are not)"));
            None
        }
    }
}

fn bounds(ctx: &mut Ctx, layer: &str, n: &Node) -> Bounds {
    let [a, b, c, d, e, f] = n.transform.as_coeffs();
    if b.abs() > 1e-9 || c.abs() > 1e-9 || (a - 1.0).abs() > 1e-9 || (d - 1.0).abs() > 1e-9 {
        ctx.refuse(layer, "a rotated, scaled or flipped layer is not drawn yet");
    }
    let cl = &n.child_layout;
    if cl.min_w.is_some() || cl.max_w.is_some() || cl.min_h.is_some() || cl.max_h.is_some() {
        ctx.refuse(layer, "minimum and maximum sizes are not laid out yet");
    }
    let pin_h = match n.constraints.horizontal {
        HConstraint::Left => HPin::Left,
        HConstraint::Right => HPin::Right,
        HConstraint::LeftRight => HPin::LeftRight,
        HConstraint::Center => HPin::Centre,
        HConstraint::Scale => HPin::Scale,
    };
    let pin_v = match n.constraints.vertical {
        VConstraint::Top => VPin::Top,
        VConstraint::Bottom => VPin::Bottom,
        VConstraint::TopBottom => VPin::TopBottom,
        VConstraint::Center => VPin::Centre,
        VConstraint::Scale => VPin::Scale,
    };
    Bounds { x: e, y: f, w: n.width, h: n.height, size_w: sizing(cl.sizing_h), size_h: sizing(cl.sizing_v), pin_h, pin_v }
}

fn common_refusals(ctx: &mut Ctx, layer: &str, n: &Node) {
    if n.effects.iter().any(|e| e.visible) {
        ctx.refuse(layer, "effects (shadows, blurs) are not drawn yet");
    }
    if n.is_mask {
        ctx.refuse(layer, "masks are not drawn yet");
    }
    if !plain_blend(n.blend) {
        ctx.refuse(layer, "a layer blend mode other than normal is not drawn yet");
    }
}

fn surface(ctx: &mut Ctx, layer: &str, n: &Node) -> Surface {
    let fills = n.fills.iter().filter(|p| p.visible).filter_map(|p| solid(ctx, layer, p, "fill")).collect();
    let strokes: Vec<&figcraft_color::Paint> = n.strokes.iter().filter(|p| p.visible).collect();
    let stroke = match strokes.as_slice() {
        [] => Stroke::Unstroked,
        _ if n.stroke.weight <= 0.0 => Stroke::Unstroked,
        [p] if n.stroke.align == StrokeAlign::Inside && n.stroke.sides.is_none() && n.stroke.dashes.is_empty() => {
            solid(ctx, layer, p, "stroke").map(|paint| Stroke::Inside { paint, weight: n.stroke.weight }).unwrap_or(Stroke::Unstroked)
        }
        _ => {
            ctx.refuse(layer, "only one solid, inside, undashed stroke of one weight is drawn yet");
            Stroke::Unstroked
        }
    };
    if n.corners.smoothing > 0.0 {
        ctx.refuse(layer, "smoothed corners are not drawn yet");
    }
    Surface { fills, stroke, corners: n.corners.radii, opacity: thousandths(f64::from(n.opacity)), clips: n.clip }
}

fn arrange(ctx: &mut Ctx, layer: &str, n: &Node) -> Arrange {
    let Some(al) = &n.layout else { return Arrange::Free };
    if al.mode == LayoutMode::Grid {
        ctx.refuse(layer, "grid auto layout is not laid out yet");
    }
    if al.wrap {
        ctx.refuse(layer, "wrapping auto layout is not laid out yet");
    }
    if al.reverse_z {
        ctx.refuse(layer, "auto layout drawn first child on top is not drawn yet");
    }
    let cross = match al.cross_align {
        PCross::Min => CrossAlign::Start,
        PCross::Center => CrossAlign::Centre,
        PCross::Max => CrossAlign::End,
        PCross::Baseline => {
            ctx.refuse(layer, "baseline alignment is not laid out yet");
            CrossAlign::Start
        }
    };
    let main = match al.main_align {
        PMain::Min => MainAlign::Start,
        PMain::Center => MainAlign::Centre,
        PMain::Max => MainAlign::End,
        PMain::SpaceBetween => MainAlign::Between,
        PMain::SpaceAround => MainAlign::Around,
        PMain::SpaceEvenly => MainAlign::Evenly,
    };
    // Picabia's insets: padding, plus the frame's own inside stroke when strokes are included.
    let included = al.strokes_included && n.has_visible_stroke() && n.stroke.align == StrokeAlign::Inside;
    let stroke = if included { n.stroke.sides.unwrap_or([n.stroke.weight; 4]) } else { [0.0; 4] };
    let pad = |i: usize| al.padding[i].max(0.0) + stroke[i].max(0.0);
    Arrange::Stack { horizontal: al.mode == LayoutMode::Horizontal, gap: al.gap, padding: [pad(0), pad(1), pad(2), pad(3)], main, cross }
}

fn label(ctx: &mut Ctx, doc: &Document, layer: &str, id: NodeId, n: &Node, t: &figcraft_doc::TextData) -> Option<UiNode> {
    if !t.runs.is_empty() {
        ctx.refuse(layer, "a text of more than one style is not drawn yet");
    }
    if t.characters.contains(['\n', '\u{2028}', '\u{2029}']) {
        ctx.refuse(layer, "a text of more than one line is not drawn yet");
    }
    if t.auto_resize == TextAutoResize::Truncate {
        ctx.refuse(layer, "truncated text is not drawn yet");
    }
    if t.style.case != TextCase::Original || t.style.decoration != TextDecoration::None {
        ctx.refuse(layer, "text case and decorations are not drawn yet");
    }
    let align_h = match t.align_h {
        TextAlignH::Left => AlignH::Left,
        TextAlignH::Center => AlignH::Centre,
        TextAlignH::Right => AlignH::Right,
        TextAlignH::Justified => {
            ctx.refuse(layer, "justified text is not drawn yet");
            AlignH::Left
        }
    };
    let align_v = match t.align_v {
        TextAlignV::Top => AlignV::Top,
        TextAlignV::Middle => AlignV::Middle,
        TextAlignV::Bottom => AlignV::Bottom,
    };
    let paint = match n.fills.iter().filter(|p| p.visible).collect::<Vec<_>>().as_slice() {
        [p] => solid(ctx, layer, p, "text fill")?,
        _ => {
            ctx.refuse(layer, "a text needs exactly one visible fill");
            return None;
        }
    };
    let font = ctx.font(layer, &t.style.font_family, &t.style.font_style)?;
    let (natural_w, line) = TextMeasure.text_size(doc, id).unwrap_or((n.width, n.height));
    let laid = figcraft_text::layout_cached(t, None, None);
    let baseline = laid.lines.first().map(|l| l.baseline - l.top).unwrap_or(line);
    let style = TextStyle {
        font,
        size: t.style.font_size,
        line,
        baseline,
        tracking: t.style.letter_spacing.resolve(t.style.font_size),
        paint,
        align_h,
        align_v,
    };
    let chars = t.characters.trim();
    let words = match chars.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        Some(name) => match constructor(name) {
            Some(slot) => Words::Bound { slot, font },
            None => {
                ctx.refuse(layer, format!("`{name}` is not a slot name (letters and digits, in kebab or snake case)"));
                return None;
            }
        },
        None => Words::Literal { text: t.characters.clone(), width: natural_w },
    };
    common_refusals(ctx, layer, n);
    Some(UiNode::Label { bounds: bounds(ctx, layer, n), style, words })
}

fn text_prop(n: &Node, name: &str) -> Option<String> {
    match &n.kind {
        NodeKind::Instance { props, .. } => match props.get(name) {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// The document with instance `id` showing variant `state` of its `State` property.
fn in_state(doc: &Document, id: NodeId, state: &str) -> Result<Document, String> {
    let mut s = Session::with_document(doc.clone());
    s.execute("instance.setProperties", &json!({"ids": [id], "properties": {STATE_PROP: state}})).map_err(|e| e.to_string())?;
    s.doc().cloned().map_err(|e| e.to_string())
}

fn states_of(doc: &Document, n: &Node) -> Vec<String> {
    let NodeKind::Instance { component, .. } = &n.kind else { return Vec::new() };
    let set = doc.node(*component).and_then(|c| c.parent).filter(|p| doc.node(*p).is_some_and(|s| matches!(s.kind, NodeKind::ComponentSet { .. })));
    let Some(set) = set else { return Vec::new() };
    figcraft_engine::components::variant_options(doc, set).into_iter().find(|(k, _)| k == STATE_PROP).map(|(_, v)| v).unwrap_or_default()
}

fn pressable(ctx: &mut Ctx, doc: &Document, layer: &str, id: NodeId, n: &Node, action: &str) -> Option<UiNode> {
    let Some(action) = constructor(action) else {
        ctx.refuse(layer, format!("`{action}` is not an action name (letters and digits, in kebab or snake case)"));
        return None;
    };
    let states = states_of(doc, n);
    if !states.iter().any(|s| s == STATES[0]) {
        ctx.refuse(layer, "a pressable instance needs a component set with a `State` variant property and a `State=Idle` variant");
        return None;
    }
    let mut shown = Vec::new();
    for state in STATES {
        if !states.iter().any(|s| s == state) {
            shown.push(None);
            continue;
        }
        match in_state(doc, id, state) {
            Ok(d) => shown.push(d.node(id).and_then(|v| frame(ctx, &d, layer, id, v))),
            Err(e) => {
                ctx.refuse(layer, format!("showing its {state} variant failed: {e}"));
                shown.push(None);
            }
        }
    }
    let idle = shown.first().cloned().flatten()?;
    let pick = |i: usize| Box::new(shown.get(i).cloned().flatten().unwrap_or_else(|| idle.clone()));
    Some(UiNode::Pressable { action, idle: Box::new(idle.clone()), hovered: pick(1), pressed: pick(2), selected: pick(3) })
}

fn frame(ctx: &mut Ctx, doc: &Document, layer: &str, _id: NodeId, n: &Node) -> Option<UiNode> {
    common_refusals(ctx, layer, n);
    let surface = surface(ctx, layer, n);
    let arrange = arrange(ctx, layer, n);
    let flows = n.layout.is_some();
    let mut kids = Vec::new();
    for c in &n.children {
        let Some(k) = doc.node(*c) else { continue };
        if !k.visible {
            continue;
        }
        let path = format!("{layer}/{}", k.name);
        if flows && k.child_layout.absolute {
            ctx.refuse(&path, "a layer ignoring its parent's auto layout is not laid out yet");
            continue;
        }
        if let Some(u) = node(ctx, doc, &path, *c, k) {
            kids.push(u);
        }
    }
    Some(UiNode::Frame { bounds: bounds(ctx, layer, n), arrange, surface, kids })
}

fn node(ctx: &mut Ctx, doc: &Document, layer: &str, id: NodeId, n: &Node) -> Option<UiNode> {
    let inner = match &n.kind {
        NodeKind::Instance { .. } if text_prop(n, ACTION_PROP).is_some() => {
            pressable(ctx, doc, layer, id, n, &text_prop(n, ACTION_PROP).unwrap_or_default())
        }
        k if k.is_frame_like() => frame(ctx, doc, layer, id, n),
        NodeKind::Rectangle => {
            common_refusals(ctx, layer, n);
            Some(UiNode::Frame { bounds: bounds(ctx, layer, n), arrange: Arrange::Free, surface: surface(ctx, layer, n), kids: Vec::new() })
        }
        NodeKind::Text(t) => label(ctx, doc, layer, id, n, t),
        NodeKind::Group | NodeKind::BooleanOp { .. } => {
            ctx.refuse(layer, "groups are not laid out yet: use a frame");
            None
        }
        _ => {
            ctx.refuse(layer, "only frames, rectangles, texts and instances are drawn yet (vectors, ellipses, polygons, stars and lines are not)");
            None
        }
    }?;
    match text_prop(n, SHOWN_PROP) {
        None => Some(inner),
        Some(flag) => match constructor(&flag) {
            Some(flag) => Some(UiNode::Shown { flag, node: Box::new(inner) }),
            None => {
                ctx.refuse(layer, format!("`{flag}` is not a flag name (letters and digits, in kebab or snake case)"));
                None
            }
        },
    }
}

/// The design's screens as the game's interface, or every layer it refuses.
pub fn design(doc: &mut Document) -> Result<Design, Vec<Refusal>> {
    let mut ctx = Ctx { refusals: Vec::new(), fonts: Vec::new(), font_ids: BTreeMap::new() };
    let page = doc.pages.iter().copied().find(|p| doc.node(*p).is_some_and(|n| n.name == SCREENS_PAGE));
    let Some(page) = page else {
        return Err(vec![Refusal { layer: SCREENS_PAGE.into(), why: format!("the design has no page named `{SCREENS_PAGE}`") }]);
    };
    figcraft_layout::layout_page(doc, page, &TextMeasure);
    let doc: &Document = doc;
    let mut screens: Vec<(Constructor, UiNode)> = Vec::new();
    for id in doc.node(page).map(|p| p.children.clone()).unwrap_or_default() {
        let Some(n) = doc.node(id) else { continue };
        let layer = format!("{SCREENS_PAGE}/{}", n.name);
        if !n.kind.is_frame_like() {
            ctx.refuse(&layer, "a screen is a top-level frame");
            continue;
        }
        let Some(which) = constructor(&n.name) else {
            ctx.refuse(&layer, "a screen is named for one of the game's screens, in kebab case (hud-play)");
            continue;
        };
        if screens.iter().any(|(c, _)| *c == which) {
            ctx.refuse(&layer, "two screens have this name");
            continue;
        }
        if let Some(root) = frame(&mut ctx, doc, &layer, id, n) {
            screens.push((which, root));
        }
    }
    if ctx.refusals.is_empty() { Ok(Design { fonts: ctx.fonts, screens }) } else { Err(ctx.refusals) }
}
