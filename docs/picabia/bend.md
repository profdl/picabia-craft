# Designing a game's interface: `picabia-bend`

`apps/picabia-bend` turns a design's `Screens` page into the interface of a game. The design is
converted once (`convert.rs`, into the tree of `model.rs`) and emitted for the side that draws it:

- **Unreal Slate** (`picabia-bend slate`), for Timber Tactician (`ue-bend`), where the engine
  draws and lays out the screens and the game, in Bend, specifies them: which screen is shown,
  what each reads and which commands each admits (`bend/Interface/Screens.bend`). The game prints
  that specification as a schema (`bend/Interface/Screens_Schema.bend`); the design is held to it
  both ways before anything is generated, and the generated widgets are bound to each screen's
  read and commands by the C names of the game's generated header.
- **Bend data** (`picabia-bend -o`), a module over a game's own interface vocabulary that the
  game lays out itself, with `picabia-bend oracle` holding its layout to Picabia's.

```sh
picabia-bend slate ui/Timber.figcraft --schema screens.tsv \
  --public Source/BendRuntime/Public --private Source/BendRuntime/Private \
  --font Roboto-Medium.ttf --font Roboto-Bold.ttf
picabia-bend ui/Timber.figcraft -o bend/Ui/Timber.bend --font Roboto-Medium.ttf --font Roboto-Bold.ttf
```

Only the fonts given with `--font` are used (never the machine's): pass the files the design is
drawn in.

## Conventions

| In the design | In the game |
|---|---|
| A page named `Screens`, one top-level frame per screen, named in kebab case (`hud-play`) | The game's screen of that name: every screen the game has must be there, and no other |
| A text whose characters are `{boards_left}` | The screen's count `boards_left`, shown as a number |
| An instance with a text property `@action` set (`pick-saw`) | Pressable: a click gives the screen's command `pick-saw` |
| Its component set's `State` variants: `Idle`, and optionally `Hovered`, `Pressed`, `Selected` | What it shows in each state; a state without a variant shows `Idle` |
| An instance with a text property `@shown` set (`while-building`) | Shown only while the screen's flag `while_building` holds |

A property left empty binds nothing: a component offering `@action` or `@shown` gives it an empty
default. Every other property is the design's own (a button's `label`, say). With `slate`, a
screen's every count, flag and command must be bound by something in the design, and the design
may name nothing the screen lacks; the schema's screen may also read a selection, which shows the
command in force in its pressable's `Selected` variant.

## The schema

One fact a line, its fields by tabs: `screen <name> <c lower> <C upper>`, `command <screen> <name>
<C enumerator>`, `count <screen> <slot> <C field>`, `flag <screen> <slot> <C field>`, `selection
<screen> <slot> <C selection field> <C command field>`.

## What Slate is generated

`BendScreens.h` (the engine's types of each screen's read and commands, a widget per screen that
gives only its own commands, and `SBendScreens`, the switcher over them), `BendScreens.cpp` (the
widgets), and `BendScreensWire.h` (the parse of a shown screen from the C ABI and the print of a
command). Auto layout becomes horizontal and vertical boxes with the design's padding, gaps and
alignments; a free frame's children sit on a constraint canvas anchored as their constraints pin
them; surfaces are rounded box brushes (fills composed, one inside stroke, radii); opacity and
clipping are kept; a pressable's `Idle`, `Hovered` and `Pressed` variants are one button style
and its `Selected` variant another, so its variants may differ only in fill, stroke and text
colour; texts are in the design's fonts at its sizes and letter spacing. Hover and press are
Slate's. Line heights are the font's own.

## What is drawn

Frames, rectangles and single-line, single-style texts; solid fills; one solid inside stroke;
corner radii; opacity; clipping. Auto layout on one line (fixed, hug and fill sizes, padding, gap,
the six main-axis alignments, start, centre and end across) and constraints in frames without auto
layout. Everything else (gradients, effects, vectors and other shapes, groups, grids, wrapping,
min and max sizes, rotation, layers ignoring auto layout) is refused with the layer's path, all
refusals at once.
