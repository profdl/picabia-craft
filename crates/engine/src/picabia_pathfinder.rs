//! PICABIA: the `pathfinder` generator (a stub until Pathfinder is wired in).
//!
//! The framework ([`crate::picabia_generate`]) calls [`compute`] for a layer whose generator row
//! is a `pathfinder`, and [`apply_result`] when Apply (or a `.fig`/Penpot bake) turns the result
//! into layers. This stub returns the gathered input shapes unchanged, as fills in their colours.

use figcraft_doc::{Document, Node, NodeId};
use figcraft_modifiers::Pathfinder;
use figcraft_render::picabia::generated::{GenFrame, GenItem, GenPaint, GenShape};
use serde_json::json;

use crate::Result;
use crate::picabia_generate::GenOut;

/// Layer `n`'s Pathfinder result for `frame` (its input copies in `frame.inputs`), with params `p`
/// (clamped). The result is in the layer's own space, back first.
pub fn compute(doc: &Document, n: &Node, frame: &GenFrame, _p: &Pathfinder) -> GenOut {
    let g = crate::picabia_stencil::gather_design(doc, n, &frame.inputs);
    let shapes = g.paints.len();
    let items: Vec<GenItem> = g
        .paints
        .into_iter()
        .zip(g.sources)
        .map(|(p, s)| GenItem { path: p.path, paint: GenPaint::Fill { even_odd: false }, rgba: p.colour, source: Some(s.node), name: s.name })
        .collect();
    let faces = items.len();
    let mut out = GenOut::new(items, json!({"shapes": shapes, "faces": faces}));
    out.warnings = g.warnings;
    out
}

/// Replace layer `id` by the group Apply makes of its result `shape`: a group of the same name,
/// one vector layer per face, back first, each named after its source layer.
pub fn apply_result(doc: &mut Document, id: NodeId, shape: &GenShape) -> Result<NodeId> {
    let name = doc.get(id)?.name.clone();
    crate::picabia_generate::replace_with_items(doc, id, shape, &name)
}
