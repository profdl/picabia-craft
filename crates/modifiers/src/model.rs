//! The stack model: a [`Stack`] of [`Modifier`]s, each with an id, an enabled flag and a
//! [`ModifierKind`] carrying its parameters.
//!
//! Every parameter struct reads with `#[serde(default)]` (a missing field takes its default) and
//! writes every field, camelCase. Fields a struct doesn't know are kept in [`Modifier::extra`];
//! kinds this build doesn't know (or whose params don't read) become [`ModifierKind::Other`] with
//! all their params in `extra`, so nothing is lost on a round trip.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{MAX_STACK_LEN, ModError, STACK_VERSION, lenient};

/// The kinds this build can add, as `(kind id, label)`, in menu order.
pub const KINDS: &[(&str, &str)] = &[
    ("linearArray", "Linear array"),
    ("circularArray", "Circular array"),
    ("gridArray", "Grid array"),
    ("mirror", "Mirror"),
    ("transformEach", "Transform each"),
    ("randomize", "Randomize"),
];

/// The label of kind id `kind` ("Linear array"), or `None` for a kind this build doesn't know.
pub fn kind_label(kind: &str) -> Option<&'static str> {
    KINDS.iter().find(|(k, _)| *k == kind).map(|(_, l)| *l)
}

/// One layer's modifier stack. `modifiers[0]` runs first.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    /// The format version read from `"v"` ([`STACK_VERSION`] when missing).
    pub version: u64,
    pub modifiers: Vec<Modifier>,
    /// Top-level fields other than `v` and `stack`, kept on a round trip.
    pub extra: Map<String, Value>,
}

impl Default for Stack {
    fn default() -> Self {
        Stack { version: STACK_VERSION, modifiers: Vec::new(), extra: Map::new() }
    }
}

impl Stack {
    pub fn is_empty(&self) -> bool {
        self.modifiers.is_empty()
    }

    pub fn len(&self) -> usize {
        self.modifiers.len()
    }

    /// The position of the modifier with id `id`.
    pub fn position(&self, id: &str) -> Option<usize> {
        self.modifiers.iter().position(|m| m.id == id)
    }

    /// Whether any modifier is enabled and of a kind this build evaluates.
    pub fn is_active(&self) -> bool {
        self.modifiers.iter().any(|m| m.enabled && !matches!(m.kind, ModifierKind::Other { .. }))
    }

    /// A fresh modifier id: `m<n>` with `n` one past the highest `m<number>` in the stack.
    pub fn next_id(&self) -> String {
        let max = self.modifiers.iter().filter_map(|m| m.id.strip_prefix('m')?.parse::<u64>().ok()).max().unwrap_or(0);
        format!("m{}", max.saturating_add(1))
    }

    /// Insert `m` at `at` (clamped to the end), giving it a fresh id when its id is empty or
    /// already taken. Refused when the stack is full ([`MAX_STACK_LEN`]).
    pub fn insert(&mut self, at: usize, mut m: Modifier) -> Result<&Modifier, ModError> {
        if self.modifiers.len() >= MAX_STACK_LEN {
            return Err(ModError::TooManyModifiers { count: self.modifiers.len() + 1 });
        }
        if m.id.is_empty() || self.position(&m.id).is_some() {
            m.id = self.next_id();
        }
        let at = at.min(self.modifiers.len());
        self.modifiers.insert(at, m);
        self.modifiers.get(at).ok_or_else(|| ModError::Invalid("insert failed".into()))
    }

    /// Give every modifier with an empty or repeated id a fresh one.
    pub(crate) fn fix_ids(&mut self) {
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..self.modifiers.len() {
            let taken = self.modifiers.get(i).is_none_or(|m| m.id.is_empty() || seen.contains(&m.id));
            if taken {
                let id = self.next_id();
                if let Some(m) = self.modifiers.get_mut(i) {
                    m.id = id;
                }
            }
            if let Some(m) = self.modifiers.get(i) {
                seen.insert(m.id.clone());
            }
        }
    }
}

/// One modifier row.
#[derive(Clone, Debug, PartialEq)]
pub struct Modifier {
    /// Unique within its stack (`m1`, `m2`…).
    pub id: String,
    pub enabled: bool,
    pub kind: ModifierKind,
    /// Fields the kind's parameter struct doesn't know (all params for [`ModifierKind::Other`]).
    pub extra: Map<String, Value>,
}

