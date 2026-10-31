//! PICABIA: stencil layers, engine side.
//!
//! A layer whose modifier stack ends in a `stencil` row draws as a laser-cut katagami sheet
//! instead of its design. Here:
//! - [`gather`]: every visible leaf under the layer as closed outlines in the layer's own space,
//!   with its fill colour and paint order (the renderer's own outline code: strokes outlined,
//!   text as glyph outlines, primitives, transforms and booleans applied; frame clips and masks
//!   applied), including the copies the rows above the stencil make and the copies of stacks on
//!   layers inside it. Gradients cut as their mean colour and images are left out, with warnings.
//! - [`compute_now`]: gather, `figcraft_stencil::compute`, and keep the result in
//!   `figcraft_render::picabia::stencil`'s store with a snapshot of its inputs.
//! - [`refresh`] (the canvas, every frame): recompute what's stale off the UI thread (native: one
//!   worker thread per layer; wasm: on the UI thread once the layer has stopped changing for
//!   [`SETTLE`]), drawing the latest result meanwhile. [`busy`] says a result is still coming.
//! - [`ensure`]: compute what's stale now (exports, headless renders, commands).
//! - [`info`] / [`info_json`]: the swatch list, stats, warnings and islands
//!   (`modifier.stencilInfo` and the Design panel); the tie bars for the canvas markers.

use std::any::Any;
use std::sync::{Arc, Mutex, MutexGuard};

use figcraft_color::{Paint as Fill, PaintKind};
use figcraft_doc::{BoolOp, Document, Node, NodeId, NodeKind};
use figcraft_geom::Affine;
use figcraft_geom::kurbo::BezPath;
use figcraft_render::FillRule;
use figcraft_render::picabia::stencil::{self as store, Frame, StencilShape};
use figcraft_stencil::{ColourInfo, Island, Paint, Stats};
use serde_json::{Value, json};

/// Deepest nesting gathered below the stencil layer.
const MAX_DEPTH: usize = 64;
/// The most layer visits (copies count once each) one gather makes.
const MAX_VISITS: usize = 200_000;
/// Background computes running at once (native).
#[cfg(not(target_arch = "wasm32"))]
const MAX_JOBS: usize = 2;
/// wasm computes on the UI thread: only once a layer has stopped changing for this long.
#[cfg(target_arch = "wasm32")]
const SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

/// The painted outlines of a stencil layer's design.
#[derive(Clone, Debug, Default)]
pub struct Gathered {
    /// In the layer's own space, bottom first.
    pub paints: Vec<Paint>,
    /// What couldn't be cut as drawn (each said once).
    pub warnings: Vec<String>,
}

struct G<'a> {
    doc: &'a Document,
    out: Gathered,
    visits: usize,
    full: bool,
}

