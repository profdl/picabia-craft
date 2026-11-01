# Modifiers

> Params as of P8; `imageTrace` and `pathfinder` (P13, P14) are specified ahead of the code.

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
  do not add copies. They always act on the **final** copies, after every array step has run,
  wherever their row sits in the stack: each copy gets its own values, and `i`/`n` count all the
  copies. (Their position among the arrays doesn't change the result; their order among
  themselves does.)
- A **generator** step (`imageTrace`, `pathfinder`) replaces what the layer draws with new
  geometry computed from it. The copies the steps above it made are its **input** (folded into one
  result); the steps below it act on the **result**: arrays copy it, per-instance steps vary those
  copies, a stencil cuts it. Only the first enabled generator counts; later ones are ignored with a
  warning. Per-instance steps above a generator vary its input copies, those below vary the result's.
- A **stencil** step ends the stack (see `stencil` below): it adds no copies, and the enabled steps
  below it are ignored with one warning.
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
`originX`/`originY`). A motif tiles without overlapping itself only when it fits the group's
fundamental domain (the cell divided by the copies per cell, next to the origin): a cell-sized
motif is fine for p1 but overlaps its own rotations and reflections in the others. Guides draw the
lattice lines through the origin, from the first to the last line the copies touch (reflections
that land before the origin are enclosed too). Copies per cell: p1 1, p2 2, pm 2, pg 2, cm 2, pmm 4, pmg 4, pgg 4, cmm 4, p4 4,
p4m 8, p4g 8, p3 3, p3m1 6, p31m 6, p6 6, p6m 12. The centred groups (cm, cmm) use their primitive
cell, a parallelogram with edges `(w, 0)` and `(w/2, h/2)`.

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `group` | choice | `p1` `p2` `pm` `pg` `cm` `pmm` `pmg` `pgg` `cmm` `p4` `p4m` `p4g` `p3` `p3m1` `p31m` `p6` `p6m` | `p4m` | The symmetry group |
| `cols` | cells | 1 to 100 | 3 | Cells along the first lattice vector |
| `rows` | cells | 1 to 100 | 3 | Cells along the second lattice vector (cols x rows x copies per cell x earlier copies is capped at 10 000) |
| `cellWidth` | pt | 0.001 to 1 000 000 | 100 | Lattice width, every group. For the square groups (p4*) it is the side; for the hexagonal groups (p3*, p6*) it is the lattice constant |
| `cellHeight` | pt | 0.001 to 1 000 000 | 100 | Lattice height: p1, p2, pm, pg, cm, pmm, pmg, pgg, cmm only. Hidden for the square and hexagonal groups, which it can't shape |
| `angle` | deg | 1 to 179 | 90 | Angle between the lattice vectors: p1 and p2 only (the other lattices have their angle fixed by the group); hidden for the rest |
| `originX` | pt | any | 0 | Moves the symmetry origin from the layer's top-left corner, x. Every group but p1 (which has no symmetry element to move; hidden there) |
| `originY` | pt | any | 0 | Same, y |

Which inputs apply: `group`, `cols`, `rows` and `cellWidth` to all 17; `cellHeight` to the oblique,
rectangular and centred groups; `angle` to p1 and p2; `originX`/`originY` to all but p1. The panel
hides the fields a group ignores and keeps their values, so switching groups and back loses
nothing; the evaluator ignores them too.

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

### stencil

Turns the layer, with every copy the rows above it made, into a laser-cut **katagami** stencil: one
sheet of paper or film whose cut-outs let resist paste through. It is the last step: it adds no
copies (the copies above it stand), and enabled rows below it are ignored with one warning (only
the first enabled stencil counts). Apply bakes only the rows above it; the stencil row moves onto
the new group, live.

**Status: P10.** The sheet is computed (visible regions by paint order, colour selection,
clean-up, margin) and drawn on the canvas and in exports. **Islands** (paper pieces the cut
separates from the sheet, such as a crescent inside a cut swoosh) are found and held: by tie bars
(`tieMode: ties`), by a lattice of thin bars across the cut (`pattern`, ties for the islands it
misses), or kept and flagged (`gauze`). Laser export (kerf, cut order, bed size) and `separate`
come in P12; the ties layer (hand-drawn ties, no-tie zones) later.

