//! PICABIA: generated layers, engine side (the `imageTrace` and `pathfinder` generator rows).
//!
//! A layer whose stack has a generator row draws the generator's result instead of its design
//! (`figcraft_render::picabia::generated`). Here, shared by every generator kind:
//! - [`compute`]: the per-kind registry. Each kind lives in its own file and provides
//!   `compute(doc, node, frame, params) -> GenOut` and `apply_result(doc, id, shape) ->
//!   Result<NodeId>`: [`crate::picabia_trace`] and [`crate::picabia_pathfinder`].
//! - [`compute_now`] / [`prepare`]: compute a layer's result (the stale generated layers inside it
//!   first, deepest first, so a pathfinder over a traced child reads the fresh trace) and keep it
//!   in the render-side store with a snapshot of its inputs. Scheduling (off the UI thread, or
//!   after a settle on wasm) is shared with the stencil: [`crate::picabia_jobs`].
//! - [`info`] / [`result_info`]: `modifier.resultInfo` and the Design panel's status line.
//! - [`apply`] / [`bake`]: Apply and the `.fig`/Penpot bake replace the layer by a group of real
//!   vector layers, one per result item ([`replace_with_items`]), with the rows below the generator
//!   moved onto the group.

use std::any::Any;
use std::sync::Arc;

use figcraft_color::{Color, Paint};
use figcraft_doc::{Document, Node, NodeId, NodeKind, VectorNetwork, WindingRule};
use figcraft_geom::Affine;
use figcraft_geom::kurbo::Shape as _;
use figcraft_modifiers::{GeneratorKind, ModifierKind, PLUGIN_KEY, Stack};
use figcraft_render::picabia::generated::{self as store, GenFrame, GenItem, GenPaint, GenShape};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

/// The most generated layers one [`prepare`] computes inside a layer.
const MAX_NESTED: usize = 1000;

/// What a generator kind's `compute` returns.
#[derive(Clone, Debug, Default)]
pub struct GenOut {
    /// The result, in the layer's own space (the input copies folded in), bottom first.
    pub items: Vec<GenItem>,
    /// Shown in the panel and `modifier.resultInfo`.
    pub warnings: Vec<String>,
    /// The kind's stats (`modifier.resultInfo`'s `stats`; an object).
    pub stats: Value,
    /// No result to draw: the layer draws its design as usual (say why in `warnings`).
    pub keep_design: bool,
}

impl GenOut {
    /// A result of `items` with `stats`.
    pub fn new(items: Vec<GenItem>, stats: Value) -> GenOut {
        GenOut { items, warnings: Vec::new(), stats, keep_design: false }
    }

    /// No result: the layer draws its design as usual, with `warning`.
    pub fn keep_design(warning: impl Into<String>) -> GenOut {
        GenOut { items: Vec::new(), warnings: vec![warning.into()], stats: json!({}), keep_design: true }
    }
}

/// Layer `n`'s generator result for `frame` (its generator row, input copies and a-priori bounds),
/// by the row's kind.
pub fn compute(doc: &Document, n: &Node, frame: &GenFrame) -> GenOut {
    match &frame.kind {
        GeneratorKind::ImageTrace(p) => crate::picabia_trace::compute(doc, n, frame, p),
        GeneratorKind::Pathfinder(p) => crate::picabia_pathfinder::compute(doc, n, frame, p),
    }
}

/// Replace layer `id` of `doc` by the group Apply makes of its result `shape`, by its generator
/// kind (`None`: `id` has no generator row). The group is returned.
pub fn apply_result(doc: &mut Document, id: NodeId, shape: &GenShape) -> Result<Option<NodeId>> {
    let n = doc.get(id)?;
    let Some(frame) = store::frame(doc, n) else { return Ok(None) };
    let g = match frame.kind {
        GeneratorKind::ImageTrace(_) => crate::picabia_trace::apply_result(doc, id, shape)?,
        GeneratorKind::Pathfinder(_) => crate::picabia_pathfinder::apply_result(doc, id, shape)?,
    };
    Ok(Some(g))
}