/// What a modifier does, with its parameters.
#[derive(Clone, Debug, PartialEq)]
pub enum ModifierKind {
    LinearArray(LinearArray),
    CircularArray(CircularArray),
    GridArray(GridArray),
    Mirror(Mirror),
    TransformEach(TransformEach),
    Randomize(Randomize),
    /// A kind this build doesn't know, or a known kind whose params don't read (`error` says
    /// why). Kept on a round trip, ignored when evaluating.
    Other {
        kind: String,
        error: Option<String>,
    },
}

/// `linearArray`: `count` copies in a row, each `offsetX`/`offsetY` points further from the one
/// before, turned `rotateEach` degrees more and scaled by `scaleEach` once more (around its own
/// centre).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LinearArray {
    /// Copies including the original (clamped to `1..=MAX_COUNT` when evaluating).
    #[serde(deserialize_with = "lenient::count")]
    pub count: u32,
    /// Points between neighbouring copies.
    pub offset_x: f64,
    pub offset_y: f64,
    /// Degrees each copy turns more than the one before.
    pub rotate_each: f64,
    /// Factor each copy is scaled by relative to the one before (1 = same size).
    pub scale_each: f64,
}

impl Default for LinearArray {
    fn default() -> Self {
        LinearArray { count: 3, offset_x: 100.0, offset_y: 0.0, rotate_each: 0.0, scale_each: 1.0 }
    }
}

/// `circularArray`: `count` copies on a circle of `radius` around the pivot offset by
/// `centerX`/`centerY`, from `startAngle` over `sweep` degrees; `align` turns each copy to follow
/// the circle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CircularArray {
    #[serde(deserialize_with = "lenient::count")]
    pub count: u32,
    pub radius: f64,
    pub start_angle: f64,
    pub sweep: f64,
    pub center_x: f64,
    pub center_y: f64,
    pub align: bool,
}

impl Default for CircularArray {
    fn default() -> Self {
        CircularArray { count: 6, radius: 100.0, start_angle: 0.0, sweep: 360.0, center_x: 0.0, center_y: 0.0, align: true }
    }
}

/// `gridArray`: `rows` × `cols` copies, `gapX`/`gapY` points apart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GridArray {
    #[serde(deserialize_with = "lenient::count")]
    pub rows: u32,
    #[serde(deserialize_with = "lenient::count")]
    pub cols: u32,
    pub gap_x: f64,
    pub gap_y: f64,
}

impl Default for GridArray {
    fn default() -> Self {
        GridArray { rows: 3, cols: 3, gap_x: 100.0, gap_y: 100.0 }
    }
}

/// Which axis a [`Mirror`] reflects across.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MirrorAxis {
    /// A vertical line through the pivot: left and right swap.
    #[default]
    Vertical,
    /// A horizontal line through the pivot: top and bottom swap.
    Horizontal,
    /// Both lines: four quadrants.
    Both,
}

/// `mirror`: reflect across `axis` through the pivot moved `offset` points along the reflection
/// direction; with `keepOriginal` the previous set stays too.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Mirror {
    pub axis: MirrorAxis,
    pub offset: f64,
    pub keep_original: bool,
}

impl Default for Mirror {
    fn default() -> Self {
        Mirror { axis: MirrorAxis::Vertical, offset: 0.0, keep_original: true }
    }
}

/// `transformEach`: move, turn and scale every instance around its own centre; with
/// `progressive` the amounts ramp from none on the first instance to the full amount on the last.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TransformEach {
    pub offset_x: f64,
    pub offset_y: f64,
    pub rotate: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub progressive: bool,
    /// Seeded per-copy variation: each amount above is multiplied by `1 + jitter · r`, `r` in
    /// -1..=1 (0 = off).
    pub jitter: f64,
    #[serde(deserialize_with = "lenient::seed")]
    pub jitter_seed: u32,
}

impl Default for TransformEach {
    fn default() -> Self {
        TransformEach { offset_x: 0.0, offset_y: 0.0, rotate: 0.0, scale_x: 1.0, scale_y: 1.0, progressive: false, jitter: 0.0, jitter_seed: 1 }
    }
}

