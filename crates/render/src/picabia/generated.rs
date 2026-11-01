//! Picabia: generated layers, whose modifier stack has a generator row (`imageTrace`,
//! `pathfinder`).
//!
//! A generated layer draws the generator's **result** instead of its design: geometry the engine
//! computes from the layer (layer 5; this crate is layer 3 and can't call the tracer or
//! Pathfinder) and keeps in the store here ([`store`]) with a snapshot of what it was computed
//! from ([`super::snapshot`]). The snapshot is the layer's subtree with its stack **cut after the
//! generator row** (the rows below only copy the result: changing them doesn't recompute), the
//! input copies (the rows above) and every copy inside it. Render, exports, bounds and hit
//! testing read the result back ([`lookup`]: fresh or stale); a layer with no result yet draws its
//! design as usual.
//!
//! The result is in the layer's own space and already holds the input copies; it is drawn once
//! per copy of the result (the layer's instance transforms, from the rows below the generator).
//! A stencil row after the generator wins: the layer draws the sheet cut from the result.

use std::any::Any;
use std::sync::Arc;

use figcraft_doc::{Document, Node, NodeId, StrokeCap, StrokeJoin};
use figcraft_geom::kurbo::{BezPath, ParamCurveNearest, Shape as _};
use figcraft_geom::{Affine, Point, Rect};
use figcraft_modifiers::{GeneratorKind, ModifierKind, PLUGIN_KEY};
use tiny_skia::{FillRule, Pixmap};

use super::snapshot::{Inputs, Store};
use super::{each_copy, has_stack, instance_transforms};

/// Where a generated layer's result can lie, worked out from its stack and box (no geometry).
#[derive(Clone, Debug, PartialEq)]
pub struct GenFrame {
    /// The generator row's id.
    pub modifier: String,
    /// Its kind and params, clamped.
    pub kind: GeneratorKind,
    /// The copies the rows above the generator make (its input), as maps of the layer's own space
    /// (`N⁻¹ · T · N`, `N` its transform); the identity alone when there are none.
    pub inputs: Vec<Affine>,
    /// What the layer paints (its paint bounds, children's copies included) through every input
    /// copy, in its own space: where the result lies a priori.
    pub content: Rect,
}

/// The input copies of `n`'s generator in its own space, with the row's id and kind; `None` when
/// `n` has no (counted) generator row.
fn row(doc: &Document, n: &Node) -> Option<(String, GeneratorKind, Vec<Affine>)> {
    if !has_stack(n) {
        return None;
    }
    let out = figcraft_modifiers::node_instances_in_doc(doc, n)?;
    let g = out.generator.as_ref()?;
    let nt = n.transform;
    let mut inputs = Vec::new();
    if nt.is_finite() && nt.determinant().abs() > 1e-12 {
        let inv = nt.inverse();
        inputs.extend(g.inputs.iter().map(|i| inv * i.transform * nt).filter(|c| c.is_finite()));
    }
    if inputs.is_empty() {
        inputs.push(Affine::IDENTITY);
    }
    Some((g.modifier.clone(), g.kind.clone(), inputs))
}

/// `local` (in the layer's own space) through every copy in `inputs`.
fn through(inputs: &[Affine], local: Rect) -> Option<Rect> {
    let r = inputs.iter().map(|c| figcraft_geom::transform_rect_bbox(*c, local)).reduce(|a, b| a.union(b))?;
    r.is_finite().then_some(r)
}

/// `n`'s generator frame; `None` when its stack has no counted generator row (none enabled, or a
/// stencil above it).
pub fn frame(doc: &Document, n: &Node) -> Option<GenFrame> {
    let (modifier, kind, inputs) = row(doc, n)?;
    let local = crate::draw::local_paint_bounds(doc, n, doc.world_transform(n.id), 0, None);
    let content = through(&inputs, local)?;
    Some(GenFrame { modifier, kind, inputs, content })
}

