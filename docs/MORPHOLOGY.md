# Morphology — the organelles the picture did not draw

**Status: implemented**, in the six commits §9 lays out. What follows is the design and the
argument behind it; §12's decisions were taken as recommended, with the two the user settled
noted in place — limbs **may** be drawn over neighbouring cells, and the shell is a **ring**.

One thing in §8 is outstanding and is written down rather than quietly dropped: the photographic
check. `tools/check_outline.py` compares what the shader actually put on the screen against what
it was told to draw, and it needs a window this work could not get one of — every screenshot taken
during it came back an empty surface. Everything else in §8 landed, plus one thing that was not in
the plan: `tests/shader_syntax.rs`, which parses and validates the WGSL with naga so that a syntax
error is a failing test in a second rather than a layer that silently does not draw.

Read `docs/OVERLAPS.md` first. It is the record of what opening the cell renderer cost last time,
and §3 of this document is largely an argument about how not to pay that again.

---

## 1. What was missing

Every organelle a cell carried was drawn the same way: a small round blob on a ring at `0.45` of
the radius, in slot order, sized `0.28 r`, coloured by type. That is a reasonable picture of a
*vesicle*. It is not a picture of a cilium, and it is not a picture of anything that reaches
outside the membrane — of which the catalogue has six.

Nothing a cell had built changed its silhouette. A cell with four flagella and a drawn spike was
drawn as a circle with five grey dots in it.

| # | type | was drawn as | state that exists to draw from | now |
| --- | --- | --- | --- | --- |
| 0 | membrane | the outline, the wall | radius, integrity | ✔ body |
| 1 | nucleus | violet dot | — | ✔ interior |
| 2 | mitochondrion | orange dot | — | ✔ interior |
| 3 | chloroplast | green dot | — | ✔ interior |
| 4 | vacuole | blue dot | — | ✔ interior |
| 5 | pump | grey-white dot | — | rim, not ring |
| 6 | **cilium** | pale yellow dot | mount angle (16), signed thrust | **outside** |
| 7 | chemosensor | pink dot | — | ✔ interior |
| 8 | photosensor | yellow dot | — | ✔ interior |
| 9 | touch sensor | brown dot | — | ✔ interior |
| 10 | **junction port** | *grey* | the links themselves | **rim** |
| 11 | lysosome | *grey* | pathway | colour only |
| 12 | **spike** | *grey* | signed extension × param | **outside** |
| 13 | oscillator | *grey* | period, phase | colour only |
| 14 | **holdfast** | *grey* | grip effort, reach ½ square, filter | **outside** |
| 15 | **shell** | *grey* | cover, `0..7/8` of the body | **on the body** |
| 18 | diazosome | *grey* | — | colour only |
| 19 | chemosynth | *grey* | — | colour only |
| 20 | lipid droplet | *grey* | — | colour only |
| 22 | **flagellum** | *grey* | mount angle, thrust ×1.5 | **outside** |
| 28 | **exoenzyme** | *grey* | throttle | **outside** |

*grey* means the type fell through the `_ =>` arm of `slide::organelle_colour`. **Eleven of the
twenty drawable types were drawn in the same grey**, so a spike, a shell, a holdfast, a flagellum
and a lipid droplet were the same mark. `slide::cell_colour` had the same hole from the other end:
it tinted from six types and ignored the rest, so a heavily armoured cell and a bare one came out
the same colour.

The work therefore fell into three tiers of very different cost and risk:

* **Colour** (11 types). No renderer change at all. One table, read by the dots and the body.
* **The body** (shell). Opens `cell.wgsl` and the body's vertex layout. One commit, landed last.
* **Limbs** (cilium, flagellum, spike, holdfast, exoenzyme, and the junction). New geometry
  outside the membrane. A new pipeline, which is §3.

---

## 2. The rule: what the picture is allowed to say

