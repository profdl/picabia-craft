# Picabia roadmap

**Stage:** P0 (fork, rebrand, spec) landing; P1 (model, evaluator, commands) in progress.

Picabia is a thin fork of [FigCraft](https://github.com/storytold/figcraft): everything FigCraft does,
plus a live, non-destructive **modifier stack** (arrays, mirrors, per-copy transforms, randomisation).
New code lives in new crates and files; edits to FigCraft's files are small marked seams listed in
[`SEAMS.md`](SEAMS.md) (`rg "PICABIA:"` finds them). Upstream is merged weekly. Crate names, the
`.figcraft` file extension and the licence (MIT OR Apache-2.0) are unchanged.

| Phase | What | Status |
|---|---|---|
| P0 | Fork, rebrand (name, ids, data folders, About, icon), CI guards, spec and docs | Branding, icon, CI and docs done; Linux/FreeBSD/Flatpak/Windows-installer id renames deferred (see below) |
| P1 | `figcraft-modifiers` crate (model, parse with caps, `evaluate` for linear array) and `modifier.*` commands | In progress |
| P2 | Draw copies, bounds, hit testing, exports (svg/pdf/png; bake for .fig/Penpot/Sketch) | Not started |
| P3 | Design panel "Modifiers" section, built from FigCraft's own widgets | Not started |
| P4 | Circular, grid, mirror, transform each, randomize; stacking and caps | Not started |
| P5 | `modifier.apply`, menus, canvas handles, layers badge | Not started |
| P6 | Variable bindings and per-copy expressions | Not started |
| P7 | Wallpaper groups and copy to shape | Wallpaper (PIC7.1) and copy to shape (PIC7.2–7.4: points, along path, populate; target checks, remapping on copy) done; the panel's target picker is P3's |

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
