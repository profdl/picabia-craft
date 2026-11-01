//! Picabia harness for vectorcraft-pathops's integration tests. It reproduces
//! `pathops/tests/it/main.rs` at the pinned commit except for `mod prop_pathops;`, which needs
//! VectorCraft's `vectorcraft-testkit` (not vendored). Registered as `[[test]] name = "it"`.
//! Integration tests: one test binary for the crate, one module per area.
// Unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../pathops/tests/common/mod.rs"]
mod common;

#[path = "../pathops/tests/it/hostile_params.rs"]
mod hostile_params;
#[path = "../pathops/tests/it/ops.rs"]
mod ops;
#[path = "../pathops/tests/it/pathfinder_pinned.rs"]
mod pathfinder_pinned;
#[path = "../pathops/tests/it/props.rs"]
mod props;
#[path = "../pathops/tests/it/regressions.rs"]
mod regressions;
