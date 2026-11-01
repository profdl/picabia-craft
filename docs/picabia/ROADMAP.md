# Picabia roadmap

**Stage:** P0–P7 landed (2026-10-29); first upstream merge done (`PIC-merge: upstream 96eed159..f702105e`).
Stencil P8–P10 landed (2026-10-31). Next: stencil P11 (repeat-aware islands, limits, materials), then polish (handles for
the remaining kinds, per-kind undo labels) and the release packaging ids.

Picabia is a thin fork of [FigCraft](https://github.com/storytold/figcraft): everything FigCraft does,
plus a live, non-destructive **modifier stack** (arrays, mirrors, per-copy transforms, randomisation).
New code lives in new crates and files; edits to FigCraft's files are small marked seams listed in
[`SEAMS.md`](SEAMS.md) (`rg "PICABIA:"` finds them). Upstream is merged weekly. Crate names, the
`.figcraft` file extension and the licence (MIT OR Apache-2.0) are unchanged.

Grades are by behaviour, erring low (A: done and checked in the app; B: works, gaps listed; C: partial).

| Phase | What | Grade | Status |
|---|---|---|---|
| P0 | Fork, rebrand (name, ids, data folders, About, icon), CI guards, spec and docs | B | Name, macOS/web ids, data folder, About, placeholder icon, CI guards and docs done. Linux/FreeBSD/Flatpak/Windows-installer ids and the binary name deferred (below) |
| P1 | `figcraft-modifiers` crate (model, parse with caps, cached evaluate) and `modifier.*` commands | A | Commands add/remove/move/set/toggle/copy/paste/list/remove_all/duplicate/targets/apply; junk sweep; stack survives save/reopen, copy/paste, duplicate, Alt-drag, collab and branch merge (tests); CLI and MCP smoke tests |
| P2 | Draw copies, bounds, hit testing, exports | B | Render, paint/export/dirty bounds, LOD, hit test/hover/marquee as the source, svg/pdf write every copy, .fig/Penpot bake. Gaps: nested copies aren't hit-tested per copy; backdrop blur on copies in svg/pdf; masks and boolean operands with stacks |
| P3 | Design panel "Modifiers" section, built from FigCraft's own widgets | B+ | Built like Effects (header +, rows, kind dropdown, eye, minus, row menu, drag reorder, Mixed, scrub as one step); variable and expression chips; copy-to-shape target picker from `modifier::target_choices` (shared by the selection), refused targets flagged with the reason; Duplicate carries bindings. Screenshots light/dark checked. Linear-array icon reads slightly small |
| P4 | Circular, grid, mirror, transform each, randomize; stacking and caps | B | All kinds and block stacking with per-stage indices; product cap before allocating with a warning. No 10k-copy interactivity benchmark yet |
| P5 | `modifier.apply`, menus, canvas handles, layers badge | B | Apply (and `upTo`), Object/context Modifiers submenu, layers badge, Inspect and a11y lines; handles for mirror axis, ring centre/radius, linear offset, grid gaps (snap, Esc cancel, one undo step). No handles yet for wallpaper, transform each, randomize, copy to shape or for a row whose panel settings are open (per-row focus) |
| P6 | Variable bindings and per-copy expressions | A- | Bindings keyed by modifier id (survive reorder, save, collab, copy/paste/duplicate); expressions `i`, `n`, `t`, `value` with a chip in the panel |
| P7 | Wallpaper groups and copy to shape | B | 17 plane groups; copy to shape on points/along the outline/populate with target checks (self, ancestor, descendant, page, missing, loops), remapping on duplicate/paste/component instances, panel picker. The canvas pick-a-layer tool (instead of the dropdown) isn't built |
| P8 | Stencil foundations: `pathops` offset/open/close, `figcraft-stencil` crate (L3), `stencil` modifier kind (terminal row) | B+ | Offset passes a seeded jagged fuzz sweep; the stencil row saves, loads and draws; Apply keeps it live. S0: the engine computes off the UI thread (wasm: on the UI thread after 300 ms idle) into a render-side store keyed by node id + exact input snapshot, and stale sheets draw meanwhile. traced.svg computes in 97 ms natively, 300 jagged shapes in 0.67 s. wasm compute time not measured |
| P9 | Stencil core: gather, visible regions, colour modes, clean-up, sheet, canvas preview, panel | B | Gather (strokes, text, booleans, clips, masks, copies above the row); threshold/pick/invert; area filter, opening/closing, simplify; paper and dyed-cloth previews on the canvas and in PNG/SVG/PDF; `modifier.stencilInfo`, `modifier.stencilFromSelection`; panel Colours/Size/Clean-up/Preview with the swatch list. Gaps: LOD path and `render_alone`/Copy as PNG can show the design or a stale sheet; clicking a cut-out hole selects the sheet; swatches come from the first selected layer only |
| P10 | Stencil islands and ties | B+ | Island detection (nesting, contour tree); automatic ties (shortest gap, ties per island, width, spread, direction, seed; deterministic), tied outside in, boxed-in islands may tie to an already-held neighbour; gauze mode; pattern (lattice) ties: grid/horizontal/vertical, size, spacing, angle, everywhere or only around islands, fallback ties for missed islands; panel Ties group, stats, canvas markers; traced.svg ends with zero untied islands (~0.1 s). Checked in the app by the owner. Gaps: ties layer (hand-drawn ties, no-tie zones) and text option not built; untied-island marker is a bounding box; panel hides the crate's two untied warnings by exact text |
| P12 | Vendored VectorCraft crates (geom, pathops, trace) | C | `vendor/vectorcraft/` holds VectorCraft's `geom`, `pathops` and `trace` at `5a9a14fd`, byte-identical except three manifests; sha256 list checked by `cargo xtask vendor-check` (part of `ci`); `cargo xtask vendor-vectorcraft <commit>` re-syncs. Their own tests pass here (geom 80, pathops 136+65, trace 226+14). Nothing in Picabia uses them yet. Gaps: no modifier calls them, VectorCraft's property tests not vendored (need its testkit), trace is L2 so an L2 consumer needs a layer exception |
| P12.5 | `figcraft-vcbridge` adapter crate (L3) | B | Paths (`BezPath` <-> VectorCraft `PathData`, same kurbo 0.13, non-finite dropped), Pathfinder (`Op`, budgets: 5000 shapes, the vendored segment-pair limit as `TooComplex`) and Image Trace (`TraceSettings` for every imageTrace field, 15 presets checked against VectorCraft's, pixel and anchor budgets, cancel). Unit tests only; no modifier calls it yet |
| P14 | Interface scope: prototyping, Dev Mode, boards and live sessions left out | B | Hidden from the menus, Actions palette (Recents too), shortcuts (Shift+E, Shift+D, Alt+9, Alt+Cmd+Enter, Shift+Space…), Keyboard shortcuts panel, toolbar modes (Draw and Design only), right panel (no people, Present, Share or Prototype tab) and home screen (New board, Join with a code, Online, their tips). The engine keeps every command, so the CLI, MCP and control channel still run prototypes, and documents keep interactions, flows and annotations; variables are untouched. `picabia_hide` (one switch, set at startup; upstream tests run with it off). Not hidden: opening an existing board file, `--join`/`#join=` invite links, annotations |

**Updating the vendored code:** `cargo xtask vendor-vectorcraft <commit> [--from <checkout>]`, review
manifest changes, `cargo xtask ci`, commit (details in `vendor/vectorcraft/README.md`). Never edit a vendored `.rs` file.

## Known issues

- **Pre-existing upstream test failures** (fail on pristine upstream `96eed159` with the PIC0.2 pin
  applied, proven in a temporary worktree on 2026-10-29; not Picabia's):
  `figcraft-engine board_tables_tests::a_double_click_types_in_the_cell_and_its_row_grows_with_the_text`
  and `figcraft-ui-egui clipboard_fence_tests::a_synthetic_copy_shortcut_leaves_the_system_clipboard_alone`.
  `cargo xtask ci` stops at its test step on these two on this machine; every other gate passes.
- **Clippy 1.96 lints on upstream code** (`nonminimal_bool` in `crates/text/src/fontdb.rs`,
  `manual_range_contains` in `crates/vectordrawable`): fail on pristine upstream too; allowed by two
  one-line seams (PIC0.7, see `SEAMS.md`) until upstream fixes them.
- Undo labels are generic ("Set modifier", "Add modifier"), not per kind ("Add linear array").
- Wallpaper: the lattice guides start at the layer's corner while the motifs sit on the lattice
  nodes, so the guides run past the copies on the right and bottom.
- The warning colour token on white reads light (the panel uses FigCraft's `warning` token as-is).

## Deferred

- Per-kind undo labels.
- Canvas handles for: the row whose settings are open (focus one row's handles), wallpaper,
  transform each, randomize, copy to shape.
- Hit testing nested copies per copy (a copy inside a copied group selects the outer source).
- Masks and boolean operands that carry a stack.
- Backdrop blur on copies in svg/pdf exports.
- Linux/FreeBSD/Flatpak/Windows-installer bundle ids and file names (below); renaming the binary
  from `figcraft` to `picabia`.
- Linear-array icon: draw slightly larger to match the other kind icons.
- A canvas pick-layer tool for copy-to-shape targets (today: the dropdown).
- A 10k-copy pan/zoom benchmark.

## P0 branding, as built

- Name, window titles, About window, Windows resources, web title: "Picabia". The app's identity is
  one file, `apps/figcraft/src/picabia.rs` (`APP_NAME`, `APP_ID = org.picabia.Picabia`, `CONFIG_DIR`).
- Settings, recovery copies and plugins live in the `Picabia` config folder, so Picabia and FigCraft
  run side by side without sharing state. macOS bundle id `org.picabia.Picabia`.
- The shipped binary is still called `figcraft` (the package, `--control` scripting and `cargo run -p
  figcraft` all use it). Renaming it touches `apps/figcraft/Cargo.toml`, every packaging script and
  the CI workflows, which would conflict on each upstream merge, so it is not a cheap seam; revisit
  when the first release is prepared (a `[[bin]] name = "picabia"` entry is the least invasive way).
- Deferred (release packaging, not yet built from this fork): Linux `.desktop`/metainfo/mime/Flatpak
  file names and ids (`ai.storyteller.figcraft.*` to `org.picabia.Picabia.*`), the Windows installer
  (`figcraft.wxs`, install-policy, which still install under `ArtCraft\FigCraft`), the macOS
  executable and icon file names inside the bundle, and the `figcraft://` invite scheme.
- The ArtCraft logos (`docs/brand/`) are not shown by Picabia: the About window, README and
  wordmark no longer use them. The files stay in the tree (they are upstream's, under their own
  licence, and deleting them would conflict on every merge).
- The app icon is an original placeholder (eight dots on a ring), see `assets/app-icon/README.md`.

## Links

- Modifiers, user-facing: [`modifiers.md`](modifiers.md)
- Seams register: [`SEAMS.md`](SEAMS.md)
- FigCraft's own docs: [`../../ROADMAP.md`](../../ROADMAP.md), [`../gaps.md`](../gaps.md)
