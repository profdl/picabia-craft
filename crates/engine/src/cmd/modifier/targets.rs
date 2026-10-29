//! PICABIA: copy-to-shape targets in the `modifier.*` commands, and modifier bindings carried by
//! `modifier.copy`/`modifier.paste`/`modifier.duplicate`.
//!
//! Every stack edit ([`super::edit`]) checks the copy-to-shape targets it sets or re-enables
//! ([`check_changed`]): the source itself, an ancestor or descendant, another page, a missing layer
//! or a loop back through other targets is refused (`figcraft_modifiers::check_target`). Targets
//! already in a stack aren't re-checked, so a deleted target doesn't block unrelated edits (it
//! places no copies, with a warning). `modifier.targets` lists the layers a layer can copy onto,
//! for the Design panel's target picker.

use std::collections::BTreeSet;

use figcraft_doc::{Document, NodeId};
use figcraft_modifiers::{Modifier, Stack, binding_key, field_type, parse_binding_key};
use serde_json::{Map, Value, json};

use super::{pick, read, write};
use crate::params::{get, targets as target_ids};
use crate::{Result, Session, bad};

/// The field a copied row carries its variable bindings in (`{field: variable id}`); taken off
/// before the row is read back.
pub(super) const BOUND_FIELD: &str = "__picabiaBound";

/// The enabled copy-to-shape `(modifier id, target)` pairs of `stack`.
fn pairs(stack: &Stack) -> BTreeSet<(String, String)> {
    stack.targets().map(|(m, t)| (m.to_string(), t.to_string())).collect()
}

/// Refuse the copy-to-shape targets `after` sets on layer `id` that `before` didn't have.
pub(super) fn check_changed(cmd: &str, doc: &Document, id: NodeId, before: &Stack, after: &Stack) -> Result<()> {
    let old = pairs(before);
    for (m, t) in pairs(after).difference(&old) {
        let target = figcraft_modifiers::parse_target(t).map_err(|e| bad(cmd, format!("layer {id}, modifier {m}: {e}")))?;
        figcraft_modifiers::check_target(doc, id, target).map_err(|e| bad(cmd, format!("layer {id}, modifier {m}: {e}")))?;
    }
    Ok(())
}

/// What the panel's target picker shows for layer `source`: the layers it can copy onto, as
/// `(id, name)` in layer order (see `figcraft_modifiers::valid_targets`).
pub fn target_choices(doc: &Document, source: NodeId) -> Vec<(NodeId, String)> {
    figcraft_modifiers::valid_targets(doc, source).into_iter().filter_map(|t| Some((t, doc.node(t)?.name.clone()))).collect()
}

/// Whether layer `source` can copy onto `target`; `Err` is the reason (for the picker's tooltip or
/// the panel's warning).
pub fn check_target(doc: &Document, source: NodeId, target: NodeId) -> std::result::Result<(), String> {
    figcraft_modifiers::check_target(doc, source, target)
}

/// `modifier.targets {id?, modifier? | index?}`: the layers `id` (default: the first selected) can
/// copy onto, and, for a picked copy-to-shape row, its current target and whether it is valid.
pub(super) fn list_targets(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "modifier.targets";
    let id = target_ids(s, p).first().copied().ok_or_else(|| bad(CMD, "select a layer or pass `id`"))?;
    let doc = s.doc()?;
    doc.get(id)?;
    let rows: Vec<Value> = target_choices(doc, id)
        .into_iter()
        .map(|(t, name)| json!({"id": t, "name": name, "kind": doc.node(t).map(|n| n.kind.label()).unwrap_or_default()}))
        .collect();
    let mut out = json!({"id": id, "targets": rows});
    if get(p, "modifier").is_some() || get(p, "index").is_some() {
        let stack = read(CMD, doc, id)?;
        let at = pick(CMD, p, &stack, id)?;
        if let Some(Modifier { kind: figcraft_modifiers::ModifierKind::CopyToShape(c), .. }) = stack.modifiers.get(at) {
            out["target"] = json!(c.target);
            let check = figcraft_modifiers::parse_target(&c.target).and_then(|t| check_target(doc, id, t));
            out["valid"] = json!(check.is_ok());
            if let Err(e) = check {
                out["reason"] = json!(e);
            }
        }
    }
    Ok(out)
}

/// `rows` (copied from layer `id`'s stack) with each row's bindings in [`BOUND_FIELD`].
pub(super) fn attach_bindings(doc: &Document, id: NodeId, rows: &mut [Value]) {
    let Some(n) = doc.node(id) else { return };
    for row in rows {
        let Some(mid) = row.get("id").and_then(Value::as_str).map(str::to_string) else { continue };
        let bound: Map<String, Value> = n
            .bound
            .iter()
            .filter_map(|(k, v)| match parse_binding_key(k) {
                Some((m, field)) if m == mid => Some((field.to_string(), Value::String(v.clone()))),
                _ => None,
            })
            .collect();
        if !bound.is_empty()
            && let Some(o) = row.as_object_mut()
        {
            o.insert(BOUND_FIELD.into(), Value::Object(bound));
        }
    }
}

/// A copied row without its carried bindings, and those bindings (`(field, variable id)`).
pub(super) fn split_bindings(row: &Value) -> (Value, Vec<(String, String)>) {
    let mut row = row.clone();
    let bound = row.as_object_mut().and_then(|o| o.remove(BOUND_FIELD));
    let pairs = match bound {
        Some(Value::Object(m)) => m.into_iter().filter_map(|(f, v)| Some((f, v.as_str()?.to_string()))).collect(),
        _ => Vec::new(),
    };
    (row, pairs)
}

/// Bind modifier `m` (now in layer `id`'s stack) to `bound` (`(field, variable id)`), skipping
/// fields its kind doesn't take.
pub(super) fn bind(s: &mut Session, id: NodeId, m: &Modifier, bound: &[(String, String)]) -> Result<()> {
    if bound.is_empty() {
        return Ok(());
    }
    let n = s.doc_mut()?.get_mut(id)?;
    for (field, var) in bound {
        if field_type(&m.kind, field).is_some() {
            n.bound.insert(binding_key(&m.id, field), var.clone());
        }
    }
    Ok(())
}

/// `modifier.duplicate {ids?, modifier? | index?}`: a copy of the row right below it, with a fresh
/// id and the row's variable bindings.
pub(super) fn duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "modifier.duplicate";
    let mut made = Vec::new();
    for id in super::layers(CMD, s, p)? {
        let mut stack = read(CMD, s.doc()?, id)?;
        let at = pick(CMD, p, &stack, id)?;
        let Some(orig) = stack.modifiers.get(at).cloned() else { continue };
        let mut row = vec![orig.to_value()];
        attach_bindings(s.doc()?, id, &mut row);
        let (_, bound) = split_bindings(row.first().unwrap_or(&Value::Null));
        let mut copy = orig;
        copy.id = String::new();
        let m = stack.insert(at + 1, copy).map_err(|e| bad(CMD, format!("layer {id}: {e}")))?.clone();
        write(s, id, &stack)?;
        bind(s, id, &m, &bound)?;
        made.push(json!({"id": id, "modifier": m.id}));
    }
    let first = made.first().map(|a| a["modifier"].clone()).unwrap_or(Value::Null);
    Ok(json!({"modifier": first, "duplicated": made}))
}