**Islands and ties.** An island is a hole in the cut: the cut's contours nest (an outline, its
holes, cut-outs in those holes...), and every hole is the outline of a piece of paper. Its depth
counts the islands around it. In `ties` mode each island gets up to `tiesPerIsland` straight bars
`tieWidthMm` wide across the cut to paper already held: the outline of the cut-out it sits in, or
an island of the same cut-out tied before it (so a cluster of islands is tied outside in, each to
its neighbour). Bars go along the shortest gaps, run half a width into the paper on both sides, and
never touch another island, another cut-out or the sheet's edge. `tieSpread` keeps an island's
bars apart around it; `tieAngle` prefers gaps along that direction (or the opposite one), and
`tieSeed` decides between equally short gaps. After tying the islands are looked for again: any
still floating is counted in the stats (`untied`) with a warning. The bars (and the lattice that
became paper) are in the result for the preview overlay.

Lengths here are **millimetres** at the stencil's physical size, unlike the point-based steps:
`mm per point = widthMm / layer width` (the height follows the aspect ratio), and every `...Mm`
limit converts to layer units with that factor. Colours are `"#rrggbb"` texts (`#rgb` and any case
read too; they are written lowercase).

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `colourMode` | choice | `threshold`, `pick` (`separate` reads and saves, P12) | `threshold` | `threshold`: colours whose luminance (Rec. 709 on sRGB-decoded values, 0 to 1) is above `threshold` are cut, so lighter is cut. `pick`: the colours in `cutColours` are cut, every other colour stays paper. `separate` (one stencil per colour) evaluates as `threshold` with a warning for now. An unknown value reads as `threshold` |
| `cutColours` | list of colours | at most 256 | none | `pick`: the colours cut out (exact match after rounding to 8 bits). Entries that aren't colours are left out, repeats kept once; one colour text reads as a list of one |
| `threshold` | 0 to 1 | 0 to 1 | 0.5 | `threshold`: the luminance level |
| `invert` | on/off | | off | Swap what is cut and what stays paper (both modes) |
| `widthMm` | mm | 1 to 10 000 | 300 | The design's physical width |
| `marginMm` | mm | 0 to 1000 | 15 | Paper border around the design (the sheet is the design's bounds plus this) |
| `minCutAreaMm2` | mm² | 0 to 10 000 | 0.5 | Cut pieces and paper holes smaller than this are dropped |
| `minCutWidthMm` | mm | 0 to 100 | 0.3 | Cut slivers narrower than this are removed (an opening: shrink, then grow) |
| `minBridgeMm` | mm | 0 to 100 | 0.5 | Paper bridges narrower than this are removed (a closing: grow, then shrink) |
| `toleranceMm` | mm | 0 to 10 | 0.05 | Outline simplification |
| `preview` | choice | `paper`, `cloth` | `paper` | Canvas preview: the paper sheet with its cut-outs open, or the dyed cloth |
| `paperColour` | colour | | `#c8a165` | The sheet in the paper preview (kakishibu brown) |
| `pasteColour` | colour | | `#1f3a8a` | Where the paste goes through, in the cloth preview (indigo) |
| `clothColour` | colour | | `#f4efe6` | The undyed cloth, in the cloth preview |
| `tieMode` | choice | `ties`, `gauze` | `ties` | What holds the **islands** (paper pieces the cut separates from the sheet, such as a crescent inside a cut swoosh). `ties`: each island gets tie bars to the paper around it. `gauze`: no ties; the islands are kept (a gauze or silk mesh laid on the stencil holds them) and flagged. `pattern`: a lattice of thin paper bars across the cut (the `pattern...` fields) holds every island it crosses; islands it misses get ties as in `ties` mode. An unknown value reads as `ties` |
| `tieWidthMm` | mm | 0.05 to 50 | 1 | Width of each tie bar |
| `tiesPerIsland` | count | 1 to 8 | 2 | Tie bars per island, along the shortest cut gaps to the paper enclosing it |
| `tieSpread` | on/off | | on | Keep an island's ties apart (spread around it) instead of all on its very shortest gaps |
| `tieAngle` | degrees or none | wrapped to 0 to 360 | none | Preferred tie direction (0 = to the right, 90 = up on the canvas; the opposite direction counts too). `null` = no preference |
| `tieSeed` | whole number | | 1 | Breaks ties between equally short gaps (same result on every platform) |
| `patternType` | choice | `grid`, `horizontal`, `vertical` | `grid` | `pattern`: the lattice's bars. An unknown value reads as `grid` |
| `patternWidthMm` | mm | 0.05 to 50 | 0.6 | `pattern`: bar width (the tie size). Bars thinner than `minBridgeMm` give a warning |
| `patternSpacingMm` | mm | 0.5 to 500, at least twice the width | 8 | `pattern`: centre-to-centre spacing of the bars |
| `patternAngle` | degrees | wrapped to 0 to 360 | 0 | `pattern`: rotates the lattice (counter-clockwise on the canvas). The lattice is anchored at the sheet's top-left corner, so it stays put when the design changes |
| `patternWhere` | choice | `everywhere`, `islands` | `everywhere` | `pattern`: across every cut-out, or only in the cut-outs that border an island. An unknown value reads as `everywhere` |
| `material` | choice | `paper`, `film`, `card`, `custom` | `paper` | The sheet material. Setting a preset fills `minCutWidthMm`, `minBridgeMm`, `tieWidthMm` and `minCornerAngle` (unless the same edit sets them): paper 0.3 / 0.5 / 1 / 30 (the defaults), film (mylar, PET) 0.2 / 0.4 / 0.6 / 20, card 0.5 / 1 / 1.5 / 35. Changing one of those four away from the preset makes it `custom`. An unknown value reads as `paper` |
| `limitMode` | choice | `fix`, `flag` | `fix` | `fix`: clean-up removes narrow slivers and bridges and drops small pieces. `flag`: clean-up only simplifies; every cut or bridge that breaks the limits stays and is reported. An unknown value reads as `fix` |
| `minCornerAngle` | degrees | 0 to 179 | 30 | Paper corners sharper than this are flagged |
| `hoshi` | on/off | | on | Registration holes (hoshi) at the corners of the sheet, or of the tile |
| `hoshiDiameterMm` | mm | 0.2 to 20 | 1.5 | Diameter of the registration holes |
| `repeat` | choice | `auto`, `off` | `auto` | `auto`: when the last array row above the stencil is a `wallpaper` or `gridArray` (and no `transformEach` or `randomize` row varies the copies), islands and ties follow its repeat (a grid of one column or row repeats one way only). `off`: the design is one picture. An unknown value reads as `auto` |
| `output` | choice | `sheet`, `tile` | `sheet` | `sheet`: the whole design. `tile`: one repeat cell plus margin and registration holes, for step-and-repeat printing; without a repeat it cuts the sheet, with a warning. An unknown value reads as `sheet` |

Numbers outside their range are clamped when evaluating (a warning names the field); a colour that
doesn't read takes its default. `cutColours` can't be bound to a variable.

### imageTrace

Traces a layer's image into vector shapes, live: change a field and the trace follows. It is a
**generator** step (see Evaluation order). The tracer is VectorCraft's Image Trace (vendored
unchanged, `vendor/vectorcraft/trace`), so the fields, presets and defaults are VectorCraft's.