impl G<'_> {
    fn warn(&mut self, w: &str) {
        if !self.out.warnings.iter().any(|x| x == w) {
            self.out.warnings.push(w.to_string());
        }
    }

    /// Layer `n` placed by `parent_xf` (its parent's space → the stencil layer's), each copy of a
    /// stack on it.
    fn node(&mut self, n: &Node, parent_xf: Affine, clip: Option<&BezPath>, depth: usize) {
        if !n.visible || n.opacity <= 0.0 || self.full {
            return;
        }
        if store::is_stencil(n) {
            self.warn("a stencil inside the stencil is cut from its design");
        }
        match figcraft_render::picabia::instance_transforms(self.doc, n) {
            Some(ts) => {
                for t in ts {
                    self.once(n, parent_xf * t * n.transform, clip, depth);
                }
            }
            None => self.once(n, parent_xf * n.transform, clip, depth),
        }
    }

    /// One placement of `n`: its own space → the stencil layer's by `xf`.
    fn once(&mut self, n: &Node, xf: Affine, clip: Option<&BezPath>, depth: usize) {
        self.visits += 1;
        if depth > MAX_DEPTH || self.visits > MAX_VISITS {
            self.warn("the design is nested too deep or repeats too often; part of it is left out");
            self.full = true;
            return;
        }
        if !xf.is_finite() {
            return;
        }
        if n.effects.iter().any(|e| e.visible) {
            self.warn("effects (shadows, blurs) aren't cut");
        }
        let doc = self.doc;
        match &n.kind {
            NodeKind::Group => self.children(n, xf, clip, depth),
            NodeKind::Text(_) => self.text(n, xf, clip),
            NodeKind::BooleanOp { .. } => {
                if let Some((p, rule)) = figcraft_render::node_outline(doc, n) {
                    self.emit(p, rule, xf, &n.fills, clip);
                }
                self.stroke(n, xf, clip);
            }
            _ => {
                let closed = figcraft_doc::geometry::is_closed(n);
                if closed {
                    self.fills(n, xf, clip);
                }
                if !n.children.is_empty() && !n.hides_contents() {
                    if n.clip && n.kind.is_frame_like() {
                        let own = figcraft_render::node_outline(doc, n).map(|(mut p, rule)| {
                            p.apply_affine(xf);
                            normalised(p, rule)
                        });
                        let inner = match (own, clip) {
                            (Some(o), Some(c)) => figcraft_pathops::boolean_step(&o, c, BoolOp::Intersect).ok(),
                            (Some(o), None) => Some(o),
                            (None, c) => c.cloned(),
                        };
                        self.children(n, xf, inner.as_ref(), depth);
                    } else {
                        self.children(n, xf, clip, depth);
                    }
                }
                self.stroke(n, xf, clip);
            }
        }
    }

    /// A shape's fills: per vector region when regions have their own paints.
    fn fills(&mut self, n: &Node, xf: Affine, clip: Option<&BezPath>) {
        if let NodeKind::Vector { network } = &n.kind
            && network.regions.iter().any(|r| r.fills.is_some())
        {
            let rounded = network.rounded();
            let net = rounded.as_ref().unwrap_or(network);
            for (i, region) in net.regions.iter().enumerate() {
                let Some(path) = net.region_path(i) else { continue };
                let rule = if region.winding == figcraft_doc::WindingRule::EvenOdd { FillRule::EvenOdd } else { FillRule::Winding };
                let paints = region.fills.as_deref().unwrap_or(&n.fills);
                self.emit(path, rule, xf, paints, clip);
            }
            return;
        }
        if let Some((p, rule)) = figcraft_render::node_outline(self.doc, n) {
            self.emit(p, rule, xf, &n.fills, clip);
        }
    }

    fn stroke(&mut self, n: &Node, xf: Affine, clip: Option<&BezPath>) {
        if let Some((area, rule)) = figcraft_render::stroke_fill(self.doc, n) {
            self.emit((*area).clone(), rule, xf, &n.strokes, clip);
        }
    }

    /// Glyph outlines, per run (runs can have their own fills), then their strokes.
    fn text(&mut self, n: &Node, xf: Affine, clip: Option<&BezPath>) {
        let Some((layout, dy)) = figcraft_text::layout_node(n) else { return };
        let gxf = xf * Affine::translate((0.0, dy));
        let mut runs: std::collections::BTreeMap<usize, BezPath> = std::collections::BTreeMap::new();
        for g in &layout.glyphs {
            if g.color(1.0).is_some() {
                self.warn("colour glyphs (emoji) aren't cut");
                continue;
            }
            runs.entry(g.style).or_default().extend(g.path().elements().iter().copied());
        }
        for d in &layout.decorations {
            runs.entry(d.style).or_default().extend(d.path().elements().iter().copied());
        }
        for (k, path) in &runs {
            let fills = layout.styles.get(*k).and_then(|s| s.fills.as_ref()).unwrap_or(&n.fills);
            self.emit(path.clone(), FillRule::Winding, gxf, fills, clip);
        }
        if n.has_visible_stroke() {
            let even_odd = n.stroke.sides.is_some() || (n.stroke.dashes.is_empty() && n.stroke.align != figcraft_doc::StrokeAlign::Center);
            let rule = if even_odd { FillRule::EvenOdd } else { FillRule::Winding };
            for path in runs.values() {
                let area = figcraft_render::draw::stroke_area(n, path, true);
                self.emit((*area).clone(), rule, gxf, &n.strokes, clip);
            }
        }
    }

    /// `n`'s children in paint order; a mask clips the layers above it up to the next mask.
    fn children(&mut self, n: &Node, xf: Affine, clip: Option<&BezPath>, depth: usize) {
        let doc = self.doc;
        let kids: Vec<Option<&Node>> = n.children.iter().map(|c| doc.node(*c)).collect();
        for run in Document::paint_runs_of(&kids, n.layout.as_ref().is_some_and(|l| l.reverse_z)) {
            let Some(Some(first)) = kids.get(run.start) else { continue };
            if first.is_mask && first.visible {
                if first.mask_type == figcraft_doc::MaskType::Luminance {
                    self.warn("luminance masks are cut as their outline");
                }
                let mut m = (*first).clone();
                m.is_mask = false;
                let shape = figcraft_render::subtree_outline(doc, &m, 0).map(|(mut p, rule)| {
                    p.apply_affine(xf * m.transform);
                    normalised(p, rule)
                });
                let inner = match (shape, clip) {
                    (Some(s), Some(c)) => figcraft_pathops::boolean_step(&s, c, BoolOp::Intersect).ok(),
                    (Some(s), None) => Some(s),
                    (None, _) => None,
                };
                // A mask with no outline shows nothing.
                let Some(inner) = inner else { continue };
                for c in kids.get(run.start + 1..run.end).unwrap_or(&[]).iter().flatten() {
                    self.node(c, xf, Some(&inner), depth + 1);
                }
            } else {
                for c in kids.get(run.clone()).unwrap_or(&[]).iter().flatten() {
                    self.node(c, xf, clip, depth + 1);
                }
            }
        }
    }

    /// One painted area: `path` (in its layer's space, read by `rule`) placed by `xf`, filled by
    /// the top visible paint of `paints`, cut to `clip`.
    fn emit(&mut self, mut path: BezPath, rule: FillRule, xf: Affine, paints: &[Fill], clip: Option<&BezPath>) {
        if path.elements().is_empty() || self.full {
            return;
        }
        let Some(colour) = self.colour(paints) else { return };
        if self.out.paints.len() > figcraft_stencil::MAX_PAINTS {
            // `compute` refuses past its cap with a warning: one more is enough to say so.
            self.full = true;
            return;
        }
        path.apply_affine(xf);
        let mut path = normalised(path, rule);
        if let Some(c) = clip {
            match figcraft_pathops::boolean_step(&path, c, BoolOp::Intersect) {
                Ok(p) => path = p,
                Err(_) => {
                    self.warn("a shape couldn't be clipped; it is cut whole");
                }
            }
        }
        if path.elements().is_empty() {
            return;
        }
        let order = u32::try_from(self.out.paints.len()).unwrap_or(u32::MAX);
        self.out.paints.push(Paint { path, colour, order });
    }

    /// The colour a stack of paints shows: the top visible one (a gradient: its mean colour).
    fn colour(&mut self, paints: &[Fill]) -> Option<[f32; 4]> {
        let top = paints.iter().rev().find(|p| p.visible && p.opacity > 0.0)?;
        match &top.kind {
            PaintKind::Solid { color } => Some([color.r, color.g, color.b, color.a * top.opacity]),
            PaintKind::Gradient(g) => {
                self.warn("gradients are cut as their mean colour");
                let n = g.stops.len().max(1) as f32;
                let sum = g.stops.iter().fold([0.0f32; 4], |a, s| [a[0] + s.color.r, a[1] + s.color.g, a[2] + s.color.b, a[3] + s.color.a]);
                Some([sum[0] / n, sum[1] / n, sum[2] / n, sum[3] / n * top.opacity])
            }
            PaintKind::Image(_) | PaintKind::Pattern(_) => {
                self.warn("images and patterns aren't cut; they're left out");
                None
            }
        }
    }
}

