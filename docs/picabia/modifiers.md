# Modifiers

> Params as of P4 (this page follows the code).

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

### randomize

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `position` | pt | >= 0 | 0 | Maximum random shift in x and y |
| `rotation` | deg | >= 0 | 0 | Maximum random rotation, plus or minus |
| `scale` | factor | >= 0 | 0 | Maximum random scale change, plus or minus (0.2 = 80% to 120%) |
| `seed` | integer | 0 to 4 294 967 295 | 1 | Same seed, same result on every platform and in the browser |

Randomize derives its numbers from `seed` and the copy index only (SplitMix64 inside the crate, no
platform generator), so a copy never changes when other copies are added after it.

## Canvas guides

Each array shows guides while its layer is selected: linear, the offset line from the first to the
last copy; circular, the ring and its centre; grid, the outline of the whole lattice; mirror, the
axis line(s). Transform each and randomize show none.