/// Whether `n`'s stack has a counted generator row: an enabled generator with no enabled stencil
/// above it (a cheap check: the stack is cached).
pub fn is_generated(n: &Node) -> bool {
    if !has_stack(n) {
        return false;
    }
    let Some(Ok(stack)) = figcraft_modifiers::node_stack(n) else { return false };
    stack.modifiers.iter().filter(|m| m.enabled).find_map(|m| match &m.kind {
        ModifierKind::Stencil(_) => Some(false),
        k if k.is_generator() => Some(true),
        _ => None,
    }) == Some(true)
}

/// How a [`GenItem`] is painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GenPaint {
    /// Filled: even-odd, or non-zero.
    Fill { even_odd: bool },
    /// Stroked along the path (a centre line), `width` in the layer's units.
    Stroke { width: f64, cap: StrokeCap, join: StrokeJoin },
}

/// One painted path of a generator's result.
#[derive(Clone, Debug, PartialEq)]
pub struct GenItem {
    /// In the layer's own space.
    pub path: BezPath,
    pub paint: GenPaint,
    /// Straight (not premultiplied) RGBA, 0–1, as FigCraft colours are.
    pub rgba: [f32; 4],
    /// The layer it came from, when it came from one (Apply names and hit testing use it).
    pub source: Option<NodeId>,
    /// The name Apply gives its layer.
    pub name: String,
}

impl GenItem {
    /// The item's painted area as a fill path, its fill rule (`true`: even-odd): a fill as it is,
    /// a stroke outlined at its width.
    pub fn area(&self) -> (std::borrow::Cow<'_, BezPath>, bool) {
        match self.paint {
            GenPaint::Fill { even_odd } => (std::borrow::Cow::Borrowed(&self.path), even_odd),
            GenPaint::Stroke { width, cap, join } => {
                let props = figcraft_doc::StrokeProps { weight: width, cap_start: cap, cap_end: cap, join, ..figcraft_doc::StrokeProps::default() };
                (std::borrow::Cow::Owned(figcraft_pathops::stroke_center(&self.path, width, &props)), false)
            }
        }
    }

    /// How far the paint reaches past the path (half a stroke's width; 0 for a fill).
    pub fn reach(&self) -> f64 {
        match self.paint {
            GenPaint::Fill { .. } => 0.0,
            // Square caps and miter joins reach further than half the width.
            GenPaint::Stroke { width, .. } if width.is_finite() => width.max(0.0),
            GenPaint::Stroke { .. } => 0.0,
        }
    }

    /// The painted bounds, in the layer's space.
    pub fn bounds(&self) -> Rect {
        let r = self.reach();
        self.path.bounding_box().inflate(r, r)
    }
}

/// A generator's computed result, in the layer's own space.
pub struct GenShape {
    /// Bottom first.
    pub items: Vec<GenItem>,
    /// The items' painted bounds together (`Rect::ZERO` without items).
    pub bounds: Rect,
    /// The generator couldn't make a result to draw (the layer isn't one it works on, or the
    /// result was too complex): the layer draws its design as usual, and no items are kept.
    pub keep_design: bool,
    /// What the engine keeps with it (its stats and warnings).
    pub extra: Option<Arc<dyn Any + Send + Sync>>,
}

impl GenShape {
    /// The result `items` (bottom first), with what the engine keeps with them.
    pub fn new(items: Vec<GenItem>, extra: Option<Arc<dyn Any + Send + Sync>>) -> GenShape {
        let bounds = items.iter().map(GenItem::bounds).filter(Rect::is_finite).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
        GenShape { items, bounds, keep_design: false, extra }
    }

    /// No result: the layer draws its design as usual ([`GenShape::keep_design`]).
    pub fn design(extra: Option<Arc<dyn Any + Send + Sync>>) -> GenShape {
        GenShape { items: Vec::new(), bounds: Rect::ZERO, keep_design: true, extra }
    }
}