**Which layers.** FigCraft has no image layer: an image is an image fill on a rectangle, frame,
ellipse or any other shape. So `imageTrace` works on **any layer with a visible image fill**. It
traces the layer **as it draws**: the crop, scale mode, rotation and adjustments of the fill, the
layer's shape (rounded corners, an ellipse), its other fills, its strokes and its children. Outside
the shape counts as transparent and is never traced. A layer without a visible image fill draws as
usual, with the warning "imageTrace needs a layer with an image fill".

**Resolution.** The layer is rendered at its image's own pixel density (the densest image fill's
pixels per layer point, clamped to 0.05 to 16), so a 4000 px photo in a 400 pt box is traced at
4000 px. `paths`, `noise` and `strokeWidth` count those **image pixels**, as in VectorCraft. The
rendered raster is capped at 16 megapixels natively and 4 megapixels in the browser (scaled down,
with a warning), and the result at 8 million anchors natively and 2 million in the browser (past
it: the warning "the trace is too detailed", and the previous result or the image stays drawn).

**What it draws.** The traced paths in their palette colours, bottom first, in place of the whole
layer: filled regions (non-zero), and for Create: Strokes, centre lines stroked `strokeWidth` wide
(round caps and joins). The image stays in the document, untouched. An empty, fully transparent or
fully ignored image traces to nothing (a warning says so). The stats line in the panel and
`modifier.resultInfo` give the paths, anchors and colours.

