//! Picabia: layers whose modifier stack ends in a `stencil` row.
//!
//! The stencil sheet is computed by the engine (`figcraft-stencil`, layer 3 like this crate, so
//! render can't call it). The engine puts each result in the process-wide store here
//! ([`store`]), together with a snapshot of what it was computed from ([`Inputs`]: the layer's
//! subtree and every copy transform in it). Render, exports, bounds and hit testing read it back
//! ([`lookup`]): **fresh** when the document still matches the snapshot, otherwise the layer's
//! latest result (**stale**: shown while the engine recomputes). A layer with no result yet draws
//! its design as usual.
//!
//! The sheet's place doesn't need the result: [`frame`] works it out from the params, the layer's
//! box and the stack's repeat (bounds and hit testing use it on every frame). With `output: tile`
//! the sheet is one repeat cell plus the margin, the cell chosen as `figcraft-stencil` chooses it
//! ([`tile_cell`]).
//!
//! Every store bumps [`generation`]; the retained renderer and the canvas job key fold it into
//! their "fonts changed" counter ([`crate::picabia::frame_generation`]), so a result that arrives
//! repaints the canvas.

use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use figcraft_doc::{Document, Node, NodeId};
use figcraft_geom::kurbo::Vec2;
use figcraft_geom::kurbo::{BezPath, Shape as _};
use figcraft_geom::{Affine, Point, Rect};
use figcraft_modifiers::{ModifierKind, PLUGIN_KEY, Repeat, Stencil, StencilOutput, StencilPreview, StencilTieMode};
use tiny_skia::{FillRule, Pixmap};

use super::{has_stack, instance_transforms};

/// The most results kept (one per stencil layer and document state; oldest dropped first).
const MAX_ENTRIES: usize = 64;
/// The most layers an [`Inputs`] snapshot records; a bigger subtree compares its first ones.
const MAX_INPUT_NODES: usize = 100_000;
/// The most lattice cells a sheet may span (`figcraft-stencil` ignores a finer repeat).
const MAX_CELLS: f64 = 10_000.0;

/// Where a stencil layer's sheet lies, worked out from its params and box (no geometry).
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The stencil row's id.
    pub modifier: String,
    /// Its params, clamped.
    pub params: Stencil,
    /// The copies the rows above the stencil make, as maps of the layer's own space (identity:
    /// the layer itself): `node.transform⁻¹ · T · node.transform`.
    pub copies: Vec<Affine>,
    /// The layer's box through every copy, in its own space: the design the stencil is cut from.
    pub layer_bounds: Rect,
    /// Millimetres per layer unit (`widthMm / layer_bounds width`).
    pub mm_per_unit: f64,
    /// The sheet, in the layer's own space: `layer_bounds` grown by `marginMm`; with tile output
    /// (and a usable repeat) the tile's: [`Frame::cell`]'s bounds grown by the margin.
    pub sheet: Rect,
    /// The stack's repeat lattice ([`figcraft_modifiers::Output::repeat`]) in the layer's own
    /// space (the space the design is gathered in), what `figcraft_stencil::compute_repeat` takes.
    pub repeat: Option<Repeat>,
    /// With tile output: the repeat cell the tile is cut from, its four corners (layer space).
    pub cell: Option<[Point; 4]>,
}