The microscope's one job is to show what a cell *is* and what it is *doing*, from state that
actually exists. `slide::cell_colour`'s note puts it as "the picture shows what a cell is rather
than what the analysis layer has decided to call it". A limb is a much stronger claim than a
colour, so each form below is tied to a named quantity, and what would be a lie is written down
next to it.

The load-bearing case is the spike. `OrganelleType::EM_MECHANICAL` already argues this out for the
emission bands, and the argument transfers whole:

> a predator at rest is indistinguishable from anything else its size, and becomes unmistakable the
> instant it extends. Ambush is available; ambush while armed is not.

A spike drawn from the fact that the cell *has* one would contradict that. A spike drawn from
`control[0]` — out when it is out, gone when it is sheathed — is the same statement the energy
signature makes, said in the picture. That is the shape every form here should have.

Three quantities are **not** simulated and any use of them is a drawing convention, to be labelled
as one: where on the cell a non-directional organelle sits, which way a shell faces, and the phase
of a beat. SPEC §6 is explicit that position within a cell is not state.

---

## 3. Where this goes, and what it must not touch

### It is not the body's quad

The obvious move — union the limbs into the body's signed-distance field so they get the membrane
ring, the shading and the haze for free — fails on three counts, and each is enough on its own.

1. **The quad has no room.** `FIELD_FILL` is `0.65`, and the remaining `0.35` is not slack: the
   wobble reaches a fifth and the antialiasing fade needs the rest, which is written down in both
   `cellmesh.rs` and `cell.wgsl` and was learned by the population coming out square. A flagellum is
   one to three radii long. Making room means moving `FIELD_FILL`, which is hard-coded in two
   places and calibrated against the wobble amplitude.
2. **Every cell would pay.** The quad is per cell, not per limb, so a slide where 4% of cells carry
   a cilium would draw every cell into a quad sized for a flagellum. At the tier where limbs are
   visible, fill rate is the cost.
3. **The seams would cut them off.** The body field is intersected with up to twelve half-planes.
   A flagellum crossing a shared wall would be sliced at the wall, which is right for a body and
   wrong for a limb.

### It is not a wider `CellMaterial` either

Adding attributes to `CellMaterial` changes the body's vertex layout, and every probe that
photographs the body — `shader_probe`, `nine_cells`, `overlap_detector`, `swell_probe`,
`packing_probe` — is re-baselined in the same commit. That is precisely the state in which a
regression in the picture stops being attributable, which is the failure `docs/OVERLAPS.md` is
about. The whole value of those probes is that they are *unchanged*.

### It is a third pipeline

```
crates/mm-app/src/limbmesh.rs   arithmetic: parts -> vertex buffers. No Bevy. Tested headless.
crates/mm-app/src/limb.wgsl     the forms, as signed-distance fields.
crates/mm-app/src/limbpipe.rs   LimbMaterial, the attributes and the mesh.
```

`limbpipe.rs` and not four more items in `cellpipe.rs`, on the same argument the section above
makes: the body's layout, materials and shader are finished work, and a change in how a cell looks
has to stay attributable to something that touched them.

**Two mesh entities, either side of the cells.** `LIMB_Z` is 0.9, under the cell mesh at 1.0, so a
limb's root disappears behind the body it grows from and there is no join to draw. `OVER_Z` is
1.05, and only the junctions are in it — see §4.6. `limbmesh::over_cells` is the single answer to
which layer a form belongs in, because a form in the wrong one is invisible or is drawn over a body
it should be behind and neither says which line was wrong.

The property that makes this the right shape: **the body path is not opened at all** for commits
1–5. `cell.wgsl`, `cellmesh.rs`, the two existing materials and every body probe are untouched and
provably so, so any change in how a cell looks is a bug in the new pipeline and nowhere else. Only
the shell (§5) opens the body, and it is last and alone.

### The layout

Narrow, because a limb needs none of the seams:

