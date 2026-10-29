# Modifiers

> Params as of P1 (the first implementation may rename or add fields; this page follows the code).

A **modifier stack** is a list of live, non-destructive steps attached to a layer. The layer keeps
its own geometry; the stack describes how it is repeated and transformed. Edit the layer or any
modifier and every copy follows. Shapes, vectors, text, groups and frames can carry a stack. Copies
are drawn, exported and hit-tested as part of their source layer: clicking a copy selects the
source, and copies never take part in auto layout.

## Evaluation order

**The top row runs first**, like a recipe. Each step acts on everything the step above produced:

- **Array steps** (linear, circular, grid, mirror) repeat the **whole previous set as a block**.
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
`index`, `n` and `t` drive the progressive ramp, the randomize seed and, later, per-copy expressions.

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

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `count` | copies | 1 to 1000 | 6 | Total copies, original included |
| `radius` | pt | >= 0 | 150 | Ring radius |
| `startAngle` | deg | any | 0 | Angle of the first copy |
| `sweep` | deg | -360 to 360 | 360 | Angle covered; a full 360 spreads copies evenly with no overlap |
| `centerX` | pt | any | 0 | Ring centre, offset from the pivot, x |
| `centerY` | pt | any | 0 | Ring centre, offset from the pivot, y |
| `align` | on/off | | on | Rotate each copy to face along the ring |

### gridArray

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `rows` | copies | 1 to 1000 | 3 | Rows (rows x cols is capped at 10 000) |
| `cols` | copies | 1 to 1000 | 3 | Columns |
| `gapX` | pt | any | 20 | Space between columns (added to the layer's width) |
| `gapY` | pt | any | 20 | Space between rows (added to the layer's height) |

### mirror

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `axis` | choice | `vertical`, `horizontal`, `both` | `vertical` | `vertical` flips left to right across a vertical line; `both` gives four quadrants |
| `offset` | pt | any | 0 | Moves the axis away from the pivot (perpendicular to it) |
| `keepOriginal` | on/off | | on | On: previous set plus its reflection. Off: only the reflection |

### transformEach

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `rotate` | deg | any | 0 | Rotation applied to every instance, about its own pivot |
| `scaleX` | factor | > 0 | 1 | Horizontal scale of every instance |
| `scaleY` | factor | > 0 | 1 | Vertical scale of every instance |
| `offsetX` | pt | any | 0 | Shift of every instance, x |
| `offsetY` | pt | any | 0 | Shift of every instance, y |
| `progressive` | ramp | | off | Scales the effect by `t`, so the first copy is untouched and the last gets the full amount (shape of the ramp: linear, ease in, ease out) |
| `jitter` | | | off | Adds a seeded random amount per copy to the fields above (amount and seed) |

### randomize

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `position` | pt | >= 0 | 0 | Maximum random shift in x and y |
| `rotation` | deg | >= 0 | 0 | Maximum random rotation, plus or minus |
| `scale` | factor | >= 0 | 0 | Maximum random scale change, plus or minus (0.2 = 80% to 120%) |
| `seed` | integer | 0 to 4 294 967 295 | 1 | Same seed, same result on every platform and in the browser |

Randomize derives its numbers from `seed` and the copy index only, so a copy never changes when
other copies are added after it.