/// `n`'s stencil frame, `None` when its stack doesn't end in an enabled stencil row (or it has no
/// usable size).
pub fn frame(doc: &Document, n: &Node) -> Option<Frame> {
    if !has_stack(n) {
        return None;
    }
    let out = figcraft_modifiers::node_instances_in_doc(doc, n)?;
    let row = out.stencil.as_ref()?;
    let nt = n.transform;
    let mut copies = Vec::new();
    if nt.is_finite() && nt.determinant().abs() > 1e-12 {
        let inv = nt.inverse();
        copies.extend(out.transforms().map(|t| inv * t * nt).filter(|c| c.is_finite()));
    }
    if copies.is_empty() {
        copies.push(Affine::IDENTITY);
    }
    let local = n.local_rect();
    let layer_bounds = copies.iter().map(|c| figcraft_geom::transform_rect_bbox(*c, local)).reduce(|a, b| a.union(b))?;
    let finite = [layer_bounds.x0, layer_bounds.y0, layer_bounds.x1, layer_bounds.y1].iter().all(|v| v.is_finite());
    let w = layer_bounds.width();
    let mm_per_unit = row.params.width_mm / w;
    if !finite || w <= 0.0 || !mm_per_unit.is_finite() || mm_per_unit <= 0.0 {
        return None;
    }
    let margin = row.params.margin_mm / mm_per_unit;
    let mut sheet = layer_bounds.inflate(margin, margin);
    if !sheet.is_finite() {
        return None;
    }
    let repeat = out.repeat.and_then(|r| layer_repeat(r, nt));
    let mut cell = None;
    if row.params.output == StencilOutput::Tile
        && row.params.tie_mode != StencilTieMode::Pattern
        && let Some(c) = repeat.and_then(|r| tile_cell(r, layer_bounds, margin))
    {
        let b = c.iter().skip(1).fold(Rect::from_points(c[0], c[0]), |b, p| b.union_pt(*p)).inflate(margin, margin);
        if b.is_finite() {
            sheet = b;
            cell = Some(c);
        }
    }
    Some(Frame { modifier: row.modifier.clone(), params: row.params.clone(), copies, layer_bounds, mm_per_unit, sheet, repeat, cell })
}

/// Repeat `r` (in the layer's parent space) in the layer's own space: through `nt⁻¹`.
fn layer_repeat(r: Repeat, nt: Affine) -> Option<Repeat> {
    if !nt.is_finite() || nt.determinant().abs() <= 1e-12 {
        return None;
    }
    let inv = nt.inverse();
    let lin = |v: Vec2| inv * Point::new(v.x, v.y) - inv * Point::ORIGIN;
    let out = Repeat { origin: inv * r.origin, a: lin(r.a), b: lin(r.b) };
    [out.origin.x, out.origin.y, out.a.x, out.a.y, out.b.x, out.b.y].iter().all(|v| v.is_finite()).then_some(out)
}

fn cross(u: Vec2, v: Vec2) -> f64 {
    u.x * v.y - u.y * v.x
}

/// The repeat cell a tile is cut from (layer space): the lattice cell of `r` containing the
/// centre of `design`, its corners `o, o + a, o + a + b, o + b`. A strip (one zero vector) has a
/// cell spanning the design across the repeat. `None` where `figcraft-stencil` ignores the repeat:
/// degenerate (zero or parallel vectors) or over 10 000 cells on the sheet (`design` grown by
/// `margin`). Mirrors `figcraft_stencil`'s lattice (`Lattice::new`, `cell_at`) exactly; the
/// engine's tests check the two agree.
pub fn tile_cell(r: Repeat, design: Rect, margin: f64) -> Option<[Point; 4]> {
    let finite = [r.origin.x, r.origin.y, r.a.x, r.a.y, r.b.x, r.b.y].iter().all(|v| v.is_finite());
    let size = design.width().abs().max(design.height().abs());
    if !(finite && size.is_finite() && size > 0.0) {
        return None;
    }
    let eps = size * 1e-9;
    let (la, lb) = (r.a.hypot(), r.b.hypot());
    let (origin, a, b, two_d) = match (la > eps, lb > eps) {
        (false, false) => return None,
        (true, true) => {
            if cross(r.a, r.b).abs() <= la * lb * 1e-9 {
                return None;
            }
            (r.origin, r.a, r.b, true)
        }
        (one_a, _) => {
            let (a, len) = if one_a { (r.a, la) } else { (r.b, lb) };
            let n = Vec2::new(-a.y, a.x) / len;
            let d = design.abs();
            let across = [d.origin(), Point::new(d.x1, d.y0), Point::new(d.x1, d.y1), Point::new(d.x0, d.y1)].map(|p| (p - r.origin).dot(n));
            let (u0, u1) = across.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &u| (lo.min(u), hi.max(u)));
            if u1 - u0 <= eps {
                return None;
            }
            (r.origin + n * u0, a, n * (u1 - u0), false)
        }
    };
    let det = cross(a, b);
    let coords = |p: Point| {
        let d = p - origin;
        (cross(d, b) / det, cross(a, d) / det)
    };
    let sheet = design.abs().inflate(margin, margin);
    let corners = [Point::new(sheet.x0, sheet.y0), Point::new(sheet.x1, sheet.y0), Point::new(sheet.x1, sheet.y1), Point::new(sheet.x0, sheet.y1)];
    let (s0, s1, t0, t1) = corners
        .iter()
        .map(|&p| coords(p))
        .fold((f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY), |(w, x, y, z), (s, t)| (w.min(s), x.max(s), y.min(t), z.max(t)));
    let cells = (s1.ceil() - s0.floor()) * if two_d { t1.ceil() - t0.floor() } else { 1.0 };
    if !(cells.is_finite() && cells <= MAX_CELLS) {
        return None;
    }
    let (s, t) = coords(design.abs().center());
    let (i, j) = (s.floor() as i64, if two_d { t.floor() as i64 } else { 0 });
    let o = origin + (a * i as f64 + b * j as f64);
    let c = [o, o + a, o + a + b, o + b];
    c.iter().all(|p| p.x.is_finite() && p.y.is_finite()).then_some(c)
}

