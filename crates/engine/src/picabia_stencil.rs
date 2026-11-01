//! PICABIA: stencil layers, engine side.
//!
//! A layer whose modifier stack ends in a `stencil` row draws as a laser-cut katagami sheet
//! instead of its design. Here:
//! - [`gather`]: every visible leaf under the layer as closed outlines in the layer's own space,
//!   with its fill colour and paint order (the renderer's own outline code: strokes outlined,
//!   text as glyph outlines, primitives, transforms and booleans applied; frame clips and masks
//!   applied), including the copies the rows above the stencil make and the copies of stacks on
//!   layers inside it. Gradients cut as their mean colour and images are left out, with warnings.
//! - [`compute_now`]: gather, `figcraft_stencil::compute_repeat` (with the stack's repeat in the
//!   layer's space), and keep the result in
//!   `figcraft_render::picabia::stencil`'s store with a snapshot of its inputs.
//! - [`refresh`] (the canvas, every frame), [`ensure`] (exports, headless renders) and [`busy`]:
//!   the scheduler shared with generated layers ([`crate::picabia_jobs`]).
//! - [`info`] / [`info_json`]: the swatch list, stats, warnings, islands and problems
//!   (`modifier.stencilInfo` and the Design panel); the tie bars, problems, registration holes
//!   and repeat cell for the canvas markers.

use std::any::Any;
use std::sync::Arc;

use figcraft_color::{Paint as Fill, PaintKind};
use figcraft_doc::{BoolOp, Document, Node, NodeId, NodeKind};
use figcraft_geom::kurbo::{BezPath, PathEl};
use figcraft_geom::{Affine, Point};
pub use figcraft_modifiers::Repeat;
use figcraft_render::FillRule;
use figcraft_render::picabia::stencil::{self as store, Frame, StencilShape};
use figcraft_stencil::{ColourInfo, Paint, Stats};
pub use figcraft_stencil::{Island, Problem, ProblemKind};
use serde_json::{Value, json};

/// Deepest nesting gathered below the stencil layer.
const MAX_DEPTH: usize = 64;
/// The most layer visits (copies count once each) one gather makes.
const MAX_VISITS: usize = 200_000;

/// The painted outlines of a stencil layer's design.
#[derive(Clone, Debug, Default)]
pub struct Gathered {
    /// In the layer's own space, bottom first.
    pub paints: Vec<Paint>,
    /// Where each paint came from (same index as [`Gathered::paints`]).
    pub sources: Vec<Source>,
    /// What couldn't be cut as drawn (each said once).
    pub warnings: Vec<String>,
}

/// Where a gathered paint came from ([`Gathered::sources`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    /// The layer that paints it (for a generated layer's result: the item's source, else the
    /// generated layer).
    pub node: NodeId,
    /// That layer's name (a generated result item: the item's name).
    pub name: String,
    /// It is a stroke outlined into a shape of its own.
    pub is_stroke: bool,
    /// Its colour is a gradient's or an image's mean colour (not the paint as drawn).
    pub gradient_or_image: bool,
}

/// What [`gather_design`] does differently from a stencil's gather.
#[derive(Clone, Copy, Debug, Default)]
struct Opts {
    /// This layer's own generator is bypassed: its design is gathered, not its result.
    bypass: Option<NodeId>,
    /// Image and pattern fills are gathered as their mean colour (a stencil leaves them out).
    images: bool,
}

struct G<'a> {
    doc: &'a Document,
    out: Gathered,
    visits: usize,
    full: bool,
    opts: Opts,
}