/// `n` with what doesn't change its generator's result reset: its place, name and opacity, and
/// every row below the generator (they only copy the result).
fn neutral_root(n: &Node) -> Node {
    let mut r = n.clone();
    r.transform = Affine::IDENTITY;
    r.name.clear();
    r.opacity = 1.0;
    if let Some(Ok(stack)) = figcraft_modifiers::node_stack(n) {
        let mut st = (*stack).clone();
        if let Some((at, _)) = st.generator() {
            st.modifiers.truncate(at + 1);
        }
        r.plugin_data.insert(PLUGIN_KEY.to_string(), figcraft_modifiers::to_json(&st));
    }
    r
}

/// The root's input copies in its own space (as [`frame`] has them).
fn root_copies(doc: &Document, n: &Node) -> (Vec<Affine>, Option<figcraft_modifiers::Repeat>) {
    (row(doc, n).map(|(_, _, inputs)| inputs).unwrap_or_default(), None)
}

static STORE: Store<GenShape> = Store::new(neutral_root, root_copies);

/// What layer `id`'s generator result would be computed from now.
pub fn inputs(doc: &Document, id: NodeId) -> Option<Inputs> {
    STORE.inputs(doc, id)
}

/// Bumped by every [`store`] (and [`clear`]).
pub fn generation() -> u64 {
    STORE.generation()
}

/// Keep `shape`, layer `id`'s result computed from `inputs`.
pub fn store(id: NodeId, inputs: Inputs, shape: GenShape) {
    STORE.store(id, inputs, shape);
}

/// Forget every result (tests).
pub fn clear() {
    STORE.clear();
}

/// Layer `n`'s result: computed from what `doc` holds now (`true`: fresh) or its latest one
/// (`false`: stale, the engine is recomputing). `None` when none was ever computed, or `n` isn't a
/// generated layer (any more).
pub fn lookup(doc: &Document, n: &Node) -> Option<(Arc<GenShape>, bool)> {
    if !is_generated(n) {
        return None;
    }
    STORE.lookup(doc, n)
}

/// Whether layer `n` has a result computed from what `doc` holds now.
pub fn is_fresh(doc: &Document, n: &Node) -> bool {
    lookup(doc, n).is_some_and(|(_, fresh)| fresh)
}

/// Where generated layer `n` can paint in its own space: the design `local` (its paint bounds
/// without its own copies) through the input copies, and the result it draws (fresh or stale).
/// `None`: not a generated layer.
fn local_reach(n: &Node, doc: &Document, local: Rect) -> Option<Rect> {
    if !is_generated(n) {
        return None;
    }
    let (_, _, inputs) = row(doc, n)?;
    let mut r = through(&inputs, local).map_or(local, |c| c.union(local));
    if let Some((s, _)) = STORE.lookup(doc, n)
        && !s.items.is_empty()
        && s.bounds.is_finite()
    {
        r = r.union(s.bounds);
    }
    Some(r)
}

/// [`local_reach`] for `r`, the layer's own paint bounds: what [`super::parent_bbox`] measures in
/// place of `r`. `r` unchanged for a layer that isn't generated.
pub(crate) fn reach_of(doc: &Document, n: &Node, r: Rect) -> Rect {
    local_reach(n, doc, r).unwrap_or(r)
}

/// Page-space bounds of everything generated layer `id` can paint, every copy of the result
/// included. `None`: not a generated layer.
pub fn world_content(doc: &Document, id: NodeId) -> Option<Rect> {
    let n = doc.node(id)?;
    if !is_generated(n) {
        return None;
    }
    let local = local_reach(n, doc, crate::draw::local_paint_bounds(doc, n, doc.world_transform(n.id), 0, None))?;
    let p = n.parent.map(|p| doc.world_transform(p)).unwrap_or(Affine::IDENTITY);
    let copies = instance_transforms(doc, n).unwrap_or_else(|| vec![Affine::IDENTITY]);
    copies.iter().map(|t| figcraft_geom::transform_rect_bbox(p * *t * n.transform, local)).filter(Rect::is_finite).reduce(|a, b| a.union(b))
}

