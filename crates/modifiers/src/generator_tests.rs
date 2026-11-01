use figcraft_geom::{Affine, Rect};
use serde_json::{Value, json};

use crate::*;

const B: Rect = Rect::new(0.0, 0.0, 20.0, 10.0);

fn stack(mods: Value) -> Stack {
    parse(&json!({"v": 1, "stack": mods}).to_string()).unwrap()
}

fn trace_of(row: Value) -> ImageTrace {
    match Modifier::from_value(&row).unwrap().kind {
        ModifierKind::ImageTrace(p) => p,
        k => panic!("not a trace: {k:?}"),
    }
}

fn set(m: &mut Modifier, props: Value) {
    m.set_props(props.as_object().unwrap()).unwrap();
}

fn trace(m: &Modifier) -> &ImageTrace {
    let ModifierKind::ImageTrace(p) = &m.kind else { panic!("trace") };
    p
}

fn xs(ts: impl Iterator<Item = Affine>) -> Vec<f64> {
    ts.map(|t| t.translation().x).collect()
}

#[test]
fn defaults_match_the_spec() {
    let t = ImageTrace::default();
    assert_eq!(
        (t.preset, t.mode, t.palette, t.method),
        (TracePreset::Default, TraceMode::BlackAndWhite, TracePalette::Limited, TraceMethod::Abutting)
    );
    assert_eq!((t.colors, t.color_detail, t.threshold, t.paths, t.corners, t.noise), (6, 50.0, 128, 50.0, 75.0, 25));
    assert!(t.fills && !t.strokes && !t.snap_curves_to_lines && !t.ignore_white && t.ignore_color.is_none());
    assert_eq!(t.stroke_width, 10.0);
    let p = Pathfinder::default();
    assert_eq!((p.op, p.precision, p.outline_width), (PathfinderOp::Divide, 0.01, 1.0));
    assert_eq!(kind_label("imageTrace"), Some("Image trace"));
    assert_eq!(kind_label("pathfinder"), Some("Pathfinder"));
    assert!(ModifierKind::default_for("imageTrace").unwrap().is_generator());
    assert!(!ModifierKind::default_for("stencil").unwrap().is_generator());
    // Every field is written, camelCase.
    let v = Modifier::new("m1", "imageTrace").unwrap().to_value();
    for k in [
        "preset",
        "mode",
        "palette",
        "colors",
        "colorDetail",
        "threshold",
        "paths",
        "corners",
        "noise",
        "method",
        "fills",
        "strokes",
        "strokeWidth",
        "snapCurvesToLines",
        "ignoreWhite",
        "ignoreColor",
    ] {
        assert!(v.get(k).is_some(), "{k}: {v}");
    }
    let v = Modifier::new("m1", "pathfinder").unwrap().to_value();
    assert_eq!((v["op"].clone(), v["precision"].clone(), v["outlineWidth"].clone()), (json!("divide"), json!(0.01), json!(1.0)));
}

#[test]
fn round_trip_keeps_values_and_unknown_fields() {
    let row = json!({"id": "t", "enabled": true, "kind": "imageTrace", "preset": "custom", "mode": "color", "palette": "documentLibrary",
        "colors": 12, "ignoreWhite": true, "ignoreColor": "#ABC", "future": [1, 2]});
    let m = Modifier::from_value(&row).unwrap();
    let back = m.to_value();
    assert_eq!(back["future"], json!([1, 2]));
    assert_eq!(back["palette"], json!("documentLibrary"), "documentLibrary reads and writes");
    assert_eq!(back["ignoreColor"], json!("#aabbcc"));
    assert_eq!(Modifier::from_value(&back).unwrap(), m);
    let row = json!({"id": "p", "kind": "pathfinder", "op": "minusBack", "precision": "0.5", "x": true});
    let m = Modifier::from_value(&row).unwrap();
    assert_eq!(m.kind, ModifierKind::Pathfinder(Pathfinder { op: PathfinderOp::MinusBack, precision: 0.5, ..Pathfinder::default() }));
    assert_eq!(m.to_value()["x"], json!(true));
}

#[test]
fn lenient_parsing() {
    // Unknown preset: custom; unknown mode, palette, method: the defaults.
    let t = trace_of(json!({"kind": "imageTrace", "preset": "posterize", "mode": "sepia", "palette": "?", "method": 3}));
    assert_eq!(
        (t.preset, t.mode, t.palette, t.method),
        (TracePreset::Custom, TraceMode::BlackAndWhite, TracePalette::Limited, TraceMethod::Abutting)
    );
    // Whole numbers round; a colour that doesn't read is none.
    let t = trace_of(json!({"kind": "imageTrace", "colors": 7.6, "threshold": "200", "noise": -3, "ignoreColor": "teal"}));
    assert_eq!((t.colors, t.threshold, t.noise, t.ignore_color), (8, 200, 0, None));
    let m = Modifier::from_value(&json!({"kind": "pathfinder", "op": "shatter"})).unwrap();
    assert_eq!(m.kind, ModifierKind::Pathfinder(Pathfinder::default()));
    // A mistyped number: the row doesn't read (as for every kind).
    let m = Modifier::from_value(&json!({"kind": "pathfinder", "precision": "fine"})).unwrap();
    assert!(matches!(m.kind, ModifierKind::Other { error: Some(_), .. }));
}

