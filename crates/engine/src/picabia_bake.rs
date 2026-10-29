//! Picabia: exports to formats that don't know modifier stacks (`.fig`, Penpot) get the copies baked.
//!
//! A layer with an active stack becomes a group of its copies (each a deep copy of the layer with
//! the instance transform applied, its stack taken off), in the layer's place. Inside an auto
//! layout frame the layer stays where it is (copies never take part in auto layout) and the other
//! copies follow it as absolutely positioned siblings. The export lists a warning saying so.

use std::borrow::Cow;

use figcraft_doc::{Document, NodeId, NodeKind};

/// The most layers one export bakes (each makes up to `MAX_INSTANCES_PER_NODE` copies).
const MAX_BAKED_LAYERS: usize = 10_000;
/// The most copies one export makes in all.
const MAX_BAKED_COPIES: usize = 200_000;

/// `doc` with every modifier stack baked into copies (borrowed unchanged when it has none), and the
/// export warnings that go with it.
pub fn baked(doc: &Document) -> (Cow<'_, Document>, Vec<String>) {
    let mut stacked: Vec<(usize, NodeId)> = doc
        .ids()
        .filter(|id| doc.node(*id).and_then(figcraft_render::picabia::instance_transforms).is_some())
        .map(|id| (doc.ancestors(id).len(), id))
        .collect();
    if stacked.is_empty() {
        return (Cow::Borrowed(doc), Vec::new());
    }
    // Deepest first: a stacked group then copies layers already baked.
    stacked.sort_by_key(|(depth, id)| (std::cmp::Reverse(*depth), *id));
    let mut out = doc.clone();
    let (mut layers, mut copies, mut skipped) = (0usize, 0usize, 0usize);
    for (_, id) in stacked {
        if layers >= MAX_BAKED_LAYERS || copies >= MAX_BAKED_COPIES {
            skipped += 1;
            continue;
        }
        match bake_one(&mut out, id, MAX_BAKED_COPIES - copies) {
            Some(n) => {
                layers += 1;
                copies += n;
            }
            None => skipped += 1,
        }
    }
    let mut warnings = Vec::new();
    if layers > 0 {
        warnings.push(format!(
            "{layers} layer{} with modifiers exported as {copies} plain copies (modifiers are a Picabia feature this format doesn't have)",
            if layers == 1 { "" } else { "s" }
        ));
    }
    if skipped > 0 {
        warnings
            .push(format!("{skipped} layer{} with modifiers exported without their copies (too many copies)", if skipped == 1 { "" } else { "s" }));
    }
    (Cow::Owned(out), warnings)
}

/// Replace layer `id` with its copies (at most `room`); how many copies it made.
fn bake_one(doc: &mut Document, id: NodeId, room: usize) -> Option<usize> {
    let n = doc.node(id)?.clone();
    let ts = figcraft_render::picabia::instance_transforms(&n)?;
    let parent_id = n.parent?;
    let parent = doc.node(parent_id)?;
    let index = parent.children.iter().position(|c| *c == id)?;
    let auto_layout = parent.layout.is_some();
    let ts: Vec<_> = ts.into_iter().take(room.max(1)).collect();
    let made = ts.len();
    if auto_layout {
        // The layer keeps its slot as copy 0; the others are absolute siblings after it.
        for (k, t) in ts.iter().enumerate().skip(1) {
            let c = doc.duplicate_into(id, parent_id, Some(index + k)).ok()?;
            let cn = doc.node_mut(c)?;
            cn.transform = *t * n.transform;
            cn.child_layout.absolute = true;
            figcraft_render::picabia::strip_stack(cn);
        }
        if let Some(first) = ts.first()
            && let Some(orig) = doc.node_mut(id)
        {
            orig.transform = *first * n.transform;
            figcraft_render::picabia::strip_stack(orig);
        }
        return Some(made);
    }
    let group = doc.create(parent_id, NodeKind::Group, &n.name, 0.0, 0.0, 0.0, 0.0).ok()?;
    doc.reparent(group, parent_id, Some(index)).ok()?;
    for t in &ts {
        let c = doc.duplicate_into(id, group, None).ok()?;
        let cn = doc.node_mut(c)?;
        cn.transform = *t * n.transform;
        figcraft_render::picabia::strip_stack(cn);
    }
    doc.remove(id).ok()?;
    let _ = doc.fit_to_children(group);
    Some(made)
}

#[cfg(test)]
mod tests {
    use figcraft_doc::{Document, NodeKind};

    use super::baked;
    use crate::picabia_canvas_tests::LINEAR3;

    #[test]
    fn stacks_bake_into_a_group_of_copies() {
        let mut d = Document::new("bake");
        let page = d.first_page().unwrap();
        let r = d.create(page, NodeKind::Rectangle, "R", 10.0, 10.0, 20.0, 20.0).unwrap();
        let (same, w) = baked(&d);
        assert!(matches!(same, std::borrow::Cow::Borrowed(_)) && w.is_empty(), "nothing to bake: unchanged");
        d.get_mut(r).unwrap().plugin_data.insert(figcraft_modifiers::PLUGIN_KEY.into(), LINEAR3.into());
        let (out, w) = baked(&d);
        assert_eq!(w.len(), 1, "{w:?}");
        let p = out.node(page).unwrap();
        assert_eq!(p.children.len(), 1);
        let g = out.node(p.children[0]).unwrap();
        assert!(matches!(g.kind, NodeKind::Group));
        assert_eq!(g.children.len(), 3);
        assert_eq!(out.world_bounds(g.id).unwrap(), figcraft_geom::Rect::new(10.0, 10.0, 110.0, 30.0));
        assert!(g.children.iter().all(|c| out.node(*c).unwrap().plugin_data.is_empty()));
    }
}
