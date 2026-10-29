//! PICABIA: modifier stack commands (`modifier.*`).
//!
//! A layer's stack lives in `plugin_data["picabia.modifiers"]` (see `figcraft_modifiers`), so
//! files, undo, duplicate, copy/paste, collaboration and branches carry it like any layer data.
//! Every editing command is one undo step; `ids` (or `id`) default to the selection and name the
//! layers, as everywhere in FigCraft. A modifier in a stack is picked with `modifier` (its id,
//! e.g. `"m1"`) or `index` (0 = the top row, which runs first).
//!
//! A layer whose modifier data is damaged refuses edits (its data is kept) until
//! `modifier.remove_all` clears it.

use figcraft_doc::{Document, NodeId};
use figcraft_modifiers::{Modifier, PLUGIN_KEY, Stack};
use serde_json::{Map, Value, json};

use super::{CommandSpec, ok};
use crate::params::{boolean, get, opt_index, req_index, req_targets, string, targets};
use crate::{EngineError, Result, Session, bad, spec};

mod apply;
#[cfg(test)]
mod apply_tests;

pub use apply::MAX_APPLY_LAYERS;

const KIND_LIST: &str = "linearArray|circularArray|gridArray|mirror|transformEach|randomize";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "modifier.add",
            "Add modifier",
            "{ids?, kind (linearArray|circularArray|gridArray|mirror|transformEach|randomize), props? {count?, offsetX?, ..: the kind's params, camelCase; points, degrees, scale factors}, index? (where in the stack; default the end, which runs last)} → {modifier (the new id on the first layer), added: [{id, modifier}]}",
            true,
            add
        ),
        spec!("modifier.remove", "Remove modifier", "{ids?, modifier? (id, e.g. m1) | index? (0 = top row)}", true, remove),
        spec!("modifier.move", "Move modifier", "{ids?, from (index), to (index)}", true, move_modifier),
        spec!(
            "modifier.set",
            "Set modifier",
            "{ids?, modifier? (id) | index?, props {..: params to change, camelCase; enabled too; <field>Expr: a per-copy expression on transformEach/randomize amounts (i, n, t, value; \"\" clears)}}",
            true,
            set
        ),
        spec!("modifier.toggle", "Toggle modifier", "{ids?, modifier? (id) | index?, enabled? (default: flip)}", true, toggle),
        spec!("modifier.copy", "Copy modifiers", "{id?, modifier? (id) | index? (one row; without: the whole stack)} → {copied}", false, copy),
        spec!(
            "modifier.paste",
            "Paste modifiers",
            "{ids?, replace? (replace the stacks; default: add below the existing rows)} → {pasted}",
            true,
            paste
        ),
        spec!(
            "modifier.list",
            "List modifiers",
            "{id?} → {id, modifiers: [{id, enabled, kind, label, ..params}], instances (copies drawn; 0 = drawn normally), warnings: [{modifier?, message}], error? (damaged data)}",
            false,
            list
        ),
        spec!("modifier.remove_all", "Remove all modifiers", "{ids?}", true, remove_all),
        spec!(
            "modifier.apply",
            "Apply modifiers",
            "{ids?, upTo? (index, inclusive: bake only the rows down to it; the rows after it move onto the new group and stay live)} (each layer becomes a group of real layers, one per copy, named `Name 1…n`, in its place) → {ids (the groups, selected), applied: [{id, group, copies}]}",
            true,
            apply::apply
        ),
    ]
}

/// Read-only lines for `node`'s enabled modifiers, top row first: the kind's English label (a UI
/// catalog key) and how many copies it makes where that reads plainly (`("Linear array", "×5")`,
/// `("Grid array", "3 × 4")`, `("Mirror", "")`). Empty without modifiers or with damaged data.
/// The Inspect tab and accessibility labels show these.
pub fn summary(node: &figcraft_doc::Node) -> Vec<(String, String)> {
    use figcraft_modifiers::ModifierKind as K;
    let Some(Ok(stack)) = figcraft_modifiers::node_stack(node) else { return Vec::new() };
    let cap = |n: u32| n.clamp(1, figcraft_modifiers::MAX_COUNT);
    stack
        .modifiers
        .iter()
        .filter(|m| m.enabled)
        .map(|m| {
            let amount = match &m.kind {
                K::LinearArray(p) => format!("×{}", cap(p.count)),
                K::CircularArray(p) => format!("×{}", cap(p.count)),
                K::GridArray(p) => format!("{} × {}", cap(p.rows), cap(p.cols)),
                _ => String::new(),
            };
            (m.label().to_string(), amount)
        })
        .collect()
}

