//! The design printed as a Bend module over the game's vocabulary: one def per node (a short line
//! each, children by name), the fonts' metrics once each, and `screen`, an exhaustive match over
//! the game's screens, so a screen the vocabulary lacks, or one the design lacks, fails to check.

use std::fmt::Write as _;

use crate::model::{
    AlignH, AlignV, Arrange, Bounds, CrossAlign, Design, FontId, HPin, MainAlign, Metrics, Paint, Sizing, Stroke, Surface, TextStyle, UiNode, VPin,
    Words,
};

/// Where the generated module finds the game's vocabulary, relative to itself.
pub struct Imports {
    pub vocabulary: String,
}

/// A float as a Bend `F32` literal: always with a decimal point, never an exponent.
pub fn f32_lit(v: f64) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let mut s = format!("{:.4}", v);
    while s.ends_with('0') && !s.ends_with(".0") {
        s.pop();
    }
    if s == "-0.0" {
        s = "0.0".into();
    }
    format!("({s} : F32)")
}

fn design(v: f64) -> String {
    format!("Ui.MkDesign{{{}}}", f32_lit(v))
}

/// A string as a Bend literal.
pub fn string_lit(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn sizing(s: Sizing) -> &'static str {
    match s {
        Sizing::Fixed => "Ui.Fixed{}",
        Sizing::Hug => "Ui.Hug{}",
        Sizing::Fill => "Ui.Fill{}",
    }
}

fn hpin(p: HPin) -> &'static str {
    match p {
        HPin::Left => "Ui.Left{}",
        HPin::Right => "Ui.Right{}",
        HPin::LeftRight => "Ui.LeftRight{}",
        HPin::Centre => "Ui.HCentre{}",
        HPin::Scale => "Ui.HScale{}",
    }
}

fn vpin(p: VPin) -> &'static str {
    match p {
        VPin::Top => "Ui.Top{}",
        VPin::Bottom => "Ui.Bottom{}",
        VPin::TopBottom => "Ui.TopBottom{}",
        VPin::Centre => "Ui.VCentre{}",
        VPin::Scale => "Ui.VScale{}",
    }
}

fn bounds(b: &Bounds) -> String {
    format!(
        "Ui.MkBox{{{}, {}, {}, {}, {}, {}, {}, {}}}",
        design(b.x),
        design(b.y),
        design(b.w),
        design(b.h),
        sizing(b.size_w),
        sizing(b.size_h),
        hpin(b.pin_h),
        vpin(b.pin_v)
    )
}

fn paint(p: &Paint) -> String {
    format!("Ui.Solid{{Lk.Srgb{{{}}}, Lk.MkShare{{{}}}}}", p.rgb, p.alpha)
}

fn surface(s: &Surface) -> String {
    let fills: Vec<String> = s.fills.iter().map(paint).collect();
    let stroke = match &s.stroke {
        Stroke::Inside { paint: p, weight } => format!("Ui.Inside{{{}, {}}}", paint(p), design(*weight)),
        Stroke::Unstroked => "Ui.Unstroked{}".into(),
    };
    let [a, b, c, d] = s.corners;
    format!(
        "Ui.MkSurface{{[{}], {}, Ui.MkCorners{{{}, {}, {}, {}}}, Lk.MkShare{{{}}}, {}}}",
        fills.join(", "),
        stroke,
        design(a),
        design(b),
        design(c),
        design(d),
        s.opacity,
        if s.clips { "Ui.Clips{}" } else { "Ui.Spills{}" }
    )
}

fn arrange(a: &Arrange) -> String {
    match a {
        Arrange::Free => "Ui.Free{}".into(),
        Arrange::Stack { horizontal, gap, padding: [t, r, b, l], main, cross } => format!(
            "Ui.Stack{{{}, {}, Ui.MkPadding{{{}, {}, {}, {}}}, {}, {}}}",
            if *horizontal { "Ui.Horizontal{}" } else { "Ui.Vertical{}" },
            design(*gap),
            design(*t),
            design(*r),
            design(*b),
            design(*l),
            match main {
                MainAlign::Start => "Ui.Start{}",
                MainAlign::Centre => "Ui.Centre{}",
                MainAlign::End => "Ui.End{}",
                MainAlign::Between => "Ui.Between{}",
                MainAlign::Around => "Ui.Around{}",
                MainAlign::Evenly => "Ui.Evenly{}",
            },
            match cross {
                CrossAlign::Start => "Ui.CrossStart{}",
                CrossAlign::Centre => "Ui.CrossCentre{}",
                CrossAlign::End => "Ui.CrossEnd{}",
            }
        ),
    }
}

