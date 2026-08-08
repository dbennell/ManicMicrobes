# Every cell on the slide is exactly as stiff as every other

An investigation prompted by a fair observation — *cells in close proximity pack and squish
together, and a yeast cell does not do that; it circle-packs and holds its shape* — and a
proposal that followed it, to let a cell invest in microtubules and turgidity to resist being
deformed.

Short version, and it is three answers rather than one:

1. **The physics is already close to circle packing.** Measured, a settled pack rests at 94.3% of
   its touching distance against a core floor of 95.0%, with **zero pairs** driven through it.
   Cells are not being squashed by the solver.
2. **The squish is drawn on purpose.** `slide::area_swell` grows each cell until what survives its
   neighbours' seams encloses the area the cell actually has — that is what "separates a foam from
   a gravel pile", in its own words, and it is the reason a packed sheet reads as polygons however
   little the physics overlaps them.
3. **But the proposal is right anyway, and cheaper than it looks.** Stiffness is a global constant
   today: `CORE_PERMILLE`, one number, identical for every cell in every scenario. And the
   mechanism that ought to make a cell rigid is **already being paid for and does nothing** —
   turgor is charged as a quadratic energy bill and has no mechanical consequence whatever.
   Making it one needs no organelle, no opcode and no new state.

---

## 1. What the physics actually does

Measured, not reasoned about. `cargo test -p mm-app --test packing_probe --release -- --ignored`,
default `BiologyConfig`. The core floor is `CORE_PERMILLE` = 950, so a pair may compress to 95.0%
of the distance at which their outlines merely touch, and past that the response is sixteen times
stiffer.

| slide | pop | pairs past the core | closest pair | area | solute, p50 |
| --- | ---: | ---: | ---: | ---: | ---: |
| bench, settled | 220 | 0 (0.0%) | 94.3% | 24% | 0.0 |
| growth, dividing | 54 | 0 (0.0%) | 90.7% | 82% | 2.0 |
| growth, +200 no births | 51 | 0 (0.0%) | 94.3% | 83% | 3.7 |
| growth, +800 no births | 39 | 0 (0.0%) | 94.6% | 60% | 9.4 |

> **Nothing anywhere is through the core**, and the worst pair on the slide sits within a
> percentage point of resting exactly on it.

The trajectory says the same over time: area climbs to 118% and `deep%` reads 0.0% at all but
three of twenty-four samples, peaking at 12.5% during the fastest burst of division — which is a
daughter placed inside her mother, not a crowd collapsing.

So the ~5% resting compression a pack shows is **the soft band, and the soft band is the design**.
SPEC §6.4 is explicit that contact is not a non-penetration constraint and that the resting
overlap is the tissue rather than an error, for a reason that survives inspection: circles that do
not overlap cannot tile a plane, and a crowd solved to convergence is a bag of marbles with holes
between them.

### The dial that does decide it

The one number that turns a pack into a pile is not the core. It is `split_pressure`, swept in the
same probe:

| `split_pressure` | pop | area | pairs past the core | closest pair |
| ---: | ---: | ---: | ---: | ---: |
| 3.0 | 120 | 147% | 11.2% | **44.7%** |
| 2.0 | 95 | 141% | 4.0% | 56.1% |
| 1.5 | 85 | 127% | 2.8% | 69.0% |
| **1.0 (default)** | 69 | 111% | 0.0% | 93.0% |
| 0.6 | 62 | 106% | 0.0% | 94.1% |

At 3.0 cells are pressed to 45% of tangency — nearly halfway inside one another — and an eighth of
all pairs are through the core. The core is stiff, not infinite, and `MAX_SHOVE` caps it further;
enough load defeats it. **Whether a slide squashes is decided by how many cells are allowed to
exist, not by how hard they resist.** Worth knowing before tuning any stiffness at all.

---

## 2. The squish is in the renderer, and it is deliberate

This is the `OVERLAPS.md` discipline arriving on schedule: the complaint is about the picture, so
rule out the data before touching the physics.

`slide::area_swell` holds every seam still and grows the circle until the clipped shape encloses
the cell's true area. Its own documentation says exactly what it is for:

> A cell is a bag of nearly incompressible fluid. Squeeze it and it does not lose volume; it
> changes shape and bulges out wherever nothing is holding it in. … This is what separates a foam
> from a gravel pile, and it is the thing that was missing when a packed crowd still read as a
> heap of pebbles with holes between them.

