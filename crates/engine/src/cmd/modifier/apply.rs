//! PICABIA: `modifier.apply`: bake a layer's modifier copies into real layers.
//!
//! Each source layer is replaced, in the same parent and at the same z position (an auto layout
//! frame places the group where the layer was), by a group named like the layer that holds one real
//! layer per copy, `Name 1…n`. Each is a deep copy of the source (new ids for the whole subtree,
//! through the same machinery as Duplicate, so instances stay linked and references inside the
//! copy point at the copy) with the copy's transform baked in (`instance * node.transform`) and
//! the stack taken off. The groups end up selected.
//!
//! `upTo` (an index, inclusive) bakes only `stack[0..=upTo]`; the rows after it move onto the new
//! group and keep running live (the recommended reading of "apply up to here": what was baked is
//! now one layer, and the rest of the recipe runs on it). Array rows repeat the whole previous set
//! as a block anyway, so they repeat the group exactly as they repeated the copies (a mirror's
//! axis, placed relative to the pivot, is shifted from the group's centre back to the source's
//! pivot). Still, the group's pivot is its own centre, so what
//! turns or scales *about* the pivot moves differently: an aligned circular array turns each block
//! about the group's centre, a linear array's rotate/scale each pivots there, a mirror across both
//! axes shifts one offset for both, and per-copy rows (transform each, randomize, wallpaper) now
//! act on the whole block, as a modifier on any group does.
//!
//! A layer whose stack makes no copies (empty, all rows off, or one unmoved copy) is left alone;
//! when no target has anything to apply the command is refused. Main components and layers inside
//! instances are refused: their copies can't become free layers there.

use figcraft_doc::{ChildLayout, Node, NodeId, NodeKind};
use figcraft_geom::{Affine, Vec2};
use figcraft_modifiers::{MirrorAxis, ModifierKind, PLUGIN_KEY, Stack};
use serde_json::{Value, json};

use super::{layers, read, write};
use crate::cmd::edit::{roots, z_sorted};
use crate::params::opt_index;
use crate::{EngineError, Result, Session, bad};

const CMD: &str = "modifier.apply";

/// Where a modifier param's variable binding is kept in `Node::bound`: `picabia.modifiers/<id>/<field>`.
const BIND_PREFIX: &str = "picabia.modifiers/";

/// The most layers one `modifier.apply` creates (copies × layers per copy), checked before copying.
pub const MAX_APPLY_LAYERS: usize = 100_000;

pub(super) fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    let up_to = opt_index(CMD, p, "upTo")?;
    let ids = layers(CMD, s, p)?;
    let ids = {
        let doc = s.doc()?;
        z_sorted(doc, &roots(doc, &ids))
    };
    let mut budget = MAX_APPLY_LAYERS;
    let mut applied = Vec::new();
    for id in ids {
        if let Some((group, copies)) = apply_one(s, id, up_to, &mut budget)? {
            applied.push((id, group, copies));
        }
    }
    if applied.is_empty() {
        return Err(EngineError::Disabled(CMD.into(), "no modifier copies to apply (no active modifiers)".into()));
    }
    let groups: Vec<NodeId> = applied.iter().map(|a| a.1).collect();
    s.set_selection(groups.clone());
    let rows: Vec<Value> = applied.iter().map(|(id, g, n)| json!({"id": id, "group": g, "copies": n})).collect();
    Ok(json!({"ids": groups, "applied": rows}))
}

