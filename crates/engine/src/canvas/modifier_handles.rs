//! PICABIA: on-canvas handles of a selected layer's modifier stack (modelled on
//! [`crate::arc_handles`]).
//!
//! Which modifier: the selection is one unlocked, visible layer with an active stack; its handles
//! are those of the **last enabled modifier that has handles** (the one nearest the bottom of the
//! panel's list, i.e. the one that runs last). The Design panel keeps which rows are open in egui
//! memory, which the engine can't see, so "the open row" isn't used.
//!
//! - **Mirror**: a knob at the end of each axis line (the top of a vertical axis, the left of a
//!   horizontal one). Dragging it moves the axis across: `offset`. With `both`, either knob moves
//!   both lines (one `offset`).
//! - **Circular array**: the ring's **centre** (drag: `centerX`/`centerY`, which carries the whole
//!   ring with it; Shift keeps the move horizontal or vertical) and a **radius** knob on the ring
//!   opposite the original (drag along the start direction: `radius`; the centre offset stays, so
//!   the original stays put and the ring grows away from it).
//! - **Linear array**: a knob at the last copy's centre, the end of the offset line. Dragging it
//!   sets `offsetX`/`offsetY` so the last copy lands under the pointer (`count` stays); Shift keeps
//!   the line at 45° steps. Hidden when `count` is 1.
//! - **Grid array**: a knob on the middle of the lattice outline's right side (`gapX`) and one on
//!   its bottom side (`gapY`). Dragging moves that side of the whole lattice; Shift makes both gaps
//!   the same. Hidden for a single column / row.
//!
//! Every handle snaps (the handle point, to other layers' and the layer's own edges and centres,
//! with the usual snap lines) unless snapping is off or Cmd/Ctrl is held. Values are in the layer's
//! parent space, like the modifier params; positions here are in page coordinates. The Move tool's
//! gesture writes the result through `modifier.set`, one undo step per drag ("Set modifier"), and
//! Esc cancels the drag back to where it started.

use figcraft_doc::{Document, NodeId};
use figcraft_geom::{Affine, Point, Rect, Vec2};
use figcraft_modifiers::{CircularArray, GridArray, LinearArray, MAX_COUNT, MAX_LENGTH, Mirror, MirrorAxis, ModifierKind};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::canvas::tools::{Gesture, PointerEvent, ToolId};
use crate::snap::{Guide, Targets};

/// One modifier handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModHandle {
    /// The knob on a mirror's vertical axis line (moves it left / right).
    MirrorVertical,
    /// The knob on a mirror's horizontal axis line (moves it up / down).
    MirrorHorizontal,
    /// A circular array's ring centre.
    RingCenter,
    /// A circular array's radius knob, on the ring opposite the original.
    RingRadius,
    /// The end of a linear array's offset line (the last copy's centre).
    LinearEnd,
    /// The middle of a grid lattice's right side.
    GridGapX,
    /// The middle of a grid lattice's bottom side.
    GridGapY,
}

/// The handles shown for the selection (page coordinates).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModHandles {
    pub id: NodeId,
    /// The modifier they edit: its id and its row index in the stack.
    pub modifier: String,
    pub index: usize,
    pub handles: Vec<(ModHandle, [f64; 2])>,
}

impl ModHandles {
    /// Where handle `h` is, if shown.
    pub fn at(&self, h: ModHandle) -> Option<[f64; 2]> {
        self.handles.iter().find(|(k, _)| *k == h).map(|(_, p)| *p)
    }
}

/// The live readout of a modifier handle drag: the handle, its label and where the handle is now.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModHint {
    pub handle: ModHandle,
    pub label: String,
    pub at: [f64; 2],
}

/// A guide of the selected layer's stack on the page (dashed on the canvas).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PageGuide {
    Line {
        from: [f64; 2],
        to: [f64; 2],
    },
    /// A ring through the parent transform: `center + a·cos θ + b·sin θ`.
    Ellipse {
        center: [f64; 2],
        a: [f64; 2],
        b: [f64; 2],
    },
    Point([f64; 2]),
}

