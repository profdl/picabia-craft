# Modifiers

> Params as of P7 (this page follows the code).

A **modifier stack** is a list of live, non-destructive steps attached to a layer. The layer keeps
its own geometry; the stack describes how it is repeated and transformed. Edit the layer or any
modifier and every copy follows. Shapes, vectors, text, groups and frames can carry a stack. Copies
are drawn, exported and hit-tested as part of their source layer: clicking a copy selects the
source, and copies never take part in auto layout.

## Evaluation order

**The top row runs first**, like a recipe. Each step acts on everything the step above produced:

- **Array steps** (linear, circular, grid, mirror, wallpaper) repeat the **whole previous set as a block**.
  Grid, then Circular is a ring of grids. Copy 0 of an array is the identity: the original is one of
  the `count` copies.
- **Per-instance steps** (transform each, randomize) change each instance around its own pivot and
  do not add copies.
- A disabled step is skipped. An empty or fully disabled stack draws the layer normally.
- Limits: at most 64 steps, `count` at most 1000 per array, and at most **10 000 instances** per
  layer in total. A stack that would exceed this is cut at the last step that fits, with a warning.

## Pivot and units

- Pivot: the centre of the layer's bounding box, in its parent's coordinate space. Mirror axes and
  per-instance rotation and scale use it. The circular array's `centerX`/`centerY` are an offset
  from this pivot.
- Lengths are points, angles are degrees (positive turns clockwise on screen, like FigCraft's
  rotation field), scale is a factor (1 = 100%).
- Copies are placed in the parent's space before the layer's own transform:
  `world = parent * instance * layer`.

## Index, n and t

Every copy has a flat **index** `i` (0 = the original). After stacking, each array step repeats the
previous block, so `i = k * n_prev + i_prev` where `k` is the copy number within that step. `n` is
the total number of copies and `t = i / (n - 1)` (0 when `n = 1`) runs from 0 to 1 across them.
`index`, `n` and `t` drive the progressive ramp, the randomize seed and per-copy expressions (below).

## Export

SVG, PDF, PNG and JPG draw the copies natively. `.fig`, Penpot and Sketch have no modifier stack,
so those exports bake the copies into a group (as Apply would) and add an export warning. Files keep
the stack: opening a Picabia file in plain FigCraft keeps the data and shows the layer without
copies.

## Parameters

Stored on the layer as the plugin-data key `picabia.modifiers`: a JSON string
`{"v":1,"stack":[{"id":"m1","enabled":true,"kind":"linearArray", ...}]}`. Fields are flat camelCase
next to `kind`. Unknown kinds and fields survive a save and are ignored when drawing. The tables
list defaults and the range a field is clamped to; non-finite numbers are refused.

### linearArray

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `count` | copies | 1 to 1000 | 3 | Total copies, original included |
| `offsetX` | pt | any | 100 | Step between copies, along x |
| `offsetY` | pt | any | 0 | Step between copies, along y |
| `rotateEach` | deg | any | 0 | Extra rotation added per copy, about the pivot of each copy |
| `scaleEach` | factor | > 0 | 1 | Scale multiplied in per copy (1 = no change) |

### circularArray

The original is copy 0 and sits on the ring at `startAngle`; the other copies follow around it.

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `count` | copies | 1 to 1000 | 6 | Total copies, original included |
| `radius` | pt | >= 0 | 100 | Ring radius. The ring centre is this far from the original, opposite the start direction |
| `startAngle` | deg | any | 0 | Angle of the first copy (0 = right of the ring centre, positive turns clockwise on screen) |
| `sweep` | deg | -360 to 360 | 360 | Angle covered. At 360 or -360 copies are spaced `sweep / count` apart, so the last never lands on the first. Below that the last copy lands exactly at the end of the sweep |
| `centerX` | pt | any | 0 | Moves the whole ring (and with it the first copy), x |
| `centerY` | pt | any | 0 | Moves the whole ring, y |
| `align` | on/off | | on | Turn each copy by the ring angle so it keeps facing along the ring |

### gridArray