/// Whether `n`'s stack ends in an enabled stencil row (a cheap check: the stack is cached).
pub fn is_stencil(n: &Node) -> bool {
    has_stack(n) && matches!(figcraft_modifiers::node_stack(n), Some(Ok(s)) if s.stencil().is_some())
}

/// A computed stencil, in the layer's own space.
pub struct StencilShape {
    /// The cut-outs (even-odd; non-zero reads the same).
    pub cut: BezPath,
    /// The sheet minus the cut (even-odd).
    pub paper: BezPath,
    /// The sheet rectangle.
    pub sheet: Rect,
    /// What the engine keeps with it (its swatches, stats and warnings).
    pub extra: Option<Arc<dyn Any + Send + Sync>>,
}

/// What a stencil was computed from: the layer's subtree (the layer with what doesn't change the
/// geometry left out: its place, name, opacity and the preview colours) and every copy transform
/// in it. Compared, not hashed: a match is exact.
#[derive(Clone, Debug)]
pub struct Inputs {
    root: Node,
    nodes: Vec<Node>,
    /// Per layer with a stack (pre-order): its copy transforms (the layer's own: in its space).
    copies: Vec<(NodeId, Vec<Affine>)>,
    /// The layer's repeat lattice, in its own space (it follows from the stack and the copies,
    /// compared anyway: the result depends on it).
    repeat: Option<Repeat>,
    fonts: u64,
    /// Some value doesn't equal itself (a NaN): any snapshot of the layer then counts as a match,
    /// or it would be recomputed forever.
    unstable: bool,
}

impl PartialEq for Inputs {
    fn eq(&self, o: &Self) -> bool {
        self.fonts == o.fonts
            && self.root == o.root
            && same_repeat(self.repeat, o.repeat)
            && self.nodes == o.nodes
            && self.copies.len() == o.copies.len()
            && self.copies.iter().zip(&o.copies).all(|((a, ta), (b, tb))| a == b && same_affines(ta, tb))
    }
}

fn same_affines(a: &[Affine], b: &[Affine]) -> bool {
    // Copies of a moved layer come out of `N⁻¹ · T · N` with last-bit differences.
    let close = |x: f64, y: f64| (x - y).abs() <= 1e-9 * (1.0 + x.abs().max(y.abs()));
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.as_coeffs().iter().zip(q.as_coeffs()).all(|(x, y)| close(*x, y)))
}

