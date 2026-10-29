//! PICABIA: layer ids inside Picabia modifier stacks follow copies.
//!
//! A Picabia `copyToShape` modifier names another layer in its stack (`plugin_data` under
//! [`MODIFIERS_KEY`], `{"stack":[{"kind":"copyToShape","target":"12",..}]}`). Upstream copying
//! (duplicate, paste, component instances) maps ids but doesn't look inside plugin data, so the
//! copying paths call [`remap_modifier_targets`] with their id map: a target copied together with
//! its source points at the copy; one left behind keeps pointing at the original.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{Document, Node, NodeId};

/// The plugin-data key Picabia keeps a layer's modifier stack under.
pub const MODIFIERS_KEY: &str = "picabia.modifiers";
/// Stacks longer than this aren't rewritten (Picabia refuses to read them anyway).
const MAX_STACK_BYTES: usize = 256 * 1024;

/// Point the copy-to-shape targets in `n`'s stack that `map` covers at their copies. Damaged or
/// unrelated data is left alone; the text is rewritten only when a target changed.
pub fn remap_modifier_targets(n: &mut Node, map: &BTreeMap<NodeId, NodeId>) {
    let Some(src) = n.plugin_data.get(MODIFIERS_KEY) else { return };
    if src.len() > MAX_STACK_BYTES || !src.contains("copyToShape") {
        return;
    }
    let Ok(mut v) = serde_json::from_str::<Value>(src) else { return };
    let mut changed = false;
    for m in v.get_mut("stack").and_then(Value::as_array_mut).into_iter().flatten() {
        let Some(o) = m.as_object_mut() else { continue };
        if o.get("kind").and_then(Value::as_str) != Some("copyToShape") {
            continue;
        }
        let old = match o.get("target") {
            Some(Value::String(s)) => s.parse::<NodeId>().ok(),
            Some(Value::Number(x)) => x.as_u64().map(NodeId),
            _ => None,
        };
        if let Some(new) = old.and_then(|t| map.get(&t)) {
            o.insert("target".into(), Value::String(new.to_string()));
            changed = true;
        }
    }
    if changed && let Ok(s) = serde_json::to_string(&v) {
        n.plugin_data.insert(MODIFIERS_KEY.into(), s);
    }
}

/// [`remap_modifier_targets`] on the layers `ids` of `doc` that have a stack (others aren't
/// touched, so they stay shared with earlier versions of the document).
pub fn remap_targets_in(doc: &mut Document, ids: impl IntoIterator<Item = NodeId>, map: &BTreeMap<NodeId, NodeId>) {
    for id in ids {
        if doc.node(id).is_some_and(|n| n.plugin_data.get(MODIFIERS_KEY).is_some_and(|s| s.contains("copyToShape")))
            && let Some(n) = doc.node_mut(id)
        {
            remap_modifier_targets(n, map);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    fn stack(target: &str) -> String {
        format!(r#"{{"v":1,"stack":[{{"id":"m1","kind":"copyToShape","target":{target}}},{{"id":"m2","kind":"linearArray","target":"5"}}]}}"#)
    }

    #[test]
    fn targets_in_the_map_follow_and_others_stay() {
        let mut n = Node::new(NodeId(1), NodeKind::Rectangle, "R", 10.0, 10.0);
        let map = BTreeMap::from([(NodeId(5), NodeId(50))]);
        for (t, want) in [("\"5\"", Some("50")), ("5", Some("50")), ("\"0:5\"", Some("50")), ("\"6\"", None)] {
            n.plugin_data.insert(MODIFIERS_KEY.into(), stack(t));
            let before = n.plugin_data[MODIFIERS_KEY].clone();
            remap_modifier_targets(&mut n, &map);
            let v: Value = serde_json::from_str(&n.plugin_data[MODIFIERS_KEY]).unwrap();
            match want {
                Some(w) => assert_eq!(v["stack"][0]["target"], w),
                None => assert_eq!(n.plugin_data[MODIFIERS_KEY], before, "unchanged text"),
            }
            // Only copy-to-shape rows are touched.
            assert_eq!(v["stack"][1]["target"], "5");
        }
        for junk in ["", "{", "[]", r#"{"stack":7}"#, r#"{"stack":[1,"copyToShape",null]}"#] {
            n.plugin_data.insert(MODIFIERS_KEY.into(), junk.into());
            remap_modifier_targets(&mut n, &map);
            assert_eq!(n.plugin_data[MODIFIERS_KEY], junk);
        }
    }
}