The consequence for this investigation is the important one, and it is a trap for the proposal in
§4: **area conservation is independent of how much the cells overlap.** Raise every cell's core to
99% so that a pack rests almost tangent, and `area_swell` will inflate each cell into the gaps
that opens up, and the sheet will be drawn as the same foam it is now. A rigid pack and a floppy
pack would look identical.

So a stiffness that is meant to be *visible* has to reach the renderer as well as the solver, and
the physical story says how: a bag bulges into free space and **a walled cell does not**. The
swell gain becomes per-cell, `1 − rigidity`. That is a change to `slide.rs` and it is a legitimate
one — `area_swell` is already `pub` precisely so the shader bench can drive it on cells no
simulation made, and `phantom.rs` can exercise a per-cell gain without a graphics stack. It is a
picture of the physics, which is `slide.rs`'s job, and not chrome, which UI.md §8.1 keeps out.

**Built, and it is one line at the call site.** `area_swell` is unchanged and still `pub` for the
bench; `squash_of` applies `1 + (1 − rigidity) × (swell − 1)`. `marble_probe` measures the axis on
a fixed lattice of 220 inert cells:

| membrane | rigidity | mean swell | |
| ---: | ---: | ---: | --- |
| — (no turgor) | 0.00 | 1.237 | today's foam, exactly |
| 24 | 0.09 | 1.215 | what the ancestors build: 1.8% off, imperceptible |
| 64 | 0.25 | 1.178 | |
| 128 | 0.50 | 1.118 | |
| 200 | 0.78 | 1.051 | |
| 255 | 1.00 | 1.000 | marbles — the true circle, cut by its seams, gaps left |

The default picture is preserved and the marble end is *bought*: membrane 24 → 255 is 5.1× the
structural matter to build and 6.8× the upkeep to carry, on top of holding the solute that
pressurises the wall, which `osmotic_upkeep` charges for quadratically.

Note which rigidity the renderer reads. `biology::rigidity` is wall × turgor and nothing else;
`neighbours::core_permille` multiplies it by `MetabolicRates::rigidity_gain` before letting it
change the physics. The split is deliberate — swell has no counterpart anywhere in the simulation,
because nothing in the engine has an opinion about a cell's *shape*, so it is free to vary per
cell in every world, while the stiffness that changes what the simulation does stays behind a
scenario knob and off by default.

---

## 3. Turgor is a bill that buys nothing

Here is the finding that makes the proposal cheap.

`MetabolicRates::osmotic_upkeep` charges a cell for the solute it holds, quadratically in whole
capacities over `osmotic_threshold` (four capacities). Its own documentation gives the magnitude:

> a cell one capacity over pays a thirty-second of an energy unit a tick — noise against a working
> loadout's 0.45 — and one at the measured converged median of ten pays about a thousand `Q10`,
> which is more than twice its whole organelle bill.

And the packing probe says who is paying it. The `solute, p50` column of §1 is interior contents
as a multiple of interior capacity: **the median cell in a settled growth slide carries 3.7 to 9.4
capacities**, the ninetieth percentile reaches 10.7, and the fullest cell measured is at **13.1**.
Against a threshold of four. `gradient_probe::rind_to_core` found the same and added the part that
matters here — the *buried* cells are the full ones:

| contacts | n | mass | energy | fill |
| --- | ---: | ---: | ---: | ---: |
| 0–2 (rind) | 2 | 15.9 | 2,893 | 633% |
| 3–4 | 133 | 35.3 | 3,394 | 823% |
| 5–6 | 158 | 62.7 | 3,988 | 1,105% |
| 7+ (core) | 13 | 77.3 | 4,237 | **1,295%** |

> **The cells deepest in a pack are the ones already paying the largest turgor bill, and turgor is
> the physical property that would make them resist being deepest.** The charge exists, the load
> exists, the measurement exists, and the mechanical consequence is missing.

Two smaller things fall out of the same hole. `vacuoles 0` in every row of the probe — **no cell
in any measured run has ever built one**, which is unsurprising when a vacuole's only effect is to
take solute out of the osmotic count and so reduce a bill. And a cell has no reason to prefer
being turgid over being lean, because being turgid is strictly a cost.

---

## 4. What stiffness should be made of

A real cell resists deformation with a **wall** and a **pressure**, multiplied. A wall with no
turgor is plasmolysed and floppy; turgor with no wall bursts. Yeast circle-pack because they have
both; animal tissue deforms into polygons because it has the second and not the first.