Copies run row by row from the original at the top left.

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `rows` | copies | 1 to 1000 | 3 | Rows (rows x cols x earlier copies is capped at 10 000) |
| `cols` | copies | 1 to 1000 | 3 | Columns |
| `gapX` | pt | any | 100 | Space between neighbouring copies' bounds, x (the step is the layer's width plus the gap) |
| `gapY` | pt | any | 100 | Space between rows, y (the step is the layer's height plus the gap) |

### mirror

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `axis` | choice | `vertical`, `horizontal`, `both` | `vertical` | `vertical` flips left to right across a vertical line; `horizontal` flips top to bottom; `both` gives four sets (original, left-right, top-bottom, both) |
| `offset` | pt | any | 0 | Moves the axis line away from the pivot, perpendicular to it (for `both`, each line moves this far) |
| `keepOriginal` | on/off | | on | On: previous set plus its reflection(s). Off: only the reflection(s); `both` then gives three sets |

### wallpaper

The 17 plane (wallpaper) symmetry groups. The layer is the motif in cell 0 at the identity; the
other copies are the group's motif transforms in cell 0 plus the same set in every other cell of a
`cols` x `rows` lattice (copies run cell by cell, columns fastest, so copy 0 is the original).
Rotation centres and mirror or glide axes pass through the layer's **top-left corner** (plus
`originX`/`originY`), so a motif drawn inside its cell tiles without overlapping itself. Guides
show the lattice. Copies per cell: p1 1, p2 2, pm 2, pg 2, cm 2, pmm 4, pmg 4, pgg 4, cmm 4, p4 4,
p4m 8, p4g 8, p3 3, p3m1 6, p31m 6, p6 6, p6m 12. The centred groups (cm, cmm) use their primitive
cell, a parallelogram with edges `(w, 0)` and `(w/2, h/2)`.

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `group` | choice | `p1` `p2` `pm` `pg` `cm` `pmm` `pmg` `pgg` `cmm` `p4` `p4m` `p4g` `p3` `p3m1` `p31m` `p6` `p6m` | `p4m` | The symmetry group |
| `cols` | cells | 1 to 100 | 3 | Cells along the first lattice vector |
| `rows` | cells | 1 to 100 | 3 | Cells along the second lattice vector (cols x rows x copies per cell x earlier copies is capped at 10 000) |
| `cellWidth` | pt | 0.001 to 1 000 000 | 100 | Lattice width. For the square groups (p4*) it is the side; for the hexagonal groups (p3*, p6*) it is the lattice constant |
| `cellHeight` | pt | 0.001 to 1 000 000 | 100 | Lattice height for the oblique and rectangular groups (p1, p2, pm, pg, cm, pmm, pmg, pgg, cmm); ignored by the square and hexagonal groups |
| `angle` | deg | 1 to 179 | 90 | Angle between the lattice vectors for the oblique groups p1 and p2; ignored by the others |
| `originX` | pt | any | 0 | Moves the symmetry origin from the layer's top-left corner, x |
| `originY` | pt | any | 0 | Same, y |

### copyToShape