/// What the Design panel and `modifier.resultInfo` show about a generator's result.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// The generator row's id.
    pub modifier: String,
    /// Its kind id (`imageTrace`, `pathfinder`).
    pub kind: &'static str,
    pub warnings: Vec<String>,
    /// The kind's stats (an object).
    pub stats: Value,
    /// The result's colours (`#rrggbb`), largest area first.
    pub colours: Vec<String>,
    /// Items in the result.
    pub items: usize,
    /// The layer draws its design (no result).
    pub keep_design: bool,
}

/// The result's colours, largest painted area first (a stroke counts its length times its width).
fn colours(items: &[GenItem]) -> Vec<String> {
    let mut by: Vec<(String, f64)> = Vec::new();
    for it in items {
        let [r, g, b, _] = it.rgba.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        let hex = figcraft_modifiers::hex_colour([r, g, b]);
        let area = match it.paint {
            GenPaint::Fill { .. } => it.path.area().abs(),
            GenPaint::Stroke { width, .. } => it.path.perimeter(0.1) * width.max(0.0),
        };
        let area = if area.is_finite() { area } else { 0.0 };
        match by.iter_mut().find(|(h, _)| *h == hex) {
            Some((_, a)) => *a += area,
            None => by.push((hex, area)),
        }
    }
    by.sort_by(|a, b| b.1.total_cmp(&a.1));
    by.into_iter().map(|(h, _)| h).collect()
}

/// The generated layers strictly inside `id`, deepest first (each before the layers holding it).
fn nested(doc: &Document, id: NodeId) -> Vec<NodeId> {
    let mut v: Vec<NodeId> = doc.subtree(id).into_iter().skip(1).filter(|c| doc.node(*c).is_some_and(store::is_generated)).collect();
    v.truncate(MAX_NESTED);
    v.reverse();
    v
}

/// Bring the generated layers layer `id` reads up to date, synchronously: the stale ones inside
/// it (deepest first) and, with `own`, its own generator's result.
pub fn prepare(doc: &Document, id: NodeId, own: bool) {
    for c in nested(doc, id) {
        if doc.node(c).is_some_and(|n| !store::is_fresh(doc, n)) {
            compute_one(doc, c);
        }
    }
    if own && doc.node(id).is_some_and(|n| store::is_generated(n) && !store::is_fresh(doc, n)) {
        compute_one(doc, id);
    }
}

/// Compute layer `id`'s generator result now (the stale generated layers inside it first) and
/// keep it for the renderer. `None`: not a generated layer.
pub fn compute_now(doc: &Document, id: NodeId) -> Option<Arc<Info>> {
    prepare(doc, id, false);
    compute_one(doc, id)
}

fn compute_one(doc: &Document, id: NodeId) -> Option<Arc<Info>> {
    let n = doc.node(id)?;
    let frame = store::frame(doc, n)?;
    let inputs = store::inputs(doc, id)?;
    let out = compute(doc, n, &frame);
    let info = Arc::new(Info {
        modifier: frame.modifier.clone(),
        kind: frame.kind.id(),
        warnings: out.warnings,
        stats: if out.stats.is_object() { out.stats } else { json!({}) },
        colours: colours(&out.items),
        items: out.items.len(),
        keep_design: out.keep_design,
    });
    let extra: Arc<dyn Any + Send + Sync> = info.clone();
    let shape = if out.keep_design { GenShape::design(Some(extra)) } else { GenShape::new(out.items, Some(extra)) };
    store::store(id, inputs, shape);
    Some(info)
}

/// Layer `n`'s latest result info: `true` when computed from what `doc` holds now. `None` when
/// nothing was computed for it yet (or it isn't a generated layer).
pub fn info(doc: &Document, n: &Node) -> Option<(Arc<Info>, bool)> {
    let (shape, fresh) = store::lookup(doc, n)?;
    let info = shape.extra.clone()?.downcast::<Info>().ok()?;
    Some((info, fresh))
}

/// `info` as `modifier.resultInfo` returns it.
pub fn info_json(info: &Info, fresh: bool) -> Value {
    json!({
        "modifier": info.modifier,
        "kind": info.kind,
        "fresh": fresh,
        "pending": false,
        "warnings": info.warnings,
        "stats": info.stats,
        "colours": info.colours,
        "items": info.items,
        "drawsDesign": info.keep_design,
    })
}