Both quantities already exist here.

| | where it already lives | what it costs today |
| --- | --- | --- |
| **wall** | `membrane.param`, plus `control[1]` investment (`metabolism.rs:620`) | build matter, upkeep, and it already buys damage tolerance |
| **turgor** | `biology::osmotic_load`, the sum of free solute | `osmotic_upkeep`, quadratic, and it buys nothing |

So:

```rust
/// How stiff a cell is, `Q10`. Zero is a bag; one is a walled sphere.
///
/// A product, not a sum: a wall with no turgor is plasmolysed and a pressure with no
/// wall has nothing to push against.
fn rigidity(...) -> i32 {
    q10_scale(wall_strength, turgor_fraction)
}

/// Where this cell stops compressing, in permille of its own radius.
fn core_permille(rigidity: i32) -> i32 {
    CORE_FLOOR + ((CORE_CEIL - CORE_FLOOR) * rigidity) / Q10_ONE
}
```

and the one line in `neighbours::correction_for` that currently reads

```rust
let core = ((want as i64 * CORE_PERMILLE as i64) / 1000) as i32;
```

becomes each cell contributing its own incompressible core:

```rust
let core = ((ri as i64 * p_i as i64 + rj as i64 * p_j as i64) / 1000) as i32;
```

Four properties, all of which have to hold and all of which do:

- **Symmetric.** Both sides of a pair compute the same `core` from the same two cells, which
  separation has needed since it became Jacobi — each cell computes its own share and the two must
  agree without talking.
- **Reduces exactly.** With every `p` equal to 950 this is the expression it replaces, digit for
  digit, so nothing changes for a world that does not use it.
- **No new state.** Rigidity is derived fresh each tick from mass, organelles and interior, like
  `radii`, `crowding` and `pressure` — scratch, excluded from equality, hashing and snapshots.
  **Hard rule 7 is untouched**, which is the single best thing about this proposal.
- **Integer, order-free, deterministic.** Nothing here reads a neighbour's derived value, only its
  radius and its own permille.

### What it buys that is not cosmetic

- **A trade with no arbitrary number in it.** Hold matter in solution and be rigid but pay the
  quadratic bill; polymerise it into a vacuole and store it cheaply but go floppy. That is the
  glycogen-against-glucose trade `biology.rs:261` already describes, with the missing half
  supplied — and it gives the vacuole, which nothing has ever built, a reason to exist.
- **Stratification without a porosity term.** `FEEDING.md` §8 item 3 wants the interior of a pack
  to be a bad place to be. A rigid cell resists being buried; a floppy one is cheap and gets
  squashed. That is a second, independent route to the same end and it costs nothing in the fluid
  solver, which is the piece already furthest from its gate.
- **Two body plans instead of one.** A lineage that invests in wall and turgor circle-packs and
  keeps its shape; one that does not deforms and tiles. Neither is written down anywhere in the
  engine, which is the design rule (no cell-type enum) working as intended.

### The optional half, which needs measuring before it ships

Turgor above what the wall can hold should burst the cell — osmotic lysis, which is real and which
would make the trade two-sided rather than a free ratchet towards being maximally full. The
machinery is there: it is membrane damage, charged like every other kind. It is left out of the
recommendation because it is a new way to die and belongs behind its own measurement, not bolted
onto a change that is otherwise strictly additive.

---

## 5. Microtubules are a different want

Worth separating, because the proposal paired them with turgidity and they do different jobs.

**Compression resistance does not come from microtubules.** A yeast holds its shape with a cell
wall and turgor; that is §4 and it needs no new organelle. What a cytoskeleton is actually for, in
the material that prompted all this, is holding a structure *out* into the water — the heliozoan's
axopodia have "a central supporting rod of microtubules that gives it this rigid structure", and a
suctorian's tentacles are "supported by an internal cylinder of microtubules". That is a rigid
projection, which is much closer to the spike than to the core floor, and it is a separate
mechanism with a separate cost.

And it is expensive in the only currency that is actually short. `FEEDING.md` §5 measured the
catalogue: **one type slot left**, and the flagellum needs it because the cilium has no free
control word to become one. A cytoskeleton would be the second claimant on the last slot, which is
the argument for §6 of that document — widen the catalogue to 32 on the `n + 16` pairing — rather
than an argument for building it now.

**Recommendation: take the turgor half, defer the microtubule half.** They were proposed together
and only one of them is blocked on anything.