/// The layers a command edits: `ids`/`id` or the selection; pages are refused.
fn layers(cmd: &str, s: &Session, p: &Value) -> Result<Vec<NodeId>> {
    let ids = req_targets(cmd, s, p)?;
    let doc = s.doc()?;
    for id in &ids {
        if doc.get(*id)?.is_page() {
            return Err(bad(cmd, format!("{id} is a page; modifiers go on layers")));
        }
    }
    Ok(ids)
}

/// Layer `id`'s stack (empty without modifier data); damaged data is refused, not overwritten.
fn read(cmd: &str, doc: &Document, id: NodeId) -> Result<Stack> {
    match doc.get(id)?.plugin_data.get(PLUGIN_KEY) {
        None => Ok(Stack::default()),
        Some(src) => figcraft_modifiers::parse(src).map_err(|e| bad(cmd, format!("layer {id}: {e} (modifier.remove_all clears it)"))),
    }
}

/// Store `stack` on layer `id` (an empty stack removes the data).
fn write(s: &mut Session, id: NodeId, stack: &Stack) -> Result<()> {
    let n = s.doc_mut()?.get_mut(id)?;
    if stack.is_empty() {
        n.plugin_data.remove(PLUGIN_KEY);
    } else {
        n.plugin_data.insert(PLUGIN_KEY.to_string(), figcraft_modifiers::to_json(stack));
    }
    Ok(())
}

/// Read, change with `f`, and write back every target's stack.
fn edit(cmd: &str, s: &mut Session, p: &Value, mut f: impl FnMut(&mut Stack, NodeId) -> Result<()>) -> Result<()> {
    for id in layers(cmd, s, p)? {
        let mut stack = read(cmd, s.doc()?, id)?;
        f(&mut stack, id)?;
        write(s, id, &stack)?;
    }
    // Variable bindings of rows that went go too.
    crate::picabia_bind::prune(n);
    Ok(())
}

/// Which modifier `p` names (`modifier` id or `index`) in `stack`, on layer `id`.
fn pick(cmd: &str, p: &Value, stack: &Stack, id: NodeId) -> Result<usize> {
    let at = match (get(p, "modifier"), opt_index(cmd, p, "index")?) {
        (Some(m), None) => {
            let m = m.as_str().ok_or_else(|| bad(cmd, "`modifier` must be a modifier id such as \"m1\""))?;
            stack.position(m).ok_or_else(|| bad(cmd, format!("layer {id} has no modifier `{m}`")))?
        }
        (None, Some(i)) => i,
        (Some(_), Some(_)) => return Err(bad(cmd, "pass `modifier` or `index`, not both")),
        (None, None) => return Err(bad(cmd, "`modifier` (an id) or `index` required")),
    };
    if at >= stack.len() {
        return Err(bad(cmd, format!("layer {id} has no modifier at index {at} ({} in its stack)", stack.len())));
    }
    Ok(at)
}

fn props_of<'a>(cmd: &str, p: &'a Value, required: bool) -> Result<Option<&'a Map<String, Value>>> {
    match get(p, "props") {
        None if required => Err(bad(cmd, "`props` required")),
        None => Ok(None),
        Some(Value::Object(o)) => Ok(Some(o)),
        Some(_) => Err(bad(cmd, "`props` must be an object")),
    }
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "modifier.add";
    let kind = string(p, "kind").ok_or_else(|| bad(CMD, format!("`kind` required ({KIND_LIST})")))?;
    let mut template = Modifier::new("", &kind).ok_or_else(|| bad(CMD, format!("unknown kind `{kind}` (one of {KIND_LIST})")))?;
    if let Some(props) = props_of(CMD, p, false)? {
        template.set_props(props).map_err(|e| bad(CMD, e.to_string()))?;
    }
    let at = opt_index(CMD, p, "index")?;
    let mut added = Vec::new();
    edit(CMD, s, p, |stack, id| {
        let m = stack.insert(at.unwrap_or(usize::MAX), template.clone()).map_err(|e| bad(CMD, format!("layer {id}: {e}")))?;
        added.push(json!({"id": id, "modifier": m.id}));
        Ok(())
    })?;
    let first = added.first().map(|a| a["modifier"].clone()).unwrap_or(Value::Null);
    Ok(json!({"modifier": first, "added": added}))
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "modifier.remove";
    edit(CMD, s, p, |stack, id| {
        let at = pick(CMD, p, stack, id)?;
        stack.modifiers.remove(at);
        Ok(())
    })?;
    ok()
}

