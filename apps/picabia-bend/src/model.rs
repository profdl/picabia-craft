//! A design's screens as a checked tree of frames, texts and pressable instances, with their auto
//! layout and constraints as designed: what the converter builds and the Slate emitter folds.

/// How a node sizes on one axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sizing {
    Fixed,
    Hug,
    Fill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HPin {
    Left,
    Right,
    LeftRight,
    Centre,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VPin {
    Top,
    Bottom,
    TopBottom,
    Centre,
    Scale,
}

/// A node's box as designed: where it sits in a free parent, its size, sizing and pins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub size_w: Sizing,
    pub size_h: Sizing,
    pub pin_h: HPin,
    pub pin_v: VPin,
}

/// A solid paint: sRGB packed `0xRRGGBB`, its alpha in thousandths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paint {
    pub rgb: u32,
    pub alpha: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stroke {
    Inside { paint: Paint, weight: f64 },
    Unstroked,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub fills: Vec<Paint>,
    pub stroke: Stroke,
    /// Corner radii `[top left, top right, bottom right, bottom left]`.
    pub corners: [f64; 4],
    /// Thousandths.
    pub opacity: u32,
    pub clips: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainAlign {
    Start,
    Centre,
    End,
    Between,
    Around,
    Evenly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossAlign {
    Start,
    Centre,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arrange {
    /// Auto layout: horizontal or vertical, the gap, padding `[top, right, bottom, left]`.
    Stack {
        horizontal: bool,
        gap: f64,
        padding: [f64; 4],
        main: MainAlign,
        cross: CrossAlign,
    },
    Free,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignH {
    Left,
    Centre,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignV {
    Top,
    Middle,
    Bottom,
}

/// A font, by the number the generated file gives it.
pub type FontId = u32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub font: FontId,
    pub size: f64,
    pub tracking: f64,
    pub paint: Paint,
    pub align_h: AlignH,
    pub align_v: AlignV,
}

/// A name from the game's slot vocabulary, as its Bend constructor (`boards_left` is
/// `BoardsLeft`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Constructor(pub String);

#[derive(Clone, Debug, PartialEq)]
pub enum Words {
    Literal { text: String },
    Bound { slot: Constructor },
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiNode {
    Frame { bounds: Bounds, arrange: Arrange, surface: Surface, kids: Vec<UiNode> },
    Label { bounds: Bounds, style: TextStyle, words: Words },
    Shown { flag: Constructor, node: Box<UiNode> },
    Pressable { action: Constructor, idle: Box<UiNode>, hovered: Box<UiNode>, pressed: Box<UiNode>, selected: Box<UiNode> },
}

/// A font the design is drawn in: its family and style, as the font database names them.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub family: String,
    pub style: String,
}

/// A generated design: its fonts (by id) and its screens, each by its constructor.
#[derive(Clone, Debug, PartialEq)]
pub struct Design {
    pub fonts: Vec<Face>,
    pub screens: Vec<(Constructor, UiNode)>,
}