---

## 6. Two things the spec says that are not true

Found while checking §1, reported here under CLAUDE.md's rule that a wrong spec gets said out loud
rather than quietly implemented around. Both are in SPEC §6.4.

**"The core fraction is shared with the renderer … they must be changed together."** They are not
the same fraction and have not been for some time. `neighbours::CORE_PERMILLE` is 950; `slide::
MIN_FACE` is 0.55, and its own comment records why:

> Setting this to the core made it bind on every contact in the pack rather than on the rare bad
> one, so the seam stopped being the plane through the crossing outlines and became the clamp, and
> cells came out as wedges. … a clear mistake, worth recording so it is not tried again.

The two are a floor on centre distance and a floor on where a cell may be cut, they catch different
cases, and coupling them was tried and reverted. `CORE_PERMILLE`'s own doc comment repeats the
stale claim and should lose it.

This matters for §4 rather than being pedantry: it is what makes per-cell stiffness safe. If the
renderer's clamp really did track the core, a per-cell core would force a per-instance clamp
through the vertex layout, and `OVERLAPS.md` is exactly about what that costs. It does not, so
the seam stays a pure function of positions and radii, and the only renderer change on the table is
the swell gain of §2 — which is optional, and only affects whether the effect is visible.

**"Crowding damage is charged on core penetration only."** It is charged on the whole compression,
and `neighbours.rs` records the reversal in place: charging on core penetration was tried, and
with a core this tight *nothing* penetrates it, so the measure read zero for every cell on the
slide and quietly deleted crowding pressure altogether.

---

## 7. Built, and what it turned out to be for

§4 was written as a way to make an idle charge buy something and to unblock terminal
differentiation. It was built for a different reason and works for a third.

`transport_probe` had established that making light rival — the only resource a cell here cannot
manufacture — removes the advantage of being buried in a crowd and then **stops**. Pushing
occlusion harder does not push further, because the escape route is *size*: a shaded cell grows
less, shrinks, overlaps its neighbours less, and is therefore shaded less. It is a self-limiting
feedback, not a dial.

Stiffness closes that route, and the mechanism is narrower than "rigid cells resist crowding":

> `pressure` is normalised against the band between touching and the core. A rigid cell has a
> **narrow band**, so it reads near-maximum pressure as soon as it is touching anything at all.
> **Stiffness turns `pressure` from a measure of depth into a measure of count** — and a count of
> neighbours does not fall when a cell shrinks. Neither does turgor: `osmotic_load` is what a cell
> holds and interior capacity does not scale with mass.

Measured on the thicket conditions of §1's follow-up — carbon 40, `fluid_interval` 8, five seeds,
mutation off — with `light_occlusion` at `Q10/8`:

| `rigidity_gain` | population | carbon contrast | **buried** |
| --- | ---: | ---: | ---: |
| off | 263 | 98% | 100% [99–103] |
| ¼ | 267 | 84% | 101% [98–104] |
| 1× | 260 | 80% | 100% [99–102] |
| 4× | 262 | 67% | 99% [98–100] |
| **16×** | 281 | **20%** | **96% [92–98]** |

At 16× every seed is below 100 and the carbon gradient has sharpened five-fold. **That is the
first condition in this project's history where the middle of a crowd is a worse place to be than
the rind**, which is what SPEC §17.8 set out to produce and what every previous attempt failed at.

Three caveats that belong with the number:

- **It does nothing alone.** With occlusion off, the same sweep moves `buried` only from 118% to
  109%. And it does not work with *more* occlusion either — at `Q10/2` the figure stays at
  106–114% at every gain. The four settings of `scenarios/the_thicket.ron` are one setting.
- **The gain is large because the wall term is badly scaled**, not because the effect is weak. Wall
  strength is normalised against the largest membrane a cell could build and the ancestors build
  24 of a possible 255, so most of the dial is spent compensating for that. Calibration is M8's;
  the number recorded here is the measured one rather than the tidy one.
- **A dormant middle, not a dead core.** §17.8 wants a growing rind, a dormant middle and a dead
  core. This reaches the first two.

`rigidity_gain` is zero in `MetabolicRates::default`, so every measurement taken before it is
untouched — `core_permille` returns exactly the constant those measurements were made against, and
`neighbours::rigidity_is_off_by_default_and_never_softens_a_cell` asserts it. §4's other claims
still stand and are still unbuilt: the vacuole trade, the two body plans, and the osmotic-lysis
half.

