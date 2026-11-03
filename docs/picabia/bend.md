# Designing a game's interface: `picabia-bend`

`apps/picabia-bend` turns a design's `Screens` page into a Bend module over a game's interface
vocabulary (Timber Tactician's `bend/Vocabulary/Ui.bend` and `Ui_Slots.bend`, in `ue-bend`). The
game lays the screens out itself, as Picabia's auto layout and constraints lay them out, and holds
its layout to Picabia's with `picabia-bend oracle`.

```sh
picabia-bend ui/Timber.figcraft -o bend/Ui/Timber.bend --font Roboto-Medium.ttf --font Roboto-Bold.ttf
bend bend/tests/Ui_Design.bend | picabia-bend oracle ui/Timber.figcraft --font …
```

Only the fonts given with `--font` are used (never the machine's): pass the files the design is
drawn in.

## Conventions

| In the design | In the game |
|---|---|
| A page named `Screens`, one top-level frame per screen, named in kebab case (`hud-play`) | `S.HudPlay{}`: every screen of the game's vocabulary must be there, and no other |
| A text whose characters are `{boards_left}` | The game's text slot `S.BoardsLeft{}`, measured with the font's advances |
| An instance with a text property `@action` set (`pick-saw`) | Pressable: a click gives `S.PickSaw{}` |
| Its component set's `State` variants: `Idle`, and optionally `Hovered`, `Pressed`, `Selected` | What it shows in each state; a state without a variant shows `Idle` |
| An instance with a text property `@shown` set (`while-building`) | Shown only while the game's flag `S.WhileBuilding{}` holds |

A property left empty binds nothing: a component offering `@action` or `@shown` gives it an empty
default. Every other property is the design's own (a button's `label`, say).

## What is drawn

Frames, rectangles and single-line, single-style texts; solid fills; one solid inside stroke;
corner radii; opacity; clipping. Auto layout on one line (fixed, hug and fill sizes, padding, gap,
the six main-axis alignments, start, centre and end across) and constraints in frames without auto
layout. Everything else (gradients, effects, vectors and other shapes, groups, grids, wrapping,
min and max sizes, rotation, layers ignoring auto layout) is refused with the layer's path, all
refusals at once.