fn move_modifier(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "modifier.move";
    let (from, to) = (req_index(CMD, p, "from")?, req_index(CMD, p, "to")?);
    edit(CMD, s, p, |stack, id| {
        let n = stack.len();
        if from >= n || to >= n {
            return Err(bad(CMD, format!("layer {id} has {n} modifiers (indexes 0..{})", n.saturating_sub(1))));
        }
        let m = stack.modifiers.remove(from);
        stack.modifiers.insert(to, m);
        Ok(())
    })?;
    ok()
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "modifier.set";
    let props = props_of(CMD, p, true)?.cloned().unwrap_or_default();
    let mut edited = Vec::new();
    edit(CMD, s, p, |stack, id| {
        let at = pick(CMD, p, stack, id)?;
        let m = stack.modifiers.get_mut(at).ok_or_else(|| bad(CMD, "no such modifier"))?;
        let before = m.kind.params();
        m.set_props(&props).map_err(|e| bad(CMD, format!("layer {id}: {e}")))?;
        edited.push((id, m.id.clone(), before, m.kind.params()));
        Ok(())
    })?;
    // A param set by hand drops its variable binding, as FigCraft's own fields do.
    for (id, m, before, after) in edited {
        crate::picabia_bind::detach_edited(s.doc_mut()?.get_mut(id)?, &m, &before, &after);
    }
    ok()
}

fn toggle(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "modifier.toggle";
    let enabled = match get(p, "enabled") {
        None => None,
        Some(_) => Some(boolean(p, "enabled").ok_or_else(|| bad(CMD, "`enabled` must be true or false"))?),
    };
    edit(CMD, s, p, |stack, id| {
        let at = pick(CMD, p, stack, id)?;
        let m = stack.modifiers.get_mut(at).ok_or_else(|| bad(CMD, "no such modifier"))?;
        m.enabled = enabled.unwrap_or(!m.enabled);
        Ok(())
    })?;
    ok()
}

fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "modifier.copy";
    let id = targets(s, p).first().copied().ok_or_else(|| bad(CMD, "select a layer"))?;
    let stack = read(CMD, s.doc()?, id)?;
    let rows: Vec<Value> = if get(p, "modifier").is_some() || get(p, "index").is_some() {
        let at = pick(CMD, p, &stack, id)?;
        stack.modifiers.get(at).map(Modifier::to_value).into_iter().collect()
    } else {
        stack.modifiers.iter().map(Modifier::to_value).collect()
    };
    if rows.is_empty() {
        return Err(EngineError::Disabled(CMD.into(), format!("layer {id} has no modifiers")));
    }
    let copied = rows.len();
    s.clipboard.picabia_modifiers = rows;
    Ok(json!({"copied": copied}))
}

fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "modifier.paste";
    if s.clipboard.picabia_modifiers.is_empty() {
        return Err(EngineError::Disabled(CMD.into(), "no copied modifiers".into()));
    }
    let rows = s.clipboard.picabia_modifiers.iter().map(Modifier::from_value).collect::<std::result::Result<Vec<_>, _>>();
    let rows = rows.map_err(|e| bad(CMD, e.to_string()))?;
    let replace = boolean(p, "replace").unwrap_or(false);
    edit(CMD, s, p, |stack, id| {
        if replace {
            stack.modifiers.clear();
        }
        for m in &rows {
            stack.insert(usize::MAX, m.clone()).map_err(|e| bad(CMD, format!("layer {id}: {e}")))?;
        }
        Ok(())
    })?;
    Ok(json!({"pasted": rows.len()}))
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "modifier.list";
    let id = targets(s, p).first().copied().ok_or_else(|| bad(CMD, "select a layer or pass `id`"))?;
    let node = s.doc()?.get(id)?;
    let mut out = json!({"id": id, "modifiers": [], "instances": 0, "warnings": []});
    match figcraft_modifiers::node_stack(node) {
        None => {}
        Some(Err(e)) => out["error"] = json!(e.to_string()),
        Some(Ok(stack)) => {
            let rows: Vec<Value> = stack
                .modifiers
                .iter()
                .map(|m| {
                    let mut v = m.to_value();
                    v["label"] = json!(m.label());
                    v
                })
                .collect();
            out["modifiers"] = json!(rows);
            let o = figcraft_modifiers::evaluate(&stack, figcraft_modifiers::node_bounds(node));
            out["instances"] = json!(o.instances.len());
            out["warnings"] = json!(o.warnings.iter().map(|w| json!({"modifier": w.modifier, "message": w.message})).collect::<Vec<_>>());
        }
    }
    Ok(out)
}

fn remove_all(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "modifier.remove_all";
    for id in layers(CMD, s, p)? {
        if s.doc()?.get(id)?.plugin_data.contains_key(PLUGIN_KEY) {
            let n = s.doc_mut()?.get_mut(id)?;
            n.plugin_data.remove(PLUGIN_KEY);
            crate::picabia_bind::prune(n);
        }
    }
    ok()
}