---

## 8. What firmness is for

§7 gave the mechanism a use it was not designed for and this is the one it was: firmness costs
matter and upkeep, and until it bought something a genome could *want*, it was a rendering
preference with a bill attached.

**It is drag.** A bag of fluid pressed into other bags of fluid has a wide, flattened, sticky
contact and drags badly; a hard round body has very little contact and slips past. So
`neighbours::CONTACT_FRICTION` — the fraction of a sliding cell's speed that survives a tick,
which was one number for every cell — scales with firmness. A limp cell keeps a quarter of it, as
everything did before; a marble keeps almost all.

Measured, one cilium at full power driving one swimmer into a fixed lattice of limp cells,
sampled while it is still inside the crowd:

| swimmer | membrane | firmness | squares by tick 7 / 14 / 21 / 28 |
| --- | ---: | ---: | --- |
| limp | 24 | 0.00 | 5.73 · 9.72 · 13.61 · 17.31 |
| soft | 24 | 0.09 | 5.73 · 9.94 · 13.89 · 17.90 |
| firm | 200 | 0.78 | 5.71 · 10.25 · 14.27 · 19.43 |
| marble | 255 | 1.00 | 5.70 · 10.63 · 15.40 · 19.24 |

Monotone, and worth about a tenth over one crossing. That is the right size for it: it compounds
over a hunt, it is free to a cell that is soft and staying where it is, and it is paid for in wall
and turgor by one that is not.

> **Which makes firmness a strategy rather than a look.** Being soft is fine if you photosynthesise
> — sitting in a mat of your own kind in the light is the whole plan, and a tessellating mat is
> what that looks like. It is ruinous if you hunt, because the thing you are hunting is inside a
> crowd, and getting in, through and out again is exactly the manoeuvre a soft cell cannot make.

Two things found by measuring it rather than assuming it, both recorded in place:

- **The gain saturates.** `rigidity_gain` multiplies wall times turgor and clamps at one, so at a
  gain of sixteen anything above a sixteenth is fully firm — and a membrane of 24 out of 255 is
  already 0.094. Every cell on the slide read 1.00 and the first run of the swim probe produced
  four different bodies and one identical distance. `the_marbles.ron` uses a gain of **one**, where
  `marble.mm` reads 1.00 and an ancestor reads 0.09.
- **A cilium at full power crosses a 32-square slide in well under a hundred ticks**, so the first
  version of the probe measured every swimmer against the far wall. A measurement that saturates
  is a measurement of the boundary.

---

## 9. The recommendation

1. **Correct SPEC §6.4** points 2 and 3, and drop the stale sentence from `CORE_PERMILLE`'s doc
   comment. Documentation only, no behaviour, and it is a prerequisite for anyone reasoning about
   the rest of this.
2. **Make the core per-cell**, as §4. Rigidity from wall × turgor, both already present and both
   already paid for; `CORE_PERMILLE` becomes `CORE_FLOOR`/`CORE_CEIL` with today's value as the
   point where a default cell lands, so a world that ignores it is unchanged. Scratch, so rule 7
   is untouched. An organelle-catalogue semantics change — `membrane.control[1]` gains a second
   meaning — so ISA 5 → 6, which `FEEDING.md` §6 also wants and which should be one bump.
3. **Re-run `packing_probe`** against a lineage forced turgid and one forced lean. The claim to
   test is that the two settle at measurably different depths and different populations, and that
   the turgid one is not simply poorer — the bill is quadratic and it may swamp the benefit at the
   default rate, in which case the finding is which of the two numbers is wrong.
4. **Then, and only if 3 says the physics moved**, gate `area_swell` on rigidity so the difference
   is visible. Not before: an invisible mechanism that works is a better problem than a visible one
   that does not, and §2 is the reason a rigid pack currently would not look any different.
5. **Osmotic lysis** — turgor past what the wall holds does damage — behind its own measurement.
6. **The cytoskeleton, after the catalogue is widened**, and as a rigid *projection* rather than as
   a stiffness term. It is not what makes a yeast a sphere.

Items 1 and 2 are the whole of the change; 1 costs nothing and 2 adds no state. What makes it worth
doing is not that cells will look rounder. It is that a quadratic energy charge which every cell in
every run has been paying since §17.7 landed would finally be buying something, and that "how much
does this lineage resist being squashed" would become a trait rather than a constant.
