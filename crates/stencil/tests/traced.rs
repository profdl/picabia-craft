//! The stencil of a real traced design (`fixtures/traced.svg`: three flat colours, 920 × 752,
//! 85 jagged paths plus tracer specks).
//!
//! The SVG is read by a minimal test-only parser (class fills from the `<style>` block; `path`,
//! `rect` and `circle` in document order; `translate`/`rotate` transforms), enough for this file.
//! `figcraft-svg` (L4) would do as a dev-dependency (`cargo xtask layers` skips dev-deps) but pulls
//! in text and fonts for no gain here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;

use figcraft_geom::kurbo::{Affine, BezPath, Circle, Rect, Shape};
use figcraft_geom::split_subpaths;
use figcraft_modifiers::Stencil;
use figcraft_stencil::{Paint, compute, mm_per_unit, visible_regions};

const SVG: &str = include_str!("fixtures/traced.svg");
const BOUNDS: Rect = Rect::new(0.0, 0.0, 920.0, 752.0);

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let len = tag.get(start..)?.find('"')?;
    tag.get(start..start + len)
}

fn num(tag: &str, name: &str) -> f64 {
    attr(tag, name).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0)
}

/// `translate(x y) rotate(deg)` sequences, applied left to right as SVG composes them.
fn transform(s: &str) -> Affine {
    let mut xf = Affine::IDENTITY;
    for part in s.split(')') {
        let Some((name, args)) = part.split_once('(') else { continue };
        let a: Vec<f64> = args.split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty()).map(|t| t.parse().unwrap()).collect();
        let op = match name.trim() {
            "translate" => Affine::translate((a[0], a.get(1).copied().unwrap_or(0.0))),
            "rotate" => Affine::rotate(a[0].to_radians()),
            other => panic!("transform {other} not handled"),
        };
        xf *= op;
    }
    xf
}

/// `.cls-N { fill: #rrggbb; }` from the style block.
fn class_fills(svg: &str) -> HashMap<String, [f32; 4]> {
    let mut out = HashMap::new();
    let mut rest = svg;
    while let Some(i) = rest.find(".cls-") {
        rest = &rest[i + 1..];
        let name: String = rest.chars().take_while(|c| !c.is_whitespace() && *c != '{').collect();
        let body = &rest[rest.find('{').unwrap()..rest.find('}').unwrap()];
        let hex = body.split("fill:").nth(1).unwrap().trim().trim_end_matches(';').trim();
        let c = figcraft_modifiers::parse_hex_colour(hex).unwrap();
        out.insert(name, [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]);
    }
    out
}

/// Every filled element of the fixture as a paint, bottom first.
fn traced() -> Vec<Paint> {
    let fills = class_fills(SVG);
    let mut paints = Vec::new();
    for chunk in SVG.split('<').skip(1) {
        let tag = chunk.split('>').next().unwrap();
        let name = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap();
        let path = match name {
            "path" => BezPath::from_svg(attr(tag, "d").unwrap()).unwrap(),
            "rect" => Rect::new(num(tag, "x"), num(tag, "y"), num(tag, "x") + num(tag, "width"), num(tag, "y") + num(tag, "height")).to_path(0.01),
            "circle" => Circle::new((num(tag, "cx"), num(tag, "cy")), num(tag, "r")).to_path(0.01),
            _ => continue,
        };
        let xf = attr(tag, "transform").map(transform).unwrap_or(Affine::IDENTITY);
        let colour = fills[attr(tag, "class").unwrap()];
        paints.push(Paint { path: xf * path, colour, order: paints.len() as u32 });
    }
    paints
}

#[test]
fn the_fixture_parses() {
    let paints = traced();
    assert_eq!(paints.len(), 100, "85 paths, 12 rects, 3 circles");
    let bb = paints.iter().fold(Rect::ZERO, |b, p| b.union(p.path.bounding_box()));
    assert!((bb.width() - 920.0).abs() < 1.0 && (bb.height() - 752.0).abs() < 1.0, "{bb:?}");
    let v = visible_regions(&paints, 0.04, 0.0);
    assert_eq!(v.regions.len(), 3);
    let total: f64 = v.regions.iter().map(|r| r.area).sum();
    assert!(total > BOUNDS.area() * 0.95 && total < BOUNDS.area() * 1.001, "{total}");
}

#[test]
fn the_traced_design_makes_a_sane_stencil() {
    let paints = traced();
    let p = Stencil::default();
    let out = compute(&paints, &p, BOUNDS).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert!(!out.cut.elements().is_empty() && !out.paper.elements().is_empty());
    let (sheet, cut, paper) = (out.sheet.area(), out.cut.area(), out.paper.area());
    assert!(cut > 0.01 * sheet && paper > 0.3 * sheet, "cut {cut}, paper {paper}, sheet {sheet}");
    assert!((paper - (sheet - cut)).abs() < sheet * 1e-9);
    // Specks gone: nothing below the area limit is left.
    let mpu = mm_per_unit(p.width_mm, BOUNDS).unwrap();
    let min = p.min_cut_area_mm2 / (mpu * mpu);
    assert!(out.stats.dropped_specks > 0, "{:?}", out.stats);
    for s in split_subpaths(&out.cut) {
        assert!(s.area().abs() >= min * 0.8, "contour of {} < {min}", s.area());
    }
    let sw: Vec<(&str, bool)> = out.colours.iter().map(|c| (c.hex.as_str(), c.cut)).collect();
    assert_eq!(sw.len(), 3, "{sw:?}");
    assert!(sw.contains(&("#e7cb5b", true)) && sw.contains(&("#cbab9e", false)) && sw.contains(&("#28262e", false)), "{sw:?}");
    // The cut is the yellow, cleaned: close to its visible area.
    let yellow = out.colours.iter().find(|c| c.cut).unwrap().area;
    assert!((out.stats.area_cut_mm2 - yellow).abs() < yellow * 0.05, "{} vs {yellow}", out.stats.area_cut_mm2);
    // Inverted: the two darker colours are cut instead.
    let inv = compute(&paints, &Stencil { invert: true, ..p }, BOUNDS).unwrap();
    assert!(inv.cut.area() > cut && inv.stats.cut_pieces > 0);
}