/// What a modifier handle drag carries (see the Move tool's gesture).
#[derive(Clone, Debug)]
pub struct ModDrag {
    pub id: NodeId,
    pub modifier: String,
    pub handle: ModHandle,
    /// The handle minus the press point (page): the handle stays where it was grabbed.
    pub grab: Vec2,
    /// Where the handle was when the drag started (page; Shift's axis lock measures from it).
    pub start: Point,
    /// What it snaps to (`None`: snapping off).
    pub targets: Option<Targets>,
}

/// How near the pointer must be to pick a handle (screen pixels), as for arc handles.
const HIT_PX: f64 = 7.0;
/// Hit order when two handles coincide (the first wins).
const ORDER: [ModHandle; 7] = [
    ModHandle::RingRadius,
    ModHandle::RingCenter,
    ModHandle::LinearEnd,
    ModHandle::GridGapX,
    ModHandle::GridGapY,
    ModHandle::MirrorVertical,
    ModHandle::MirrorHorizontal,
];

/// `v` if finite and within the evaluator's length cap, else clamped (non-finite: 0).
fn len(v: f64) -> f64 {
    if v.is_finite() { v.clamp(-MAX_LENGTH, MAX_LENGTH) } else { 0.0 }
}

fn count(c: u32) -> u32 {
    c.clamp(1, MAX_COUNT)
}

fn pt(p: Point) -> [f64; 2] {
    [p.x, p.y]
}

fn sane_zoom(zoom: f64) -> f64 {
    if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 }
}

/// A layer as its modifier handles see it: its parent → page transform and its box in the
/// parent's space (whose centre is the pivot).
struct Frame {
    parent: Affine,
    bounds: Rect,
}

impl Frame {
    fn of(doc: &Document, id: NodeId) -> Option<Self> {
        let n = doc.node(id)?;
        let parent = n.parent.map(|p| doc.world_transform(p)).unwrap_or(Affine::IDENTITY);
        let ok = parent.is_finite() && parent.determinant().is_finite() && parent.determinant().abs() > 1e-12;
        let bounds = figcraft_modifiers::node_bounds(n);
        let finite = [bounds.x0, bounds.y0, bounds.x1, bounds.y1].iter().all(|v| v.is_finite());
        (ok && finite).then_some(Self { parent, bounds })
    }

    fn pivot(&self) -> Point {
        self.bounds.center()
    }

    fn page(&self, q: Point) -> Point {
        self.parent * q
    }

    fn local(&self, p: Point) -> Point {
        self.parent.inverse() * p
    }
}

/// The unit vector at `deg` degrees (clockwise on screen from the right).
fn dir(deg: f64) -> Vec2 {
    let r = if deg.is_finite() { deg.clamp(-1.0e6, 1.0e6) } else { 0.0 }.to_radians();
    Vec2::new(r.cos(), r.sin())
}

/// How far a mirror's axis line reaches each way from the pivot (as its guide draws it).
fn reach(b: Rect) -> f64 {
    (b.width().max(b.height()) * 1.5).max(100.0)
}

fn ring_centre(c: &CircularArray, f: &Frame) -> Point {
    f.pivot() - dir(c.start_angle) * len(c.radius).max(0.0) + Vec2::new(len(c.center_x), len(c.center_y))
}

fn grid_far(g: &GridArray, b: Rect) -> (f64, f64) {
    let (cols, rows) = (f64::from(count(g.cols)), f64::from(count(g.rows)));
    (b.x1 + (b.width() + len(g.gap_x)) * (cols - 1.0), b.y1 + (b.height() + len(g.gap_y)) * (rows - 1.0))
}

