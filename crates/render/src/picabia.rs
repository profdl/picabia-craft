//! Picabia: layers with a modifier stack draw, measure, hit-test and export as all their copies.
//!
//! A layer's stack lives in its plugin data (see `figcraft_modifiers`);
//! [`figcraft_modifiers::node_instances_in_doc`] turns it into instance transforms, cached (a
//! `copyToShape` modifier reads its target layer from the document, so every entry point here
//! takes the document). Each instance transform `T` applies in the layer's
//! **parent** space before its own transform: `world = parent_world * T * node.transform`. A layer
//! without a stack (the usual case) costs one plugin-data check here and draws exactly as before.
//!
//! Everything copy-aware outside this file is a marked `// PICABIA:` seam that calls in here: the
//! renderer's draw and bounds walks, the export walkers (`figcraft-svg`, `figcraft-pdf`), the
//! engine's hit testing and marquee, and the canvas hover outline.

use std::cell::Cell;

use figcraft_doc::geometry::BoolResult;
use figcraft_doc::{Document, Node, NodeId};
use figcraft_geom::{Affine, Point, Rect};

pub use figcraft_modifiers::PLUGIN_KEY;

pub mod stencil;

/// What retained renders and canvas jobs compare to know text or stencils changed under them:
/// the text layout generation (fonts arriving) mixed with [`stencil::generation`] (a stencil
/// result arriving), so either repaints.
pub fn frame_generation() -> u64 {
    figcraft_text::layout_generation().wrapping_add(stencil::generation().wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// The most layer draws one top-level draw makes through nested stacks (a stacked group holding
/// stacked layers multiplies). Past it, inner stacks draw fewer copies.
pub const MAX_NESTED_COPIES: usize = 100_000;
/// The deepest the copy-aware walks here go below the layer they start at.
const MAX_WALK_DEPTH: usize = 256;
/// The most layers the copy-aware walks here visit.
const MAX_WALK_NODES: usize = 200_000;

thread_local! {
    /// How many copies the draws already on the stack multiply to (1 outside any stack).
    static NESTED: Cell<usize> = const { Cell::new(1) };
}

/// Whether `n` has modifier data at all (a plugin-data lookup; doesn't parse).
pub fn has_stack(n: &Node) -> bool {
    !n.plugin_data.is_empty() && n.plugin_data.contains_key(PLUGIN_KEY)
}

/// `n`'s instance transforms (parent space, before `n.transform`), or `None` to treat it as one
/// layer. Never empty when `Some`. `doc` holds `n` (copy-to-shape targets are read there).
pub fn instance_transforms(doc: &Document, n: &Node) -> Option<Vec<Affine>> {
    if !has_stack(n) {
        return None;
    }
    let out = figcraft_modifiers::node_instances_in_doc(doc, n)?;
    let v: Vec<Affine> = out.transforms().filter(|t| t.is_finite()).collect();
    (!v.is_empty()).then_some(v)
}

/// Call `f` once per instance transform of `n` (parent space, in draw order) and return `true`;
/// return `false` without calling it when `n` has no active stack (the caller draws it once as
/// usual). Copies nested inside copies share [`MAX_NESTED_COPIES`].
pub fn each_copy(doc: &Document, n: &Node, mut f: impl FnMut(Affine)) -> bool {
    let Some(ts) = instance_transforms(doc, n) else { return false };
    let outer = NESTED.with(Cell::get).max(1);
    let room = (MAX_NESTED_COPIES / outer).max(1);
    let take = ts.len().min(room);
    NESTED.with(|c| c.set(outer.saturating_mul(take)));
    for t in ts.into_iter().take(take) {
        f(t);
    }
    NESTED.with(|c| c.set(outer));
    true
}

/// The bounding box in `n`'s parent space of `r` (in `n`'s own space) drawn by every copy of `n`:
/// `transform_rect_bbox(n.transform, r)` for a layer without a stack.
pub fn parent_bbox(doc: &Document, n: &Node, r: Rect) -> Rect {
    let one = figcraft_geom::transform_rect_bbox(n.transform, r);
    let copies = match instance_transforms(doc, n) {
        Some(ts) => ts.iter().map(|t| figcraft_geom::transform_rect_bbox(*t * n.transform, r)).fold(one, |a, b| a.union(b)),
        None => one,
    };
    // A stencil layer draws its sheet (only that when it draws one repeat tile).
    match stencil::parent_sheet(doc, n) {
        Some(s) if stencil::draws_tile(doc, n) => s,
        Some(s) => copies.union(s),
        None => copies,
    }
}

/// Whether `id` or one of its ancestors has modifier data.
fn chain_has_stack(doc: &Document, id: NodeId) -> bool {
    let mut cur = Some(id);
    let mut guard = 0;
    while let Some(c) = cur {
        let Some(n) = doc.node(c) else { break };
        if n.is_page() || guard > 10_000 {
            break;
        }
        if has_stack(n) {
            return true;
        }
        cur = n.parent;
        guard += 1;
    }
    false
}

/// Page-space bounds of `local` (in `id`'s own space) wherever `id` draws: through its own copies
/// and its ancestors' copies. `transform_rect_bbox(world_transform(id), local)` when nothing on the
/// way up has a stack.
pub fn world_bbox(doc: &Document, id: NodeId, local: Rect) -> Rect {
    if !chain_has_stack(doc, id) {
        return figcraft_geom::transform_rect_bbox(doc.world_transform(id), local);
    }
    let mut r = local;
    let mut cur = Some(id);
    let mut guard = 0;
    while let Some(c) = cur {
        let Some(n) = doc.node(c) else { break };
        if n.is_page() || guard > 10_000 {
            break;
        }
        r = parent_bbox(doc, n, r);
        cur = n.parent;
        guard += 1;
    }
    r
}

/// The page-space maps from `id`'s original to each of its copies (`P · T · P⁻¹`, `P` its parent's
/// world transform), the original itself (an identity instance) left out. Empty without a stack.
pub fn page_copies(doc: &Document, id: NodeId) -> Vec<Affine> {
    let Some(n) = doc.node(id) else { return Vec::new() };
    let Some(ts) = instance_transforms(doc, n) else { return Vec::new() };
    let p = n.parent.map(|p| doc.world_transform(p)).unwrap_or(Affine::IDENTITY);
    if !p.is_finite() || p.determinant().abs() < 1e-12 {
        return Vec::new();
    }
    let inv = p.inverse();
    ts.into_iter().filter(|t| !is_identity(*t)).map(|t| p * t * inv).filter(|c| c.is_finite()).collect()
}

fn is_identity(a: Affine) -> bool {
    a.as_coeffs().iter().zip(Affine::IDENTITY.as_coeffs()).all(|(x, y)| (x - y).abs() < 1e-12)
}

/// Visit `root` and the visible, unlocked layers below it (pre-order, capped).
fn walk(doc: &Document, root: NodeId, f: &mut dyn FnMut(NodeId, &Node)) {
    let mut stack = vec![(root, 0usize)];
    let mut seen = 0usize;
    while let Some((id, depth)) = stack.pop() {
        seen += 1;
        if seen > MAX_WALK_NODES {
            return;
        }
        let Some(n) = doc.node(id) else { continue };
        if !n.visible || n.locked {
            continue;
        }
        f(id, n);
        if depth < MAX_WALK_DEPTH && !n.hides_contents() {
            stack.extend(n.children.iter().rev().map(|c| (*c, depth + 1)));
        }
    }
}

/// Copy-aware [`Document::world_render_bounds`]: also everything the copies of `id`'s descendants
/// paint and, when `own`, the copies of `id` itself.
pub fn render_bounds(doc: &Document, id: NodeId, own: bool) -> Option<Rect> {
    // A stencil drawing one repeat tile draws only that.
    if own
        && let Some(n) = doc.node(id)
        && stencil::draws_tile(doc, n)
        && let Some(s) = stencil::world_sheet(doc, id)
    {
        return Some(s);
    }
    let base = doc.world_render_bounds(id)?;
    let extra = copies_render_bounds(doc, id, own, 0);
    // A stack without an identity copy (copy to shape, mirror without the original) doesn't draw
    // the original where it is.
    if own && extra.is_some() && !draws_original(doc, id) {
        return extra;
    }
    Some(extra.map_or(base, |e| base.union(e)))
}

/// What the copies inside `id` (and of `id` when `own`) add to its render bounds, `None` when no
/// stack is involved.
fn copies_render_bounds(doc: &Document, id: NodeId, own: bool, depth: usize) -> Option<Rect> {
    let n = doc.node(id)?;
    if depth > MAX_WALK_DEPTH || (!n.visible && depth > 0) {
        return None;
    }
    let clips = n.clip && n.kind.is_frame_like() && !n.kind.bounds_follow_children();
    let mut extra: Option<Rect> = None;
    if !clips && !n.hides_contents() {
        for c in &n.children {
            if let Some(e) = copies_render_bounds(doc, *c, true, depth + 1) {
                extra = Some(extra.map_or(e, |x| x.union(e)));
            }
        }
    }
    if !own {
        return extra;
    }
    // A stencil layer draws its sheet (copies of it included: they're in the sheet); one drawing
    // a repeat tile draws only that.
    let sheet = stencil::world_sheet(doc, id);
    if sheet.is_some() && stencil::draws_tile(doc, n) {
        return sheet;
    }
    let copies = page_copies(doc, id);
    if copies.is_empty() {
        return union_some(extra, sheet);
    }
    let mut b = doc.world_render_bounds(id)?;
    if let Some(e) = extra {
        b = b.union(e);
    }
    let start = draws_original(doc, id).then_some(b);
    let all = copies.iter().map(|c| figcraft_geom::transform_rect_bbox(*c, b)).fold(start, |a, c| Some(a.map_or(c, |a| a.union(c))));
    union_some(all, sheet)
}

fn union_some(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    }
}

/// Whether layer `id` draws where it is: no active stack, or one with an identity copy.
fn draws_original(doc: &Document, id: NodeId) -> bool {
    doc.node(id).and_then(|n| instance_transforms(doc, n)).is_none_or(|ts| ts.iter().any(|t| is_identity(*t)))
}

/// Copy-aware [`Document::page_content_bounds`]: the copies of the page's layers count.
pub fn page_content_bounds(doc: &Document, page: NodeId) -> Option<Rect> {
    let base = doc.page_content_bounds(page)?;
    let p = doc.node(page)?;
    Some(p.children.iter().filter_map(|c| copies_render_bounds(doc, *c, true, 1)).fold(base, |a, b| a.union(b)))
}

/// Whether every clipping frame strictly between `root` and `id` holds page point `p`.
fn clips_allow(doc: &Document, root: NodeId, id: NodeId, p: Point) -> bool {
    for a in doc.ancestors(id) {
        if a == root {
            break;
        }
        let Some(an) = doc.node(a) else { break };
        if an.is_page() {
            break;
        }
        if an.clip && an.kind.is_frame_like() {
            let w = doc.world_transform(a);
            if w.determinant().abs() < 1e-12 || !an.local_rect().contains(w.inverse() * p) {
                return false;
            }
        }
    }
    true
}

/// Whether `a` paints over `b` (both on the same page): a layer paints over its ancestors, and
/// siblings in paint order.
pub fn paints_over(doc: &Document, a: NodeId, b: NodeId) -> bool {
    if a == b {
        return false;
    }
    let chain = |id: NodeId| {
        let mut c = doc.ancestors(id);
        c.reverse();
        c.push(id);
        c
    };
    let (ca, cb) = (chain(a), chain(b));
    let i = ca.iter().zip(&cb).take_while(|(x, y)| x == y).count();
    let (Some(xa), Some(xb)) = (ca.get(i), cb.get(i)) else {
        // One holds the other: the inner one paints over.
        return ca.len() > cb.len();
    };
    let Some(parent) = i.checked_sub(1).and_then(|j| ca.get(j)).and_then(|p| doc.node(*p)) else { return false };
    let pos = |x: &NodeId| parent.paint_order().position(|c| c == *x);
    pos(xa) > pos(xb)
}

/// Click hit testing with copies: the layer under page point `p` within `root`, where a copy of a
/// layer is hit as the layer itself (the leaf the original would give at the same spot of the
/// copy). `normal` is what hit testing without copies found; the front-most of the two wins.
pub fn hit_with_copies(doc: &Document, root: NodeId, p: Point, tol: f64, result: BoolResult, normal: Option<NodeId>) -> Option<NodeId> {
    let hit = copies_hit(doc, root, p, normal, |id, q| figcraft_doc::geometry::hit_in(doc, id, q, tol, result));
    stencil_hit(doc, root, p, hit)
}

/// The front-most of `normal` and the stencil layers within `root` whose sheet holds page point
/// `p` (the sheet hides the design: a click anywhere on it hits the layer).
fn stencil_hit(doc: &Document, root: NodeId, p: Point, normal: Option<NodeId>) -> Option<NodeId> {
    let mut best = normal;
    // A stencil drawing one repeat tile hides its design off the tile: nothing of it is hit there.
    if let Some(b) = best
        && tile_hides(doc, b, p)
    {
        best = None;
    }
    walk(doc, root, &mut |id, n| {
        if id == root || !stencil::is_stencil(n) || !clips_allow(doc, root, id, p) || !stencil::sheet_contains(doc, id, p) {
            return;
        }
        best = match best {
            None => Some(id),
            // Something inside it was hit: that layer, as without the stencil.
            Some(b) if b == id || doc.is_ancestor(id, b) => Some(b),
            Some(b) if paints_over(doc, id, b) => Some(id),
            keep => keep,
        };
    });
    best
}

/// Whether layer `id` is (or is inside) a stencil drawing one repeat tile and page point `p` is
/// off that tile's sheet.
fn tile_hides(doc: &Document, id: NodeId, p: Point) -> bool {
    let mut cur = Some(id);
    let mut steps = 0usize;
    while let Some(c) = cur {
        steps += 1;
        if steps > MAX_WALK_DEPTH + 1 {
            return false;
        }
        let Some(n) = doc.node(c) else { return false };
        if stencil::is_stencil(n) && stencil::draws_tile(doc, n) && !stencil::sheet_contains(doc, c, p) {
            return true;
        }
        cur = n.parent;
    }
    false
}

/// [`hit_with_copies`] in outline mode: copies are hit on their outlines too
/// ([`figcraft_doc::geometry::hit_outlines`]), so a click inside one reaches what's behind it.
pub fn hit_outlines_with_copies(
    doc: &Document,
    root: NodeId,
    p: Point,
    tol: f64,
    result: BoolResult,
    also: figcraft_doc::geometry::OutlineHits,
    normal: Option<NodeId>,
) -> Option<NodeId> {
    let hit = copies_hit(doc, root, p, normal, |id, q| figcraft_doc::geometry::hit_outlines(doc, id, q, tol, result, also));
    stencil_hit(doc, root, p, hit)
}

/// The front-most of `normal` and the leaf `leaf_at(layer, point in the original's place)` finds
/// under each copy of a layer within `root`.
fn copies_hit(doc: &Document, root: NodeId, p: Point, normal: Option<NodeId>, leaf_at: impl Fn(NodeId, Point) -> Option<NodeId>) -> Option<NodeId> {
    let mut best = normal;
    walk(doc, root, &mut |id, _| {
        let copies = page_copies(doc, id);
        if copies.is_empty() || !clips_allow(doc, root, id, p) {
            return;
        }
        for c in copies {
            if c.determinant().abs() < 1e-12 {
                continue;
            }
            let q = c.inverse() * p;
            let Some(leaf) = leaf_at(id, q) else { continue };
            best = match best {
                None => Some(leaf),
                Some(b) if b == id || doc.is_ancestor(id, b) || paints_over(doc, leaf, b) => Some(leaf),
                keep => keep,
            };
        }
    });
    best
}

/// Page-space bounds of every copy of `id` (its box), without the original.
pub fn copy_boxes(doc: &Document, id: NodeId) -> Vec<Rect> {
    let Some(w) = doc.world_bounds(id) else { return Vec::new() };
    let mut out: Vec<Rect> = page_copies(doc, id).into_iter().map(|c| figcraft_geom::transform_rect_bbox(c, w)).collect();
    // A stencil layer's sheet counts as one of its copies.
    out.extend(stencil::world_sheet(doc, id));
    out
}

fn touches(b: Rect, r: Rect) -> bool {
    b.x0 <= r.x1 && b.x1 >= r.x0 && b.y0 <= r.y1 && b.y1 >= r.y0
}

/// Marquee with copies: adds to `hits` the layers whose copies (or whose descendants' copies) touch
/// page rect `r`. Not `deep`: `scope`'s children (what a plain marquee picks from); `deep`: the
/// stacked layers themselves anywhere under `scope`, unless something inside them is already hit.
pub fn marquee_copies(doc: &Document, scope: NodeId, r: Rect, deep: bool, hits: &mut Vec<NodeId>) {
    let Some(s) = doc.node(scope) else { return };
    let touched = |id: NodeId| copy_boxes(doc, id).into_iter().any(|b| touches(b, r));
    if deep {
        let mut found = Vec::new();
        walk(doc, scope, &mut |id, _| {
            if id != scope && touched(id) {
                found.push(id);
            }
        });
        for id in found {
            if !hits.contains(&id) && !hits.iter().any(|h| doc.is_ancestor(id, *h) || doc.is_ancestor(*h, id)) {
                hits.push(id);
            }
        }
        return;
    }
    for c in s.children.clone() {
        if hits.contains(&c) || !doc.node(c).is_some_and(|n| n.visible && !n.locked) {
            continue;
        }
        let mut any = false;
        walk(doc, c, &mut |id, _| any = any || touched(id));
        if any {
            hits.push(c);
        }
    }
}

/// The `(source, target)` pairs of `doc`'s copy-to-shape modifiers (enabled rows with a readable
/// target id), for the retained renderer: when a target changes, its sources repaint.
pub fn target_links(doc: &Document) -> Vec<(NodeId, NodeId)> {
    doc.nodes().filter(|n| has_stack(n)).flat_map(|n| figcraft_modifiers::stack_targets(Some(n)).into_iter().map(move |t| (n.id, t))).collect()
}

/// The sources in `links` whose target is `changed`, inside it or holds it, in `old` or `new` (a
/// target's outline follows its layers and its ancestors' transforms).
pub fn dependents(old: &Document, new: &Document, links: &[(NodeId, NodeId)], changed: NodeId) -> Vec<NodeId> {
    let hit = |doc: &Document, t: NodeId| t == changed || doc.is_ancestor(changed, t) || doc.is_ancestor(t, changed);
    let mut out: Vec<NodeId> = links.iter().filter(|(_, t)| hit(old, *t) || hit(new, *t)).map(|(s, _)| *s).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Node `n` with its own stack taken off (exports that draw one copy at a time and place it).
pub fn strip_stack(n: &mut Node) {
    n.plugin_data.remove(PLUGIN_KEY);
}

#[cfg(test)]
#[path = "picabia_tests.rs"]
mod tests;