| location | attribute | meaning |
| --- | --- | --- |
| 0 | position | world, already rotated by the CPU |
| 1 | uv | `-1..1`, the limb-local frame: `+x` root to tip, `+y` across |
| 2 | colour | the owning cell's colour after haze and vignette |
| 3 | `limb_a: vec4` | `x` form, `y` extent (signed), `z` phase, `w` aspect |
| 4 | `limb_b: vec4` | `x` count, `y` inner, `z` taper, `w` seed |

The CPU emits **rotated** corners, so `uv` is already the limb's own frame and the shader needs no
direction and no trigonometry. A long thin flagellum then gets a long thin quad rather than a
square one sized for the worst rotation, which is most of the fill rate back.

Two `vec4`s rather than one, because bandwidth is free at the tier limbs are drawn at and a
component that means three different things by form is exactly the kind of double meaning
`OVERLAPS.md` records the cost of. **The shader owns each form's proportions and the CPU owns the
quad**: `LimbDot::width` is the widest the form ever reaches — the wave envelope, the tuft's arc —
so a whip is a fixed fraction of one half-width and its wave is another, and every constant plus
its swing comes to at most one. A form that reached past that would be clipped to the rectangle and
come out with a straight edge somebody would read as meant.

---

## 4. The forms

All lengths are in the cell's drawn radius `r` unless stated. Each is one `sd_*` function in
`limb.wgsl`, composed the way the body's is: distance field, `fwidth` antialias, the same
`lum`/wall treatment as `blob` so a limb reads as the same material as the cell.

### 4.1 Cilium — form 1

A **tuft**, not a hair: `docs/FEEDING.md` §7 and the catalogue both say a cilium organelle is many
small ones where a flagellum is one large one, and the picture should say the same before the
physics is consulted.

* Mount: `sensing::cilium_direction(o)` — sixteen angles from `control[1]`. **Real.**
* Count: `2 + param/64`, so 2–5 hairs, spread over ±25° of the mount. Convention.
* Length: `0.35 r`, root width `0.06 r`, tapering to a point.
* Beat: each hair bent by `A·sin(2π·phase + k·i)`, amplitude `A ∝ |cilium_thrust(o)|`. **Real.**
* Direction of travel: the sign of `control[0]`. A cilium beating backwards beats visibly
  backwards. **Real**, and worth having — it is a thing a genome can do that nothing shows.

A cell at zero throttle has cilia that hang still. It has still built them and is still paying for
them, which is what distinguishes this from the spike: a cilium is not a weapon and hiding it would
say nothing.

*The lie to avoid:* thrust magnitude drives amplitude, not length. Length is `param`, which is what
the cell built.

### 4.2 Flagellum — form 2

One whip, longer than the body.

* Length `(1.2 + 1.3·param/255)·r`. **Real** (`param`).
* Centreline `y(s) = A·s·sin(2π(1.5 s − phase))` — amplitude growing along the length, which is
  what a flagellar wave does and what stops it reading as a wiggly line.
* `A ∝ |cilium_thrust(o)|`, wave travelling towards the tip for positive thrust and towards the
  root for negative. **Real.**
* Width `0.08 r` at the root to `0.02 r` at the tip.
* SDF: minimum distance to a 12-point polyline of the centreline. Cheaper and better conditioned
  than solving for the closest point on a sine.

### 4.3 Spike — form 3

* Length `0.9 r · extension`, where extension is `clamp(control[0], 0, Q10_ONE)` per slot.
  **Real, and the whole point** — see §2.
* Base half-width `0.10 r · (0.3 + param/255)`. **Real.**
* Root sunk `0.15 r` inside the membrane, so the join is hidden by the body drawn over it.
* Slightly concave taper, so it reads as a barb rather than an arrow.
* Not emitted at all when the drawn length is under half a pixel — a sheathed spike is nothing,
  not a stub.

### 4.4 Holdfast — form 4

