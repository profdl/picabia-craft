//! Picabia's adapter to the vendored VectorCraft crates (layer 3): the only code that knows both
//! apps' types. FigCraft's `kurbo` paths go in, VectorCraft's Pathfinder booleans and Image Trace
//! run, `kurbo` paths come out. Pure and headless (no egui, render or engine).
//!
//! * [`path`]: `BezPath` ↔ VectorCraft's `PathData`, fill rules, transforms. Non-finite input
//!   becomes an empty path, never a NaN.
//! * [`pathfinder`]: the ten Pathfinder operations over a back-to-front stack, with budgets.
//! * [`trace`]: Image Trace over an RGBA8 raster, with pixel and anchor budgets and cancel.
//!
//! Nothing here panics: bad input is an empty result or a [`BridgeError`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod error;
pub mod path;
pub mod pathfinder;
pub mod trace;

pub use error::BridgeError;