Places copies of everything above it on another layer, the **target**: its outline (its own
shape; a group or boolean is the outlines of its layers; not the target's own modifier copies), read
in this layer's parent space. Each copy puts the layer's pivot on a point of the outline, scaled by
`scale` around the pivot and, with `align`, turned to the outline's direction there. The original is
not kept: every copy sits on the target (the layer is still selectable where it is). Like the other
arrays it repeats the whole earlier set at each point, so a linear array above it puts that row of
copies at every point. Moving, resizing or editing the target moves the copies (the evaluation cache
keys on a hash of the target's outline, and the canvas repaints the copies when the target changes).
A hidden target works (handy for guide paths).

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `target` | layer id | text | none | The target layer's id (`"12"`; a number reads too). Empty: no copies, a warning asks for one |
| `mode` | choice | `points`, `alongPath`, `populate` | `alongPath` | `points`: one copy at each anchor (segment ends; a closed outline's start once). `alongPath`: `count` copies evenly spaced by arc length, over every subpath in order; a closed outline spaces them `length / count` apart (none doubles the first), an open one `length / (count - 1)` so both ends get one. `populate`: `count` copies at seeded pseudo-random points inside the outline (nonzero winding) |
| `count` | copies | 1 to 1000 | 10 | Copies for `alongPath` and `populate` (`points` makes one per anchor, at most 1000) |
| `scale` | factor | 0 to 100 | 1 | Scale of every copy, about its pivot |
| `align` | on/off | | on | `points` and `alongPath`: turn each copy so its x axis follows the outline (the outgoing segment at an anchor; an open outline's last anchor uses the incoming one). `populate` ignores it |
| `seed` | integer | 0 to 4 294 967 295 | 1 | Seed for `populate`; same seed, same points everywhere |

**Refused targets.** `modifier.add`/`modifier.set`/`modifier.paste` refuse a target that is the
layer itself, an ancestor or descendant of it, a page, a layer on another page, a missing layer, or
one whose own stack copies back onto this layer (directly or through other copy-to-shape targets).
A file that has such a target anyway (or whose target was deleted later) draws the layer without
copies and the panel shows the reason; other edits to the layer still work, and undoing the delete
brings the copies back. `target` can't be bound to a variable.

**Copying layers.** Duplicate, copy and paste, page duplicates and component instances point a
copied source at the copy of its target when both were copied together; a source copied alone keeps
copying onto the original target.

**Picking a target.** `modifier.targets {id, modifier?}` lists the layers a layer can copy onto (and
whether the row's current target is valid); the Design panel's target dropdown offers the same list.

### transformEach

The change happens around each copy's own pivot and in its own frame (so a copy that was turned
moves along its turned axes).

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `rotate` | deg | any | 0 | Rotation of every instance |
| `scaleX` | factor | 0 to 100 | 1 | Horizontal scale of every instance |
| `scaleY` | factor | 0 to 100 | 1 | Vertical scale of every instance |
| `offsetX` | pt | any | 0 | Shift of every instance, x |
| `offsetY` | pt | any | 0 | Shift of every instance, y |
| `progressive` | on/off | | off | Scales the effect by `t`: the first copy is untouched and the last gets the full amount (offset and rotation grow linearly, scale goes from 1 to the full factor). One linear ramp; ease shapes are not implemented |
| `jitter` | factor | 0 to 10 | 0 | Each amount above is multiplied by `1 + jitter x r`, `r` between -1 and 1 drawn per copy and amount (0 = off) |
| `jitterSeed` | integer | 0 to 4 294 967 295 | 1 | Seed for `jitter` |
| `offsetXExpr`, `offsetYExpr`, `rotateExpr`, `scaleXExpr`, `scaleYExpr` | text | | none | Per-copy expressions (see Expressions) |

### randomize

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `position` | pt | >= 0 | 0 | Maximum random shift in x and y |
| `rotation` | deg | >= 0 | 0 | Maximum random rotation, plus or minus |
| `scale` | factor | >= 0 | 0 | Maximum random scale change, plus or minus (0.2 = 80% to 120%) |
| `seed` | integer | 0 to 4 294 967 295 | 1 | Same seed, same result on every platform and in the browser |
| `positionExpr`, `rotationExpr`, `scaleExpr` | text | | none | Per-copy expressions for each amount (see Expressions) |

Randomize derives its numbers from `seed` and the copy index only (SplitMix64 inside the crate, no
platform generator), so a copy never changes when other copies are added after it.

## Variable bindings

Any modifier param can take a FigCraft variable, like a layer's width or corner radius: numbers take
number variables, on/off fields boolean ones, choices (`axis`, `group`) string variables naming a
choice. A binding is stored in the layer's own variable bindings (`Node.bound`) under the key
`picabia.modifiers/<modifier id>/<field>`, for example `picabia.modifiers/m1/count`, so files, undo,
live collaboration and branches carry it like any FigCraft binding.

- Bind and unbind with FigCraft's command: `variable.bind {ids?, field: "picabia.modifiers/m1/count",
  variable}` (`variable: null` unbinds). It refuses a modifier the layer doesn't have, a param the
  modifier doesn't have, and a variable of the wrong type.
- The value is written into the stack whenever bindings resolve: after the bind, when the variable's
  value changes, and when the layer's mode changes (`variable.setMode` on the layer, a frame or the
  page). Modes switch values, e.g. a `copies` variable of 4 in Light and 7 in Dark.
- Bindings follow the modifier's **id**, not its row: moving rows keeps them.
- Removing a modifier (or all of them) drops its bindings. Setting a bound param by hand
  (`modifier.set`) detaches it, as editing a bound FigCraft field does; setting other params keeps it.
- A value that doesn't fit (a count over 1000 is clamped, a choice that doesn't exist) is skipped;
  a binding never breaks the stack.

## Expressions

The amounts of `transformEach` (`offsetX`, `offsetY`, `rotate`, `scaleX`, `scaleY`) and
`randomize` (`position`, `rotation`, `scale`) can each be driven by a per-copy expression, stored as
`<field>Expr` next to the field (`"rotateExpr": "i * 15"`) and set with
`modifier.set {modifier, props: {"rotateExpr": "i * 15"}}` (an empty text or `null` clears it).
`modifier.set` refuses texts that don't parse (the error says where).

| Name | Meaning |
|---|---|
| `i` | The copy's index (0 = the original) |
| `n` | The number of copies |
| `t` | `i / (n - 1)`, 0 when `n = 1` |
| `value` | The field's own value (a bound variable's value when the field is bound) |
| `pi` | 3.14159… |

Operators, loosest first: `+ -`, `* / %`, unary `-`, `^` (power, right-associative). Functions:
`sin cos tan` (degrees, like every angle in Picabia), `asin acos atan atan2(y, x)` (degrees),
`abs floor ceil round sqrt sign exp ln`, `min max` (1 to 8 arguments), `mod(a, b)`,
`clamp(x, lo, hi)`, `lerp(a, b, t)`, `rad`, `deg`, and `rand(k)` (a fixed number in 0 to 1 for each
`k`, the same on every platform).

| Expression | Effect |
|---|---|
| `rotate = i * 15` | 0°, 15°, 30°… |
| `offsetY = sin(t * 360) * 40` | One wave across the set |
| `scaleX = value * (1 - t / 2)` | Shrinks to half the field's scale |
| `rotation = value * t` (randomize) | No randomness on the first copy, full on the last |

An expression gives that copy's full amount: it replaces the `progressive` ramp for its field
(`t` is there to ramp), while `jitter` still applies. Results are clamped to the field's range, and a
result that isn't a number (`1 / 0`) falls back to the field's value. Expressions are at most 256
characters and 32 levels deep, so they evaluate in bounded time; a damaged one in a file is ignored
with a warning.

## Canvas guides

Each array shows guides while its layer is selected: linear, the offset line from the first to the
last copy; circular, the ring and its centre; grid, the outline of the whole lattice; mirror, the
axis line(s). Transform each and randomize show none.

## Canvas handles

With the Move tool and one unlocked layer selected, the **last enabled modifier that has handles**
shows them (the Design panel's open row lives in the UI, so it doesn't choose). They look and behave
like FigCraft's arc handles: white dots ringed in the selection colour, larger under the pointer, a
readout beside the dragged one, one undo step per drag ("Set modifier"), Esc cancels, and the
dragged point snaps to other layers and the layer's own edges and centre (Cmd/Ctrl or View ▸ Snap
off turns that off).

| Modifier | Handle | Drag sets | Shift |
|---|---|---|---|
| mirror | a knob at the end of each axis line | `offset` | |
| circularArray | the ring centre (dot inside) | `centerX`, `centerY` (the whole ring moves) | horizontal or vertical only |
| circularArray | a knob on the ring opposite the original | `radius` (the original stays; the ring grows away from it) | |
| linearArray | the last copy's centre (shown when `count` > 1) | `offsetX`, `offsetY` (`count` stays) | 45° steps |
| gridArray | the middle of the lattice's right / bottom side (more than one column / row) | `gapX` / `gapY` | both gaps equal |
