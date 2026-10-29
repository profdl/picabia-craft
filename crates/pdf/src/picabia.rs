//! Picabia: a layer with a modifier stack is drawn once per copy.
//!
//! Each copy other than the original is the layer drawn as usual under a pushed transform
//! `P·T·P⁻¹` (`P` maps its parent's space to the PDF page, `T` the instance transform), so its
//! vectors and the images placed for effects (which the renderer draws for one copy:
//! `figcraft_render::render_alone` takes the layer's own stack off) move together. Clips pushed by
//! frames above were set before that transform and stay where the frames are.

use figcraft_doc::NodeId;
use figcraft_geom::Affine;
use krilla::surface::Surface;

use super::{Exporter, to_xf};

impl Exporter<'_> {
    /// Draw node `id` (its parent's space mapped to the page by `parent_xf`) once per copy;
    /// `false` when it has no active stack (or is a mask): the caller draws it once.
    pub(crate) fn node_copies(&mut self, s: &mut Surface, id: NodeId, parent_xf: Affine, depth: usize, as_mask: bool) -> bool {
        let doc = self.doc;
        let Some(n) = doc.node(id) else { return false };
        if n.is_mask || !parent_xf.is_finite() || parent_xf.determinant().abs() < 1e-12 {
            return false;
        }
        let inv = parent_xf.inverse();
        figcraft_render::picabia::each_copy(doc, n, |t| {
            let c = parent_xf * t * inv;
            if t == Affine::IDENTITY || !c.is_finite() {
                self.node_once(s, id, parent_xf, depth, as_mask);
                return;
            }
            s.push_transform(&to_xf(c));
            self.node_once(s, id, parent_xf, depth, as_mask);
            s.pop();
        })
    }
}

#[cfg(test)]
mod tests {
    use figcraft_color::{Color, Paint};
    use figcraft_doc::{Document, Effect, EffectKind, NodeKind, Shadow};

    use crate::export_nodes_pdf;

    fn stacked(effects: bool) -> (Document, figcraft_doc::NodeId) {
        let mut d = Document::new("picabia");
        let page = d.first_page().unwrap();
        let r = d.create(page, NodeKind::Rectangle, "R", 10.0, 10.0, 20.0, 20.0).unwrap();
        let n = d.get_mut(r).unwrap();
        n.fills = vec![Paint::solid(Color::from_hex("FF0000").unwrap())];
        if effects {
            n.effects.push(Effect { kind: EffectKind::DropShadow(Shadow::default()), visible: true });
            n.effects.push(Effect { kind: EffectKind::LayerBlur(figcraft_doc::Blur::uniform(2.0)), visible: true });
        }
        n.plugin_data.insert(
            figcraft_render::picabia::PLUGIN_KEY.into(),
            r#"{"v":1,"stack":[{"id":"m1","enabled":true,"kind":"linearArray","count":3,"offsetX":40,"offsetY":0}]}"#.into(),
        );
        (d, r)
    }

    #[test]
    fn a_stacked_layer_exports_every_copy() {
        let (d, r) = stacked(false);
        let pdf = export_nodes_pdf(&d, &[r]).unwrap();
        let s = crate::tests::inflated(&pdf);
        // Three fills, two of them under a pushed translation.
        assert!(s.matches(" cm").count() >= 2, "{s}");
        let raw = String::from_utf8_lossy(&pdf);
        assert!(raw.contains("/MediaBox [0 0 100 20]") || raw.contains("/MediaBox[0 0 100 20]"), "the page covers every copy");
    }

    #[test]
    fn stacked_layers_with_raster_effects_export_without_crashing() {
        let (d, r) = stacked(true);
        assert!(export_nodes_pdf(&d, &[r]).is_ok());
    }
}
