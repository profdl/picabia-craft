# Picabia roadmap

**Stage:** P0–P7 landed (2026-10-29); latest upstream merge `PIC-merge: upstream f702105e..53259cf5` (2026-11-01; landed as a PR: `cargo xtask ci` not yet run on it, the sync run could not download crates).
Stencil P8–P12 landed (2026-10-31): the stencil exports for the laser, and a test card measures each
material. Laser control (P17, stencil phase P13) landed in simulation (2026-11-01): Picabia writes GRBL G-code for any design
and streams it to the laser itself. Next: connect the X-Carve, air run, power ladder and test card, and replace the material guesses, then polish (handles for the
remaining kinds, per-kind undo labels), DXF and `separate` colour mode, and the release packaging ids.

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
| P11 | Stencil repeat, limits and materials | B | Repeat-aware islands and ties under a Wallpaper/Grid row (3×3 periodic block, ties placed once per cell and repeated, fallback hold); repeat tile output (one cell + margin, placed without waiting on compute, exported by svg/pdf); registration holes (sheet margin corners, tile cell corners); limit checks (narrow paper, narrow cut, small cut/hole, sharp corner, untied) with fix or flag mode, panel rows per kind that cycle and pan to canvas markers; material presets (paper, film, card, custom) filling the limits in one step. Gaps: in fix mode narrow checks only look near ties, hoshi and sheet/tile edges; pattern ties ignore the repeat; a 3×3 repeat of traced.svg takes ~3.3 s native (off the UI thread), wasm unmeasured; the cell choice is duplicated in render (layering; a test guards drift); tile-mode stats mix sheet island counts with tile cut counts; preset values are guesses until measured with the test card |
| P12 | Stencil laser export and test card | C | Export for laser… (`modifier.stencilExport`, dialog, panel button, Object ▸ Modifiers): SVG at true size in mm, hairline, Engrave/Score/Cut layers with configurable colours; kerf/2 into the removed side (cut-outs shrink, outline grows); cleaned contours cut inner first, the outline last, nearest neighbour; bed split into overlapping sheets with matching registration holes and score lines, loose pieces counted per sheet; engraved label (name, size, material, sheet k of n, THIS SIDE UP) in an original single-stroke font; settings remembered; the dialog's summary is a dry run. Laser test card… (`modifier.stencilTestCard`): 120 × 80 mm ladders of bridges, holes, slits and corner angles marked at the material's limits, and a kerf gauge. Graded C: never opened in laser software or cut. Gaps: score lines cross cut-outs; an overlap band with no clear spot drops its registration hole (warning); contour direction not normalised; no DXF; wasm timing unmeasured |
| P12 | Vendored VectorCraft crates (geom, pathops, trace) | C | `vendor/vectorcraft/` holds VectorCraft's `geom`, `pathops` and `trace` at `5a9a14fd`, byte-identical except three manifests; sha256 list checked by `cargo xtask vendor-check` (part of `ci`); `cargo xtask vendor-vectorcraft <commit>` re-syncs. Their own tests pass here (geom 80, pathops 136+65, trace 226+14). Nothing in Picabia uses them yet. Gaps: no modifier calls them, VectorCraft's property tests not vendored (need its testkit), trace is L2 so an L2 consumer needs a layer exception |
| P12.5 | `figcraft-vcbridge` adapter crate (L3) | B | Paths (`BezPath` <-> VectorCraft `PathData`, same kurbo 0.13, non-finite dropped), Pathfinder (`Op`, budgets: 5000 shapes, the vendored segment-pair limit as `TooComplex`) and Image Trace (`TraceSettings` for every imageTrace field, 15 presets checked against VectorCraft's, pixel and anchor budgets, cancel). Unit tests only; no modifier calls it yet |
| P13 | `imageTrace` generator | B | **Framework** (PIC13.0) plus the **trace** (PIC13.1–13.3): any layer with a visible image fill is rendered alone in its own space at its densest image fill's pixel density (Fill/Fit/Crop/Tile and 90° rotations; clamped 0.05–16 px/pt; capped at 16 MP native, 1 MP wasm (PIC13.4; 4 MP froze the page for up to ~6 s), and 16384 px a side before allocating, with a warning) and traced by VectorCraft's tracer with every field mapped; paths placed at each input copy, fills non-zero, strokes as centre lines (`strokeWidth`/density, round joins); its own effects left out; stats (paths, anchors, colours, tracedPx) and warnings (no image, empty, scaled down, slower over 1500 px, too detailed keeps the previous result). Panel: preset, mode, the mode's slider, palette, an Advanced group (paths, corners, noise, method, Fills/Strokes with one kept on, stroke width, snap, ignore white with an optional colour), "Tracing…"/"Paths · Anchors · Colours" status; checked in the app light and dark (photo, B&W and flat logos, trace → stencil). A 4000×3000 synthetic photo traces end to end through `modifier.resultInfo` in 0.65–1.3 s natively (release, Apple M5 Pro, by preset; the alone render 40–110 ms). The tracer alone at the 4 MP wasm cap, in Node's V8 (not a browser): 0.3–5.4 s by preset, 1.4–10× native; on wasm it runs on the UI thread after the 300 ms settle, so a large photo freezes the page that long. Gaps: no cancel of a stale trace (the job runs to the end); the anchor-budget fallback is untested (8 M anchors can't be reached in a test); wasm time not measured in a browser (Node only), and a wasm trace blocks the UI (capped at 1 MP to keep it near a second; next: trace in a web worker and raise the cap); Shapes, Auto Grouping, Gradients, Document Library and logo colours not offered; Mixed-mode multi-selection shows every slider |
| P14 | `pathfinder` generator | B | All ten operations through VectorCraft's Pathfinder (vendored, via `figcraft-vcbridge`) on a group, frame or boolean group's design (copies above the row, frame fill back-most, a boolean group's operands, strokes and text outlined, clips and masks). Shapes keyed by colour; faces keep their source's colour and name. Panel: operation dropdown (divider before Unite), precision, outline width (outline only), "Shapes N → Faces N" and warnings. Apply bakes a group of per-face vectors (undoable); hit testing, save/reopen, SVG/PDF export, `.fig` bake tested; checked in the app (light and dark). Gaps: gradients and images draw as their mean colour (VectorCraft keeps them whole); outline width 0 draws nothing (warned); a dense pile of big curved shapes (over 25,000 overlapping bounding-box pairs) keeps the design with a warning, and just under that can take seconds (270 big ellipses: 8 s, off the UI thread natively, on it in the browser); first 5000 shapes only; no per-op preview of face counts |
| P16 | Interface scope: prototyping, Dev Mode, boards and live sessions left out | B | Hidden from the menus, Actions palette (Recents too), shortcuts (Shift+E, Shift+D, Alt+9, Alt+Cmd+Enter, Shift+Space…), Keyboard shortcuts panel, toolbar modes (Draw and Design only), right panel (no people, Present, Share or Prototype tab) and home screen (New board, Join with a code, Online, their tips). The engine keeps every command, so the CLI, MCP and control channel still run prototypes, and documents keep interactions, flows and annotations; variables are untouched. `picabia_hide` (one switch, set at startup; upstream tests run with it off). Check for updates hidden and the startup check off (PIC16.1): FigCraft's release feed would offer FigCraft over Picabia; Picabia follows FigCraft by merging upstream, and gets its own feed with its release packaging. Not hidden: opening an existing board file, `--join`/`#join=` invite links, annotations |
| P17 | Laser control (stencil phase P13): G-code and a GRBL sender | C | **G-code** for any selected design (closed outlines cut inner first, open paths scored), stencils (per sheet) and the test card, plus a **power ladder**: GRBL laser mode (`M4`, travel `G0`, beam off `M5` at start, after each operation and at the end), Y flipped to the machine's front-left origin, per-material cut/score/engrave speed, power and passes (guesses for a 2.8 W diode), estimated time; `laser.gcode`, `modifier.stencilGcode`, the export dialog's Format choice. **`figcraft-grbl`** (written from the GRBL 1.1 wiki only): status/settings/alarm parsing, 128-byte character counting, handshake, stop = hold → reset → `M5`, an error or alarm mid-job stops it; native serial (`serialport`, MPL-2.0) and a simulated GRBL. **Engine link** (`laser.*`): worker thread, connect/home/unlock/jog/set origin/frame/start/pause/resume/stop, refuses to start unless armed in the desktop app (60 s, one start; scripts, MCP and plugins can't arm), Idle, `$32=1` and on the bed; a running job is stopped on disconnect, a panicking command or frame, and quit. **Laser window** (File ▸ Laser…): port, state, jog pad, profile, job, operation table, six-point safety checklist before Start, always-visible Stop and Shift+Esc anywhere. A full run checked in the app against the simulator (connect, frame, checklist, running, paused, stopped, alarm, done). Graded C: never connected to a real GRBL or cut. Gaps: no canvas overlay of the head and job; placement on the bed only partly checked without homing (a warning, frame first); jogs not bed-checked (soft limits `$20` guard); time left jumps during a run; a failed job's elapsed shows 0:00 and its 'Line N' differs from 'Failed at line'; clipping ignored for designs (cut whole, warned); per-material table only through the job's operation table; Linux/Windows/FreeBSD serial builds unchecked; browser build has no laser control (G-code export works) |

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

- imageTrace in the browser: trace in a web worker (off the UI thread, as natively) and raise the 1 MP wasm cap back to 4 MP or more.
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