/// Bake layer `id`: `Some((group, copies))`, or `None` when its stack makes no copies.
fn apply_one(s: &mut Session, id: NodeId, up_to: Option<usize>, budget: &mut usize) -> Result<Option<(NodeId, usize)>> {
    let doc = s.doc()?;
    let stack = read(CMD, doc, id)?;
    if stack.is_empty() {
        return Ok(None);
    }
    let last = stack.len() - 1;
    let cut = up_to.unwrap_or(last);
    if cut > last {
        return Err(bad(CMD, format!("layer {id} has {} modifiers: `upTo` is at most {last}", stack.len())));
    }
    let baked = Stack { modifiers: stack.modifiers.get(..=cut).map(<[_]>::to_vec).unwrap_or_default(), ..stack.clone() };
    let rest = Stack { modifiers: stack.modifiers.get(cut + 1..).map(<[_]>::to_vec).unwrap_or_default(), ..stack.clone() };
    let node = doc.get(id)?;
    let out = figcraft_modifiers::evaluate_with(&baked, figcraft_modifiers::node_bounds(node), &figcraft_modifiers::DocTargets::new(doc, id));
    let unmoved = matches!(out.instances.as_slice(), [one] if one.transform == Affine::IDENTITY);
    if out.instances.is_empty() || unmoved {
        return Ok(None);
    }
    let parent = node.parent.ok_or_else(|| bad(CMD, format!("{id} is a page; modifiers go on layers")))?;
    let refuse = |why: &str| Err(bad(CMD, format!("\"{}\" {why}", node.name)));
    if matches!(node.kind, NodeKind::Component { .. } | NodeKind::ComponentSet { .. }) {
        return refuse("is a main component: apply its modifiers on an instance of it instead");
    }
    if crate::components::owning_instance(doc, parent).is_some() {
        return refuse("is inside an instance: edit its main component, or detach the instance");
    }
    let per_copy = doc.subtree(id).len().max(1);
    let wanted = per_copy.saturating_mul(out.instances.len());
    if wanted > *budget {
        return Err(bad(CMD, format!("applying would make {wanted} layers (at most {MAX_APPLY_LAYERS} at once)")));
    }
    *budget -= wanted;
    let (name, own, child_layout) = (node.name.clone(), node.transform, node.child_layout.clone());
    // Variable bindings of the rows that stay live go with them to the group.
    let kept: Vec<String> = rest.modifiers.iter().map(|m| format!("{BIND_PREFIX}{}/", m.id)).collect();
    let moved: Vec<_> =
        node.bound.iter().filter(|(k, _)| kept.iter().any(|p| k.starts_with(p.as_str()))).map(|(k, v)| (k.clone(), v.clone())).collect();
    let src_pivot = figcraft_modifiers::node_bounds(node).center();
    let index = doc.index_in_parent(id);

    let doc = s.doc_mut()?;
    let gid = doc.alloc_id();
    let mut g = Node::new(gid, NodeKind::Group, name.clone(), 0.0, 0.0);
    g.child_layout = child_layout;
    doc.insert(g, parent, index)?;
    for (k, inst) in out.instances.iter().enumerate() {
        let c = crate::components::copy_in_file(doc, id, gid, None)?;
        let n = doc.get_mut(c)?;
        n.transform = inst.transform * own;
        n.name = format!("{name} {}", k + 1);
        n.plugin_data.remove(PLUGIN_KEY);
        n.bound.retain(|k, _| !k.starts_with(BIND_PREFIX));
        n.child_layout = ChildLayout::default();
    }
    doc.remove(id)?;
    doc.fit_to_children(gid)?;
    doc.fit_ancestors(gid);
    figcraft_layout::coherent_sizing(doc, gid);
    if !rest.is_empty() {
        let pivot = figcraft_modifiers::node_bounds(doc.get(gid)?).center();
        let rest = repivot(rest, src_pivot - pivot);
        write(s, gid, &rest)?;
        s.doc_mut()?.get_mut(gid)?.bound.extend(moved);
    }
    Ok(Some((gid, out.instances.len())))
}

/// Shift the pivot-relative params of `stack` by `d` (the old pivot minus the new one), so its
/// rows place copies as they did around the old pivot. Only a mirror's axis can follow: the other
/// kinds either don't depend on the pivot (translations) or turn about it, which no param moves.
fn repivot(mut stack: Stack, d: Vec2) -> Stack {
    if !(d.x.is_finite() && d.y.is_finite()) {
        return stack;
    }
    for m in &mut stack.modifiers {
        if let ModifierKind::Mirror(mirror) = &mut m.kind {
            mirror.offset += match mirror.axis {
                MirrorAxis::Vertical | MirrorAxis::Both => d.x,
                MirrorAxis::Horizontal => d.y,
            };
        }
    }
    stack
}