fn same_repeat(a: Option<Repeat>, b: Option<Repeat>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let t = |r: Repeat| Affine::new([r.a.x, r.a.y, r.b.x, r.b.y, r.origin.x, r.origin.y]);
            same_affines(&[t(a)], &[t(b)])
        }
        _ => false,
    }
}

/// `n` with what doesn't change its stencil's geometry reset.
fn neutral_root(n: &Node) -> Node {
    let mut r = n.clone();
    r.transform = Affine::IDENTITY;
    r.name.clear();
    r.opacity = 1.0;
    if let Some(Ok(stack)) = figcraft_modifiers::node_stack(n) {
        let mut st = (*stack).clone();
        let d = Stencil::default();
        for m in &mut st.modifiers {
            if let ModifierKind::Stencil(p) = &mut m.kind {
                p.preview = d.preview;
                p.paper_colour.clone_from(&d.paper_colour);
                p.paste_colour.clone_from(&d.paste_colour);
                p.cloth_colour.clone_from(&d.cloth_colour);
            }
        }
        r.plugin_data.insert(PLUGIN_KEY.to_string(), figcraft_modifiers::to_json(&st));
    }
    r
}

/// The subtree below `root` (pre-order, capped), `root` left out.
fn descendants<'a>(doc: &'a Document, root: &Node, mut f: impl FnMut(&'a Node) -> bool) {
    let mut stack: Vec<NodeId> = root.children.iter().rev().copied().collect();
    let mut seen = 0usize;
    while let Some(id) = stack.pop() {
        seen += 1;
        if seen > MAX_INPUT_NODES {
            return;
        }
        let Some(n) = doc.node(id) else { continue };
        if !f(n) {
            return;
        }
        stack.extend(n.children.iter().rev().copied());
    }
}

/// The root's copies and repeat in its own space (as [`frame`] has them).
fn root_copies(doc: &Document, n: &Node) -> (Vec<Affine>, Option<Repeat>) {
    frame(doc, n).map(|f| (f.copies, f.repeat)).unwrap_or_default()
}

/// What layer `id`'s stencil would be computed from now.
pub fn inputs(doc: &Document, id: NodeId) -> Option<Inputs> {
    let n = doc.node(id)?;
    let mut nodes = Vec::new();
    let (own, repeat) = root_copies(doc, n);
    let mut copies = vec![(id, own)];
    descendants(doc, n, |c| {
        nodes.push(c.clone());
        if has_stack(c)
            && let Some(ts) = instance_transforms(doc, c)
        {
            copies.push((c.id, ts));
        }
        true
    });
    let mut i = Inputs { root: neutral_root(n), nodes, copies, repeat, fonts: figcraft_text::layout_generation(), unstable: false };
    let copy = i.clone();
    i.unstable = copy != i;
    Some(i)
}

impl Inputs {
    /// Whether `doc`'s layer `n` still matches this snapshot (without copying the subtree).
    fn matches(&self, doc: &Document, n: &Node) -> bool {
        if self.unstable {
            return true;
        }
        if self.fonts != figcraft_text::layout_generation() || self.root != neutral_root(n) {
            return false;
        }
        let Some((_, own)) = self.copies.first() else { return false };
        let (now, repeat) = root_copies(doc, n);
        if !same_affines(own, &now) || !same_repeat(self.repeat, repeat) {
            return false;
        }
        let (mut i, mut k, mut same) = (0usize, 1usize, true);
        descendants(doc, n, |c| {
            same = self.nodes.get(i) == Some(c);
            i += 1;
            if same
                && has_stack(c)
                && let Some(ts) = instance_transforms(doc, c)
            {
                same = self.copies.get(k).is_some_and(|(cid, stored)| *cid == c.id && same_affines(stored, &ts));
                k += 1;
            }
            same
        });
        same && i == self.nodes.len() && k == self.copies.len()
    }
}

struct Entry {
    id: NodeId,
    inputs: Arc<Inputs>,
    shape: Arc<StencilShape>,
    stamp: u64,
}

