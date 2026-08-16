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

## 1. What is missing

Every organelle a cell carries is drawn the same way: a small round blob on a ring at `0.45` of
the radius, in slot order, sized `0.28 r`, coloured by type (`slide::organelle_dots`). That is a
reasonable picture of a *vesicle*. It is not a picture of a cilium, and it is not a picture of
anything that reaches outside the membrane — of which the catalogue now has six.

Nothing a cell has built changes its silhouette. A cell with four flagella and a drawn spike is
drawn as a circle with five grey dots in it.

| # | type | drawn today | state that exists to draw from | belongs |
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

*grey* means the type falls through the `_ =>` arm of `slide::organelle_colour`. **Eleven of the
twenty drawable types are drawn in the same grey**, so a spike, a shell, a holdfast, a flagellum
and a lipid droplet are at present the same mark. `slide::cell_colour` has the same hole: it tints
from six types and ignores the rest, so a heavily armoured cell and a bare one are the same colour.

The work therefore falls into three tiers of very different cost and risk:

* **Colour** (11 types). No renderer change at all. Two match arms.
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
crates/mm-app/src/cellpipe.rs   + LimbMaterial, beside CellMaterial and DotMaterial.
```

One more mesh entity, drawn at **z ≈ 0.9** — above the junction sprites at `0.5`, below the cell
mesh at `1.0` — so a limb's root disappears under the body it grows from and needs no join drawn.
One more draw call, at a tier where the whole population on screen is a few hundred cells.

The property that makes this the right shape: **the body path is not opened at all** for commits
1–4. `cell.wgsl`, `cellmesh.rs`, the two existing materials and every body probe are untouched and
provably so, so any change in how a cell looks is a bug in the new pipeline and nowhere else. Only
the shell (§5) opens the body, and it is last and alone.

### The layout

Narrow, because a limb needs none of the seams:

| location | attribute | meaning |
| --- | --- | --- |
| 0 | position | world, already rotated by the CPU |
| 1 | uv | `-1..1`, the limb-local frame: `+x` outward, `+y` across |
| 2 | colour | the owning cell's colour after haze and vignette |
| 3 | `limb: vec4` | `x` form code, `y` extent `0..1`, `z` phase `0..1`, `w` aspect |

The CPU emits **rotated** corners, so `uv` is already the limb's own frame and the shader needs no
direction and no trigonometry. A long thin flagellum then gets a long thin quad rather than a
square one sized for the worst rotation, which is most of the fill rate back.

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

* **Hard, in contact — form 6, a band.** A short bar *across* the shared wall, perpendicular to
  the line of centres, drawn **above** the cells at z ≈ 1.1. Length is the contact chord, which
  `slide::squash_of` already knows. This is what a desmosome looks like and it makes a colony read
  as one riveted body instead of as touching discs.
* **Hard, stretched — form 6 with extent.** The band elongates into a taut strut spanning only the
  *gap* between the two drawn outlines, not the centres.
* **Strain.** `junction::distance(i,j)` against `rest` and `config.breaking_strain` gives
  `0..1` to breaking. A junction near its limit thins and lightens. **Real**, and the most useful
  new thing on the slide: a body about to come apart says so before it does.
* **Soft — form 7, a channel.** Faint, dashed, under the cells, unchanged in weight. A soft
  junction is a conversation, not a body.

`JunctionLine` gains `strain: f32` and the two membrane-edge endpoints rather than the centres.

*Deferred:* motes travelling along a soft junction while it is actually transferring. Transfer
volume is not on the `Frame`.

---

## 5. The shell, which is not a limb

Shell coverage is a **scalar** — `organelle::shell_cover` returns one number, `0..7/8` of the body,
and the catalogue is explicit that the same `control[0]` closes the shell and shades the cell under
it because "it is one surface doing one thing". There is no direction anywhere in it.

So the first cut draws a scalar: **a mineral rim of thickness and opacity proportional to cover**,
all the way round, over the membrane wall, plus the body under it darkened by the same fraction —
which is not decoration, it is the mechanic, since a shell reduces the light reaching the
chloroplasts by exactly the fraction of the body it covers.

An *arc* covering `cover × 2π` of the perimeter is prettier and reads as a test rather than a
coating. It is also an invented direction. §12.3.

This is the only part of the plan that opens `cell.wgsl` and the body's vertex layout: one
attribute, `armour: f32`, and about ten lines in `blob`. It lands **last and alone**, with
`check_outline.py` run before and after, and the body probes deliberately re-baselined in that
commit and no other.

**Before any of that**, commit 1 gets most of the value for none of the risk by putting silica-grey
into `cell_colour` weighted by shell `param`. A cell that has invested in a shell then looks like
it, exactly as a cell that has invested in chloroplasts looks green.

---

## 6. The data path

`CellDot` gains `limbs: Vec<LimbDot>`, built in the same walk that builds `organelles`, under the
same `detailed && near` gate:

```rust
pub struct LimbDot {
    pub kind: mm_core::OrganelleType,
    pub dx: f32, pub dy: f32,   // root, offset from the cell centre, in squares
    pub ux: f32, pub uy: f32,   // outward direction, unit
    pub length: f32,            // in squares
    pub width: f32,
    pub extent: f32,            // 0..1 — how far out, how open, how hard it grips
    pub phase: f32,             // 0..1
}
```

The only `mm-core` change is a two-function extraction, so that "how far is *this* spike out" and
"how much does *this* shell cover" have one definition and it is not in the renderer:

```rust
ecology::spike_extension_of(&Organelle) -> i32     // factored out of spike_extension
organelle::shell_cover_of(&Organelle) -> i32       // factored out of shell_cover
```

Both are pure integer refactors of loop bodies that already exist, covered by the existing tests
for the per-cell sums. `sensing::cilium_direction`, `sensing::cilium_thrust` and
`sensing::holdfast_grip_of` are already per-organelle and need nothing.

### The clock

`phase = fract(tick × RATE + hash(cell_id, slot))`, computed on the CPU from `frame.tick`.

**Never wall-clock.** A paused slide must be still — cilia beating on a stopped world would be the
renderer inventing time — and a screenshot at tick N must be reproducible, which the whole bench
harness (`MM_BENCH_AT=30`) depends on.

---

## 7. Level of detail

Limbs at `Lod::Organelles` and above, matching the organelle dots, behind a `view.limbs` toggle
beside `view.organelles` in the View menu.

There is a real argument for the spike and the flagellum at `Lod::Packed` — they are the two most
behaviourally informative marks on the slide, they are long enough to read at twelve pixels a cell,
and the mixed benchmark says only 3.8% of cells carry a cilium and 7.7% a holdfast, so it is a few
percent of the population and not a doubling of the geometry.

Do not do it in the first pass. Measure the Organelles tier first, then widen deliberately with a
figure in hand. `Lod::Packed` is the tier the M10 frame budget is measured at.

---

## 8. How this is tested

The discipline is `docs/OVERLAPS.md`'s and it is applied to the new pipeline *before* it can go
wrong rather than after.

1. **`phantom.rs` gains limbs.** A bench panel drawing every form across a sweep of extent, param
   and phase, with no simulation behind it, through the same shader and vertex layout the
   microscope uses. A wrong-looking spike is then attributable to the shader or to the data in one
   run instead of one run per hypothesis.
2. **`limbmesh.rs` is tested headless**, as `cellmesh.rs` is: every attribute the same length, no
   index past the end, a zero-length limb emitting no quad, corners exactly the rotated frame.
3. **`tests/limb_probe.rs`**, alongside `shader_probe`, asserting numbers rather than looks:
   * a sheathed spike puts **zero** pixels outside the body;
   * a fully extended one reaches `0.9 r ± 1px` in the mount direction and nowhere else;
   * a flagellum's envelope width tracks thrust monotonically;
   * a holdfast at zero grip and one at full grip differ in tip position by the expected amount;
   * the halo's integrated alpha tracks throttle.
4. **A two-background photograph** for the spike and the flagellum, via `tools/check_outline.py`,
   which is the only thing that measures what the shader actually put on the screen against what it
   was told to draw.
5. **Regression, and this is the important one.** For commits 1–4, `shader_probe`, `nine_cells`,
   `overlap_detector`, `swell_probe`, `packing_probe` and `frame_cost` pass **unmodified and
   byte-identical**, because none of those commits touches a file the body is drawn from. Commit 6
   is the only one permitted to move them.
6. **`frame_cost`** must not move at all at whole-slide zoom, because limbs are off at that tier.
   Add a figure at the Organelles tier and gate on it.

---

## 9. Staging

Each of these is one concern and one commit.

| # | commit | touches | risk |
| --- | --- | --- | --- |
| 1 | the eleven missing colours, and shell/spike/holdfast into `cell_colour` | `slide.rs`, two match arms | none |
| 2 | `LimbDot`, the two `mm-core` extractions, the parts walk | `slide.rs`, `ecology.rs`, `organelle.rs` | none — no renderer |
| 3 | `limbmesh.rs`, `limb.wgsl`, `LimbMaterial`, the mesh entity — **spike only** | new files, `cellpipe.rs`, `main.rs` | the pipeline itself |
| 4 | cilium, flagellum, holdfast, exoenzyme | `limb.wgsl`, `slide.rs` | forms only |
| 5 | the junction: membrane-edge endpoints, strain, the contact band, into the limb mesh | `slide.rs`, `limb.wgsl`, `main.rs` | drawing above the cells |
| 6 | the shell, on the body | `cell.wgsl`, `cellmesh.rs`, `cellpipe.rs` | **re-baselines the body probes** |

Commit 3 carries the whole pipeline for one simple form, so that if the pipeline is wrong it is
wrong with a triangle in it and not with five forms to argue about.

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

## 12. Decisions that want review

1. **May a limb be drawn over a neighbour?** A spike wounds the cell it is touching, so a spike
   drawn over its victim is honest — and the body path spent days ensuring nothing is ever drawn
   over anything. *Recommend: yes, explicitly, and say so in `OVERLAPS.md`, and teach
   `overlap_detector` to ignore the limb mesh — or it fires on every predator on the slide.*
2. **Where do non-directional limbs mount?** Spike, holdfast and exoenzyme have no simulated
   direction. The ring-in-slot-order convention extends naturally and is stable. The alternative —
   point the spike at the nearest neighbour, which is where the damage actually goes — is more
   honest about the mechanic and less honest about the state, and jitters as the crowd shifts.
   *Recommend the ring.*
3. **Shell: ring or arc?** §5. A ring invents nothing; an arc looks like a test and invents a
   direction. *Recommend the ring, and revisit only if the ring reads as a membrane thickening
   rather than as armour.*
4. **Does a paused slide beat?** *Recommend no* — phase from tick, per §6.
5. **Spike and flagellum at `Lod::Packed`?** §7. *Recommend not in the first pass.*
6. **Is the exoenzyme halo drawn per cell or accumulated per square?** It dissolves matter into the
   *square*, and two neighbouring exoenzyme cells are digesting one shared volume. Per cell is
   cheap and slightly overstates it; per square is honest and is a second field on the `Frame`.
   *Recommend per cell for now, and note it.*