/// Modifier `kind`'s handles in the parent's space (empty: it has none).
fn kind_handles(kind: &ModifierKind, f: &Frame) -> Vec<(ModHandle, Point)> {
    let (b, pv) = (f.bounds, f.pivot());
    match kind {
        ModifierKind::Mirror(Mirror { axis, offset, .. }) => {
            let o = len(*offset);
            let v = (ModHandle::MirrorVertical, Point::new(pv.x + o, pv.y - reach(b)));
            let h = (ModHandle::MirrorHorizontal, Point::new(pv.x - reach(b), pv.y + o));
            match axis {
                MirrorAxis::Vertical => vec![v],
                MirrorAxis::Horizontal => vec![h],
                MirrorAxis::Both => vec![v, h],
            }
        }
        ModifierKind::CircularArray(c) => {
            let centre = ring_centre(c, f);
            vec![(ModHandle::RingCenter, centre), (ModHandle::RingRadius, centre - dir(c.start_angle) * len(c.radius).max(0.0))]
        }
        ModifierKind::LinearArray(LinearArray { count: n, offset_x, offset_y, .. }) => {
            let n = count(*n);
            if n < 2 {
                return vec![];
            }
            vec![(ModHandle::LinearEnd, pv + Vec2::new(len(*offset_x), len(*offset_y)) * f64::from(n - 1))]
        }
        ModifierKind::GridArray(g) => {
            let (x1, y1) = grid_far(g, b);
            let mut v = Vec::new();
            if count(g.cols) > 1 {
                v.push((ModHandle::GridGapX, Point::new(x1, (b.y0 + y1) / 2.0)));
            }
            if count(g.rows) > 1 {
                v.push((ModHandle::GridGapY, Point::new((b.x0 + x1) / 2.0, y1)));
            }
            v
        }
        _ => vec![],
    }
}

/// The layer whose modifier handles show: the only selected layer, unlocked and visible, with an
/// active stack; and the modifier (index, id, kind) that gets handles.
fn target(doc: &Document, sel: &[NodeId]) -> Option<(NodeId, Frame, usize, String, ModifierKind)> {
    let [id] = sel else { return None };
    let n = doc.node(*id)?;
    if n.locked || !n.visible || n.is_page() {
        return None;
    }
    let stack = figcraft_modifiers::node_stack(n)?.ok()?;
    let f = Frame::of(doc, *id)?;
    let (i, m) = stack.modifiers.iter().enumerate().rev().find(|(_, m)| m.enabled && !kind_handles(&m.kind, &f).is_empty())?;
    Some((*id, f, i, m.id.clone(), m.kind.clone()))
}

/// The modifier handles of the selection, if it is one layer whose stack has a modifier with
/// handles.
pub fn handles(doc: &Document, sel: &[NodeId]) -> Option<ModHandles> {
    let (id, f, index, modifier, kind) = target(doc, sel)?;
    let handles = kind_handles(&kind, &f).into_iter().map(|(h, q)| (h, pt(f.page(q)))).filter(|(_, p)| p.iter().all(|v| v.is_finite())).collect();
    Some(ModHandles { id, modifier, index, handles })
}

/// The modifier handle under page point `p` at `zoom` (the nearest; see [`ORDER`] for ties), with
/// its layer and modifier id.
pub fn hit(doc: &Document, sel: &[NodeId], p: Point, zoom: f64) -> Option<(NodeId, String, ModHandle)> {
    let hs = handles(doc, sel)?;
    let tol = HIT_PX / sane_zoom(zoom);
    let mut best: Option<(ModHandle, f64)> = None;
    for h in ORDER {
        let Some(q) = hs.at(h) else { continue };
        let d = (Point::new(q[0], q[1]) - p).hypot();
        if d <= tol && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((h, d));
        }
    }
    best.map(|(h, _)| (hs.id, hs.modifier.clone(), h))
}

/// The selected layer's stack guides on the page (every enabled stage's, as the evaluator gives
/// them), for one selected layer with an active stack.
pub fn guides(doc: &Document, sel: &[NodeId]) -> Vec<PageGuide> {
    let [id] = sel else { return vec![] };
    let (Some(n), Some(f)) = (doc.node(*id), Frame::of(doc, *id)) else { return vec![] };
    let Some(out) = figcraft_modifiers::evaluate_node_in_doc(doc, n) else { return vec![] };
    let lin = |v: Vec2| {
        let o = f.page(Point::ZERO);
        f.page(v.to_point()) - o
    };
    let g = out.guides.iter().map(|g| match g {
        figcraft_modifiers::Guide::Line { from, to } => PageGuide::Line { from: pt(f.page(*from)), to: pt(f.page(*to)) },
        figcraft_modifiers::Guide::Circle { center, radius } => {
            let (a, b) = (lin(Vec2::new(*radius, 0.0)), lin(Vec2::new(0.0, *radius)));
            PageGuide::Ellipse { center: pt(f.page(*center)), a: [a.x, a.y], b: [b.x, b.y] }
        }
        figcraft_modifiers::Guide::Point(p) => PageGuide::Point(pt(f.page(*p))),
    });
    g.filter(|g| match g {
        PageGuide::Line { from, to } => from.iter().chain(to).all(|v| v.is_finite()),
        PageGuide::Ellipse { center, a, b } => center.iter().chain(a).chain(b).all(|v| v.is_finite()),
        PageGuide::Point(p) => p.iter().all(|v| v.is_finite()),
    })
    .collect()
}

