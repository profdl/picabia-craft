//! Picabia stencil geometry (layer 3): turn a layer's painted outlines into a laser-cut katagami
//! stencil sheet. Pure geometry: the engine gathers the outlines ([`Paint`]s in the layer's space),
//! this crate computes the sheet.
//!
//! # Pipeline
//! 1. [`visible_regions`]: what each colour shows once everything above it is painted (top-down
//!    subtraction, same-colour runs merged first; booleans, not an arrangement).
//! 2. [`Selector`]: which colours are cut (`threshold` by [`luminance`], or `pick`; `invert` swaps),
//!    [`colour_infos`] for the swatch list, at the physical scale [`mm_per_unit`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod colour;
mod rings;
mod visible;

pub use colour::{ColourInfo, Selector, colour_infos, luminance, mm_per_unit};
pub use visible::{Region, Visible, rgb8, visible_regions};

use figcraft_geom::kurbo::BezPath;

/// The most outline points (after flattening) the whole design may have; shapes past it are left
/// out with a warning.
pub const MAX_POINTS: usize = 2_000_000;

/// One painted outline of the design.
#[derive(Clone, Debug, PartialEq)]
pub struct Paint {
    /// The filled outline in the layer's space, read non-zero (open subpaths closed).
    pub path: BezPath,
    /// Fill colour: sRGB 0–1 and alpha. Translucent paints count as opaque.
    pub colour: [f32; 4],
    /// Paint order, 0 = bottom. Equal orders stack in input order.
    pub order: u32,
}

#[cfg(test)]
mod colour_tests;
#[cfg(test)]
mod visible_tests;