* A stalk reaching `HOLDFAST_REACH` — half a substrate square — past the body. **Real**, and note
  that it scales with the *world*, not with the cell, because the constant does.
* Three rootlets splaying at the tip. Convention.
* Tension from `sensing::holdfast_grip_of(o)`: limp and curled at zero, straight and taut at full.
  **Real**, and the readable one — a cell that has let go looks like it has let go.
* Thickness from `param`. **Real.**

*Deferred:* the holdfast is also the filter surface (`ecology::filter_strength`), and what it
catches depends on slip, which is per-cell scratch and not on the `Frame`. Fanning the rootlets
with slip would be the honest picture of filter feeding and needs one more field carried out.

The curl is seeded so two feet on one cell do not slump identically, and getting that seeded badly
is what `a_holdfast_that_has_let_go_hangs_off_its_own_axis` caught: `hash21` folds its input through
`fract(p * 0.1031)`, so a constant `y` against an `x` walking by small integers is nearly a straight
ramp into it and ten of twelve seeds slumped the same way. Both components vary with the seed now.

### 4.5 Exoenzyme — form 5

Not a limb. A **halo**: one ring quad centred on the cell, inner radius the body, outer the body
plus half a square, alpha `0.25 × throttle`, edge modulated by the same `hash21` the body uses so
it reads as a cloud rather than a ring. Under the cells.

Colour a sickly yellow-green — it is a leaky public good digesting the water, and it should look
like something you would not want to be standing in.

### 4.6 The junction — forms 6 and 7

The junction is the one thing on this list that *is* drawn and is drawn wrong.

Today: a stretched sprite from **centre to centre**, at z `0.5`, under the cells. On a packed pair
— which is every pair a hard junction holds — the entire line is inside the two bodies and
invisible. It becomes visible exactly when the pair is pulled apart, which is backwards: a hard
junction is most structural when the cells are pressed together, and is *least* trustworthy when
it is stretched.

Proposed, and moved into the limb mesh so it stops being an entity per link:

* **Hard, in contact — form 6, a band.** A bar *across* the shared wall, perpendicular to the line
  of centres, drawn **over** the cells at `OVER_Z`. Square ends, because a desmosome is a patch of
  wall and a rounded end reads as a rod lying on top of the pair. This is what makes a colony read
  as one riveted body instead of as touching discs.
* **Hard, stretched — the same form, turned.** The bar lies along the line of centres instead and
  spans only the *gap* between the two drawn outlines. One junction in two states rather than two
  things.
* **Strain.** `junction::distance(i,j)` against `rest` and `config.breaking_strain` gives `0..1`
  to breaking. A band thins to a third and fades towards it. **Real**, and the most useful new
  thing on the slide: a body about to come apart says so before it does.
* **Soft — form 7, a channel.** A row of pores rather than a bar, faint, and drawn over the cells
  for the same reason the band is. One is structure and the other is a conversation, and a colony
  wired for transfer should not read as one merely held together.

`JunctionLine` gains `strain`, `gap` and `span`, and the sprite pool is gone: an entity per link is
ten thousand of them in a five-thousand-cell colony, which is the cost the cell mesh removed.

`gap` is measured against the radii the cells are *drawn* at and not the physical ones. The picture
has to agree with the picture — cells are drawn `PACKING` over their true size so a settled pack has
no holes in it, and a gap from the physical radii would call a visibly overlapping pair apart.

*Deferred:* motes travelling along a soft junction while it is actually transferring. Transfer
volume is not on the `Frame`.

---

## 5. The shell, which is not a limb

Shell coverage is a **scalar** — `organelle::shell_cover` returns one number, `0..7/8` of the body,
and the catalogue is explicit that the same `control[0]` closes the shell and shades the cell under
it because "it is one surface doing one thing". There is no direction anywhere in it.