/// The current params of modifier `modifier` on layer `id`.
fn kind_of(doc: &Document, id: NodeId, modifier: &str) -> Option<ModifierKind> {
    let stack = figcraft_modifiers::node_stack(doc.node(id)?)?.ok()?;
    stack.modifiers.iter().find(|m| m.id == modifier).map(|m| m.kind.clone())
}

/// Where handle `h` of modifier `modifier` on layer `id` is now (page; the readout's anchor).
pub fn handle_at(doc: &Document, id: NodeId, modifier: &str, h: ModHandle) -> Option<[f64; 2]> {
    let f = Frame::of(doc, id)?;
    let kind = kind_of(doc, id, modifier)?;
    kind_handles(&kind, &f).into_iter().find(|(k, _)| *k == h).map(|(_, q)| pt(f.page(q)))
}

/// `v` rounded to one decimal, without a trailing `.0` (or a minus on zero).
fn one_decimal(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    let r = if r == 0.0 { 0.0 } else { r };
    if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r:.1}") }
}

/// The live readout for handle `h` of modifier `modifier` on layer `id`: `Axis 12`,
/// `Center 10, -5`, `Radius 120`, `Offset 120, 0`, `Gap X 20`.
pub fn label(doc: &Document, id: NodeId, modifier: &str, h: ModHandle) -> Option<String> {
    let kind = kind_of(doc, id, modifier)?;
    let s = match (&kind, h) {
        (ModifierKind::Mirror(m), _) => format!("Axis {}", one_decimal(len(m.offset))),
        (ModifierKind::CircularArray(c), ModHandle::RingCenter) => {
            format!("Center {}, {}", one_decimal(len(c.center_x)), one_decimal(len(c.center_y)))
        }
        (ModifierKind::CircularArray(c), _) => format!("Radius {}", one_decimal(len(c.radius).max(0.0))),
        (ModifierKind::LinearArray(l), _) => format!("Offset {}, {}", one_decimal(len(l.offset_x)), one_decimal(len(l.offset_y))),
        (ModifierKind::GridArray(g), ModHandle::GridGapY) => format!("Gap Y {}", one_decimal(len(g.gap_y))),
        (ModifierKind::GridArray(g), _) => format!("Gap X {}", one_decimal(len(g.gap_x))),
        _ => return None,
    };
    Some(s)
}

/// Start a drag of handle `h` (modifier `modifier` on layer `id`) pressed at page point `p`, with
/// `targets` to snap to (`None`: no snapping). The layer's own box is a snap target too.
pub fn begin(doc: &Document, id: NodeId, modifier: String, h: ModHandle, p: Point, targets: Option<Targets>) -> Option<ModDrag> {
    let at = handle_at(doc, id, &modifier, h)?;
    let at = Point::new(at[0], at[1]);
    let targets = targets.map(|mut t| {
        if let Some(b) = doc.world_bounds(id) {
            t.add_box(b);
        }
        t
    });
    Some(ModDrag { id, modifier, handle: h, grab: at - p, start: at, targets })
}

/// The 45° step of `v` (same length).
fn at_45(v: Vec2) -> Vec2 {
    let step = std::f64::consts::FRAC_PI_4;
    Vec2::from_angle((v.atan2() / step).round() * step) * v.hypot()
}