/// `randomize`: jitter every instance's position (points), rotation (degrees) and scale (a
/// fraction: 0.2 = up to ±20%), deterministically from `seed` and the instance index.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Randomize {
    pub position: f64,
    pub rotation: f64,
    pub scale: f64,
    #[serde(deserialize_with = "lenient::seed")]
    pub seed: u32,
}

/// The fields every modifier has besides its params.
const COMMON: [&str; 3] = ["id", "enabled", "kind"];

fn params_of<T: Serialize>(p: &T) -> Map<String, Value> {
    match serde_json::to_value(p) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Read params `rest` as `T`: numeric strings count as numbers. On success, the fields `T` doesn't
/// know are returned as extras.
fn read<T: for<'de> Deserialize<'de> + Serialize>(rest: &Map<String, Value>) -> Result<(T, Map<String, Value>), String> {
    let coerced: Map<String, Value> = rest
        .iter()
        .map(|(k, v)| {
            let v = match v {
                Value::String(_) => lenient::finite(v).and_then(serde_json::Number::from_f64).map_or_else(|| v.clone(), Value::Number),
                _ => v.clone(),
            };
            (k.clone(), v)
        })
        .collect();
    let p: T = serde_json::from_value(Value::Object(coerced)).map_err(|e| e.to_string())?;
    let known = params_of(&p);
    let extra = rest.iter().filter(|(k, _)| !known.contains_key(*k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    Ok((p, extra))
}

impl ModifierKind {
    /// The kind id written in `"kind"` (`"linearArray"`…).
    pub fn id(&self) -> &str {
        match self {
            ModifierKind::LinearArray(_) => "linearArray",
            ModifierKind::CircularArray(_) => "circularArray",
            ModifierKind::GridArray(_) => "gridArray",
            ModifierKind::Mirror(_) => "mirror",
            ModifierKind::TransformEach(_) => "transformEach",
            ModifierKind::Randomize(_) => "randomize",
            ModifierKind::Other { kind, .. } => kind,
        }
    }

    /// The default parameters of kind id `kind`, or `None` for a kind this build doesn't know.
    pub fn default_for(kind: &str) -> Option<ModifierKind> {
        Some(match kind {
            "linearArray" => ModifierKind::LinearArray(LinearArray::default()),
            "circularArray" => ModifierKind::CircularArray(CircularArray::default()),
            "gridArray" => ModifierKind::GridArray(GridArray::default()),
            "mirror" => ModifierKind::Mirror(Mirror::default()),
            "transformEach" => ModifierKind::TransformEach(TransformEach::default()),
            "randomize" => ModifierKind::Randomize(Randomize::default()),
            _ => return None,
        })
    }

    /// The parameters as a JSON object (empty for [`ModifierKind::Other`]).
    pub fn params(&self) -> Map<String, Value> {
        match self {
            ModifierKind::LinearArray(p) => params_of(p),
            ModifierKind::CircularArray(p) => params_of(p),
            ModifierKind::GridArray(p) => params_of(p),
            ModifierKind::Mirror(p) => params_of(p),
            ModifierKind::TransformEach(p) => params_of(p),
            ModifierKind::Randomize(p) => params_of(p),
            ModifierKind::Other { .. } => Map::new(),
        }
    }

    /// Read kind `kind` from its params `rest`: `(kind, extras)`. Unknown kinds and params that
    /// don't read become [`ModifierKind::Other`] with everything in the extras.
    fn read(kind: &str, rest: &Map<String, Value>) -> (ModifierKind, Map<String, Value>) {
        fn wrap<T>(r: Result<(T, Map<String, Value>), String>, f: fn(T) -> ModifierKind) -> Result<(ModifierKind, Map<String, Value>), String> {
            r.map(|(p, extra)| (f(p), extra))
        }
        let r = match kind {
            "linearArray" => wrap(read(rest), ModifierKind::LinearArray),
            "circularArray" => wrap(read(rest), ModifierKind::CircularArray),
            "gridArray" => wrap(read(rest), ModifierKind::GridArray),
            "mirror" => wrap(read(rest), ModifierKind::Mirror),
            "transformEach" => wrap(read(rest), ModifierKind::TransformEach),
            "randomize" => wrap(read(rest), ModifierKind::Randomize),
            _ => return (ModifierKind::Other { kind: kind.to_string(), error: None }, rest.clone()),
        };
        r.unwrap_or_else(|e| (ModifierKind::Other { kind: kind.to_string(), error: Some(e) }, rest.clone()))
    }
}

impl Modifier {
    /// A new enabled modifier of kind id `kind` with default params, or `None` for a kind this
    /// build doesn't know. An empty `id` is replaced by [`Stack::insert`].
    pub fn new(id: impl Into<String>, kind: &str) -> Option<Modifier> {
        Some(Modifier { id: id.into(), enabled: true, kind: ModifierKind::default_for(kind)?, extra: Map::new() })
    }

    /// The label of this modifier's kind ("Linear array"; the kind id for unknown kinds).
    pub fn label(&self) -> &str {
        kind_label(self.kind.id()).unwrap_or_else(|| self.kind.id())
    }

    /// Read one modifier object. Missing `id` reads as empty, missing `enabled` as true; anything
    /// but an object is refused.
    pub fn from_value(v: &Value) -> Result<Modifier, ModError> {
        let Value::Object(o) = v else {
            return Err(ModError::Invalid(format!("a modifier must be an object, got {}", short(v))));
        };
        let id = match o.get("id") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        let enabled = !matches!(o.get("enabled"), Some(Value::Bool(false)));
        let kind = o.get("kind").and_then(Value::as_str).unwrap_or_default();
        let rest: Map<String, Value> = o.iter().filter(|(k, _)| !COMMON.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
        let (kind, extra) = ModifierKind::read(kind, &rest);
        Ok(Modifier { id, enabled, kind, extra })
    }

    /// The modifier as one flat JSON object: `id`, `enabled`, `kind`, its params and its extras.
    pub fn to_value(&self) -> Value {
        let mut o = self.extra.clone();
        o.extend(self.kind.params());
        o.insert("id".into(), Value::String(self.id.clone()));
        o.insert("enabled".into(), Value::Bool(self.enabled));
        o.insert("kind".into(), Value::String(self.kind.id().to_string()));
        Value::Object(o)
    }

    /// Change params from `props` (camelCase names, as written in the file; `enabled` too).
    /// Refused, leaving the modifier unchanged, when a name isn't one of the kind's params, a
    /// value has the wrong type or a number isn't finite, or the kind is unknown.
    pub fn set_props(&mut self, props: &Map<String, Value>) -> Result<(), ModError> {
        if let ModifierKind::Other { kind, .. } = &self.kind {
            return Err(ModError::Invalid(format!("can't edit a `{kind}` modifier (unknown kind)")));
        }
        let mut params = self.kind.params();
        let mut enabled = self.enabled;
        for (k, v) in props {
            match k.as_str() {
                "enabled" => enabled = v.as_bool().ok_or_else(|| ModError::Invalid(format!("`enabled` must be true or false, got {}", short(v))))?,
                "id" | "kind" => return Err(ModError::Invalid(format!("`{k}` can't be changed"))),
                _ => {
                    let Some(cur) = params.get(k) else {
                        let names: Vec<&str> = params.keys().map(String::as_str).chain(["enabled"]).collect();
                        return Err(ModError::Invalid(format!("`{}` has no param `{k}` (it takes {})", self.kind.id(), names.join(", "))));
                    };
                    let v = if cur.is_number() {
                        let f = lenient::finite(v).ok_or_else(|| ModError::Invalid(format!("`{k}` must be a finite number, got {}", short(v))))?;
                        serde_json::Number::from_f64(f).map(Value::Number).ok_or_else(|| ModError::Invalid(format!("`{k}` must be finite")))?
                    } else {
                        v.clone()
                    };
                    params.insert(k.clone(), v);
                }
            }
        }
        match ModifierKind::read(self.kind.id(), &params) {
            (ModifierKind::Other { error, .. }, _) => Err(ModError::Invalid(error.unwrap_or_else(|| "bad params".into()))),
            (kind, _) => {
                self.kind = kind;
                self.enabled = enabled;
                Ok(())
            }
        }
    }
}

/// A value for an error message, cut short.
pub(crate) fn short(v: &Value) -> String {
    let s = v.to_string();
    match s.char_indices().nth(40) {
        Some((i, _)) => format!("{}…", s.get(..i).unwrap_or_default()),
        None => s,
    }
}