/// `p` as plain non-zero contours (even-odd paths simplified by their rule).
fn normalised(p: BezPath, rule: FillRule) -> BezPath {
    if rule == FillRule::EvenOdd { figcraft_pathops::simplify_with_rule(&p, true).unwrap_or(p) } else { p }
}

/// Layer `n`'s stencil frame and the painted outlines of its design (`None`: not a stencil).
pub fn gather(doc: &Document, n: &Node) -> Option<(Frame, Gathered)> {
    let frame = store::frame(doc, n)?;
    let mut g = G { doc, out: Gathered::default(), visits: 0, full: false };
    for c in &frame.copies {
        g.once(n, *c, None, 0);
    }
    Some((frame, g.out))
}

/// What the stencil panel and `modifier.stencilInfo` show.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// The stencil row's id.
    pub modifier: String,
    /// Every colour of the design, largest visible area first.
    pub colours: Vec<ColourInfo>,
    pub warnings: Vec<String>,
    pub stats: Stats,
    /// The sheet's physical size, mm.
    pub sheet_mm: (f64, f64),
    /// Shapes gathered from the design.
    pub paints: usize,
    /// Every paper island of the cut sheet (layer space), outermost first.
    pub islands: Vec<Island>,
    /// The tie bars (layer space; already part of the paper), for the canvas markers.
    pub ties: BezPath,
}