/// `modifier.set` props after dragging `d` to page point `p` (with `shift`) at `zoom`, and the snap
/// lines to show.
pub fn drag(doc: &Document, d: &ModDrag, p: Point, shift: bool, zoom: f64) -> Option<(Map<String, Value>, Vec<Guide>)> {
    let f = Frame::of(doc, d.id)?;
    let kind = kind_of(doc, d.id, &d.modifier)?;
    let mut at = p + d.grab;
    if !(at.x.is_finite() && at.y.is_finite()) {
        return None;
    }
    // Shift locks the ring centre to the larger axis of the move.
    if shift && d.handle == ModHandle::RingCenter {
        let m = at - d.start;
        at = if m.x.abs() >= m.y.abs() { Point::new(at.x, d.start.y) } else { Point::new(d.start.x, at.y) };
    }
    if shift && d.handle == ModHandle::LinearEnd {
        let pv = f.page(f.pivot());
        at = pv + at_45(at - pv);
    }
    let mut guides = Vec::new();
    // Snapping (not while Shift fixes a direction the snap would bend).
    let free = !(shift && matches!(d.handle, ModHandle::LinearEnd | ModHandle::RingCenter));
    if let Some(t) = d.targets.as_ref().filter(|_| free) {
        let (dx, dy, g) = t.snap_point(at.x, at.y, crate::canvas::tools::SNAP_PX / sane_zoom(zoom));
        let (use_x, use_y) = match d.handle {
            ModHandle::MirrorVertical | ModHandle::GridGapX => (true, false),
            ModHandle::MirrorHorizontal | ModHandle::GridGapY => (false, true),
            _ => (true, true),
        };
        if use_x {
            at.x += dx;
        }
        if use_y {
            at.y += dy;
        }
        // Keep the snap lines of the axes that snapped (a vertical line is an x snap).
        guides = g.into_iter().filter(|g| if (g.x0 - g.x1).abs() < 1e-9 { use_x && dx != 0.0 } else { use_y && dy != 0.0 }).collect();
    }
    let q = f.local(at);
    let (pv, b) = (f.pivot(), f.bounds);
    let num = |v: f64| json!(if v == 0.0 { 0.0 } else { v });
    let props: Map<String, Value> = match (&kind, d.handle) {
        (ModifierKind::Mirror(_), ModHandle::MirrorVertical) => Map::from_iter([("offset".into(), num(len(q.x - pv.x)))]),
        (ModifierKind::Mirror(_), ModHandle::MirrorHorizontal) => Map::from_iter([("offset".into(), num(len(q.y - pv.y)))]),
        (ModifierKind::CircularArray(c), ModHandle::RingCenter) => {
            let o = q - (pv - dir(c.start_angle) * len(c.radius).max(0.0));
            Map::from_iter([("centerX".into(), num(len(o.x))), ("centerY".into(), num(len(o.y)))])
        }
        (ModifierKind::CircularArray(c), ModHandle::RingRadius) => {
            // The knob sits at pivot + offset − 2r·dir: r from the pointer's place along dir.
            let base = pv + Vec2::new(len(c.center_x), len(c.center_y));
            let r = ((base - q).dot(dir(c.start_angle)) / 2.0).max(0.0);
            Map::from_iter([("radius".into(), num(len(r)))])
        }
        (ModifierKind::LinearArray(l), ModHandle::LinearEnd) => {
            let steps = f64::from(count(l.count).saturating_sub(1).max(1));
            let o = (q - pv) / steps;
            Map::from_iter([("offsetX".into(), num(len(o.x))), ("offsetY".into(), num(len(o.y)))])
        }
        (ModifierKind::GridArray(g), ModHandle::GridGapX) => {
            let gap = len((q.x - b.x1) / f64::from(count(g.cols).saturating_sub(1).max(1)) - b.width());
            let mut m = Map::from_iter([("gapX".into(), num(gap))]);
            if shift {
                m.insert("gapY".into(), num(gap));
            }
            m
        }
        (ModifierKind::GridArray(g), ModHandle::GridGapY) => {
            let gap = len((q.y - b.y1) / f64::from(count(g.rows).saturating_sub(1).max(1)) - b.height());
            let mut m = Map::from_iter([("gapY".into(), num(gap))]);
            if shift {
                m.insert("gapX".into(), num(gap));
            }
            m
        }
        _ => return None,
    };
    props.values().all(|v| v.as_f64().is_some_and(f64::is_finite)).then_some((props, guides))
}