static STORE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn lock() -> MutexGuard<'static, Vec<Entry>> {
    STORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Bumped by every [`store`] (and [`clear`]).
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Keep `shape`, layer `id`'s stencil computed from `inputs`. Replaces what was kept for the same
/// inputs; the oldest results go past [`MAX_ENTRIES`].
pub fn store(id: NodeId, inputs: Inputs, shape: StencilShape) {
    let stamp = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let mut s = lock();
    s.retain(|e| !(e.id == id && *e.inputs == inputs));
    s.push(Entry { id, inputs: Arc::new(inputs), shape: Arc::new(shape), stamp });
    if s.len() > MAX_ENTRIES {
        s.sort_by_key(|e| std::cmp::Reverse(e.stamp));
        s.truncate(MAX_ENTRIES);
    }
}

/// Forget every result (tests).
pub fn clear() {
    lock().clear();
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Layer `n`'s stencil: the result computed from what `doc` holds now (`true`: fresh) or its
/// latest one (`false`: stale, the engine is recomputing). `None` when none was ever computed.
pub fn lookup(doc: &Document, n: &Node) -> Option<(Arc<StencilShape>, bool)> {
    let mut found: Vec<(Arc<Inputs>, Arc<StencilShape>, u64)> =
        lock().iter().filter(|e| e.id == n.id).map(|e| (e.inputs.clone(), e.shape.clone(), e.stamp)).collect();
    found.sort_by_key(|(_, _, stamp)| std::cmp::Reverse(*stamp));
    // Compared outside the lock: render threads share the store.
    for (inputs, shape, _) in &found {
        if inputs.matches(doc, n) {
            return Some((shape.clone(), true));
        }
    }
    found.into_iter().next().map(|(_, s, _)| (s, false))
}

/// Whether some result was ever stored for layer `id` (cheap: no snapshot comparison).
fn has_result(id: NodeId) -> bool {
    lock().iter().any(|e| e.id == id)
}

/// The sheet of layer `id`'s latest stored result (layer space).
fn latest_sheet(id: NodeId) -> Option<Rect> {
    lock().iter().filter(|e| e.id == id).max_by_key(|e| e.stamp).map(|e| e.shape.sheet)
}

/// Whether stencil layer `n` draws only a repeat tile: tile output with a repeat, and a result to
/// draw (until the first one lands the design draws as usual). Its design's place then doesn't
/// count for bounds or hit testing, only the sheet.
pub fn draws_tile(doc: &Document, n: &Node) -> bool {
    has_stack(n) && frame(doc, n).is_some_and(|f| f.cell.is_some()) && has_result(n.id)
}

/// Layer `n`'s sheet in its own space: where [`frame`] puts it, and while the latest result is
/// stale also where that one was cut (it still draws there).
fn own_sheet(f: &Frame, n: &Node) -> Rect {
    match latest_sheet(n.id) {
        Some(s) if s.is_finite() && s != f.sheet => f.sheet.union(s),
        _ => f.sheet,
    }
}

/// Whether layer `n` has a result computed from what `doc` holds now.
pub fn is_fresh(doc: &Document, n: &Node) -> bool {
    lookup(doc, n).is_some_and(|(_, fresh)| fresh)
}

/// What draws in place of a stencil layer's design: its frame, the result (fresh or stale) and
/// the preview colours. `None`: draw the layer as usual (not a stencil, or nothing computed yet).
pub struct Drawn {
    pub frame: Frame,
    pub shape: Arc<StencilShape>,
    pub fresh: bool,
}

/// [`Drawn`] for `n`, when it is a stencil layer with a result.
pub fn drawn(doc: &Document, n: &Node) -> Option<Drawn> {
    let frame = frame(doc, n)?;
    let (shape, fresh) = lookup(doc, n)?;
    Some(Drawn { frame, shape, fresh })
}

/// One filled area of a stencil preview: a path in the layer's space, its fill rule (`true`:
/// even-odd) and colour (sRGB bytes).
pub struct Fill<'a> {
    pub path: std::borrow::Cow<'a, BezPath>,
    pub even_odd: bool,
    pub rgb: [u8; 3],
}