/// Gather and compute layer `id`'s stencil now and keep it for the renderer. `None`: not a
/// stencil layer.
pub fn compute_now(doc: &Document, id: NodeId) -> Option<Arc<Info>> {
    let n = doc.node(id)?;
    let inputs = store::inputs(doc, id)?;
    let (frame, g) = gather(doc, n)?;
    let mut warnings = g.warnings;
    let (shape, info) = match figcraft_stencil::compute(&g.paints, &frame.params, frame.layer_bounds) {
        Ok(out) => {
            warnings.extend(out.warnings);
            let mpu = out.mm_per_unit;
            let info = Info {
                modifier: frame.modifier.clone(),
                colours: out.colours,
                warnings,
                stats: out.stats,
                sheet_mm: (out.sheet.width() * mpu, out.sheet.height() * mpu),
                paints: g.paints.len(),
                islands: out.islands,
                ties: out.ties,
            };
            (StencilShape { cut: out.cut, paper: out.paper, sheet: out.sheet, extra: None }, info)
        }
        Err(e) => {
            warnings.push(e.to_string());
            let sheet = frame.sheet;
            let info = Info {
                modifier: frame.modifier.clone(),
                colours: Vec::new(),
                warnings,
                stats: Stats::default(),
                sheet_mm: (sheet.width() * frame.mm_per_unit, sheet.height() * frame.mm_per_unit),
                paints: g.paints.len(),
                islands: Vec::new(),
                ties: BezPath::new(),
            };
            let paper = figcraft_geom::kurbo::Shape::to_path(&sheet, 0.1);
            (StencilShape { cut: BezPath::new(), paper, sheet, extra: None }, info)
        }
    };
    let info = Arc::new(info);
    let extra: Arc<dyn Any + Send + Sync> = info.clone();
    store::store(id, inputs, StencilShape { extra: Some(extra), ..shape });
    Some(info)
}

/// Layer `n`'s latest stencil info: `true` when computed from what `doc` holds now. `None` when
/// nothing was computed for it yet ([`refresh`] or [`ensure`] will).
pub fn info(doc: &Document, n: &Node) -> Option<(Arc<Info>, bool)> {
    let (shape, fresh) = store::lookup(doc, n)?;
    let extra = shape.extra.clone()?;
    let info = extra.downcast::<Info>().ok()?;
    Some((info, fresh))
}

/// [`info`] as `modifier.stencilInfo` returns it.
pub fn info_json(info: &Info) -> Value {
    json!({
        "stencil": info.modifier,
        "colours": info.colours.iter().map(|c| json!({"hex": c.hex, "cut": c.cut, "area": c.area})).collect::<Vec<_>>(),
        "warnings": info.warnings,
        "stats": {
            "cutPieces": info.stats.cut_pieces,
            "droppedSpecks": info.stats.dropped_specks,
            "filledHoles": info.stats.filled_holes,
            "cutLengthMm": info.stats.cut_length_mm,
            "areaCutMm2": info.stats.area_cut_mm2,
            "islands": info.stats.islands,
            "ties": info.stats.ties,
            "untied": info.stats.untied,
            "patternHeld": info.stats.pattern_held,
        },
        "islands": info.islands.iter().map(island_json).collect::<Vec<_>>(),
        "sheetMm": {"w": info.sheet_mm.0, "h": info.sheet_mm.1},
        "shapes": info.paints,
    })
}

/// One island as `modifier.stencilInfo` lists it: its bounds in the layer's space, the ties
/// holding it, whether a tie pattern holds it and how deep it sits (0: in a cut-out of the sheet).
fn island_json(i: &Island) -> Value {
    let b = i.bounds;
    let num = |v: f64| if v.is_finite() { json!(v) } else { json!(0.0) };
    json!({"x": num(b.x0), "y": num(b.y0), "w": num(b.width()), "h": num(b.height()), "tied": i.tied, "patternHeld": i.pattern_held, "depth": i.nested_depth})
}

