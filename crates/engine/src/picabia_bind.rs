//! PICABIA: variable bindings on modifier params, engine side.
//!
//! Bindings are FigCraft's own: `Node.bound` keys `picabia.modifiers/<modifier id>/<field>` made
//! with `variable.bind` (see `figcraft_modifiers::bind`). FigCraft's binding resolution
//! (`cmd::styles::apply_bindings_matching`, one seam) hands those keys to [`apply_bindings`]; the
//! `modifier.*` commands call [`prune`] when rows go and [`detach_edited`] when a bound param is set
//! by hand.

use std::collections::BTreeMap;

use figcraft_doc::{CollectionId, Document, ModeId, Node, NodeId, VariableId, VariableValue};
use figcraft_modifiers::{PLUGIN_KEY, binding_key};
use serde_json::{Map, Value};

/// Resolve layer `id`'s modifier bindings `binds` (key → variable id) whose key passes `include`
/// in `modes` and write the values into its stack. The layer is left untouched (still shared with
/// undo snapshots) when every value is already in place.
pub(crate) fn apply_bindings(
    doc: &mut Document,
    id: NodeId,
    modes: &BTreeMap<CollectionId, ModeId>,
    binds: &[(String, VariableId)],
    include: &dyn Fn(&str) -> bool,
) {
    let values: Vec<(String, VariableValue)> =
        binds.iter().filter(|(k, _)| include(k)).filter_map(|(k, v)| Some((k.clone(), doc.variables.resolve(v, modes)?))).collect();
    if values.is_empty() {
        return;
    }
    let Some(src) = doc.node(id).and_then(|n| n.plugin_data.get(PLUGIN_KEY)) else { return };
    let Some(next) = figcraft_modifiers::apply_bound_values(src, &values) else { return };
    if let Ok(n) = doc.get_mut(id) {
        n.plugin_data.insert(PLUGIN_KEY.to_string(), next);
    }
}

/// Drop `n`'s modifier bindings whose modifier is no longer in its stack (all of them without a
/// stack). True when one went.
pub(crate) fn prune(n: &mut Node) -> bool {
    let stack = n.plugin_data.get(PLUGIN_KEY).and_then(|src| figcraft_modifiers::parse(src).ok());
    let stale = figcraft_modifiers::stale_binding_keys(&n.bound, stack.as_ref());
    for k in &stale {
        n.bound.remove(k);
    }
    !stale.is_empty()
}

/// An explicit edit sticks, as for FigCraft's own fields (#809): drop the bindings of the params of
/// modifier `modifier` whose value changed from `before` to `after` (the kind's params).
pub(crate) fn detach_edited(n: &mut Node, modifier: &str, before: &Map<String, Value>, after: &Map<String, Value>) {
    if n.bound.is_empty() {
        return;
    }
    for (field, v) in after {
        if before.get(field) != Some(v) {
            n.bound.remove(&binding_key(modifier, field));
        }
    }
}
