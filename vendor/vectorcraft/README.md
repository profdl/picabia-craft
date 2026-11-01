# Vendored VectorCraft crates

Source: <https://github.com/storytold/vectorcraft> (VectorCraft, the sibling Illustrator-class app;
Picabia's working fork is `~/Documents/GitHub/vectorcraft/picabia-vector`).
Pinned commit: `5a9a14fdbd5f566be5adb19a45decbbef1c22270`
Licence: MIT OR Apache-2.0 (same as FigCraft; credited in `NOTICE`).

## Why vendored

Picabia's Image Trace and Pathfinder modifiers need VectorCraft's raster tracer and its boolean
path operations. Crafting apps never share code through crates (repos don't share code), so the
three crates are copied here, unmodified, and pinned to a commit. Picabia's own code uses them as
`vectorcraft-geom`, `vectorcraft-pathops` and `vectorcraft-trace` (workspace dependencies), next to
FigCraft's own `figcraft-geom` and `figcraft-pathops`.

## Included

| Directory | Package | Layer (`cargo xtask layers`) |
|---|---|---|
| `geom/` | `vectorcraft-geom` | L0 |
| `pathops/` | `vectorcraft-pathops` | L2 |
| `trace/` | `vectorcraft-trace` | L2 |

Everything in those crates at the pinned commit: `src/`, `tests/`, `README.md`, except the files below.

## Excluded, and why

- `geom/tests/prop_geom.rs` and `pathops/tests/it/prop_pathops.rs`: property tests that use
  `vectorcraft-testkit`, which depends on VectorCraft's engine and is not vendored. Neither file is
  in this tree. (`pathops/tests/it/main.rs` is vendored but unused, see below.)
- Everything else in VectorCraft (engine, UI, apps, testkit, ...).

## Rules

- Every vendored file except the three `Cargo.toml` files is byte-identical to VectorCraft's at the
  pinned commit. Never edit them: `vendor/vectorcraft/HASHES` (sha256 of each) is checked by
  `cargo xtask vendor-check`, which is part of `cargo xtask ci`, and fails on a changed, missing or
  extra file. `.gitattributes` here (`* -text`) keeps line endings as they are.
- A fix belongs in VectorCraft; then run the sync below.
- Picabia-written files live outside the crate directories: `picabia-tests/`, this README,
  `HASHES`, `.gitattributes`.

## Manifest differences (the only edited files)

All three: `version`, `edition` and `license` still come from Picabia's `workspace.package`
(0.12.1 here, 0.19.0 upstream; same edition and licence). Dependencies come from Picabia's
`[workspace.dependencies]` (`kurbo`, `thiserror`, `serde`, `image`, `proptest`, `serde_json`) as
upstream does, and `vectorcraft-geom` is a path dependency.

- `geom/Cargo.toml`: dev-dependency `vectorcraft-testkit` removed.
- `pathops/Cargo.toml`: dev-dependency `vectorcraft-testkit` removed; `linesweeper = "0.5"` given
  directly (VectorCraft needs 0.5, Picabia's workspace pins 0.4 for FigCraft's own `figcraft-pathops`;
  both versions coexist in the lockfile); `autotests = false` plus a `[[test]] name = "it"` whose
  path is the harness `picabia-tests/pathops_it.rs` (below).
- `trace/Cargo.toml`: no change besides the lints.
- `[lints]` in all three: `workspace = true` is replaced by VectorCraft's own lint table (copied
  from its workspace, same denies: `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`,
  `unreachable`, `cognitive_complexity`, `too_many_arguments`, `wildcard_imports`), with
  `unsafe_code = "deny"` instead of FigCraft's `"forbid"`. Reason: the trace quality tests
  (`trace/tests/quality/main.rs`) carry an `#[allow(unsafe_code)]` that `forbid` rejects (error
  E0453). No lint is allowed beyond that; the shipped (`src/`) code has no `unsafe`, and clippy
  `--all-targets -D warnings` passes clean.

## Test harness

`pathops/tests/it/main.rs` declares `mod prop_pathops;`, which needs the missing testkit. So
`pathops` sets `autotests = false` and `picabia-tests/pathops_it.rs` (Picabia's file) reproduces
`main.rs`'s attributes and `#[path]`-includes `tests/common/mod.rs` and every module except
`prop_pathops`. Upstream's `main.rs` stays vendored (hashed) but is not built. The sync warns if
upstream adds a test module the harness lacks. `trace`'s tests (`quality`, `shared_boundaries`,
`enhanced_preset_name`) build as upstream does and need nothing outside the crate.

## Updating

```sh
cargo xtask vendor-vectorcraft <commit> [--from <vectorcraft checkout>]
```

Re-extracts the three crates at `<commit>` with `git archive` (never the working tree; the default
checkout is `../../vectorcraft/picabia-vector`, else `$VECTORCRAFT_DIR`), keeps our manifests,
rewrites the `Pinned commit:` line above and regenerates `HASHES`. Then: review upstream manifest
changes (the command prints the `git diff` to run) and re-apply any by hand, extend
`picabia-tests/pathops_it.rs` if it warns, run `cargo xtask ci`, update the commit id in
`docs/picabia/ROADMAP.md`, and commit.
