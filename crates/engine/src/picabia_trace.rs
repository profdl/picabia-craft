//! PICABIA: the `imageTrace` generator (a stub until the tracer is wired in).
//!
//! The framework ([`crate::picabia_generate`]) calls [`compute`] for a layer whose generator row
//! is an `imageTrace`, and [`apply_result`] when Apply (or a `.fig`/Penpot bake) turns the result
//! into layers. This stub fills the layer's box mid-grey at each input copy, with a warning.

use figcraft_doc::{Document, Node, NodeId};
use figcraft_geom::kurbo::Shape as _;
use figcraft_modifiers::ImageTrace;
use figcraft_render::picabia::generated::{GenFrame, GenItem, GenPaint, GenShape};
use serde_json::json;

use crate::Result;
use crate::picabia_generate::GenOut;

/// Layer `n`'s trace for `frame` (its input copies in `frame.inputs`, maps of its own space),
/// with params `p` (clamped). The result is in the layer's own space, bottom first.
pub fn compute(_doc: &Document, n: &Node, frame: &GenFrame, _p: &ImageTrace) -> GenOut {
    let rect = n.local_rect().to_path(0.1);
    let items = frame
        .inputs
        .iter()
        .map(|c| {
            let mut path = rect.clone();
            path.apply_affine(*c);
            GenItem { path, paint: GenPaint::Fill { even_odd: false }, rgba: [0.5, 0.5, 0.5, 1.0], source: Some(n.id), name: "#808080".into() }
        })
        .collect();
    let mut out =
        GenOut::new(items, json!({"paths": frame.inputs.len(), "anchors": 4 * frame.inputs.len(), "colours": 1, "tracedPx": {"w": 0, "h": 0}}));
    out.warnings.push("image trace not built yet".into());
    out
}

/// Replace layer `id` by the group Apply makes of its trace `shape`: "<layer> (traced)", one
/// vector layer per traced path, bottom first.
pub fn apply_result(doc: &mut Document, id: NodeId, shape: &GenShape) -> Result<NodeId> {
    let name = format!("{} (traced)", doc.get(id)?.name);
    crate::picabia_generate::replace_with_items(doc, id, shape, &name)
}