So the picture draws a scalar: **a mineral rim whose thickness and opacity go with cover**, all the
way round, over the membrane wall — plus the body under it darkened by the same fraction, which is
not decoration but the mechanic, since a shell reduces the light reaching the chloroplasts by
exactly the fraction of the body it covers.

An *arc* covering `cover × 2π` of the perimeter is prettier and reads as a test rather than a
coating. It is also an invented direction, and it is not what was built. §12.3.

This is the only part that opens `cell.wgsl` and the body's vertex layout: one attribute,
`ATTRIBUTE_ARMOUR` on `CellMaterial` alone, and about a dozen lines in `blob`. It landed **last and
alone**, and every body probe still passes unmodified — because `armour == 0` is exactly the
identity, every term the shell adds being a `mix` at zero.

`DotMaterial` does not carry it and must not: below `Lod::Packed` a cell is a dozen pixels across
and a mineral rim would be the whole cell, and keeping the narrow layout narrow is that material's
entire job. `dot_fragment` passes a zero, so the two entry points still share every line.

Commit 1 got the cheaper half of the same statement for none of the risk, by putting silica-grey
into `cell_colour` weighted by shell `param`. A cell that has invested in a shell reads mineral,
exactly as a cell that has invested in chloroplasts reads green.

---

## 6. The data path

`CellDot` carries `limbs: Vec<LimbDot>` beside `organelles`, built by the same walk of the slots
under the same `detailed && near` gate — one walk, because a spike's blade has to grow out of the
spike's own dot and the dot's angle is the ring convention. `CellDot::armour` is separate and is
**not** tier-gated: it is one integer divide with no allocation, and a shell changes the whole
body's colour rather than putting a mark inside it, so it reads at a zoom where a nucleus is two
pixels.

The `mm-core` change is five pure integer extractions of loop bodies that already existed, so that
effort and size are separable and each has one definition:

```rust
sensing::cilium_power(&Organelle) -> i32      // signed: a cilium can beat backwards
sensing::holdfast_effort(&Organelle) -> i32   // unsigned: there is no gripping backwards
ecology::spike_reach(&Organelle) -> i32       // zero when sheathed, however big the spike
ecology::exoenzyme_throttle(&Organelle) -> i32
organelle::shell_cover_of(&Organelle) -> i32
```

The per-cell sums are written in terms of them and are unchanged to the byte. The split is what the
picture needs: a limb's *length* is what the cell built and its *motion* is what the cell is doing,
and folding the two together would draw a large sheathed spike as a small drawn one.

### The clock

`phase = fract((tick % 20) / 20 + hash(cell_id, slot))`, on the CPU, from `frame.tick`.

**Never wall-clock.** A paused slide must be still — cilia beating on a stopped world would be the
renderer inventing time — and a screenshot at tick N must be reproducible, which the whole bench
harness depends on. The modulo is in `u64` because `tick as f32 / 20.0` loses its fraction after
about sixteen million ticks, an hour at 1×, and every cilium on the slide would quietly stop.

---

## 7. Level of detail

Limbs at `Lod::Organelles` and above, matching the organelle dots, behind View ▸ Limbs — `m`,
beside Organelles on `n`, because they are the pair: what a cell is made of, and what it is doing
with it. The shell is the exception and is drawn at every tier the body is; §6 says why.

There is a real argument for the spike and the flagellum at `Lod::Packed` — they are the two most
behaviourally informative marks on the slide, they are long enough to read at twelve pixels a cell,
and the mixed benchmark says only 3.8% of cells carry a cilium and 7.7% a holdfast, so it is a few
percent of the population and not a doubling of the geometry.

Do not do it in the first pass. Measure the Organelles tier first, then widen deliberately with a
figure in hand. `Lod::Packed` is the tier the M10 frame budget is measured at.

---

## 8. How this is tested

The discipline is `docs/OVERLAPS.md`'s, applied to the new pipeline *before* it could go wrong
rather than after.