#[test]
fn clamping() {
    let (c, w) = ImageTrace::default().clamped();
    assert_eq!(c, ImageTrace::default());
    assert!(w.is_empty(), "{w:?}");
    let wild = ImageTrace {
        colors: 1000,
        color_detail: f64::NAN,
        threshold: 999,
        paths: -4.0,
        corners: 400.0,
        noise: 0,
        stroke_width: 0.0,
        palette: TracePalette::DocumentLibrary,
        fills: false,
        strokes: false,
        ignore_color: Some("nope".into()),
        ..ImageTrace::default()
    };
    let (c, w) = wild.clamped();
    assert_eq!((c.colors, c.color_detail, c.threshold, c.paths, c.corners, c.noise, c.stroke_width), (256, 50.0, 255, 0.0, 100.0, 1, 1.0));
    assert_eq!(c.palette, TracePalette::Limited);
    assert!(c.fills && !c.strokes, "both off: fills back on");
    assert_eq!(c.ignore_color, None);
    assert_eq!(w.len(), 10, "{w:?}");
    // Logo mode takes at most 10 colours.
    let (c, _) = ImageTrace { mode: TraceMode::Logo, colors: 20, ..ImageTrace::default() }.clamped();
    assert_eq!(c.colors, 10);
    let (c, w) = Pathfinder { precision: 0.0, outline_width: f64::INFINITY, ..Pathfinder::default() }.clamped();
    assert_eq!((c.precision, c.outline_width), (0.001, 1.0));
    assert_eq!(w.len(), 2);
}

#[test]
fn presets_hold_vectorcrafts_values() {
    for p in TracePreset::ALL {
        let f = preset_fields(p.id());
        assert_eq!(f.is_some(), p != TracePreset::Custom, "{p:?}");
        assert_eq!(p.vectorcraft_name().is_some(), p != TracePreset::Custom);
        if let Some(f) = f {
            assert_eq!(f.preset, p);
            assert_eq!(f.clamped().1, Vec::<String>::new(), "{p:?} is in range");
        }
        assert_eq!(TracePreset::from_id(p.id()), Some(p));
    }
    assert_eq!(preset_fields("default"), Some(ImageTrace::default()));
    assert!(preset_fields("nope").is_none());
    let a = preset_fields("autoColor").unwrap();
    assert_eq!(
        (a.mode, a.palette, a.color_detail, a.colors, a.paths, a.corners, a.noise),
        (TraceMode::Color, TracePalette::Automatic, 85.0, 6, 65.0, 60.0, 16)
    );
    let h = preset_fields("highFidelityPhoto").unwrap();
    assert_eq!((h.colors, h.paths, h.corners, h.noise, h.method), (64, 90.0, 25.0, 4, TraceMethod::Overlapping));
    let g = preset_fields("shadesOfGray").unwrap();
    assert_eq!((g.mode, g.colors, g.method), (TraceMode::Grayscale, 8, TraceMethod::Overlapping));
    let l = preset_fields("blackAndWhiteLogo").unwrap();
    assert_eq!((l.threshold, l.paths, l.corners, l.noise, l.ignore_white, l.snap_curves_to_lines), (128, 95.0, 80.0, 8, true, true));
    let s = preset_fields("silhouettes").unwrap();
    assert_eq!((s.threshold, s.paths, s.noise, s.ignore_white), (200, 40.0, 30, true));
    let o = preset_fields("outline").unwrap();
    assert!(!o.fills && o.strokes && o.ignore_white);
    assert_eq!((o.paths, o.corners, o.noise), (80.0, 70.0, 10));
    let f = preset_fields("flatLogo").unwrap();
    assert_eq!((f.mode, f.colors, f.noise, f.snap_curves_to_lines, f.ignore_white), (TraceMode::Logo, 8, 6, true, false));
}