fn style(s: &TextStyle) -> String {
    format!(
        "Ui.MkTextStyle{{Ui.MkFontId{{{}}}, {}, {}, {}, {}, {}, {}, {}}}",
        s.font,
        design(s.size),
        design(s.line),
        design(s.baseline),
        design(s.tracking),
        paint(&s.paint),
        match s.align_h {
            AlignH::Left => "Ui.TextLeft{}",
            AlignH::Centre => "Ui.TextCentre{}",
            AlignH::Right => "Ui.TextRight{}",
        },
        match s.align_v {
            AlignV::Top => "Ui.TextTop{}",
            AlignV::Middle => "Ui.TextMiddle{}",
            AlignV::Bottom => "Ui.TextBottom{}",
        }
    )
}

struct Printer {
    defs: String,
    next: usize,
}

impl Printer {
    /// Print `n` as a def of its own (its children first, by name) and return the def's name.
    fn node(&mut self, n: &UiNode) -> String {
        let body = match n {
            UiNode::Frame { bounds: b, arrange: a, surface: s, kids } => {
                let names: Vec<String> = kids.iter().map(|k| format!("{}()", self.node(k))).collect();
                format!("Ui.Frame{{{}, {}, {}, [{}]}}", bounds(b), arrange(a), surface(s), names.join(", "))
            }
            UiNode::Label { bounds: b, style: s, words } => {
                let w = match words {
                    Words::Literal { text, width } => format!("Ui.Literal{{{}, {}}}", string_lit(text), design(*width)),
                    Words::Bound { slot, font } => format!("Ui.Bound{{S.{}{{}}, metrics_{font}()}}", slot.0),
                };
                format!("Ui.Label{{{}, {}, {}}}", bounds(b), style(s), w)
            }
            UiNode::Shown { flag, node } => {
                let inner = self.node(node);
                format!("Ui.Shown{{S.{}{{}}, {inner}()}}", flag.0)
            }
            UiNode::Pressable { action, idle, hovered, pressed, selected } => {
                let (i, h, p, s) = (self.node(idle), self.node(hovered), self.node(pressed), self.node(selected));
                format!("Ui.Pressable{{S.{}{{}}, {i}(), {h}(), {p}(), {s}()}}", action.0)
            }
        };
        let name = format!("node_{}", self.next);
        self.next += 1;
        let _ = write!(self.defs, "def {name}() -> Ui.Node:\n  {body}\n\n");
        name
    }
}

fn metrics(id: FontId, m: &Metrics) -> String {
    let adv: Vec<String> = m.advances.iter().map(|(c, w)| format!("Ui.MkAdvance{{{c}, {}}}", f32_lit(*w))).collect();
    format!(
        "# Font {id}: {} {}.\ndef metrics_{id}() -> Ui.Metrics:\n  Ui.MkMetrics{{{}, {}, [{}]}}\n\n",
        m.family,
        m.style,
        f32_lit(m.upem),
        f32_lit(m.missing),
        adv.join(", ")
    )
}

/// The design as a Bend module. `source` names the design file in the module's header.
pub fn module(d: &Design, source: &str, imports: &Imports) -> String {
    let mut out = String::new();
    let v = &imports.vocabulary;
    let _ = write!(
        out,
        "import Base\nimport {v}/Ui.bend as Ui\nimport {v}/Ui_Slots.bend as S\nimport {v}/Look.bend as Lk\n\n\
         # The interface's design, generated by picabia-bend from {source}: never edit it by hand.\n\
         # Each node is a def of its own, its children by name; `screen` gives each of the game's\n\
         # screens its root.\n\n"
    );
    for (id, m) in d.fonts.iter().enumerate() {
        out.push_str(&metrics(id as FontId, m));
    }
    let mut p = Printer { defs: String::new(), next: 0 };
    let roots: Vec<(String, String)> = d.screens.iter().map(|(c, n)| (c.0.clone(), p.node(n))).collect();
    out.push_str(&p.defs);
    out.push_str("def screen(which: S.Screen) -> Ui.Node:\n  match which:\n");
    for (c, root) in roots {
        let _ = write!(out, "    case S.{c}{{}}:\n      {root}()\n");
    }
    out
}