**Rows above and below.** Rows above make copies of the image; the trace is computed once and
drawn at each copy (an array above `imageTrace` therefore looks the same as below it). Rows below
act on the traced result: arrays copy it, a `stencil` cuts the traced vectors (photo → trace →
stencil sheet).

**Apply** bakes the rows down to the trace: the layer is replaced by a group named
"<layer> (traced)" with one vector layer per traced path, bottom first, as VectorCraft's Expand
makes it (no per-colour merge). Each is named by its colour (`#1f3a8a`); filled paths get a solid
fill, stroked ones a centre-line stroke and no fill. The image is **removed**, like Expand (Undo
brings it back). The rows below move onto the group, live.

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `preset` | choice | `default`, `autoColor`, `highFidelityPhoto`, `lowFidelityPhoto`, `threeColors`, `sixColors`, `sixteenColors`, `shadesOfGray`, `blackAndWhiteLogo`, `sketchedArt`, `silhouettes`, `lineArt`, `technicalDrawing`, `outline`, `flatLogo`, `custom` | `default` | VectorCraft's presets. Setting one fills every field below (unless the same edit sets them); changing a field away from the preset makes it `custom`. An unknown value reads as `custom` |
| `mode` | choice | `blackAndWhite`, `grayscale`, `color`, `logo` | `blackAndWhite` | Black and White, Grayscale, Color, Flat Logo. An unknown value reads as `blackAndWhite` |
| `palette` | choice | `limited`, `fullTone`, `automatic` | `limited` | `color` mode: how the colours are picked. VectorCraft's `documentLibrary` reads as `limited` with a warning |
| `colors` | count | 2 to 256 (logo: 2 to 10) | 6 | Grays (grayscale), colours (`limited`, logo) |
| `colorDetail` | % | 0 to 100 | 50 | Colours (`fullTone`, `automatic`) |
| `threshold` | level | 0 to 255 | 128 | Black and white: lighter pixels are white |
| `paths` | % | 0 to 100 | 50 | How tightly the paths follow the pixels (0.78 px at 0, 0.08 px at 100) |
| `corners` | % | 0 to 100 | 75 | More corners when higher (a corner turns more than 90 − 0.55 × `corners` degrees) |
| `noise` | image px | 1 to 100 | 25 | Areas smaller than this many pixels merge into a neighbour |
| `method` | choice | `abutting`, `overlapping` | `abutting` | `abutting`: each colour its own area; `overlapping`: each layer runs under the ones above it (no hairline gaps) |
| `fills` | on/off | | on | Create: Fills |
| `strokes` | on/off | | off | Create: Strokes (parts no wider than `strokeWidth` become stroked centre lines). Turning both off turns `fills` back on, with a warning |
| `strokeWidth` | image px | 1 to 100 | 10 | Create: Strokes: the widest part traced as a line |
| `snapCurvesToLines` | on/off | | off | Straighten nearly straight curves |
| `ignoreWhite` | on/off | | off | Leave white (or `ignoreColor`) out |
| `ignoreColor` | colour or none | | none | With `ignoreWhite`: the colour left out instead of white |

Not offered yet (VectorCraft has them): Shapes, Auto Grouping, Gradients, Document Library and the
logo colour list.

### pathfinder