/// The page-space maps from generated layer `n`'s own space to each copy of its result.
fn page_maps(doc: &Document, n: &Node) -> Vec<Affine> {
    let p = n.parent.map(|p| doc.world_transform(p)).unwrap_or(Affine::IDENTITY);
    let copies = instance_transforms(doc, n).unwrap_or_else(|| vec![Affine::IDENTITY]);
    copies.into_iter().map(|t| p * t * n.transform).filter(|m| m.is_finite() && m.determinant().abs() > 1e-12).collect()
}

/// Whether page point `p` lies on generated layer `id`'s drawn result (a fill holds it, or a stroke
/// passes within half its width plus `tol` page units), at any copy of the result. `None`: `id`
/// draws its design (not a generated layer, or no result yet).
pub fn result_contains(doc: &Document, id: NodeId, p: Point, tol: f64) -> Option<bool> {
    let n = doc.node(id)?;
    let (shape, _) = lookup(doc, n).filter(|(s, _)| !s.keep_design)?;
    for m in page_maps(doc, n) {
        let q = m.inverse() * p;
        let scale = m.determinant().abs().sqrt();
        let t = if scale > 1e-12 && tol.is_finite() { tol.max(0.0) / scale } else { 0.0 };
        if shape.items.iter().any(|it| item_holds(it, q, t)) {
            return Some(true);
        }
    }
    Some(false)
}

fn item_holds(it: &GenItem, q: Point, tol: f64) -> bool {
    if !it.bounds().inflate(tol, tol).contains(q) {
        return false;
    }
    match it.paint {
        GenPaint::Fill { even_odd } => {
            let w = it.path.winding(q);
            let inside = if even_odd { w % 2 != 0 } else { w != 0 };
            inside || (tol > 0.0 && near(&it.path, q, tol))
        }
        GenPaint::Stroke { width, .. } => near(&it.path, q, width.max(0.0) / 2.0 + tol),
    }
}

/// Whether `q` lies within `d` of `path`.
fn near(path: &BezPath, q: Point, d: f64) -> bool {
    let d2 = d * d;
    path.segments().any(|s| s.nearest(q, 1e-3).distance_sq <= d2)
}

/// The most item boxes [`world_item_boxes`] lists per copy (past it: one box for the result).
const MAX_ITEM_BOXES: usize = 4096;

/// Page-space boxes of generated layer `id`'s drawn result items at every copy (marquee); empty
/// when it draws its design.
pub fn world_item_boxes(doc: &Document, id: NodeId) -> Vec<Rect> {
    let Some(n) = doc.node(id) else { return Vec::new() };
    let Some((shape, _)) = lookup(doc, n).filter(|(s, _)| !s.keep_design) else { return Vec::new() };
    let mut out = Vec::new();
    for m in page_maps(doc, n) {
        if shape.items.len() > MAX_ITEM_BOXES {
            out.push(figcraft_geom::transform_rect_bbox(m, shape.bounds));
        } else {
            out.extend(shape.items.iter().map(|it| figcraft_geom::transform_rect_bbox(m, it.bounds())));
        }
    }
    out.retain(Rect::is_finite);
    out
}

/// Draw generated layer `n` (its parent's space mapped to `pix` by `parent_xf`) as its result,
/// once per copy of the result; `false` when it isn't one with a result (the caller draws it as
/// usual). Outline mode draws the items' outlines.
pub(crate) fn draw(ctx: &crate::draw::Ctx, pix: &mut Pixmap, n: &Node, parent_xf: Affine) -> bool {
    if !has_stack(n) {
        return false;
    }
    let Some((shape, _)) = lookup(ctx.doc, n).filter(|(s, _)| !s.keep_design) else { return false };
    if !n.visible || n.is_mask || ctx.skip.contains(&n.id) {
        return true;
    }
    let outline = ctx.outline_mode || ctx.outlined.contains(&n.id);
    if n.opacity <= 0.0 && !outline {
        return true;
    }
    let alpha = n.opacity.clamp(0.0, 1.0);
    let mut once = |t: Affine| {
        let xf = parent_xf * t * n.transform;
        if xf.is_finite() {
            draw_items(ctx, pix, &shape, xf, alpha, outline);
        }
    };
    if !each_copy(ctx.doc, n, &mut once) {
        once(Affine::IDENTITY);
    }
    true
}