1. **`tests/shader_syntax.rs`** — not in the original plan and the most useful thing here. WGSL is
   parsed when a *pipeline* is compiled, which is at draw time on a machine with a window, so a
   syntax error or a type that did not check surfaced as a layer that silently did not draw:
   several minutes and one graphics stack away from the line that was wrong, and indistinguishable
   from "the feature draws nothing". naga, at the version Bevy resolves, does the same parse and
   validation in a second with no display. Checked that it bites. It also asserts that every form
   code the mesh can emit is a constant the shader declares, by *value* — the names could agree
   while the numbers do not, and it is the numbers that travel in the vertex buffer.
2. **`limbmesh.rs` is tested headless**, as `cellmesh.rs` is: every attribute the same length, no
   index past the end, a zero-length limb emitting no quad, corners exactly the rotated frame.
3. **`tests/limb_probe.rs`**, twenty-two assertions over the two layers that fail differently — the
   geometry through `limbmesh` (right shape, wrong place) and the field through `phantom::limb`
   (wrong shape, right place). `phantom::limb` is `limb.wgsl` in Rust, the same deliberate copy
   `Drawn::outline` is. Among them: a sheathed spike emits no quad; a spike's length is its
   extension and its width is its `param`; a barb is concave and not a cone; a tuft has the hairs
   it asked for and they are anchored at the root; a beat reverses when the power does; a flagellum's
   wave grows towards its free end; a holdfast splays when it grips; a halo has no rim; a junction
   thins monotonically towards breaking; a channel is pores where a band is a bar.
4. **Regression.** `shader_probe`, `nine_cells`, `overlap_detector`, `swell_probe` and
   `packing_probe` pass **unmodified** through all six commits. For 1–5 that is by construction —
   none of them touches a file the body is drawn from. Commit 6 is the only one that opens
   `cell.wgsl`, and it still does not move them, because `armour == 0` is the identity.

5. **The photograph, for the body.** `tools/check_outline.py` on the current build, which is the
   standing calibration and the thing that says the shell has not moved the edge it was drawn
   beside:

   ```
   9 cells, 4245 rays measured
   where the shader drew the outline, minus where the data said, in pixels:
      median -0.01   p10 -0.49   p90 +0.41   mean +0.004
      within one pixel: 87.2%
   ```

6. **The limb sheet**, `MM_BENCH_LIMBS=1` or `k` in `shaderbench`: every form across a sweep of
   effort, size and phase, with a body behind each so the join reads, and no world anywhere near
   it. `phantom`'s argument applied to the outside of a body — a spike that looks wrong on the
   slide could be the field, the quad, the mount angle, the organelle's control word or the tier,
   and that is five hypotheses and a run each.

   It earned itself on the first frame, twice, both times against its own layout rather than the
   shader: the halo came out as a crescent hanging off the side of its cell, because it had been
   pushed out to the rim like a limb when it is a cloud *around* the body; and the rows read
   bottom-to-top against the legend beside them.

**Outstanding: the photograph, for the limbs.** `check_outline.py` measures *cell* outlines against
the data's, and there is no equivalent that walks a limb's edge — so `phantom::limb` and
`limb.wgsl` are agreed by reading rather than by measurement. The sheet makes a divergence
*visible*, which is most of the value, but it does not make it a number. That is the one claim here
weaker than the rest.

---

## 9. What landed, in order

One concern each, and the body path stays shut until the last.

| # | commit | touches | body probes |
| --- | --- | --- | --- |
| 1 | the eleven missing colours, and one table for the dots and the body | `slide.rs` | untouched |
| 2 | `LimbDot`, the five `mm-core` extractions, the parts walk | `slide.rs`, `ecology.rs`, `organelle.rs`, `sensing.rs` | untouched |
| 3 | `limbmesh`, `limb.wgsl`, `LimbMaterial`, the mesh — **spike only** | new files, `main.rs` | untouched |
| 4 | cilium, flagellum, holdfast, exoenzyme, and `shader_syntax` | `limb.wgsl`, `slide.rs`, `phantom.rs` | untouched |
| 5 | the junction: the boundary, strain, the band, the sprite pool gone | `slide.rs`, `limb.wgsl`, `main.rs` | untouched |
| 6 | the shell, as a ring on the body | `cell.wgsl`, `cellmesh.rs`, `cellpipe.rs` | pass unmodified |