/// What the preview paints, bottom to top: `paper` = the paper with the cut-outs open (the canvas
/// shows through); `cloth` = the cloth with the paste where the cut-outs let it through.
pub fn fills<'a>(d: &'a Drawn) -> Vec<Fill<'a>> {
    let p = &d.frame.params;
    let col = |hex: &str, fallback: &str| {
        figcraft_modifiers::parse_hex_colour(hex).or_else(|| figcraft_modifiers::parse_hex_colour(fallback)).unwrap_or([0, 0, 0])
    };
    match p.preview {
        StencilPreview::Paper => vec![Fill {
            path: std::borrow::Cow::Borrowed(&d.shape.paper),
            even_odd: true,
            rgb: col(&p.paper_colour, figcraft_modifiers::STENCIL_PAPER),
        }],
        StencilPreview::Cloth => vec![
            Fill {
                path: std::borrow::Cow::Owned(d.shape.sheet.to_path(0.1)),
                even_odd: false,
                rgb: col(&p.cloth_colour, figcraft_modifiers::STENCIL_CLOTH),
            },
            Fill { path: std::borrow::Cow::Borrowed(&d.shape.cut), even_odd: true, rgb: col(&p.paste_colour, figcraft_modifiers::STENCIL_PASTE) },
        ],
    }
}

/// Draw stencil layer `n` (its parent's space mapped to `pix` by `parent_xf`) as its preview;
/// `false` when it isn't one with a result (the caller draws it as usual). Outline mode draws
/// the design's outlines as usual.
pub(crate) fn draw(ctx: &crate::draw::Ctx, pix: &mut Pixmap, n: &Node, parent_xf: Affine) -> bool {
    if ctx.outline_mode || ctx.outlined.contains(&n.id) || !has_stack(n) {
        return false;
    }
    let Some(d) = drawn(ctx.doc, n) else { return false };
    if !n.visible || n.is_mask || n.opacity <= 0.0 || ctx.skip.contains(&n.id) {
        return true;
    }
    let xf = parent_xf * n.transform;
    if !xf.is_finite() {
        return true;
    }
    let alpha = (n.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    for f in fills(&d) {
        let Some(path) = crate::draw::ts_path(&f.path) else { continue };
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(f.rgb[0], f.rgb[1], f.rgb[2], alpha);
        paint.anti_alias = true;
        let rule = if f.even_odd { FillRule::EvenOdd } else { FillRule::Winding };
        pix.fill_path(&path, &paint, rule, crate::draw::ts_transform(xf), None);
    }
    true
}

/// The sheet of stencil layer `n` in its parent's space.
pub fn parent_sheet(doc: &Document, n: &Node) -> Option<Rect> {
    if !has_stack(n) {
        return None;
    }
    let f = frame(doc, n)?;
    Some(figcraft_geom::transform_rect_bbox(n.transform, own_sheet(&f, n)))
}

/// The sheet of stencil layer `id` on its page.
pub fn world_sheet(doc: &Document, id: NodeId) -> Option<Rect> {
    let n = doc.node(id)?;
    if !has_stack(n) {
        return None;
    }
    let f = frame(doc, n)?;
    Some(figcraft_geom::transform_rect_bbox(doc.world_transform(id), own_sheet(&f, n)))
}

/// Whether page point `p` lies on stencil layer `id`'s sheet.
pub fn sheet_contains(doc: &Document, id: NodeId, p: Point) -> bool {
    let Some(n) = doc.node(id) else { return false };
    let Some(f) = frame(doc, n) else { return false };
    let w = doc.world_transform(id);
    w.is_finite() && w.determinant().abs() > 1e-12 && f.sheet.contains(w.inverse() * p)
}

/// The cut's area in the layer's units² (tests and stats).
pub fn cut_area(s: &StencilShape) -> f64 {
    s.cut.area().abs()
}