/// [`info`] as JSON for layer `id`, without waiting: `{stencil: null}` when it isn't a stencil
/// layer; `fresh: false` while a newer result is being computed (or none yet: `pending: true`).
pub fn info_value(doc: &Document, id: NodeId) -> Value {
    let Some(n) = doc.node(id) else { return json!({"stencil": null}) };
    if !store::is_stencil(n) {
        return json!({"stencil": null});
    }
    match info(doc, n) {
        Some((i, fresh)) => {
            let mut v = info_json(&i);
            v["fresh"] = json!(fresh);
            v
        }
        None => json!({"stencil": n.id, "pending": true, "fresh": false}),
    }
}

/// The stencil layers of `doc`.
pub fn stencil_layers(doc: &Document) -> Vec<NodeId> {
    doc.nodes().filter(|n| store::is_stencil(n)).map(|n| n.id).collect()
}

/// Compute every stale stencil of `doc` now (exports and headless renders draw fresh sheets).
pub fn ensure(doc: &Document) {
    for id in stencil_layers(doc) {
        if !doc.node(id).is_some_and(|n| store::is_fresh(doc, n)) {
            compute_now(doc, id);
        }
    }
}

#[derive(Default)]
struct Sched {
    /// Layers being computed in the background.
    running: Vec<NodeId>,
    /// wasm: stale layers waiting to settle (the document they were seen in, and since when).
    #[cfg(target_arch = "wasm32")]
    waiting: Vec<(NodeId, usize, web_time::Instant)>,
}

static SCHED: Mutex<Option<Sched>> = Mutex::new(None);

fn sched() -> MutexGuard<'static, Option<Sched>> {
    SCHED.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether a stencil result is still coming (the canvas keeps repainting until it lands).
pub fn busy() -> bool {
    let s = sched();
    let Some(s) = s.as_ref() else { return false };
    #[cfg(target_arch = "wasm32")]
    if !s.waiting.is_empty() {
        return true;
    }
    !s.running.is_empty()
}

/// Bring `doc`'s stencils up to date without blocking the caller long: stale ones are recomputed
/// in the background (native) or once they've stopped changing (wasm). Call it every frame.
pub fn refresh(doc: &Arc<Document>) {
    let mut stale = Vec::new();
    for id in stencil_layers(doc) {
        let Some(n) = doc.node(id) else { continue };
        if store::is_fresh(doc, n) {
            continue;
        }
        stale.push(id);
        schedule(doc, id);
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = sched().as_mut() {
        s.waiting.retain(|w| stale.contains(&w.0));
    }
    let _ = stale;
}

#[cfg(not(target_arch = "wasm32"))]
fn schedule(doc: &Arc<Document>, id: NodeId) {
    {
        let mut g = sched();
        let s = g.get_or_insert_with(Sched::default);
        if s.running.contains(&id) || s.running.len() >= MAX_JOBS {
            return;
        }
        s.running.push(id);
    }
    struct Done(NodeId);
    impl Drop for Done {
        fn drop(&mut self) {
            if let Some(s) = sched().as_mut() {
                s.running.retain(|r| *r != self.0);
            }
        }
    }
    let doc2 = doc.clone();
    let spawned = std::thread::Builder::new().name("picabia-stencil".into()).spawn(move || {
        let _done = Done(id);
        compute_now(&doc2, id);
    });
    if spawned.is_err() {
        let _done = Done(id);
        compute_now(doc, id);
    }
}

#[cfg(target_arch = "wasm32")]
fn schedule(doc: &Arc<Document>, id: NodeId) {
    let ptr = Arc::as_ptr(doc) as usize;
    let now = web_time::Instant::now();
    let first = doc.node(id).is_some_and(|n| store::lookup(doc, n).is_none());
    let ready = {
        let mut g = sched();
        let s = g.get_or_insert_with(Sched::default);
        match s.waiting.iter_mut().find(|w| w.0 == id) {
            Some(w) if w.1 == ptr => first || now.duration_since(w.2) >= SETTLE,
            Some(w) => {
                *w = (id, ptr, now);
                first
            }
            None => {
                s.waiting.push((id, ptr, now));
                first
            }
        }
    };
    if ready {
        compute_now(doc, id);
        if let Some(s) = sched().as_mut() {
            s.waiting.retain(|w| w.0 != id);
        }
    }
}