Commit 3 carried the whole pipeline for one simple form deliberately: if the layout, the material,
the z-order or the upload had been wrong, it would have been wrong with a triangle on the screen
and not with five forms to argue about.

Two things fell out along the way that were not planned. `redraw` was at Bevy's sixteen-parameter
limit, so the meshes became one `Layers` system parameter — one job done three times, which is the
right sixteenth to give up. And `tests/shader_syntax.rs`, which §8 argues for at length.

---

## 10. Where it fits

M10.5 is *the look*, and it is done. This is not that milestone reopened: M10.7 and M10.10 both say
in terms that they do not open the renderer, and this does, so it is its own deliverable —
**M10.9, morphology** — with its own acceptance tests (§8) and its own frame-budget figure.

`docs/UI.md` §7 is normative for the front-end and says nothing about limbs. If this is accepted it
goes there, and this document keeps the argument the way `OVERLAPS.md` keeps its own.

---

## 11. What this is not

* **Not a cell-type enum by the back door.** Every form is driven by an organelle's own state. A
  cell that looks like a predator looks like one because it has a spike out.
* **Not morphology in the simulation.** Nothing here feeds back. A limb has no drag, no collision,
  no reach; the physics is unchanged and stays unchanged. `sensing::step_physics` does not learn
  that a flagellum has a shape. If a limb ever *should* have a hydrodynamic consequence, that is a
  simulation change to be argued in SPEC, not a side effect of drawing one.
* **Not angular dynamics.** A cell has no orientation and must not get one. Mount angles come from
  `control[1]`, which is a genome's choice in the cell's own frame, and that frame is fixed to the
  world. CLAUDE.md's junction rule is the same prohibition from the other end.

---

## 12. The decisions, and how they went

1. **May a limb be drawn over a neighbour?** **Yes**, decided by the user, and it is the one place
   on the slide where that is allowed. A spike wounds the cell it is touching, so a spike over its
   victim is the honest picture; the seam work `OVERLAPS.md` records was about *bodies*, which tile
   and must never be drawn twice, and a limb is not a body — it has no seam and takes part in no
   packing solve. `overlap_detector` looks at the cell mesh and must not be pointed at the limb one.
2. **Where do non-directional limbs mount?** The ring convention their dots already use, stable and
   invented-nothing-new. The alternative — point the spike at the nearest neighbour, which is where
   the damage actually goes — is more honest about the mechanic, less honest about the state, and
   jitters as the crowd shifts. The mount of a cilium or a flagellum is *not* a convention: it is
   the sixteen-angle `control[1]` and the thrust genuinely goes that way.
3. **Shell: ring or arc?** **Ring**, decided by the user. Cover is a scalar and a ring invents
   nothing; an arc looks more like a mineral test and invents a facing. Revisit only if the ring
   ever reads as a thickened membrane rather than as armour.
4. **Does a paused slide beat?** No. Phase from the tick, per §6.
5. **Spike and flagellum at `Lod::Packed`?** Not done. Limbs arrive with the organelle dots at 28
   pixels a square. Widening is a decision to take with a `frame_cost` figure in hand, and
   `Lod::Packed` is the tier the M10 frame budget is measured at.
6. **Halo per cell or per square?** Per cell, which slightly overstates two neighbouring digesters
   sharing one volume of water. Per square is the honest version and is a second field on the
   `Frame`.

Still open, and both are in §8: the two-background photograph, and a `shaderbench` panel for the
forms.