impl crate::Session {
    /// Whether the Move tool shows modifier handles now (as arc handles: not while another gesture
    /// runs, nor in Dev Mode or vector or text edit mode).
    pub(crate) fn modifier_handles_shown(&self) -> bool {
        let dragging = matches!(self.tool.gesture, Gesture::Modifier(_));
        self.tool.tool == ToolId::Move
            && !self.tool.dev_mode
            && (dragging || !self.tool.busy())
            && self.ds().is_ok_and(|ds| ds.vector_edit.is_none() && ds.text_edit.is_none())
    }

    /// The selected layer's modifier handles as the canvas shows them now.
    pub fn modifier_handles(&self) -> Option<ModHandles> {
        let ds = self.ds().ok()?;
        self.modifier_handles_shown().then(|| handles(&ds.doc, &ds.selection)).flatten()
    }

    /// The selected layer's modifier guides as the canvas shows them now (with its handles).
    pub fn modifier_guides(&self) -> Vec<PageGuide> {
        match self.ds() {
            Ok(ds) if self.modifier_handles_shown() => guides(&ds.doc, &ds.selection),
            _ => Vec::new(),
        }
    }

    /// Hovering: note the modifier handle under the pointer; whether there is one.
    pub(crate) fn modifier_hover(&mut self, doc: &Document, sel: &[NodeId], ev: &PointerEvent) -> bool {
        let h = if self.modifier_handles_shown() { hit(doc, sel, ev.p(), ev.zoom).map(|(_, _, h)| h) } else { None };
        self.tool.overlay.modifier_hover = h;
        h.is_some()
    }

    /// A press on a modifier handle starts its drag (one undo step); whether it did.
    pub(crate) fn modifier_down(&mut self, doc: &Document, sel: &[NodeId], ev: &PointerEvent) -> crate::Result<bool> {
        if !self.modifier_handles_shown() {
            return Ok(false);
        }
        let Some((id, modifier, h)) = hit(doc, sel, ev.p(), ev.zoom) else { return Ok(false) };
        let snap = self.ds().map(|d| d.view.snap_objects).unwrap_or(true) && !ev.cmd && !ev.ctrl;
        let targets = snap.then(|| Targets::gather(doc, self.page().unwrap_or(NodeId(0)), &[id]));
        let Some(d) = begin(doc, id, modifier, h, ev.p(), targets) else { return Ok(false) };
        self.begin_interaction("Set modifier")?;
        self.tool.overlay.modifier = self.modifier_hint(&d);
        self.tool.overlay.modifier_hover = Some(h);
        self.tool.gesture = Gesture::Modifier(Box::new(d));
        Ok(true)
    }

    fn modifier_hint(&self, d: &ModDrag) -> Option<ModHint> {
        let doc = self.doc().ok()?;
        let at = handle_at(doc, d.id, &d.modifier, d.handle)?;
        Some(ModHint { handle: d.handle, label: label(doc, d.id, &d.modifier, d.handle)?, at })
    }

    /// A modifier handle drag: the new params, written through `modifier.set` (inside the
    /// gesture's interaction, so the whole drag is one undo step).
    pub(crate) fn modifier_drag(&mut self, ev: &PointerEvent, d: &ModDrag) -> crate::Result<()> {
        let doc = self.doc()?.clone();
        let Some((props, guides)) = drag(&doc, d, ev.p(), ev.shift, ev.zoom) else { return Ok(()) };
        self.execute("modifier.set", &json!({"ids": [d.id], "modifier": d.modifier, "props": props}))?;
        self.tool.overlay.guides = guides;
        self.tool.overlay.modifier = self.modifier_hint(d);
        self.tool.overlay.modifier_hover = Some(d.handle);
        Ok(())
    }
}

#[cfg(test)]
#[path = "modifier_handles_tests.rs"]
mod tests;
