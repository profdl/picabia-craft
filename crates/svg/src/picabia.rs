//! Picabia: a layer with a modifier stack is written once per copy.
//!
//! Each copy other than the original is the layer written as usual inside
//! `<g transform="P·T·P⁻¹">` (`P` maps its parent's space to the SVG, `T` the instance transform),
//! so its vectors, filters and the renderings embedded for effects SVG can't build (which draw one
//! copy: `figcraft_render::render_alone` takes the layer's own stack off) all move together. The
//! clip inherited from a clipping frame above goes on a wrapper outside that transform, so it stays
//! where the frame is.

use std::fmt::Write as _;

use figcraft_doc::NodeId;
use figcraft_geom::Affine;

use crate::{W, clip_attr, matrix};

impl W<'_> {
    /// Write node `id` (its parent's space mapped to the SVG by `xf`) once per copy; `false` when
    /// it has no active stack (or is a mask): the caller writes it once.
    pub(crate) fn node_copies(&mut self, id: NodeId, xf: Affine, depth: usize) -> bool {
        let doc = self.doc;
        let Some(n) = doc.node(id) else { return false };
        if !n.visible || n.is_mask || !xf.is_finite() || xf.determinant().abs() < 1e-12 {
            return false;
        }
        if self.stencil(n, xf) || self.generated(n, xf) {
            return true;
        }
        let inv = xf.inverse();
        figcraft_render::picabia::each_copy(doc, n, |t| {
            let c = xf * t * inv;
            if t == Affine::IDENTITY || !c.is_finite() {
                self.node_once(id, xf, depth);
                return;
            }
            let inherited = self.clip.take();
            let _ = write!(self.body, "<g{}><g transform=\"{}\">", clip_attr(&inherited), matrix(c));
            self.node_once(id, xf, depth);
            self.body.push_str("</g></g>");
            self.clip = inherited;
        })
    }

    /// A stencil layer (its stack ends in a stencil row, with a computed sheet) as the canvas
    /// shows it: the paper with the cut open, or the cloth with the paste. `false`: not one.
    fn stencil(&mut self, n: &figcraft_doc::Node, xf: Affine) -> bool {
        use figcraft_render::picabia::stencil;
        let Some(d) = stencil::drawn(self.doc, n) else { return false };
        let at = xf * n.transform;
        if !at.is_finite() {
            return true;
        }
        let opacity = if n.opacity < 1.0 { format!(" opacity=\"{}\"", crate::num(f64::from(n.opacity.max(0.0)))) } else { String::new() };
        for f in stencil::fills(&d) {
            let rule = if f.even_odd { " fill-rule=\"evenodd\"" } else { "" };
            let [r, g, b] = f.rgb;
            let el = format!("<path transform=\"{}\" d=\"{}\" fill=\"#{r:02x}{g:02x}{b:02x}\"{rule}{opacity}/>", matrix(at), crate::path_d(&f.path));
            let clip = clip_attr(&self.clip);
            self.body.push_str(&crate::clipped(&clip, el));
        }
        true
    }

    /// A generated layer (a generator row with a computed result) as the canvas shows it: the
    /// result's paths at every copy of the result, fills with their fill rule, strokes with their
    /// width, cap and join. `false`: not one.
    fn generated(&mut self, n: &figcraft_doc::Node, xf: Affine) -> bool {
        use figcraft_render::picabia::generated::{self, GenPaint};
        let Some(d) = generated::drawn(self.doc, n) else { return false };
        let opacity = f64::from(n.opacity.clamp(0.0, 1.0));
        for t in &d.copies {
            let at = xf * *t * n.transform;
            if !at.is_finite() {
                continue;
            }
            for it in &d.shape.items {
                let [r, g, b, a] = it.rgba.map(|c| f64::from(c.clamp(0.0, 1.0)));
                let hex = format!("#{:02x}{:02x}{:02x}", (r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8);
                let alpha = a * opacity;
                let paint = match it.paint {
                    GenPaint::Fill { even_odd } => {
                        let rule = if even_odd { " fill-rule=\"evenodd\"" } else { "" };
                        let op = if alpha < 1.0 { format!(" fill-opacity=\"{}\"", crate::num(alpha)) } else { String::new() };
                        format!("fill=\"{hex}\"{rule}{op}")
                    }
                    GenPaint::Stroke { width, cap, join } => {
                        let op = if alpha < 1.0 { format!(" stroke-opacity=\"{}\"", crate::num(alpha)) } else { String::new() };
                        format!(
                            "fill=\"none\" stroke=\"{hex}\" stroke-width=\"{}\" stroke-linecap=\"{}\" stroke-linejoin=\"{}\"{op}",
                            crate::num(if width.is_finite() { width.max(0.0) } else { 0.0 }),
                            generated::svg_cap(cap),
                            generated::svg_join(join)
                        )
                    }
                };
                let el = format!("<path transform=\"{}\" d=\"{}\" {paint}/>", matrix(at), crate::path_d(&it.path));
                let clip = clip_attr(&self.clip);
                self.body.push_str(&crate::clipped(&clip, el));
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use figcraft_color::{Color, Paint};
    use figcraft_doc::{Document, NodeKind};

    use crate::{SvgOptions, export_svg};

    #[test]
    fn a_stacked_layer_is_written_once_per_copy() {
        let mut d = Document::new("picabia");
        let page = d.first_page().unwrap();
        let r = d.create(page, NodeKind::Rectangle, "R", 10.0, 10.0, 20.0, 20.0).unwrap();
        d.get_mut(r).unwrap().fills = vec![Paint::solid(Color::from_hex("FF0000").unwrap())];
        let plain = export_svg(&d, &[r], &SvgOptions::default()).unwrap();
        assert_eq!(plain.matches("<path").count(), 1);
        d.get_mut(r).unwrap().plugin_data.insert(
            figcraft_render::picabia::PLUGIN_KEY.into(),
            r#"{"v":1,"stack":[{"id":"m1","enabled":true,"kind":"linearArray","count":3,"offsetX":40,"offsetY":0}]}"#.into(),
        );
        let svg = export_svg(&d, &[r], &SvgOptions::default()).unwrap();
        assert_eq!(svg.matches("<path").count(), 3, "{svg}");
        assert!(svg.contains("width=\"100\""), "the export covers every copy: {svg}");
        assert!(svg.contains("transform=\"matrix(1 0 0 1 40 0)\"") || svg.contains("translate(40"), "{svg}");
    }

    #[test]
    fn copy_to_shape_writes_a_copy_on_each_target_corner() {
        let mut d = Document::new("picabia");
        let page = d.first_page().unwrap();
        let r = d.create(page, NodeKind::Rectangle, "R", 0.0, 0.0, 10.0, 10.0).unwrap();
        d.get_mut(r).unwrap().fills = vec![Paint::solid(Color::from_hex("FF0000").unwrap())];
        let t = d.create(page, NodeKind::Rectangle, "T", 50.0, 10.0, 100.0, 40.0).unwrap();
        d.get_mut(r).unwrap().plugin_data.insert(
            figcraft_render::picabia::PLUGIN_KEY.into(),
            format!(r#"{{"v":1,"stack":[{{"id":"m1","kind":"copyToShape","target":"{t}","mode":"points","align":false}}]}}"#),
        );
        let svg = export_svg(&d, &[r], &SvgOptions::default()).unwrap();
        assert_eq!(svg.matches("<path").count(), 4, "{svg}");
        // The export covers the four corners: 100 + 10 wide, 40 + 10 high.
        assert!(svg.contains("width=\"110\"") && svg.contains("height=\"50\""), "{svg}");
        // Deleting the target leaves the layer drawn once, where it is.
        d.remove(t).unwrap();
        let svg = export_svg(&d, &[r], &SvgOptions::default()).unwrap();
        assert_eq!(svg.matches("<path").count(), 1, "{svg}");
    }

    #[test]
    fn copies_inside_a_clipping_frame_keep_the_frames_clip_in_place() {
        let mut d = Document::new("picabia");
        let page = d.first_page().unwrap();
        let f = d.create(page, NodeKind::Frame, "F", 0.0, 0.0, 50.0, 50.0).unwrap();
        d.get_mut(f).unwrap().clip = true;
        let r = d.create(f, NodeKind::Rectangle, "R", 10.0, 10.0, 20.0, 20.0).unwrap();
        d.get_mut(r).unwrap().fills = vec![Paint::solid(Color::from_hex("FF0000").unwrap())];
        d.get_mut(r).unwrap().plugin_data.insert(
            figcraft_render::picabia::PLUGIN_KEY.into(),
            r#"{"v":1,"stack":[{"id":"m1","enabled":true,"kind":"linearArray","count":2,"offsetX":30,"offsetY":0}]}"#.into(),
        );
        let svg = export_svg(&d, &[f], &SvgOptions::default()).unwrap();
        // The wrapper carries the frame's clip, outside the copy's transform.
        assert!(svg.contains("<g clip-path=\"url(#"), "{svg}");
    }
}