fn ts_cap(c: StrokeCap) -> tiny_skia::LineCap {
    match c {
        StrokeCap::Round => tiny_skia::LineCap::Round,
        StrokeCap::Square => tiny_skia::LineCap::Square,
        _ => tiny_skia::LineCap::Butt,
    }
}

fn ts_join(j: StrokeJoin) -> tiny_skia::LineJoin {
    match j {
        StrokeJoin::Miter => tiny_skia::LineJoin::Miter,
        StrokeJoin::Bevel => tiny_skia::LineJoin::Bevel,
        StrokeJoin::Round => tiny_skia::LineJoin::Round,
    }
}

fn draw_items(ctx: &crate::draw::Ctx, pix: &mut Pixmap, shape: &GenShape, xf: Affine, alpha: f32, outline: bool) {
    let ts = crate::draw::ts_transform(xf);
    if outline {
        let color = if ctx.outline.light { tiny_skia::Color::WHITE } else { tiny_skia::Color::BLACK };
        let paint = tiny_skia::Paint { shader: tiny_skia::Shader::SolidColor(color), anti_alias: true, ..Default::default() };
        let stroke = tiny_skia::Stroke { width: 1.0 / ctx.scale.max(1e-6) as f32, ..Default::default() };
        for it in &shape.items {
            if let Some(p) = crate::draw::ts_path(&it.path) {
                pix.stroke_path(&p, &paint, &stroke, ts, None);
            }
        }
        return;
    }
    for it in &shape.items {
        let [r, g, b, a] = it.rgba;
        let Some(color) = tiny_skia::Color::from_rgba(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), (a * alpha).clamp(0.0, 1.0)) else {
            continue;
        };
        let Some(path) = crate::draw::ts_path(&it.path) else { continue };
        let paint = tiny_skia::Paint { shader: tiny_skia::Shader::SolidColor(color), anti_alias: true, ..Default::default() };
        match it.paint {
            GenPaint::Fill { even_odd } => {
                let rule = if even_odd { FillRule::EvenOdd } else { FillRule::Winding };
                pix.fill_path(&path, &paint, rule, ts, None);
            }
            GenPaint::Stroke { width, cap, join } => {
                if !(width.is_finite() && width > 0.0) {
                    continue;
                }
                let width = width.min(figcraft_pathops::MAX_STROKE_WIDTH);
                let stroke = tiny_skia::Stroke { width: width as f32, line_cap: ts_cap(cap), line_join: ts_join(join), ..Default::default() };
                pix.stroke_path(&path, &paint, &stroke, ts, None);
            }
        }
    }
}

/// [`GenItem`]'s cap and join as FigCraft's export walkers write them (SVG names).
pub fn svg_cap(c: StrokeCap) -> &'static str {
    match c {
        StrokeCap::Round => "round",
        StrokeCap::Square => "square",
        _ => "butt",
    }
}

/// See [`svg_cap`].
pub fn svg_join(j: StrokeJoin) -> &'static str {
    match j {
        StrokeJoin::Miter => "miter",
        StrokeJoin::Bevel => "bevel",
        StrokeJoin::Round => "round",
    }
}

/// What draws in place of a generated layer's design: the result (fresh or stale) and the page
/// copies' transforms in the layer's parent space (the instance transforms). `None`: draw the
/// layer as usual (not a generated layer, or nothing computed yet).
pub struct Drawn {
    pub shape: Arc<GenShape>,
    pub fresh: bool,
    /// The copies of the result, in the layer's parent space (before its own transform).
    pub copies: Vec<Affine>,
}

/// [`Drawn`] for `n`, when it is a generated layer with a result.
pub fn drawn(doc: &Document, n: &Node) -> Option<Drawn> {
    let (shape, fresh) = lookup(doc, n).filter(|(s, _)| !s.keep_design)?;
    let copies = instance_transforms(doc, n).unwrap_or_else(|| vec![Affine::IDENTITY]);
    Some(Drawn { shape, fresh, copies })
}