#[test]
fn set_props_applies_presets() {
    let mut m = Modifier::new("t", "imageTrace").unwrap();
    set(&mut m, json!({"preset": "sixteenColors"}));
    assert_eq!(*trace(&m), preset_fields("sixteenColors").unwrap());
    // A field set with the preset wins; the preset stays.
    set(&mut m, json!({"preset": "threeColors", "noise": 50}));
    assert_eq!(trace(&m).preset, TracePreset::ThreeColors);
    assert_eq!((trace(&m).colors, trace(&m).noise), (3, 50));
    // A field set to the preset's own value keeps it; off it: custom.
    set(&mut m, json!({"colors": 3}));
    assert_eq!(trace(&m).preset, TracePreset::ThreeColors);
    set(&mut m, json!({"colors": 4}));
    assert_eq!(trace(&m).preset, TracePreset::Custom);
    // Choosing custom keeps the fields.
    let before = trace(&m).clone();
    set(&mut m, json!({"preset": "lineArt"}));
    set(&mut m, json!({"preset": "custom"}));
    assert_eq!(trace(&m).preset, TracePreset::Custom);
    assert_eq!(trace(&m).paths, preset_fields("lineArt").unwrap().paths);
    assert_ne!(*trace(&m), before);
    // Enabled alone doesn't touch the preset.
    set(&mut m, json!({"preset": "outline"}));
    set(&mut m, json!({"enabled": false}));
    assert_eq!(trace(&m).preset, TracePreset::Outline);
    assert!(m.set_props(json!({"bogus": 1}).as_object().unwrap()).is_err());
}

#[test]
fn a_generator_alone_draws_its_result_once() {
    let out = evaluate(&stack(json!([{"id": "t", "kind": "imageTrace"}])), B);
    assert_eq!(out.transforms().collect::<Vec<_>>(), [Affine::IDENTITY]);
    let g = out.generator.as_ref().unwrap();
    assert_eq!(g.modifier, "t");
    assert_eq!(g.kind, GeneratorKind::ImageTrace(ImageTrace::default()));
    assert_eq!(g.kind.id(), "imageTrace");
    assert_eq!(g.inputs.len(), 1);
    assert_eq!(g.inputs[0].transform, Affine::IDENTITY);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    // Disabled: no generator, nothing active.
    let out = evaluate(&stack(json!([{"id": "t", "enabled": false, "kind": "pathfinder"}])), B);
    assert!(out.generator.is_none() && out.instances.is_empty());
    // Without a generator nothing changes.
    let out = evaluate(&stack(json!([{"id": "a", "kind": "linearArray", "count": 3, "offsetX": 10}])), B);
    assert!(out.generator.is_none());
    assert_eq!(out.instances.len(), 3);
    // Clamping warnings name the row; the params are clamped.
    let out = evaluate(&stack(json!([{"id": "p", "kind": "pathfinder", "precision": 99}])), B);
    assert_eq!(out.generator.as_ref().unwrap().kind, GeneratorKind::Pathfinder(Pathfinder { precision: 10.0, ..Pathfinder::default() }));
    assert_eq!(out.warnings.len(), 1);
    assert_eq!(out.warnings[0].modifier.as_deref(), Some("p"));
}

#[test]
fn rows_above_are_input_and_rows_below_copy_the_result() {
    let out = evaluate(
        &stack(json!([
            {"id": "a", "kind": "linearArray", "count": 3, "offsetX": 10},
            {"id": "t", "kind": "pathfinder", "op": "merge"},
            {"id": "b", "kind": "linearArray", "count": 2, "offsetX": 100},
        ])),
        B,
    );
    let g = out.generator.as_ref().unwrap();
    assert_eq!(xs(g.inputs.iter().map(|i| i.transform)), [0.0, 10.0, 20.0]);
    assert_eq!(xs(out.transforms()), [0.0, 100.0], "the result's copies");
    assert_eq!(out.instances[1].index, 1);
    assert_eq!(out.instances[1].stage_indices, [1], "indices count the result side only");
}

#[test]
fn per_instance_rows_vary_their_own_side() {
    // Above: they vary the input copies (i, n count the inputs); below: the result's copies.
    let out = evaluate(
        &stack(json!([
            {"id": "a", "kind": "linearArray", "count": 2, "offsetX": 10},
            {"id": "e", "kind": "transformEach", "offsetY": 5, "progressive": true},
            {"id": "t", "kind": "imageTrace"},
            {"id": "b", "kind": "linearArray", "count": 3, "offsetX": 100},
            {"id": "f", "kind": "transformEach", "offsetY": 40, "progressive": true},
        ])),
        B,
    );
    let g = out.generator.as_ref().unwrap();
    let ys = |ts: Vec<Affine>| ts.iter().map(|t| t.translation().y).collect::<Vec<_>>();
    assert_eq!(ys(g.inputs.iter().map(|i| i.transform).collect()), [0.0, 5.0]);
    assert_eq!(ys(out.transforms().collect()), [0.0, 20.0, 40.0]);
    // A per-instance row above, a lattice below: the result's copies still form the lattice.
    let out = evaluate(
        &stack(json!([
            {"id": "e", "kind": "randomize", "position": 5},
            {"id": "t", "kind": "imageTrace"},
            {"id": "g", "kind": "gridArray", "rows": 2, "cols": 2},
            {"id": "s", "kind": "stencil"},
        ])),
        B,
    );
    assert!(out.repeat.is_some(), "{:?}", out.warnings);
    // A lattice above the generator is folded into its input: no repeat for the stencil below.
    let out = evaluate(
        &stack(json!([
            {"id": "g", "kind": "gridArray", "rows": 2, "cols": 2},
            {"id": "t", "kind": "imageTrace"},
            {"id": "s", "kind": "stencil"},
        ])),
        B,
    );
    assert!(out.repeat.is_none());
}