/// `modifier.resultInfo` for layer `id`: computes the result when it is out of date.
/// `{modifier: null}` when the layer has no generator row.
pub fn result_info(doc: &Document, id: NodeId) -> Value {
    let Some(n) = doc.node(id) else { return json!({"modifier": null}) };
    if !store::is_generated(n) {
        return json!({"modifier": null});
    }
    let got = match info(doc, n) {
        Some((i, true)) => Some(i),
        _ => compute_now(doc, id),
    };
    match got {
        Some(i) => info_json(&i, true),
        None => json!({"modifier": null}),
    }
}

/// [`info`] as JSON for layer `id`, without waiting: `{modifier: null}` when it isn't a generated
/// layer; `fresh: false` while a newer result is being computed (none yet: `pending: true`).
pub fn info_value(doc: &Document, id: NodeId) -> Value {
    let Some(n) = doc.node(id) else { return json!({"modifier": null}) };
    let Some(frame) = store::frame(doc, n) else { return json!({"modifier": null}) };
    match info(doc, n) {
        Some((i, fresh)) => info_json(&i, fresh),
        None => {
            json!({"modifier": frame.modifier, "kind": frame.kind.id(), "pending": true, "fresh": false, "warnings": [], "stats": {}, "colours": []})
        }
    }
}

/// The generated layers of `doc`.
pub fn generated_layers(doc: &Document) -> Vec<NodeId> {
    doc.nodes().filter(|n| store::is_generated(n)).map(|n| n.id).collect()
}

/// The index of `stack`'s counted generator row: the first enabled generator with no enabled
/// stencil above it.
pub fn generator_index(stack: &Stack) -> Option<usize> {
    for (i, m) in stack.modifiers.iter().enumerate().filter(|(_, m)| m.enabled) {
        match &m.kind {
            ModifierKind::Stencil(_) => return None,
            k if k.is_generator() => return Some(i),
            _ => {}
        }
    }
    None
}

/// One vector layer per item of `shape`, in a new group named `name` that takes layer `id`'s
/// place (same parent and position, its child layout); `id` is removed. Fill items get a solid
/// fill and their winding, stroke items a centre stroke of their width, cap and join and no fill;
/// each is named by [`GenItem::name`] and placed by `id`'s transform (the items are in its space).
/// Bottom first. Empty items are left out. The group is returned.
pub fn replace_with_items(doc: &mut Document, id: NodeId, shape: &GenShape, name: &str) -> Result<NodeId> {
    let n = doc.get(id)?;
    let parent = n.parent.ok_or_else(|| EngineError::Other(format!("{id} is a page")))?;
    let (own, child_layout) = (n.transform, n.child_layout.clone());
    let index = doc.index_in_parent(id);
    let gid = doc.alloc_id();
    let mut g = Node::new(gid, NodeKind::Group, name, 0.0, 0.0);
    g.child_layout = child_layout;
    doc.insert(g, parent, index)?;
    for it in &shape.items {
        let bb = it.path.bounding_box();
        if !(bb.is_finite() && (bb.width() > 1e-9 || bb.height() > 1e-9)) {
            continue;
        }
        let mut path = it.path.clone();
        path.apply_affine(Affine::translate((-bb.x0, -bb.y0)));
        let mut network = VectorNetwork::from_path(&path);
        if network.segments.is_empty() {
            continue;
        }
        let colour = Paint::solid(Color { r: it.rgba[0], g: it.rgba[1], b: it.rgba[2], a: it.rgba[3].clamp(0.0, 1.0) });
        let vid = doc.alloc_id();
        let mut v = Node::new(vid, NodeKind::Vector { network: VectorNetwork::default() }, it.name.clone(), bb.width(), bb.height());
        v.transform = own * Affine::translate((bb.x0, bb.y0));
        match it.paint {
            GenPaint::Fill { even_odd } => {
                let winding = if even_odd { WindingRule::EvenOdd } else { WindingRule::NonZero };
                for r in &mut network.regions {
                    r.winding = winding;
                }
                v.fills = vec![colour];
                v.strokes = Vec::new();
            }
            GenPaint::Stroke { width, cap, join } => {
                network.regions.clear();
                v.fills = Vec::new();
                v.strokes = vec![colour];
                v.stroke.weight = if width.is_finite() { width.max(0.0) } else { 1.0 };
                v.stroke.align = figcraft_doc::StrokeAlign::Center;
                v.stroke.cap_start = cap;
                v.stroke.cap_end = cap;
                v.stroke.join = join;
            }
        }
        v.kind = NodeKind::Vector { network };
        doc.insert(v, gid, None)?;
    }
    doc.remove(id)?;
    doc.fit_to_children(gid)?;
    doc.fit_ancestors(gid);
    figcraft_layout::coherent_sizing(doc, gid);
    Ok(gid)
}