/// The paint being gathered: its layer and whether it is a stroke.
#[derive(Clone, Copy)]
struct Src<'n> {
    node: &'n Node,
    is_stroke: bool,
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
        // A generated layer is gathered as its result (not its design), unless it is the layer
        // whose own generator is gathering its design.
        if self.opts.bypass != Some(n.id)
            && let Some((shape, _)) = figcraft_render::picabia::generated::lookup(self.doc, n)
            && !shape.keep_design
        {
            for it in &shape.items {
                let (area, even_odd) = it.area();
                let rule = if even_odd { FillRule::EvenOdd } else { FillRule::Winding };
                let source = Source {
                    node: it.source.unwrap_or(n.id),
                    name: it.name.clone(),
                    is_stroke: matches!(it.paint, figcraft_render::picabia::generated::GenPaint::Stroke { .. }),
                    gradient_or_image: false,
                };
                self.push(area.into_owned(), rule, xf, it.rgba, clip, source);
            }
            return;
        }
        if n.effects.iter().any(|e| e.visible) {
            self.warn("effects (shadows, blurs) aren't cut");
        }
        let doc = self.doc;
        match &n.kind {
            NodeKind::Group => self.children(n, xf, clip, depth),
            NodeKind::Text(_) => self.text(n, xf, clip),
            // A pathfinder's own boolean group: its operands are the input (not their combination).
            NodeKind::BooleanOp { .. } if self.opts.bypass == Some(n.id) => self.children(n, xf, clip, depth),
            NodeKind::BooleanOp { .. } => {
                if let Some((p, rule)) = figcraft_render::node_outline(doc, n) {
                    self.emit(p, rule, xf, &n.fills, clip, Src { node: n, is_stroke: false });
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
                self.emit(path, rule, xf, paints, clip, Src { node: n, is_stroke: false });
            }
            return;
        }
        if let Some((p, rule)) = figcraft_render::node_outline(self.doc, n) {
            self.emit(p, rule, xf, &n.fills, clip, Src { node: n, is_stroke: false });
        }
    }

    fn stroke(&mut self, n: &Node, xf: Affine, clip: Option<&BezPath>) {
        if let Some((area, rule)) = figcraft_render::stroke_fill(self.doc, n) {
            self.emit((*area).clone(), rule, xf, &n.strokes, clip, Src { node: n, is_stroke: true });
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
            self.emit(path.clone(), FillRule::Winding, gxf, fills, clip, Src { node: n, is_stroke: false });
        }
        if n.has_visible_stroke() {
            let even_odd = n.stroke.sides.is_some() || (n.stroke.dashes.is_empty() && n.stroke.align != figcraft_doc::StrokeAlign::Center);
            let rule = if even_odd { FillRule::EvenOdd } else { FillRule::Winding };
            for path in runs.values() {
                let area = figcraft_render::draw::stroke_area(n, path, true);
                self.emit((*area).clone(), rule, gxf, &n.strokes, clip, Src { node: n, is_stroke: true });
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
    fn emit(&mut self, path: BezPath, rule: FillRule, xf: Affine, paints: &[Fill], clip: Option<&BezPath>, from: Src<'_>) {
        if path.elements().is_empty() || self.full {
            return;
        }
        let Some((colour, gradient_or_image)) = self.colour(paints) else { return };
        let source = Source { node: from.node.id, name: from.node.name.clone(), is_stroke: from.is_stroke, gradient_or_image };
        self.push(path, rule, xf, colour, clip, source);
    }

    /// One painted area in `colour`: `path` (read by `rule`) placed by `xf`, cut to `clip`.
    fn push(&mut self, mut path: BezPath, rule: FillRule, xf: Affine, colour: [f32; 4], clip: Option<&BezPath>, source: Source) {
        if path.elements().is_empty() || self.full {
            return;
        }
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
        self.out.sources.push(source);
    }

    /// The colour a stack of paints shows: the top visible one (a gradient: its mean colour), and
    /// whether it is a mean colour.
    fn colour(&mut self, paints: &[Fill]) -> Option<([f32; 4], bool)> {
        let top = paints.iter().rev().find(|p| p.visible && p.opacity > 0.0)?;
        match &top.kind {
            PaintKind::Solid { color } => Some(([color.r, color.g, color.b, color.a * top.opacity], false)),
            PaintKind::Gradient(g) => {
                self.warn(if self.opts.images { "gradients are drawn as their mean colour" } else { "gradients are cut as their mean colour" });
                let n = g.stops.len().max(1) as f32;
                let sum = g.stops.iter().fold([0.0f32; 4], |a, s| [a[0] + s.color.r, a[1] + s.color.g, a[2] + s.color.b, a[3] + s.color.a]);
                Some(([sum[0] / n, sum[1] / n, sum[2] / n, sum[3] / n * top.opacity], true))
            }
            PaintKind::Image(img) if self.opts.images => {
                self.warn("images and patterns are drawn as their mean colour");
                let c = image_mean(self.doc, img);
                Some(([c[0], c[1], c[2], c[3] * top.opacity], true))
            }
            PaintKind::Pattern(_) if self.opts.images => {
                self.warn("images and patterns are drawn as their mean colour");
                Some(([0.5, 0.5, 0.5, top.opacity], true))
            }
            PaintKind::Image(_) | PaintKind::Pattern(_) => {
                self.warn("images and patterns aren't cut; they're left out");
                None
            }
        }
    }
}

/// The mean colour of image paint `img` as drawn (its adjustments applied), sampled on a grid of at
/// most 64 × 64 pixels; mid-grey when the image doesn't decode.
fn image_mean(doc: &Document, img: &figcraft_color::ImagePaint) -> [f32; 4] {
    let Some(pm) = figcraft_render::images::get(&img.image, doc.images.get(&img.image), &img.filters, doc.color_profile) else {
        return [0.5, 0.5, 0.5, 1.0];
    };
    let (w, h) = (pm.width().max(1), pm.height().max(1));
    let (sx, sy) = ((w / 64).max(1), (h / 64).max(1));
    let (mut sum, mut n) = ([0.0f64; 4], 0.0f64);
    for y in (0..h).step_by(sy as usize) {
        for x in (0..w).step_by(sx as usize) {
            if let Some(p) = pm.pixel(x, y) {
                let c = p.demultiply();
                for (s, v) in sum.iter_mut().zip([c.red(), c.green(), c.blue(), c.alpha()]) {
                    *s += f64::from(v) / 255.0;
                }
                n += 1.0;
            }
        }
    }
    if n == 0.0 {
        return [0.5, 0.5, 0.5, 1.0];
    }
    sum.map(|s| (s / n) as f32)
}

/// `p` as plain non-zero contours (even-odd paths simplified by their rule).
fn normalised(p: BezPath, rule: FillRule) -> BezPath {
    if rule == FillRule::EvenOdd { figcraft_pathops::simplify_with_rule(&p, true).unwrap_or(p) } else { p }
}

/// Layer `n`'s stencil frame and the painted outlines of its design (`None`: not a stencil).
pub fn gather(doc: &Document, n: &Node) -> Option<(Frame, Gathered)> {
    let frame = store::frame(doc, n)?;
    let mut g = G { doc, out: Gathered::default(), visits: 0, full: false, opts: Opts::default() };
    for c in &frame.copies {
        g.once(n, *c, None, 0);
    }
    Some((frame, g.out))
}

/// The painted outlines of layer `n`'s own design (its own generator bypassed: generated layers
/// inside it count as their results) at each of `copies` (maps of its own space; the generator's
/// input copies), in its own space, bottom first: a pathfinder's input. Gathered as a stencil
/// gathers (strokes outlined into shapes of their own, text as glyphs, clips and masks applied,
/// the copies of stacks inside), except that image and pattern fills count as their mean colour.
pub fn gather_design(doc: &Document, n: &Node, copies: &[figcraft_geom::Affine]) -> Gathered {
    let mut g = G { doc, out: Gathered::default(), visits: 0, full: false, opts: Opts { bypass: Some(n.id), images: true } };
    for c in copies {
        g.once(n, *c, None, 0);
    }
    g.out
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
    /// The whole design's sheet (its bounds plus the margin), mm.
    pub sheet_mm: (f64, f64),
    /// With tile output: the tile's sheet (the repeat cell's bounds plus the margin), mm; what is
    /// drawn and exported.
    pub tile_mm: Option<(f64, f64)>,
    /// Shapes gathered from the design.
    pub paints: usize,
    /// Every paper island of the cut sheet (layer space), outermost first.
    pub islands: Vec<Island>,
    /// The tie bars (layer space; already part of the paper), for the canvas markers.
    pub ties: BezPath,
    /// What still breaks a cutting limit (layer space), at most `figcraft_stencil::MAX_PROBLEMS`
    /// ([`Stats::problems_by_kind`] counts them all).
    pub problems: Vec<Problem>,
    /// The registration holes' centres (layer space).
    pub hoshi: Vec<Point>,
    /// The lattice the islands and ties are repeat-aware along (layer space); `None` when there's
    /// no repeat or the stencil ignores it (see the warnings).
    pub repeat: Option<Repeat>,
    /// The repeat cell the panel and canvas show (layer space; the tile's cell with tile output),
    /// its four corners. `Some` exactly when [`Info::repeat`] is.
    pub cell: Option<[Point; 4]>,
}

/// Gather and compute layer `id`'s stencil now and keep it for the renderer. `None`: not a
/// stencil layer.
pub fn compute_now(doc: &Document, id: NodeId) -> Option<Arc<Info>> {
    // The generated layers it cuts (inside it, or its own generator above the stencil) first.
    crate::picabia_generate::prepare(doc, id, true);
    let n = doc.node(id)?;
    let inputs = store::inputs(doc, id)?;
    let (frame, g) = gather(doc, n)?;
    let mut warnings = g.warnings;
    let design_sheet = |mpu: f64| {
        let m = frame.params.margin_mm / mpu;
        let s = frame.layer_bounds.abs().inflate(m, m);
        (s.width() * mpu, s.height() * mpu)
    };
    let (shape, info) = match figcraft_stencil::compute_repeat(&g.paints, &frame.params, frame.layer_bounds, frame.repeat) {
        Ok(out) => {
            warnings.extend(out.warnings);
            let mpu = out.mm_per_unit;
            let cell = out.cell.as_ref().and_then(corners);
            let info = Info {
                modifier: frame.modifier.clone(),
                colours: out.colours,
                warnings,
                stats: out.stats,
                sheet_mm: design_sheet(mpu),
                tile_mm: out.tile.as_ref().map(|t| (t.sheet.width() * mpu, t.sheet.height() * mpu)),
                paints: g.paints.len(),
                islands: out.islands,
                ties: out.ties,
                problems: out.problems,
                hoshi: out.hoshi,
                repeat: if cell.is_some() { frame.repeat } else { None },
                cell,
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
                sheet_mm: design_sheet(frame.mm_per_unit),
                tile_mm: None,
                paints: g.paints.len(),
                islands: Vec::new(),
                ties: BezPath::new(),
                problems: Vec::new(),
                hoshi: Vec::new(),
                repeat: None,
                cell: None,
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

/// The four corners of a repeat cell's outline (a closed quadrilateral).
fn corners(p: &BezPath) -> Option<[Point; 4]> {
    let pts: Vec<Point> = p
        .elements()
        .iter()
        .filter_map(|e| match e {
            PathEl::MoveTo(q) | PathEl::LineTo(q) => Some(*q),
            _ => None,
        })
        .collect();
    let pts = match pts.as_slice() {
        [a, .., z] if pts.len() == 5 && a == z => pts.get(..4)?,
        _ => pts.as_slice(),
    };
    match pts {
        [a, b, c, d] => Some([*a, *b, *c, *d]),
        _ => None,
    }
}

/// Every stencil material preset (`custom` has none) as `modifier.stencilInfo` lists them.
pub fn materials_json() -> Value {
    let rows: Vec<Value> = figcraft_modifiers::StencilMaterial::ALL
        .iter()
        .filter_map(|m| {
            let l = m.limits()?;
            Some(json!({
                "id": m.id(),
                "minCutWidthMm": l.min_cut_width_mm,
                "minBridgeMm": l.min_bridge_mm,
                "tieWidthMm": l.tie_width_mm,
                "minCornerAngle": l.min_corner_angle,
                "kerfMm": l.kerf_mm,
            }))
        })
        .collect();
    Value::Array(rows)
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
            "repeatIslands": info.stats.repeat_islands,
            "problems": ProblemKind::ALL.iter().map(|k| (k.id().to_string(), json!(info.stats.problems_by_kind.get(*k)))).collect::<serde_json::Map<_, _>>(),
        },
        "islands": info.islands.iter().map(island_json).collect::<Vec<_>>(),
        "problems": info.problems.iter().map(problem_json).collect::<Vec<_>>(),
        "hoshi": info.hoshi.iter().map(|p| json!({"x": num(p.x), "y": num(p.y)})).collect::<Vec<_>>(),
        "repeat": info.repeat.map_or(Value::Null, |r| json!({"ax": num(r.a.x), "ay": num(r.a.y), "bx": num(r.b.x), "by": num(r.b.y)})),
        "cell": info.cell.map_or(Value::Null, |c| Value::Array(c.iter().map(|p| json!({"x": num(p.x), "y": num(p.y)})).collect())),
        "tile": info.tile_mm.map_or(Value::Null, |(w, h)| json!({"w": num(w), "h": num(h)})),
        "sheetMm": {"w": info.sheet_mm.0, "h": info.sheet_mm.1},
        "materials": materials_json(),
        "shapes": info.paints,
    })
}

/// A number for JSON (what isn't finite reads 0).
fn num(v: f64) -> Value {
    if v.is_finite() { json!(v) } else { json!(0.0) }
}

/// One problem as `modifier.stencilInfo` lists it: its kind and bounds in the layer's space.
fn problem_json(p: &Problem) -> Value {
    let b = p.bounds;
    json!({"kind": p.kind.id(), "x": num(b.x0), "y": num(b.y0), "w": num(b.width()), "h": num(b.height())})
}

/// One island as `modifier.stencilInfo` lists it: its bounds in the layer's space, the ties
/// holding it, whether a tie pattern holds it and how deep it sits (0: in a cut-out of the sheet).
fn island_json(i: &Island) -> Value {
    let b = i.bounds;
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

/// Compute every stale stencil (and generator result) of `doc` now (exports and headless renders
/// draw fresh sheets): [`crate::picabia_jobs::ensure`].
pub fn ensure(doc: &Document) {
    crate::picabia_jobs::ensure(doc);
}

/// Whether a stencil (or generator) result is still coming: [`crate::picabia_jobs::busy`].
pub fn busy() -> bool {
    crate::picabia_jobs::busy()
}

/// Bring `doc`'s stencils (and generator results) up to date without blocking the caller long:
/// [`crate::picabia_jobs::refresh`].
pub fn refresh(doc: &Arc<Document>) {
    crate::picabia_jobs::refresh(doc);
}
