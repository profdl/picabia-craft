//! Picabia copies in the renderer: drawing, bounds, culling, retained redraws, hit testing.

use std::sync::Arc;

use figcraft_color::{Color, Paint};
use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_geom::{Point, Rect};

use super::*;
use crate::{Options, Pixmap, Retained, View, paint_bounds, render_view};

fn px(p: &Pixmap, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * p.width() + x) * 4) as usize;
    let d = &p.data()[i..i + 4];
    [d[0], d[1], d[2], d[3]]
}

fn linear(count: u32, dx: f64) -> String {
    format!(r#"{{"v":1,"stack":[{{"id":"m1","enabled":true,"kind":"linearArray","count":{count},"offsetX":{dx},"offsetY":0}}]}}"#)
}

fn set_stack(d: &mut Document, id: NodeId, src: &str) {
    d.get_mut(id).unwrap().plugin_data.insert(PLUGIN_KEY.to_string(), src.to_string());
}

/// A red 20×20 rectangle at (10, 10) on a page.
fn doc() -> (Document, NodeId, NodeId) {
    let mut d = Document::new("picabia");
    let page = d.first_page().unwrap();
    let r = d.create(page, NodeKind::Rectangle, "R", 10.0, 10.0, 20.0, 20.0).unwrap();
    d.get_mut(r).unwrap().fills = vec![Paint::solid(Color::from_hex("FF0000").unwrap())];
    (d, page, r)
}

fn white() -> Options {
    Options { background: Some(Color::WHITE), ..Default::default() }
}

const RED: [u8; 4] = [255, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

#[test]
fn a_three_copy_linear_array_draws_three_rectangles() {
    let (mut d, page, r) = doc();
    set_stack(&mut d, r, &linear(3, 40.0));
    let pix = render_view(&d, page, &View { origin: (0.0, 0.0), scale: 1.0, width: 140, height: 40 }, &white()).unwrap();
    for x in [20, 60, 100] {
        assert_eq!(px(&pix, x, 20), RED, "copy at x = {x}");
    }
    for x in [40, 80, 120] {
        assert_eq!(px(&pix, x, 20), WHITE, "gap at x = {x}");
    }
}

#[test]
fn layers_without_stacks_draw_and_measure_as_before() {
    let (mut d, page, r) = doc();
    let v = View { origin: (0.0, 0.0), scale: 1.0, width: 140, height: 40 };
    let before = render_view(&d, page, &v, &white()).unwrap();
    let pb = paint_bounds(&d, r).unwrap();
    // Other plugin data doesn't make copies, nor does a damaged or empty stack.
    d.get_mut(r).unwrap().plugin_data.insert("someone.else".into(), "{}".into());
    for src in [None, Some("not json"), Some(r#"{"v":1,"stack":[]}"#)] {
        if let Some(s) = src {
            set_stack(&mut d, r, s);
        }
        let after = render_view(&d, page, &v, &white()).unwrap();
        assert_eq!(before.data(), after.data(), "{src:?}");
        assert_eq!(paint_bounds(&d, r).unwrap(), pb);
        assert_eq!(crate::export_bounds(&d, r), d.world_render_bounds(r));
        assert!(page_copies(&d, r).is_empty());
    }
}

#[test]
fn bounds_cover_every_copy() {
    let (mut d, _, r) = doc();
    set_stack(&mut d, r, &linear(3, 40.0));
    let pb = paint_bounds(&d, r).unwrap();
    assert!(pb.x0 <= 10.0 && pb.x1 >= 110.0, "{pb:?}");
    let eb = crate::export_bounds(&d, r).unwrap();
    assert_eq!(eb, Rect::new(10.0, 10.0, 110.0, 30.0));
    // The export rasterises them all.
    let pix = crate::render_node(&d, r, 1.0).unwrap();
    assert_eq!((pix.width(), pix.height()), (100, 20));
    assert_eq!(px(&pix, 90, 10), RED);
    assert_eq!(px(&pix, 30, 10)[3], 0);
}

#[test]
fn a_group_measures_its_childrens_copies() {
    let (mut d, page, r) = doc();
    let g = d.create(page, NodeKind::Group, "G", 0.0, 0.0, 40.0, 40.0).unwrap();
    d.reparent(r, g, None).unwrap();
    set_stack(&mut d, r, &linear(3, 40.0));
    let n = d.node(g).unwrap();
    let local = crate::draw::local_paint_bounds(&d, n, d.world_transform(n.id), 0, None);
    assert!(local.x1 >= 110.0, "{local:?}");
    assert!(crate::export_bounds(&d, g).unwrap().x1 >= 110.0);
    // A copy of the group's child far off view to the left is still drawn when the original is off view.
    let pix = render_view(&d, page, &View { origin: (50.0, 0.0), scale: 1.0, width: 80, height: 40 }, &white()).unwrap();
    assert_eq!(px(&pix, 50, 20), RED, "the third copy (x 90..110) shows at view x 40..60");
}

#[test]
fn copies_of_an_off_view_original_are_not_culled() {
    let (mut d, page, r) = doc();
    set_stack(&mut d, r, &linear(3, 200.0));
    let pix = render_view(&d, page, &View { origin: (400.0, 0.0), scale: 1.0, width: 40, height: 40 }, &white()).unwrap();
    assert_eq!(px(&pix, 20, 20), RED);
}

#[test]
fn a_stacked_groups_children_draw_in_every_copy_and_dirty_regions_cover_them() {
    let (mut d, page, r) = doc();
    let g = d.create(page, NodeKind::Group, "G", 0.0, 0.0, 40.0, 40.0).unwrap();
    d.reparent(r, g, None).unwrap();
    set_stack(&mut d, g, &linear(3, 40.0));
    let pix = render_view(&d, page, &View { origin: (0.0, 0.0), scale: 1.0, width: 140, height: 40 }, &white()).unwrap();
    assert_eq!(px(&pix, 100, 20), RED);
    // A change to the child repaints where its group's copies draw it.
    let pb = paint_bounds(&d, r).unwrap();
    assert!(pb.x1 >= 110.0, "{pb:?}");
}

#[test]
fn retained_redraws_match_full_renders_when_stacks_change() {
    let (mut d, page, r) = doc();
    set_stack(&mut d, r, &linear(3, 40.0));
    let v = View { origin: (0.0, 0.0), scale: 1.0, width: 300, height: 60 };
    let mut ret = Retained::default();
    let mut doc = Arc::new(d);
    ret.render(&doc, page, &v, &Options::default()).unwrap();
    type Step = Box<dyn Fn(&mut Document)>;
    let steps: [Step; 4] = [
        Box::new(move |d| set_stack(d, r, &linear(5, 50.0))),
        Box::new(move |d| set_stack(d, r, &linear(2, 50.0))),
        Box::new(move |d| d.get_mut(r).unwrap().transform = figcraft_geom::Affine::translate((30.0, 20.0))),
        Box::new(move |d| {
            d.get_mut(r).unwrap().plugin_data.remove(PLUGIN_KEY);
        }),
    ];
    for (i, step) in steps.iter().enumerate() {
        let mut nd = (*doc).clone();
        step(&mut nd);
        doc = Arc::new(nd);
        let (shown, pix, work) = ret.render(&doc, page, &v, &Options::default()).unwrap();
        let pix = pix.clone();
        let full = render_view(&doc, page, &shown, &Options::default()).unwrap();
        crate::retained_tests::assert_same(&pix, &full, &format!("step {i}: {work:?}"));
    }
}

#[test]
fn clicking_a_copy_hits_the_source() {
    let (mut d, page, r) = doc();
    set_stack(&mut d, r, &linear(3, 40.0));
    for x in [20.0, 60.0, 100.0] {
        let p = Point::new(x, 20.0);
        assert_eq!(hit_with_copies(&d, page, p, 1.0, None, d.hit_test(page, p, 1.0, None)), Some(r), "x = {x}");
    }
    let gap = Point::new(40.0, 20.0);
    assert_eq!(hit_with_copies(&d, page, gap, 1.0, None, d.hit_test(page, gap, 1.0, None)), None);
    // A layer drawn above a copy still wins there; the copy wins over one below it.
    let top = d.create(page, NodeKind::Rectangle, "Top", 95.0, 15.0, 10.0, 10.0).unwrap();
    let p = Point::new(100.0, 20.0);
    assert_eq!(hit_with_copies(&d, page, p, 1.0, None, d.hit_test(page, p, 1.0, None)), Some(top));
    d.reparent(top, page, Some(0)).unwrap();
    assert_eq!(hit_with_copies(&d, page, p, 1.0, None, d.hit_test(page, p, 1.0, None)), Some(r));
}

#[test]
fn a_marquee_over_a_copy_selects_the_source() {
    let (mut d, page, r) = doc();
    set_stack(&mut d, r, &linear(3, 40.0));
    let mut hits = Vec::new();
    marquee_copies(&d, page, Rect::new(85.0, 0.0, 95.0, 40.0), false, &mut hits);
    assert_eq!(hits, vec![r]);
    let mut none = Vec::new();
    marquee_copies(&d, page, Rect::new(35.0, 0.0, 45.0, 40.0), false, &mut none);
    assert!(none.is_empty());
    let mut deep = Vec::new();
    marquee_copies(&d, page, Rect::new(85.0, 0.0, 95.0, 40.0), true, &mut deep);
    assert_eq!(deep, vec![r]);
}

#[test]
fn nested_stacks_share_one_copy_budget() {
    let (mut d, page, r) = doc();
    let g = d.create(page, NodeKind::Group, "G", 0.0, 0.0, 40.0, 40.0).unwrap();
    d.reparent(r, g, None).unwrap();
    set_stack(&mut d, g, &linear(1000, 0.5));
    set_stack(&mut d, r, &linear(1000, 0.5));
    let (mut outer, mut inner) = (0usize, 0usize);
    let gn = d.node(g).unwrap().clone();
    let rn = d.node(r).unwrap().clone();
    each_copy(&d, &gn, |_| {
        outer += 1;
        each_copy(&d, &rn, |_| inner += 1);
    });
    assert_eq!(outer, 1000);
    assert!(inner <= MAX_NESTED_COPIES, "{inner}");
    // And it still renders (capped work).
    assert!(render_view(&d, page, &View { origin: (0.0, 0.0), scale: 0.1, width: 80, height: 20 }, &white()).is_some());
}

#[test]
fn render_alone_draws_one_copy_for_export_walkers() {
    let (mut d, _, r) = doc();
    set_stack(&mut d, r, &linear(3, 40.0));
    let b = crate::raster_bounds(&d, r).unwrap();
    assert_eq!(b, Rect::new(10.0, 10.0, 30.0, 30.0));
}

/// A red 10×10 source at (0, 0) copied onto the corners of a hidden 100×40 target at (50, 10).
fn copy_to_shape_doc() -> (Document, NodeId, NodeId, NodeId) {
    let mut d = Document::new("picabia");
    let page = d.first_page().unwrap();
    let r = d.create(page, NodeKind::Rectangle, "R", 0.0, 0.0, 10.0, 10.0).unwrap();
    d.get_mut(r).unwrap().fills = vec![Paint::solid(Color::from_hex("FF0000").unwrap())];
    let t = d.create(page, NodeKind::Rectangle, "T", 50.0, 10.0, 100.0, 40.0).unwrap();
    d.get_mut(t).unwrap().visible = false;
    let src = format!(r#"{{"v":1,"stack":[{{"id":"m1","kind":"copyToShape","target":"{t}","mode":"points","align":false}}]}}"#);
    set_stack(&mut d, r, &src);
    (d, page, r, t)
}

#[test]
fn copy_to_shape_draws_a_copy_on_each_corner_of_the_target() {
    let (d, page, r, t) = copy_to_shape_doc();
    let pix = render_view(&d, page, &View { origin: (0.0, 0.0), scale: 1.0, width: 200, height: 80 }, &white()).unwrap();
    // Each corner holds a copy centred on it; the original spot is empty.
    for (x, y) in [(50, 10), (150, 10), (150, 50), (50, 50)] {
        assert_eq!(px(&pix, x, y), RED, "corner ({x}, {y})");
    }
    assert_eq!(px(&pix, 2, 2), WHITE, "the original isn't kept");
    assert_eq!(px(&pix, 100, 30), WHITE);
    // Bounds and hit testing follow the copies.
    let b = paint_bounds(&d, r).unwrap();
    assert!(b.x0 <= 45.0 && b.x1 >= 155.0 && b.y1 >= 55.0, "{b:?}");
    assert_eq!(hit_with_copies(&d, page, Point::new(150.0, 50.0), 0.5, None, None), Some(r));
    assert_eq!(target_links(&d), vec![(r, t)]);
}

#[test]
fn moving_a_copy_to_shape_target_repaints_its_copies_in_retained_renders() {
    let (d, page, _r, t) = copy_to_shape_doc();
    let v = View { origin: (0.0, 0.0), scale: 1.0, width: 300, height: 120 };
    let mut ret = Retained::default();
    let mut doc = Arc::new(d);
    ret.render(&doc, page, &v, &Options::default()).unwrap();
    type Step = Box<dyn Fn(&mut Document)>;
    let steps: [Step; 3] = [
        Box::new(move |d| d.get_mut(t).unwrap().transform = figcraft_geom::Affine::translate((120.0, 60.0))),
        Box::new(move |d| d.get_mut(t).unwrap().width = 60.0),
        Box::new(move |d| {
            d.remove(t).unwrap();
        }),
    ];
    for (i, step) in steps.iter().enumerate() {
        let mut nd = (*doc).clone();
        step(&mut nd);
        doc = Arc::new(nd);
        let (shown, pix, work) = ret.render(&doc, page, &v, &Options::default()).unwrap();
        let pix = pix.clone();
        let full = render_view(&doc, page, &shown, &Options::default()).unwrap();
        crate::retained_tests::assert_same(&pix, &full, &format!("step {i}: {work:?}"));
    }
}