#[test]
fn only_the_first_generator_counts() {
    let out = evaluate(
        &stack(json!([
            {"id": "off", "enabled": false, "kind": "pathfinder"},
            {"id": "t", "kind": "imageTrace"},
            {"id": "p", "kind": "pathfinder"},
            {"id": "b", "kind": "linearArray", "count": 2, "offsetX": 30},
        ])),
        B,
    );
    assert_eq!(out.generator.as_ref().unwrap().modifier, "t");
    assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
    assert_eq!(out.warnings[0].modifier.as_deref(), Some("p"));
    assert_eq!(out.instances.len(), 2, "the rows below the ignored one still run");
}

#[test]
fn a_stencil_below_the_generator_cuts_the_result() {
    let out = evaluate(
        &stack(json!([
            {"id": "a", "kind": "linearArray", "count": 2, "offsetX": 10},
            {"id": "t", "kind": "imageTrace"},
            {"id": "b", "kind": "linearArray", "count": 3, "offsetX": 50},
            {"id": "s", "kind": "stencil"},
            {"id": "c", "kind": "mirror"},
        ])),
        B,
    );
    assert_eq!(out.generator.as_ref().unwrap().inputs.len(), 2);
    assert_eq!(out.instances.len(), 3);
    assert_eq!(out.stencil.as_ref().unwrap().modifier, "s");
    assert_eq!(out.warnings.len(), 1, "the row below the stencil: {:?}", out.warnings);
    // A stencil above a generator ends the stack before it.
    let out = evaluate(&stack(json!([{"id": "s", "kind": "stencil"}, {"id": "t", "kind": "imageTrace"}])), B);
    assert!(out.generator.is_none() && out.stencil.is_some());
}

#[test]
fn the_instance_cap_counts_each_side() {
    // 100 × 100 above (10 000: fits), 100 × 50 below (5 000: fits): 10 000 + 5 000 in all.
    let out = evaluate(
        &stack(json!([
            {"id": "a", "kind": "gridArray", "rows": 100, "cols": 100},
            {"id": "t", "kind": "pathfinder"},
            {"id": "b", "kind": "gridArray", "rows": 100, "cols": 50},
        ])),
        B,
    );
    assert_eq!(out.generator.as_ref().unwrap().inputs.len(), 10_000);
    assert_eq!(out.instances.len(), 5_000);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    // Too many above: the rows up to the generator are skipped (with a warning); the generator
    // and the rows below still run.
    let out = evaluate(
        &stack(json!([
            {"id": "a", "kind": "gridArray", "rows": 200, "cols": 100},
            {"id": "x", "kind": "linearArray", "count": 2},
            {"id": "t", "kind": "pathfinder"},
            {"id": "b", "kind": "linearArray", "count": 4, "offsetX": 5},
        ])),
        B,
    );
    assert_eq!(out.generator.as_ref().unwrap().inputs.len(), 1);
    assert_eq!(out.instances.len(), 4);
    assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
    // Too many below: capped there; a stencil further down still ends the stack.
    let out = evaluate(
        &stack(json!([
            {"id": "t", "kind": "pathfinder"},
            {"id": "a", "kind": "linearArray", "count": 200},
            {"id": "b", "kind": "linearArray", "count": 200},
            {"id": "s", "kind": "stencil"},
        ])),
        B,
    );
    assert_eq!(out.instances.len(), 200);
    assert!(out.stencil.is_some());
    assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
}

#[test]
fn nodes_carry_the_generator() {
    let mut n = figcraft_doc::Node::new(figcraft_doc::NodeId(3), figcraft_doc::NodeKind::Rectangle, "r", 20.0, 10.0);
    n.plugin_data.insert(
        PLUGIN_KEY.into(),
        to_json(&stack(json!([{"id": "a", "kind": "linearArray", "count": 2, "offsetX": 30}, {"id": "t", "kind": "imageTrace"}]))),
    );
    let out = node_instances(&n).unwrap();
    assert_eq!(out.instances.len(), 1);
    assert_eq!(out.generator.as_ref().unwrap().inputs.len(), 2);
    assert!(has_modifiers(&n));
    let s = node_stack(&n).unwrap().unwrap();
    assert_eq!(s.generator().map(|(i, m)| (i, m.id.as_str())), Some((1, "t")));
}