Illustrator-style Pathfinder on a group's contents, live. It is a **generator** step: the layer
draws the result instead of its contents. The geometry is VectorCraft's Pathfinder (vendored
unchanged, `vendor/vectorcraft/pathops`).

**Which layers.** Groups, frames and boolean groups. Anything else draws as usual with the warning
"pathfinder works on a group, frame or boolean group".

**Input.** What the layer draws, as filled shapes in paint order (back first), gathered as the
stencil gathers its design: every visible leaf, strokes outlined into shapes of their own (so a
stroke is a separate shape in front of its fill), text as glyph outlines, transforms, boolean
groups inside, frame clips and masks applied, and the **copies** the modifier stacks on the
children make. The copies the rows above `pathfinder` make are input too, so a grid array followed
by `merge` gives one merged shape. A frame's own fill is the back-most shape. A boolean group's
operands are the input (its own operation is not applied first). FigCraft's boolean groups already
do `unite`, `minusFront`, `intersect` and `exclude`; they are accepted here for completeness.

**Output.** Each resulting face keeps the fill of the shape it came from: for `divide`, the
front-most shape covering it; for the others, as below. For `outline`, each edge is stroked
`outlineWidth` wide in its shape's fill colour, with no fill, as in Illustrator. Solid fills are
kept as they are. Gradients and image fills are drawn as their mean colour, with a warning (the
stencil's rule; VectorCraft keeps them whole, which is a gap here). Hidden fills and layers are not
input.

| `op` | Result |
|---|---|
| `divide` | Every face of the arrangement, each with the front-most covering shape's fill |
| `trim` | Each shape minus what is in front of it (hidden parts removed), same-coloured shapes not merged |
| `merge` | As `trim`, then touching pieces of the same fill merged into one |
| `crop` | The front-most shape crops the shapes behind it and is removed |
| `outline` | Every edge of the arrangement as an open path, stroked in its shape's fill colour |
| `minusBack` | The front-most shape minus everything behind it |
| `unite` | Everything as one shape, with the front-most shape's fill |
| `minusFront` | The back-most shape minus everything in front of it |
| `intersect` | Where every shape overlaps, with the front-most fill |
| `exclude` | Where an odd number of shapes overlap, with the front-most fill |

**Rows below** act on the result (arrays copy it, a stencil cuts it). **Hit testing:** clicking a
face selects the layer, as clicking a copy selects its source.

**Apply** bakes the rows down to the pathfinder: the layer is replaced by a group of the same name
holding one vector layer per face (per edge for `outline`), back first, each with its source's fill
(or stroke, for `outline`) and named after its source layer. The original contents are removed
(Undo brings them back); the rows below move onto the group, live.

Limits: at most 5000 input shapes (the rest are left out, with a warning); an arrangement too
complex to compute keeps the contents drawn, with a warning.

| Field | Unit | Range | Default | Meaning |
|---|---|---|---|---|
| `op` | choice | `divide`, `trim`, `merge`, `crop`, `outline`, `minusBack`, `unite`, `minusFront`, `intersect`, `exclude` | `divide` | The operation (above). An unknown value reads as `divide` |
| `precision` | points | 0.001 to 10 | 0.01 | How closely results follow the curves; pieces thinner than this are dropped |
| `outlineWidth` | points | 0 to 100 | 1 | `outline`: the stroke width of the edges |

### Generator status

`modifier.resultInfo {id?}` gives a generator's status, computing it when it is out of date:
`{modifier, kind, fresh, pending, warnings, stats}`, where `stats` is `{paths, anchors, colours,
tracedPx: {w, h}}` for `imageTrace` and `{shapes (input), faces (output)}` for `pathfinder`, plus
`colours: ["#rrggbb"]` (the result's colours, largest area first; at the top level of the reply,
beside `stats`, with `items`, the number of result paths, and `drawsDesign`, true when the generator
made no result and the layer draws as usual). Results are computed off the UI
thread natively (the previous result stays drawn meanwhile) and on the UI thread once the layer
has stopped changing for 300 ms in the browser; exports and commands compute them first.

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
