//! Picabia modifier stacks: a live, non-destructive list of modifiers per layer.
//!
//! A layer's stack lives in its plugin data under [`PLUGIN_KEY`] (`"picabia.modifiers"`) as a JSON
//! string, so FigCraft's document model, files, collaboration and branches carry it unchanged:
//!
//! ```json
//! {"v":1,"stack":[{"id":"m1","enabled":true,"kind":"linearArray","count":5,"offsetX":120}]}
//! ```
//!
//! Params are flat camelCase fields next to `kind`. Units: points for lengths, degrees for angles,
//! factors for scale (1 = 100%). Unknown kinds and unknown fields are kept on a round trip
//! ([`parse`] then [`to_json`]) and ignored when evaluating, so a newer Picabia's files degrade
//! gracefully in an older one.
//!
//! # Pieces
//! - [`model`]: [`Stack`], [`Modifier`], [`ModifierKind`] and the per-kind parameter structs.
//! - [`parse`] / [`to_json`] / [`stack_from_value`] / [`stack_to_value`]: the one place stacks are
//!   read and written, with the caps below.
//! - [`evaluate`]: a stack plus the layer's bounds (in its parent's space) to an [`Output`] of
//!   [`Instance`] transforms, canvas [`Guide`]s and [`Warning`]s.
//! - [`node_stack`], [`evaluate_node`], [`node_instances`]: cached entry points for a
//!   [`figcraft_doc::Node`], cheap enough to call for every layer on every frame.
//!   [`evaluate_node_in_doc`] / [`node_instances_in_doc`] also resolve `copyToShape` targets
//!   ([`DocTargets`]); the cache key includes a hash of each target's outline, so moving or editing
//!   a target re-evaluates the layers that copy onto it.
//!
//! # Instance transform convention
//! [`Instance::transform`] is applied in the **parent's coordinate space, before the node's own
//! transform**: `world = parent_world * instance.transform * node.transform`. An identity
//! instance is the original layer. An empty [`Output::instances`] means "no active modifiers: draw
//! the layer normally". The pivot is the centre of the layer's bounding box in its parent's space.
//!
//! # Composition
//! The top row runs first (`stack[0]` first). Array stages repeat the **whole previous set as a
//! block**: for copy `k` with transform `T_k` and previous instance `S_j`, the new instance is
//! `T_k * S_j` (`T_k` applied after `S_j`, in parent space), with flat index
//! `k * n_prev + j`. Copy 0 of an array is the identity (the original counts toward `count`).
//! Per-instance stages apply a local transform `L_i` around the pivot before the instance's own
//! transform: `S_i * (pivot · L_i · pivot⁻¹)`.
//!
//! # Caps
//! Sources over [`MAX_SOURCE_BYTES`] and stacks over [`MAX_STACK_LEN`] are refused by [`parse`];
//! array counts are clamped to [`MAX_COUNT`]; the instance product is checked against
//! [`MAX_INSTANCES_PER_NODE`] before allocating (the stages that fit are kept, with a warning);
//! non-finite or huge params are clamped by [`evaluate`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod bind;
#[cfg(test)]
mod bind_tests;
mod cache;
mod eval;
pub mod expr;
#[cfg(test)]
mod expr_tests;
mod kinds;
#[cfg(test)]
mod kinds_tests;
mod lenient;
pub mod model;
mod node;
mod parse;
mod target;
#[cfg(test)]
mod target_tests;
#[cfg(test)]
mod tests;

pub use bind::{
    BINDING_PREFIX, apply_bound_values, binding_key, binding_takes, bound_variable, field_type, is_binding_key, parse_binding_key, stale_binding_keys,
};
pub use eval::{Guide, Instance, Output, StencilRow, Warning, evaluate, evaluate_with, splitmix64};
pub use model::{
    CircularArray, CopyMode, CopyToShape, GridArray, KINDS, LinearArray, MAX_CUT_COLOURS, Mirror, MirrorAxis, Modifier, ModifierKind, Randomize,
    Stack, Stencil, StencilColourMode, StencilPreview, TransformEach, Wallpaper, WallpaperGroup, expr_field, expr_fields, expr_key, hex_colour,
    kind_label, parse_hex_colour,
};
pub use model::{STENCIL_CLOTH, STENCIL_PAPER, STENCIL_PASTE};
pub use node::{
    evaluate_node, evaluate_node_in, evaluate_node_in_doc, evaluate_node_with, has_modifiers, node_bounds, node_instances, node_instances_in_doc,
    node_stack,
};
pub use parse::{parse, parse_cached, stack_from_value, stack_to_value, to_json};
pub use target::{
    DocTargets, MAX_LISTED_TARGETS, MAX_TARGET_ELEMENTS, NoTargets, TargetResolver, TargetShape, check_target, parse_target, stack_targets,
    valid_targets,
};

/// The plugin-data key a layer's stack is stored under.
pub const PLUGIN_KEY: &str = "picabia.modifiers";
/// The stack format version written in `"v"`.
pub const STACK_VERSION: u64 = 1;
/// The longest stack source [`parse`] reads (256 KiB).
pub const MAX_SOURCE_BYTES: usize = 256 * 1024;
/// The most modifiers one layer's stack may hold.
pub const MAX_STACK_LEN: usize = 64;
/// The most copies one array stage makes (its `count`, rows × columns, …).
pub const MAX_COUNT: u32 = 1000;
/// The most instances one layer's stack produces (the product across stages).
pub const MAX_INSTANCES_PER_NODE: usize = 10_000;
/// Lengths (offsets, radii, gaps) are clamped to ±this many points when evaluating.
pub const MAX_LENGTH: f64 = 1.0e6;
/// Angles are clamped to ±this many degrees when evaluating.
pub const MAX_ANGLE: f64 = 1.0e6;
/// Per-copy scale factors are clamped to `0..=MAX_SCALE` when evaluating.
pub const MAX_SCALE: f64 = 100.0;

/// Why a stack couldn't be read or a modifier couldn't be changed.
#[derive(Clone, Debug, PartialEq)]
pub enum ModError {
    /// The source is longer than [`MAX_SOURCE_BYTES`].
    TooLarge { len: usize },
    /// The stack holds more than [`MAX_STACK_LEN`] modifiers.
    TooManyModifiers { count: usize },
    /// The source isn't JSON.
    Json(String),
    /// The JSON doesn't describe a stack, or a parameter value is wrong.
    Invalid(String),
}

impl std::fmt::Display for ModError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModError::TooLarge { len } => write!(f, "modifier data is {len} bytes (at most {MAX_SOURCE_BYTES})"),
            ModError::TooManyModifiers { count } => write!(f, "{count} modifiers (at most {MAX_STACK_LEN} per layer)"),
            ModError::Json(e) => write!(f, "modifier data isn't valid JSON: {e}"),
            ModError::Invalid(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ModError {}