/// Bake generated layer `id` of `doc` as Apply does, from its stored result: the layer becomes
/// the kind's group of real vector layers ([`apply_result`]), and the rows below its generator
/// (with their variable bindings) move onto the group, live. `Ok(None)`: no generator row, or no
/// result to bake (none computed, or it draws its design).
pub fn bake(doc: &mut Document, id: NodeId) -> Result<Option<NodeId>> {
    let n = doc.get(id)?;
    let Some((shape, _)) = store::lookup(doc, n) else { return Ok(None) };
    if shape.keep_design {
        return Ok(None);
    }
    let stack = match n.plugin_data.get(PLUGIN_KEY).map(|src| figcraft_modifiers::parse(src)) {
        Some(Ok(s)) => s,
        _ => return Ok(None),
    };
    let Some(g) = generator_index(&stack) else { return Ok(None) };
    let rest = Stack { modifiers: stack.modifiers.get(g + 1..).map(<[_]>::to_vec).unwrap_or_default(), ..stack.clone() };
    let kept: Vec<String> = rest.modifiers.iter().map(|m| format!("{}{}/", figcraft_modifiers::BINDING_PREFIX, m.id)).collect();
    let moved: Vec<(String, String)> =
        n.bound.iter().filter(|(k, _)| kept.iter().any(|p| k.starts_with(p.as_str()))).map(|(k, v)| (k.clone(), v.clone())).collect();
    let src_pivot = figcraft_modifiers::node_bounds(n).center();
    let Some(gid) = apply_result(doc, id, &shape)? else { return Ok(None) };
    if !rest.is_empty() {
        let gn = doc.get(gid)?;
        let pivot = figcraft_modifiers::node_bounds(gn).center();
        let rest = crate::cmd::modifier::repivot(rest, src_pivot - pivot);
        let gn = doc.get_mut(gid)?;
        gn.plugin_data.insert(PLUGIN_KEY.to_string(), figcraft_modifiers::to_json(&rest));
        gn.bound.extend(moved);
        crate::picabia_bind::prune(gn);
    }
    Ok(Some(gid))
}

/// `modifier.apply` on a layer whose generator row is baked: its result computed when stale, then
/// [`bake`]. Refused when the generator draws the design (its warning says why). `budget`: the
/// layers the command may still create.
pub fn apply(s: &mut Session, id: NodeId, budget: &mut usize) -> Result<NodeId> {
    const CMD: &str = "modifier.apply";
    let doc = s.doc()?;
    let n = doc.get(id)?;
    if !store::is_fresh(doc, n) {
        compute_now(doc, id);
    }
    let Some((shape, _)) = store::lookup(doc, n) else {
        return Err(EngineError::Disabled(CMD.into(), format!("\"{}\" has no generator result to apply", n.name)));
    };
    if shape.keep_design {
        let why = info(doc, n).and_then(|(i, _)| i.warnings.first().cloned()).unwrap_or_else(|| "the generator made no result".into());
        return Err(EngineError::Disabled(CMD.into(), format!("\"{}\": {why}", n.name)));
    }
    let wanted = shape.items.len().max(1);
    if wanted > *budget {
        return Err(crate::bad(CMD, format!("applying would make {wanted} layers (at most {} at once)", crate::cmd::modifier::MAX_APPLY_LAYERS)));
    }
    *budget -= wanted;
    bake(s.doc_mut()?, id)?.ok_or_else(|| EngineError::Disabled(CMD.into(), format!("layer {id} has no generator result to apply")))
}
