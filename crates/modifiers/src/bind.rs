//! Variable bindings on modifier params.
//!
//! A binding lives in FigCraft's own `Node.bound` map under the key
//! `picabia.modifiers/<modifier id>/<field>` (field = the param's camelCase name, as in the file),
//! so files, undo, collaboration and FigCraft's `variable.bind` command carry it unchanged. Keyed by
//! the modifier's id, not its position: reordering the stack keeps it. When the bindings are
//! resolved (FigCraft's normalisation, after every edit and every variable or mode change) the
//! engine hands the Picabia keys to [`apply_bound_values`], which writes the values into the stack
//! JSON in plugin data.

use std::collections::BTreeMap;

use figcraft_doc::{Node, VariableType, VariableValue};
use serde_json::{Map, Value};

use crate::model::{ModifierKind, Stack};
use crate::{PLUGIN_KEY, parse, to_json};

/// The prefix of every Picabia binding key in `Node.bound`.
pub const BINDING_PREFIX: &str = "picabia.modifiers/";

/// The binding key of `field` of modifier `modifier` (`picabia.modifiers/m1/count`).
pub fn binding_key(modifier: &str, field: &str) -> String {
    format!("{BINDING_PREFIX}{modifier}/{field}")
}

/// `(modifier id, field)` of a Picabia binding key, or `None` for any other key.
pub fn parse_binding_key(key: &str) -> Option<(&str, &str)> {
    let (modifier, field) = key.strip_prefix(BINDING_PREFIX)?.split_once('/')?;
    (!modifier.is_empty() && !field.is_empty() && !field.contains('/')).then_some((modifier, field))
}

/// Whether `key` is a Picabia binding key (has [`BINDING_PREFIX`]).
pub fn is_binding_key(key: &str) -> bool {
    key.starts_with(BINDING_PREFIX)
}

/// The variable id bound to `field` of modifier `modifier` on `node`, if any: what the panel shows
/// as the field's variable chip.
pub fn bound_variable<'a>(node: &'a Node, modifier: &str, field: &str) -> Option<&'a str> {
    node.bound.get(&binding_key(modifier, field)).map(String::as_str)
}

/// The variable type a param takes, from its value in `kind`'s params: numbers take number
/// variables, flags boolean ones, choices (`axis`, `group`) string ones naming a choice. `None`
/// when `kind` has no such param, or it can't be bound (`<field>Expr` texts, a copy-to-shape
/// `target`, which is checked for cycles when set).
pub fn field_type(kind: &ModifierKind, field: &str) -> Option<VariableType> {
    if matches!(kind, ModifierKind::CopyToShape(_)) && field == "target" {
        return None;
    }
    match kind.params().get(field)? {
        Value::Number(_) => Some(VariableType::Number),
        Value::Bool(_) => Some(VariableType::Boolean),
        Value::String(_) if !field.ends_with("Expr") => Some(VariableType::String),
        _ => None,
    }
}

/// Whether binding key `key` on `node` can take a variable of type `kind` (`variable.bind`'s
/// check): the layer's stack has the modifier, the modifier has the param, and the types match.
/// `None` for keys that aren't Picabia's.
pub fn binding_takes(node: &Node, key: &str, kind: VariableType) -> Option<bool> {
    if !is_binding_key(key) {
        return None;
    }
    let ok = || -> Option<bool> {
        let (modifier, field) = parse_binding_key(key)?;
        let stack = parse(node.plugin_data.get(PLUGIN_KEY)?).ok()?;
        let m = stack.modifiers.get(stack.position(modifier)?)?;
        Some(field_type(&m.kind, field)? == kind)
    };
    Some(ok().unwrap_or(false))
}

/// The stack source `src` with the bound `values` (Picabia binding keys → resolved values) written
/// into their modifiers' params, or `None` when nothing changes (no such modifier or param, a
/// value of the wrong type or out of range, or already in place). Values that don't fit are
/// skipped, never an error: a binding never breaks the stack.
pub fn apply_bound_values(src: &str, values: &[(String, VariableValue)]) -> Option<String> {
    let mut stack = parse(src).ok()?;
    let mut changed = false;
    for (key, val) in values {
        let Some((modifier, field)) = parse_binding_key(key) else { continue };
        let Some(m) = stack.position(modifier).and_then(|at| stack.modifiers.get_mut(at)) else { continue };
        let v = match val {
            VariableValue::Number(x) if x.is_finite() => serde_json::Number::from_f64(*x).map(Value::Number),
            VariableValue::Boolean(b) => Some(Value::Bool(*b)),
            VariableValue::String(s) => Some(Value::String(s.clone())),
            _ => None,
        };
        let Some(v) = v else { continue };
        if field_type(&m.kind, field).is_none() {
            continue;
        }
        let before = m.clone();
        let mut props = Map::new();
        props.insert(field.to_string(), v);
        if m.set_props(&props).is_ok() && *m != before {
            changed = true;
        }
    }
    changed.then(|| to_json(&stack))
}

/// The Picabia binding keys of `bound` whose modifier isn't in `stack` (or there's no stack):
/// what removing a modifier drops.
pub fn stale_binding_keys<V>(bound: &BTreeMap<String, V>, stack: Option<&Stack>) -> Vec<String> {
    bound
        .keys()
        .filter(|k| is_binding_key(k))
        .filter(|k| match (parse_binding_key(k), stack) {
            (Some((modifier, _)), Some(st)) => st.position(modifier).is_none(),
            _ => true,
        })
        .cloned()
        .collect()
}
