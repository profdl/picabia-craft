# Picabia app icon

<!-- PICABIA: replaces FigCraft's chameleon README. The files keep their figcraft-* names so
     `cargo xtask icon`, the packaging scripts and `include_bytes!` paths stay untouched. -->

A **ring of eight dots**: one yellow original and seven orange copies around it, the picture of a
circular-array modifier. Placeholder, drawn from simple geometry.

## Palette

| Colour | Hex | Used for |
|---|---|---|
| Night | `#1d1b3a` | the full-bleed field |
| Copy orange | `#ff5a36` | the seven copies |
| Original yellow | `#ffd23f` | the original (top) |

## Geometry

A 512-unit tile (`viewBox="0 0 512 512"`), rounded square `rx=112`; eight circles of radius 52 on a
ring of radius 150 around the centre. The macOS renders pad it onto Apple's 824/1024 icon grid.

## Provenance

Original work by the Picabia contributors, MIT OR Apache-2.0. FigCraft's chameleon drawing is not
used by Picabia. See `LICENSE.txt` and `ATTRIBUTION.md`.

## Files

Names are FigCraft's (kept so upstream merges stay quiet): `figcraft.svg` and `figcraft-small.svg`
are the masters; `figcraft-<size>.png`, `figcraft.ico` and `hicolor/...` are rendered by
`cargo xtask icon` (needs `resvg`: `cargo install resvg`).
