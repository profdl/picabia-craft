//! Entry points for a document node: read its stack from plugin data and evaluate it, cached.

use std::sync::Arc;

use figcraft_doc::Node;
use figcraft_geom::{Affine, Rect};

use crate::eval::Output;
use crate::model::Stack;
use crate::{ModError, PLUGIN_KEY, cache};

/// Whether `node` has modifier data (a plugin-data lookup; doesn't parse).
pub fn has_modifiers(node: &Node) -> bool {
    node.plugin_data.contains_key(PLUGIN_KEY)
}

/// `node`'s stack: `None` without modifier data, `Some(Err)` when the data is damaged (the layer
/// draws plainly; show the error in the panel). Cached by source text.
pub fn node_stack(node: &Node) -> Option<Result<Arc<Stack>, ModError>> {
    node.plugin_data.get(PLUGIN_KEY).map(|src| cache::parsed(src))
}

/// `node`'s box `(0, 0, width, height)` through its own transform: its bounding box in its
/// parent's space, whose centre is the modifiers' pivot. Non-finite sizes read as 0.
pub fn node_bounds(node: &Node) -> Rect {
    let f = |v: f64| if v.is_finite() { v.max(0.0) } else { 0.0 };
    let xf = if node.transform.is_finite() { node.transform } else { Affine::IDENTITY };
    xf.transform_rect_bbox(Rect::new(0.0, 0.0, f(node.width), f(node.height)))
}

/// [`evaluate_node_in`] with [`node_bounds`].
pub fn evaluate_node(node: &Node) -> Option<Arc<Output>> {
    evaluate_node_in(node, node_bounds(node))
}

/// Evaluate `node`'s stack for `bounds` (its bounding box in its parent's space), cached by
/// (source, bounds). `None` means "draw the layer normally": no modifier data, damaged data, or no
/// active modifier. A `Some` always has at least one instance.
pub fn evaluate_node_in(node: &Node, bounds: Rect) -> Option<Arc<Output>> {
    let src = node.plugin_data.get(PLUGIN_KEY)?;
    cache::evaluated(src, bounds).ok().filter(|o| !o.instances.is_empty())
}

/// The renderer's call: `node`'s instance transforms (parent space, applied before
/// `node.transform`), or `None` to draw it once as usual. Costs a plugin-data lookup for a layer
/// without modifiers and a cache hit for one with.
pub fn node_instances(node: &Node) -> Option<Arc<Output>> {
    if node.plugin_data.is_empty() {
        return None;
    }
    evaluate_node(node)
}
