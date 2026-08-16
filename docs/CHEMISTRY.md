# Sixteen chemicals, and why that was never the constraint

An investigation prompted by a fair question — *we picked 16 because it sounded about right; is
it still?* — and the answer it produced, which was not the one the question expected.

Short version: **16 is fine, and roughly ten of them are scenery.** Adding chemicals cannot add
complexity, because the mechanism that would use them does not exist. What is scarce is
*reactions*, not *species*. The rest of this document is the evidence, and the design of the
change that follows from it.

**That conclusion has since been acted on, which dates the sentence above.** §3's items 1 and 3
were built — multiple metabolic pathways (§4) and per-organelle recipes in a second structural
chemical — so the mechanism that would use them now does exist, and four of the ten have stopped
being scenery. §2's table carries both readings side by side. The claim that survives unaltered
is the middle one: it was reactions that were scarce, and supplying them is what made the
existing species matter.

---

## 1. What the number costs

Measured, not reasoned about. `CHEM_COUNT` was temporarily set to 32 and the gates re-run on
20 threads:

| | 16 | 32 | change |
| --- | --- | --- | --- |
| fluid 512², every plane carrying matter | 5.18 ms | 11.05 ms | **+118%** |
| fluid 512², a realistic scenario | 4.84 ms | 4.79 ms | **+3%, p = 0.46** |
| cell tick at 60,000 cells (M2 gate) | 21.0 t/s | 21.1 t/s | **none measurable** |
| substrate memory at 512² | 16.8 MB | 33.5 MB | ×2, eagerly allocated |
| per-cell interior | 64 B | 128 B | ×2, of a 512 B budget |

The gap between the first two rows is the whole performance story, and it is not the one you
would guess. `Substrate::present[]` already skips any plane that holds nothing, exactly, so:

> **Declaring a chemical is nearly free. Stirring one is exactly linear.**

The fluid is the piece that is already furthest from its gate — 186 steps/second against the
500 M1 asks for — so each chemical actually carrying matter costs about 6% of a budget that is
already overspent. The cell tick does not care at all: at sixty thousand cells the extra 64
bytes of cytoplasm per cell is lost in the noise of VM execution and neighbour scans.

So the performance answer to "could we have a few more?" is *yes, as many as you like, as long
as they stay empty* — which is a strange answer, and a hint that the question is aimed at the
wrong thing.

---

## 2. What the sixteen actually do

Six have engine semantics. `MetabolicChemistry::default` names them:

```rust
substrate: 8,   // sugar
oxidant: 14,    // oxygen (called `brine` until it was renamed for being mistaken for filler)
waste: 11,      // carbon_dioxide
byproduct: 14,  // the same filler back again
structural: 4,  // carbon
reactive: 13,   // peroxide
```

Plus carrion at 15, named in `ecology.rs`. Note that `oxidant` and `byproduct` are the *same
index*, and `OrganelleCatalogue::closes` actually requires them to be — `byproduct` is a
vestigial name for the thing photosynthesis produces alongside the substrate, which is the
oxidant.

That leaves ten. **This inventory is the one taken when the document was written, and three of
its four rows have since stopped being true** — which is not an erratum but §3 being built. It is
updated here rather than left standing, because a stale inventory of spare capacity is the thing
somebody reads when deciding where a new chemical goes; the "then" column is what prompted §3 and
§4 and is kept so the argument still reads.

| | | then | now |
| --- | --- | --- | --- |
| `signal_a`–`signal_d` | 0–3 | **Genuinely useful.** `EMIT`, `EAT` and the chemosensor all work on them and the engine ascribes them no meaning, which is exactly right for a communication channel evolution is supposed to invent a use for. | Unchanged, and one of them is now load-bearing: `drifter_blind.mm` watches `signal_d` *because* nothing emits it, which is what makes it M3's blind control. Giving index 3 a meaning breaks that control. |
| `nitrogen` | 5 | Flagged `structural: true`, but only index 4 is *the* structural chemical. Nothing can be built out of it. | Consumed by the diazosome (ISA 7), which converts it to carbon. Still built from by nothing, and — like phosphorus and silicon — **produced by nothing**: seeding or a `Flux` is the only way any of the three enters a world. |
| `phosphorus` | 6 | As above. | **Unchanged: referenced by no mechanism anywhere.** The last entry in the table that is inert in both directions. |
| `silicon` | 7 | As above. | **Built from.** `OrganelleSpec::build_trace` (ISA 7) gives the shell a recipe of `q10(6)` silicon — the first and so far only use of a second structural chemical, which is §3 item 3. |
| `lipid`, `sulphide` | 9–10 | Carry `energy_yield` of 1536 and 768. But a mitochondrion burns `chemistry.substrate`, which is one index. **Nothing burns them.** | **Both are burnable.** `MetabolicChemistry::default` runs pathways on substrates 8, 9, 10 and 8 — §4 shipped, and it is what made the yields these two have always carried mean something. |
| `ammonia` → `detritus` | 12 | Filler. | **The slot changed hands.** Ammonia is gone; index 12 is detritus, the particulate of SPEC §17.4. A filter converts it straight to structural matter and it decays to carbon, so it is the return leg of the decomposition chain rather than filler. |

So the sentence this section used to end on — *lipid is a food no organelle can eat; phosphorus
is a building material nothing is built from* — is now half true. Lipid is eaten. Phosphorus is
still built from by nothing, and is the only chemical of the sixteen about which the original
complaint stands unaltered.

---

## 3. The actual constraint

`MetabolicChemistry` is **one struct with one set of indices**. One substrate, one oxidant, one
waste, per world, by construction. There is exactly one way to make a living, and every cell in
every scenario makes it the same way.

That is the ceiling on emergent complexity, and it is not raised by adding chemicals. Three
things would raise it, in descending order of value:

1. **Multiple metabolic pathways.** Let an organelle choose which reaction it runs. Lipid and
   sulphide become real alternative foods, with different yields and — because a chloroplast
   also picks a pathway — different producers. §4 designs this; it is what we are building.
2. **Chemical decay, which is implemented and entirely unused.** `ChemicalDef::decay_to` and
   `decay_rate` work (`world.rs:620`) and no shipped scenario sets either. That is a free
   abiotic reaction network — A becomes B at a rate — that turns inert species into an
   environment with its own gradients and timescales. It costs a scenario edit, and since
   M10.2 that is an edit anybody can make from the UI.
3. **More than one structural chemical.** Different organelles costing different monomers makes
   the local depletion of phosphorus a real spatial niche, instead of every cell competing for
   one resource everywhere.

---

## 4. Multiple metabolic pathways

### The shape

```rust
pub const PATHWAY_COUNT: usize = 4;

/// One metabolic reaction, in both directions.
///
///   respiration     substrate + oxidant  ->  waste (+ reactive) + energy
///   photosynthesis  2 waste + light      ->  substrate + oxidant
pub struct Pathway {
    pub substrate: usize,
    pub oxidant: usize,
    pub waste: usize,
    pub reactive: usize,
}

pub struct MetabolicChemistry {
    pub pathways: [Pathway; PATHWAY_COUNT],
    /// What a body is built out of. Shared: a cell is one body whatever it eats.
    pub structural: usize,
}
```

`byproduct` goes. It was constrained to equal `oxidant` and named the same thing twice.

### Which pathway an organelle runs

`control[1]`, uniformly, on every organelle that runs a reaction — mitochondrion, chloroplast,
lysosome. Reduced modulo `PATHWAY_COUNT`, so every value is legal (hard rule 4: addressing
wraps).

`control[1]` rather than `control[0]` because the lysosome's `control[0]` is already its
digestion throttle and the spike's is its extension. Putting the pathway in the same slot
everywhere is worth more than putting it first, and it leaves `control[0]` free to become a
throttle on the mitochondrion and chloroplast too, which is a thing we will want.

### Why this is the change worth making

A mitochondrion on pathway 1 can burn only pathway 1's substrate. If nothing in the world makes
that substrate, there is none to burn. So a lineage must either **pair its own chloroplast and
mitochondrion onto the same pathway**, or **eat something that makes what it burns**.

That is cross-feeding, and it is the first mechanism in this engine that makes one lineage's
waste another lineage's food *by evolution* rather than by construction. It is also, usefully,
a trait the analysis layer can already see: two cells with the same organelles and different
`control[1]` are different species doing different jobs, and the wiki will say so without
knowing anything about pathways.

### What has to stay true

- **I4, exact matter conservation.** Each pathway's reactions are balanced in the same way the
  single one is, and reported to the ledger as balanced conversions. A pathway whose waste is
  another pathway's substrate makes a chain, which is legal and interesting; a pathway that
  does not close is refused by `closes()` before the world starts.
- **I5, energy accounting.** `recompute_stored` derives the world's latent energy from what is
  actually there. It reads one substrate today; it must sum over the *distinct* substrates of
  the configured pathways, each at its own per-unit yield, counting a shared substrate once.
- **Cost.** The metabolic step must not become four scans of sixteen slots. One pass
  accumulating capacity per (type, pathway) replaces today's two `conversion_capacity` scans,
  so the restructured version does *less* slot-walking than the one it replaces.

### What building it turned up

Two things worth recording, neither of which was the point.

**A hardcoded chemical 8.** A mitochondrion's second `OGET` reading is "substrate available, so
a cell can tell starvation from idleness" — and it read chemical 8 outright rather than the
scenario's substrate. That was already wrong for any scenario posing a different loop, and
became much louder when a mitochondrion could be set to burn lipid and still be told about
sugar. Fixed as part of this; `CellHost` now carries the chemistry.

**Respiration without excretion is lethal in under two hundred ticks.** The first version of
`a_lineage_can_only_burn_what_it_is_set_to_burn` measured survival, and got a result that looked
like the mechanism working *backwards*: the cell that could eat died and the one that could not
sat there intact. It was not backwards. A cell with a mitochondrion and no genome to `EMIT`
accumulates peroxide until its membrane fails, and a cell that cannot burn anything makes no
peroxide and simply idles. The test now counts substrate burned, which is what the claim is
about. Nothing is broken — but the M9 ageing mechanism is stronger than the numbers alone
suggest, and it is worth knowing that "can it eat" and "does it live" are not the same
experiment.

### ISA version

This is an organelle-catalogue change and it changes what an existing genome does: `control[1]`
on a mitochondrion meant nothing and now selects a reaction. Hard rule 8 — ISA version bump,
1 → 2.

Cheap right now, and that is part of why it is being done now: no shipped `.ron` stamps a
version, no `.mm` carries one, and there are no saved snapshots outside the test suite. The
same change in six months costs an archive migration.

---

## 5. The recommendation on the number itself

**Keep 16.** Not because it is optimal — because ten slots are already idle, and raising the
count does not change that.

If we do want more later, **32 is the right next number**, not 20 or 24. Two reasons: the index
stays a mask rather than a division, and the default table's grouping — four signals, four
monomers, then substrates — stays inside one-bit-flip neighbourhoods, so a mutated chemical
index still tends to land on something of the same kind.

That last point deserves a correction to the comment on `CHEM_COUNT`, which says 16 keeps a
mutation "a small local perturbation rather than a 1-in-100 lottery". As stated that is
backwards: with 16, one bit-flip reaches 4 of the other 15 — 27% of the space; with 32 it
reaches 5 of 31, or 16%. A larger space makes a single mutation *more* local, not less. What is
actually valuable is that related chemicals sit adjacent, and that is a property of the layout
rather than of the count.

One thing worth fixing regardless of the number: substrate planes are allocated eagerly,
`vec![vec![0i32; n]; CHEM_COUNT]`, even though the solver already skips the empty ones. That is
16.8 MB of guaranteed zeroes at 512², and it would be 33.5 MB at 32.

---

## 6. Where the seeded carbon actually starts to bite

Written when the front end grew a control for it (`docs/UI.md` §9.6). Until then the starting
chemistry was a number in a `.ron` that nobody could change without a text editor, so nobody had
swept it, and the obvious story — *carbon is the body, matter is conserved, therefore the carbon
in the water is the carrying capacity* — had never been checked. It is wrong over the whole range
anybody would think to try.

64×64, `light: Uniform(819)`, 8 founders of `ancestor.mm`, CO₂ and oxidant held at 400 units a
square, one seed, population read at tick 20,000:

| carbon, units/square | population at 20k |
| ---: | ---: |
| 400 | 985 |
| 40 | 988 |
| 10 | 1032 |
| 4 | 347 |
| 1 | 65 |
| 0.25 | 31 |

**Flat across the top forty-fold, then a cliff.** The shipped soup seeds 400 units a square,
which is two orders of magnitude above the knee at roughly 4–10. Anyone setting out to build a
scarce world by halving the carbon — or by taking a tenth of it, as
`photosynthesis_or_die.ron` does — has changed nothing about what limits the population, and
would reasonably conclude that carbon does not matter. It matters below 10.

What limits it above the knee is not established here and should not be guessed at: the same
sweep over light intensity returned 1432, 986, 1887, 629 and 0 for 1024, 819, 512, 256 and 128,
which is not a curve. **A population on these slides oscillates** — it overshoots, starves back
and settles — so a single reading at a single tick is a phase sample, and two of them can rank
either way by luck. That is a caveat on this table too: the carbon column is trustworthy because
the effect below the knee is an order of magnitude and monotonic, not because one seed is enough.

Two things follow, one of which is a job:

1. **The `--sweep` path should report a windowed mean, not a final tick.** `mm-cli sweep` exists
   and takes `--param`; nothing about it says the number it prints is a phase sample.
2. **`soup.ron`'s 400 is worth revisiting.** Not lowering it — the soup is the control condition
   and changing it invalidates every comparison made against it — but knowing that it sits a
   hundred times above the constraint means the soup has never been a test of matter
   conservation's ecological consequences, only of its arithmetic.

## 7. The fresh slide moved to 40, and everything rings — the question is the period

`petri_of` — what the microscope opens on, and what `New slide` builds — seeded the same 400 a
square that §6 describes as sitting a hundred times above the constraint. Measuring where a
settled slide actually sits puts the working level nearer forty. It is now **40**, in all three
places the front end starts a world from: `petri_of`, the New scenario sheet's prefill, and the
value the scenario editor puts in a chemical row you add by hand. `soup.ron` and the rest of
`scenarios/` stay at 400 for item 2's reason.

**This section was first written claiming 100, and that claim was wrong.** It is left recorded
rather than quietly corrected, because the way it was wrong is the whole lesson and it is §6's
own caveat biting the person who wrote it: *"a single reading at a single tick is a phase sample,
and two of them can rank either way by luck."*

The first run went to 60,000 ticks. At 60,000 the 100 world read 14,452 and had moved less than
one percent in forty-eight thousand ticks, which is as flat as a population gets, and it was
reported as the only stable arm of four. It was a phase sample of a slower ring. It peaked at
tick **64,000** — a few thousand ticks after the measurement stopped — and had shed a quarter of
itself by 120,000.

256×256, `light: Uniform(1024)`, 16 founders of `ancestor.mm`, one seed, `--check` clean
throughout, run to 120,000:

| seeding | peak | at tick | at 120,000 | drift over the last 20,000 | distinct genomes |
| ---: | ---: | ---: | ---: | ---: | ---: |
| **40** | 23,898 | 120,000 | **23,898** | **+0.17%** | **1,739** |
| 50 | 14,975 | 16,000 | 12,834 | +17.1%, rising | 1,101 |
| 60 | 15,039 | 16,000 | 9,754 | +26.4%, rising | 865 |
| 100 | 14,469 | 64,000 | 10,081 | −24.5%, falling | 664 |
| 400 | 16,388 | 16,000 | — | — | 720 at 60,000 |

**Every level above 40 rings. What the seeding sets is the period, not the presence.** 50 and 60
are the same shape as 400 caught on the upswing; 100 is the same shape with a period longer than
the window it was first measured in. Only 40 is flat, and it holds the most diversity of any arm
by a factor of two and a half — which is the reading that most argues for it, since a slide that
loses two thirds of its lineages to each trough is a worse place to watch evolution happen than
one that does not trough.

The settled population is also *higher* with a tenth of the carbon: 23,898 at 40 against 10,081
at 100 and 9,944 at 400. So a slide seeded well above the knee is not a well-fed slide, it is a
slide with most of its matter out of circulation and a population large enough to keep knocking
itself over.

### Where the matter is, measured — and it is not where this section guessed

This section twice said the measurement it wanted was "a per-chemical total over time, which
`mm_core::metrics::Sample` does not carry". **That was false.** `Sample::chemicals` is
`[i64; CHEM_COUNT]`, filled from `World::total_matter` and written into every NDJSON line, and it
had been in the metrics files these very runs produced. The instrument was never missing; it went
unread, twice, while the thing it measures was described as unmeasurable.

Read, it refutes the hypothesis this section had been offering. The guess was that a large
overshoot locks matter in the decay chain — corpse to carrion to detritus to carbon — leaving the
survivors short. Carrion and detritus together are **about one percent** of the 400 world's matter
at every sample. Nothing is locked anywhere.

What is actually happening, in millions of `Q10` units on the 256-square slide:

| seeded 400 | carbon | CO₂ | oxygen | sugar |
| ---: | ---: | ---: | ---: | ---: |
| tick 0 | 26,847 | 26,844 | 26,844 | 0 |
| tick 12,000 | 26,734 | **177** | 40,142 | 13,298 |
| tick 60,000 | 26,578 | **60** | 40,219 | 13,375 |

**The CO₂ is gone, and the carbon never moved.** Photosynthesis is `2 waste + light -> substrate
+ oxidant`, and the arithmetic closes exactly: 26,667 of CO₂ consumed against 13,333 of sugar and
13,333 of oxygen produced. The population booms on an enormous CO₂ pool, converts essentially all
of it into a sugar-and-oxygen pool, and then starves — with twenty-six *billion* units of
structural carbon lying untouched around it. Bodies hold about 940 million of that carbon at the
peak: **three and a half percent**.

The 40 world does the same thing and then recovers:

| seeded 40 | carbon | CO₂ | oxygen | sugar |
| ---: | ---: | ---: | ---: | ---: |
| tick 12,000 | 2,330 | 82 | 3,962 | 1,277 |
| tick 60,000 | 2,560 | **114** | 3,926 | 1,241 |

CO₂ bottoms out and then climbs, and the population climbs with it. Per square that is 1.7 units
of CO₂ against the 400 world's 0.9 — **the smaller world ends up with twice as much of the thing
that actually limits it**, which is why it carries 23,524 cells where the richer one carries
9,944.

So the ring is a carbon-cycle oscillation and the seeded amount sets its amplitude. The only route
back from sugar to CO₂ is respiration, which is bounded by mitochondrial throughput, so a world
that overshoots hard converts its CO₂ faster than it can be returned and has to wait. §6's
conclusion stands and gains a mechanism: seeding far above the knee does not make a well-fed
world, it makes a world that runs its own photosynthetic substrate down and rings while it comes
back.

**What this does not settle.** Why the 400 world's CO₂ settles *lower* than the 40 world's is not
explained by anything measured here — both should relax to whatever respiration sustains, and the
richer one does not. That is the next question, and the instrument for it exists.

### The caveat, discharged

The first version of this section ended by saying that forty was one seed, flat across the last
20,000 ticks of 120,000 — *which is exactly what 100 looked like at 60,000* — and that it wanted
200,000 ticks on three seeds before anyone called it settled. That has now been run, on
`petri_of` **as it ships**, silicon included, so this is the product's world rather than the
retired three-chemical one the table above was measured on:

| seed | peak | at tick | final | from peak | drift, last 25,000 | drift, last 50,000 | genomes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 23,347 | 150,000 | 23,342 | 0.0% | +0.01% | −0.02% | 1,573 |
| 2 | 22,956 | 130,000 | 22,888 | 0.3% | −0.14% | −0.24% | 1,483 |
| 3 | 23,728 | 135,000 | 23,711 | 0.1% | −0.02% | −0.03% | 1,555 |

`--check` clean throughout all three. **It is settled**, and on the evidence that distinguishes a
plateau from a phase sample rather than on a flat-looking window: the peak lands at 130,000 to
150,000, which is *at* the plateau rather than thousands of ticks before the end, so there are
about 110,000 ticks of flat behind the final reading. The 100 world had none behind its apparent
one — it peaked at 64,000, four thousand ticks after the measurement stopped. Seed spread is 3.5%,
and each slide holds around 1,500 lineages against the 100 world's 664.

### Why it is settled, which is not what was expected

**Forty never overshoots.** It rises monotonically and comes to its capacity from below. Every
higher level shoots past what the slide can support at around tick 15,000 and then starves back.

So the ringing is not a property of how large the population is, it is a property of
*overshooting* — and the sentence that stood here explained that by a death pulse putting matter
into the decay chain, which the per-chemical totals above refute: carrion and detritus are about
one percent of the world at every sample. The overshoot matters for a different reason. A
population that exceeds its capacity converts CO₂ into sugar faster than respiration returns it,
and then has to wait for the slow leg. Forty never overshoots, so it never opens that deficit.

One honesty note on the table at the top of this section: those arms were measured on the
three-chemical world, before the fresh slide seeded silicon (§8). The ranking is unaffected —
silicon is inert until something evolves a shell — but the two tables are not one experiment, and
the 200,000-tick figures are the ones taken on what actually ships.

## 8. The three minerals, and what a seventeenth chemical would cost

§2's "now" column says nitrogen, phosphorus and silicon are **produced by nothing**. That was
traced exhaustively rather than assumed, and the finding is stronger than "they need seeding":

* **Nitrogen (5)** is consumed in exactly one place — `metabolism.rs`, the diazosome, which
  converts it to structural carbon — and returned by nothing. A one-way sink.
* **Phosphorus (6)** is touched by no mechanism at all, in either direction.
* **Silicon (7)** is consumed by the shell's recipe and returned **in full** on death, 7/8 on
  `TEAR`. It cycles; it is never made.

Every other path that moves them — `EAT`, `EMIT`, junction transfer, division, bleeding, passive
transport, diffusion — is transport between compartments, not production. So seeding or a `Flux`
is the only way any of the three enters a world.

### The consequence that is a bug

`build_trace[7] = q10(6)` on the shell is the only non-zero recipe in the catalogue, and
`biology.rs`'s affordability gate refuses a build whose ingredients are absent.
`scenarios/the_scattering.ron` is the **only** file in the repo that seeds silicon. So on every
other shipped slide — `soup.ron`, the fresh petri, all of them — **a cell that evolves a shell can
never build one.** The organelle works; there is nothing to make it out of. ISA 7 shipped armour
into worlds with no mineral in them.

### Correcting the record: which test was missing

Commit `0875570`, which fixed the seeding, blamed `tests/shell.rs` — saying its tests install
shells through `slots_mut`, that the recipe gate "has never been exercised end to end", and that
the organelle's acceptance tests all pass on a slide where it cannot be built. **That was wrong,
and it is recorded here rather than left in the history unqualified.**

`shell.rs` has `a_shell_cannot_be_built_without_silicon`, which assembles a real genome —
`IMM 100 / IMM 15 / IMM 5 / BUILD / HALT` — runs it on a cell starved of silicon and on one that
is not, asserts the shell appears in exactly one case, and further asserts that the refused build
spent *nothing*, because a half-charged build is matter destroyed. The gate was tested. It was
tested well.

The missing test was a different one, and the distinction matters for what to write next time.
Nothing checked that **a world supplies what the catalogue asks for**. Adding a recipe and seeding
the slide it needs are two edits in two crates, and no test spanned them — which is why
`the_default_world_seeds_every_ingredient_the_catalogue_needs` is quantified over the catalogue in
`mm-app` rather than being another assertion about silicon in `mm-core`. A mechanism test cannot
catch an empty world, and a world test cannot catch a broken gate; this needed both and had one.

Looking for the gap that was not there did find one next door. `tests/upper_half.rs` *does* only
install its organelles with `slots_mut`, and behind that sat ISA 9: `CellHost::build` reduced the
type operand modulo `SLOT_COUNT` rather than `CATALOGUE_SIZE`, so no genome could build anything
in the upper half at all. `BUILD 22` made a cilium. The lesson generalises past both bugs — for a
catalogue whose whole justification is the path a *mutation* takes, a test that installs an
organelle is testing the wrong half.

### Why the atmosphere has to be on the slide

The obvious way to make fixation real is to let a diazosome *import* nitrogen from off-plane, the
way a chloroplast imports energy from off-plane, and record it through `Ledger::record_injected`
so the books stay exact. That was proposed here and it is wrong, for a reason worth writing down:

> Only energy enters and leaves. Matter is neither created nor destroyed, only transferred and
> transformed.

Energy is the one wall the fishbowl is permitted — light in, heat out — so the sun is the
exception rather than the precedent. `record_injected` is scenario-setup machinery; an organelle
calling it every tick is a tap, and a closed system with a tap is a flow reactor. So if the inert
dinitrogen pool is to exist, it must exist **on the slide**, as a chemical, with fixation as a
conversion between two on-table species. That costs a seventeenth chemical, and it is the honest
price of the invariant.

What the slot buys is worth more than the slot: under an injecting port, nitrogen availability is
a parameter somebody chooses. With two on-slide pools, total nitrogen is fixed at seeding and the
*split* between locked and available is a state variable that evolves — a young slide is nearly
all inert and gated on diazotrophs, a mature one has pulled its nitrogen into circulation and the
diazosome goes vestigial on its own. Scarcity becomes a historical property of that world instead
of a number.

### What the seventeenth chemical costs

§1 measured `CHEM_COUNT` at 32 and concluded: *declaring a chemical is nearly free, stirring one
is exactly linear.* Dinitrogen is the case that conclusion does not cover, because it is present
in every square of every slide and never goes away — it is guaranteed to be stirred. Measured
against a realistic 7-plane baseline, flowing, as the marginal cost of one always-present plane:

| its mobility | 256² | 512² |
| --- | ---: | ---: |
| diffusion 0, advection 0 | −0.1% | +0.0% |
| diffusion `Q10/256`, advection 0 | +6.3% | +6.5% |
| diffusion `Q10/16`, advection 0 | +8.0% | +8.2% |
| diffusion `Q10/4`, advection full | +14.7% | +15.0% |

Memory is negligible — 0.26 MB a plane at 256², 4.19 MB at 1024².

The shape is what matters. `fluid.rs` gates diffusion on `rate > 0` and advection on
`mobility > 0` and then does identical work whatever the value, so **the step from off to on is
the expensive one** — roughly 6.4% — and the rate itself adds only about 1.7 points on top. There
is no cheap-because-slow option: it is off, or it is most of the price.

### The decision

**Diffusion on, advection off**, at about +8% of the fluid step — and the locally exhaustible pool
that follows is the point rather than a concession. A dissolved gas ought to go where the water
goes, so advection off is a deliberate departure: it makes the inert pool something a diazotroph
mat draws down faster than the neighbourhood refills, which is a fourth spatial scarcity rather
than a flat background. Diffusion stays on so an exhausted patch recovers instead of scarring
permanently.

That cost lands on the phase already furthest from its gate — §1 records the fluid at 186
steps/second against M1's 500 — so it is a real charge against an overspent budget, not a
rounding error. It is affordable because the gate's workload is 512² with all sixteen planes
carrying matter, and the product runs 256² with about seven.

### Built

All of it, at ISA 10 and 11, and the design above survived contact with one correction and two
surprises.

**ISA 10 — the recipes.** `build_trace` filled on the Redfield ratio: nitrogen at 16/106 of a
type's carbon on the enzymatic machinery and the sensors, phosphorus at 1/106 on the nucleus,
silicon already on the shell. The mobilities with them — nitrogen diffusing above the monomer
default and carried by the flow, phosphorus at **zero on both axes** so the only thing that moves
it is a cell that ate it, silicon settling like detritus.

**ISA 11 — the atmosphere and the reversal.** `chem::DINITROGEN` at index 16, and the diazosome
turned round: it converts the inert pool into the bioavailable one at an energy price, where
before it *spent* nitrogen to make carbon. `World::denitrify` is the leak back, gated on anoxic
water, and it ships at zero because the leak is the term to tune last.

**The correction.** An oxidant-gated reversion is not `decay_to` plus `decay_rate`, as this
section originally suggested. Those are one unconditional rate per chemical; denitrification is a
function of *two* chemicals at a square, so it is a mechanism and not a table edit.

**The first surprise: a recipe is charged against the cell's interior.** So an organelle costed in
nitrogen can only be built by a lineage that *eats* nitrogen — and no genome written before ISA 10
did, because there was nothing to eat it for. On `soup.ron` the ancestor went to 144 cells with no
nucleus out of 160 and the population stalled at a tenth of what it should be. Every shipped
genome gained two `EAT`s and every world in `scenarios/` gained the three minerals. That is not a
wrinkle; it is the honest price of making a requirement irreducible, and it is what the version
stamp is for.

**The second: seeding silicon did not make shells buildable.** §8's own fix, from earlier the same
day, put silicon on the fresh slide and the mm-app guard held it there — but no genome ate silicon
either, so armour stayed unreachable in practice. A world test cannot catch a gate and a gate
cannot catch an empty world; it turns out neither can catch *a cell that never goes shopping*.

`tests/nitrogen.rs` is the acceptance set: total nitrogen across both pools and every body is
constant to the unit over 200 ticks, fixation unlocks rather than transmutes and carbon does not
move while it does, a death returns what a body was built from, and reversion happens in anoxic
water and not in oxygenated water.

### Still open

The sweeps. Every level here is a *requirement* written from stoichiometry, and none of them has
been swept for where it *binds* — which is the distinction §6 exists to make, and the one that
cost the soup two orders of magnitude. In particular the sensors' nitrogen entry wants its own
line: at 618 against the mitochondrion's 773 it is not a rounding difference, and taxing
perception is a distinct pressure from taxing metabolism.

## 9. Mineral-bearing walls: where phosphorus and silicon come from

§8 settles that a reservoir which is not on the slide cannot exist, because only energy crosses
the wall of the fishbowl. For nitrogen that costs a seventeenth chemical. For phosphorus and
silicon it costs no chemical at all, because their reservoir is not a fluid — **it is rock**, and
the slide already has rock in it.

### Why this is not the same mechanism as the nitrogen pool

It is the actual difference between the two cycles rather than a convenience. **Phosphorus has no
gas phase.** Its cycle is purely sedimentary: the only primary source is the weathering of rock,
which is why it is so often the ultimate limiting nutrient on long timescales — nothing can fix
it out of the air, and a world can only wait for a mineral to dissolve. Nitrogen's reservoir is
atmospheric and therefore *everywhere*; phosphorus's is lithic and therefore *somewhere*. Silicon
is the same story as phosphorus.

So the three want three reservoirs with three geometries, and that is what makes them three
niches instead of three copies of one scarcity:

| | reservoir | geometry | mobility |
| --- | --- | --- | --- |
| nitrogen | inert dissolved pool, the seventeenth chemical | uniform, everywhere | diffuses, does not advect (§8) |
| phosphorus | **rock** | *places* | barely moves — carried by cells |
| silicon | **rock** | places | low advection; settles where shelled cells die |

Set beside phosphorus barely moving, a phosphate outcrop stops being a level and becomes a
**location**. Life clusters on rock, and colonising away from it means carrying phosphorus with
you — in vacuoles, at the cost of slots, which is what finally gives `interior_capacity` a job
beyond osmosis. "Keeps life local" becomes literal geography rather than a diffusion constant.

### The mechanism

A barrier square may hold a stock of one or more chemicals and release it into the water it
touches, at a rate set by how far the local water is from saturation.

**Concentration-dependent, which does three things at once.** It is a solubility product, so it
is the physically right law; it is self-limiting, so a wall in saturated water does nothing and
the system has an equilibrium rather than a ratchet; and it gives **biological weathering** for
free — a mat stripping phosphate out of the water next to an outcrop lowers the local
concentration and so makes the rock dissolve faster. A population accelerates its own supply,
which is true of the real thing, and the feedback runs until the stock is spent.

**Per exposed edge, not per square.** Dissolution is a surface process, and the substrate already
keeps `open_x`/`open_y` edge masks, so the exposed surface of a barrier is already computed. The
consequence is that **barrier shape becomes supply rate**: a thin reef leaches far faster per unit
of stock than a solid massif, and a massif's interior is locked until its outside has eroded. The
scenario editor's rectangle tool quietly becomes a decision about fertility, which is a great deal
of emergent geography for very little mechanism.

**A world ages.** The stock is finite, so a spent outcrop becomes ordinary rock and the slide's
fertility has a history. That is the same property §8 wants from the nitrogen split — scarcity as
something a world arrives at rather than something a number declares.

### What it costs, and why it is better than the tool that exists

No chemical slot: barriers are already `Vec<bool>`, and this is stock attached to the small
fraction of squares that are barriers, so it wants a sparse map keyed by square index — a
`BTreeMap`, since hard rule 6 forbids outcomes depending on `HashMap` order. It must serialise
(hard rule 7) and it becomes a fifth compartment for `World::total_matter`, alongside fluid,
interiors, `mass` and the trace held in organelle slots. There is precedent for exactly that: the
organelle-trace compartment was added at ISA 7 for the same reason.

`Flux::Source` is the only thing today that puts matter into a world over time, and it is an
**unbounded tap** — `per_tick`, forever, recorded as injected. That is defensible for a scenario
declaring an inlet, but it means `the_tide` and `the_drift` are open systems. A leaching wall is
finite, and so is closed by construction: nothing is created, a reservoir is drawn down. For a
mineral that is not merely the safer option, it is the more accurate one.

### Not built

Design only. §8's minerals are built and this is the source that would keep them coming; without
it a world has exactly the phosphorus and silicon it was seeded with, forever, and an outcrop is
the mechanism that would make that a geography rather than a budget.

## 10. Making the walls real: one compartment, one threshold

§9 argues that phosphorus and silicon come out of rock. This is how, and the shape of it is set by
a single line in `ledger.rs`:

> Matter deliberately removed from the world, per chemical. **Only barriers do this.**

Raising a barrier over occupied water evicts what was there, and `Ledger::record_evicted` exists so
that the loss is *said out loud* rather than silently breaking I4. It is the one genuine exit from
a world that otherwise conserves matter exactly. **The whole of this design is turning that exit
into a compartment**: matter does not leave when a wall goes up, it is held *in* the wall, and it
can come back out. Closed-matter gets stronger, not weaker, and the leak the ledger has been
apologising for since M1 stops existing.

### The state: planes, not a map, because the patches are meant to be big

The first sketch had a sparse `BTreeMap<u32, [i32; CHEM_COUNT]>`, on the reasoning that walls are
rare. They are not meant to be: the point of this is **large solid patches that give the world
structure**, and a map that is cheap while walls are rare is the wrong bet if walls are the
feature.

Dense over all seventeen chemicals is the other wrong answer — 68 bytes a square, which *doubles*
the substrate, to store zero in fourteen of them everywhere. But only a few chemicals can ever be
solid. Sugar does not form reefs.

So: **one plane per solid-capable chemical**, exactly the layout `Substrate::chem` already uses and
for the same reason its note gives — the sweep goes one chemical at a time across the whole grid,
so a plane keeps the working set contiguous where an interleaved struct would stream all of it to
do one chemical's work. Two planes to begin with, phosphorus and silicon:

| | 256² | 512² | 1024² |
| --- | ---: | ---: | ---: |
| one solid plane | 0.26 MB | 1.05 MB | 4.19 MB |
| two | 0.52 MB | 2.10 MB | 8.39 MB |

Against a substrate that is already seventeen planes, adding two is a 12% increase in fluid memory
and nothing at all in the tick, because these planes are not stirred — the whole point of a solid
is that the fluid solver never touches it.

Extending the set later costs one plane and one line. The list is the design decision, not the
container: **carbon is the interesting candidate**, because a carbonate reef locking carbon out of
circulation is a real world-structuring mechanism and §7 has already shown that carbon is not what
limits these worlds. It is left out of the first cut deliberately, because §7 also shows the carbon
cycle is the one that oscillates, and giving it a new sink is not a change to make blind.

It joins fluid, cell interiors, `mass` and organelle-held trace as a compartment of
`World::total_matter`, and it serialises (hard rule 7) and hashes. Iteration order is an array
index, so hard rule 6 is satisfied by construction rather than by choosing the right map.

### One quantity, and the wall is derived from it

The temptation is two concepts — a wall, and stuff inside it. One is enough:

* a square holds some solid mineral;
* **above a threshold it is a wall**: blocked, opaque to the fluid, impassable to cells;
* below it, the square is open and the solid is a *crust* lying in the water.

Everything else falls out. Dissolution below the threshold unblocks a square with no special case.
Precipitation above it raises a wall with no special case. `Substrate::set_blocked` already evicts
contents and rebuilds the edge masks, so the mechanics of becoming and un-becoming a wall are
built — what changes is where the evicted matter goes.

**Rock stays rock.** A blocked square with no entry in the map is the immutable barrier there has
always been: insoluble, permanent, drawn as it is now. Nothing in `fluid.rs`, `neighbours.rs` or
the cell paths needs to know the difference, because they all read `blocked` and that is unchanged.

### Dissolution: per exposed edge, towards saturation

For each mineral square, for each of its four edges facing an *open* square, mineral crosses at a
rate proportional to how far that water is below saturation. Three things follow, and they are the
reasons for this shape rather than a flat rate:

* it is a solubility product, so it is the right law;
* it is **self-limiting** — a wall in saturated water does nothing, and the system has an
  equilibrium rather than a ratchet;
* it gives **biological weathering** for free: cells stripping phosphate from the water beside an
  outcrop lower the local concentration and so dissolve the rock faster. A population accelerates
  its own supply until the stock is spent.

Per *edge* rather than per square because dissolution is a surface process, and the substrate
already keeps `open_x`/`open_y`. The consequence is that **barrier shape becomes supply rate**: a
thin reef gives up its mineral far faster per unit of stock than a massif, whose interior is locked
until the outside has gone. The scenario editor's rectangle tool quietly becomes a decision about
fertility.

### Precipitation: on surfaces, on a cadence, into empty squares — and a way in from nothing

The reverse of the same law — water above saturation gives its excess up as solid — with four
restrictions, and each of them is doing real work.

**On surfaces.** Scanning every open square for over-saturation is a full-grid pass per mineral,
landing on the phase §1 already has furthest from its gate. Growth happens on squares that already
touch solid, which are the squares the dissolution pass is visiting anyway: the same visit with the
sign flipped. It is also how crystals actually grow.

**And a nucleation scan, because otherwise the water stays supersaturated.** Surface growth alone
has a hole in it: a slide with no rock on it and rising phosphate has nowhere to precipitate, so it
climbs without limit. That is a *supersaturated solution*, which is a real thing and not one this
engine should be modelling — it is a metastable state that wants a nucleus, and the honest
behaviour is for one to appear.

The cheap version of "find a supersaturated square" is **an amortised slice**: each mineral step
walks one contiguous slice of one solid-capable plane, cycling through slices and planes by tick.
A sixty-fourth of a 256² plane is a thousand squares of linear read, and the whole grid is covered
every sixty-fourth mineral step — which on a cadence is thousands of ticks, and thousands of ticks
is the right timescale for a rock to appear out of nothing. Deterministic by construction: the
slice is a function of the tick, not of an RNG and not of iteration order.

Two cheap guards keep even that off the books most of the time. A plane whose `present` flag is
clear is skipped outright. And a plane whose *whole-world total* is below the nucleation threshold
cannot have a single square above it, however the matter is arranged, so one comparison against a
running total skips the slice entirely — which is the common case on every slide that is not
actively concentrating a mineral.

**Two thresholds, and the gap between them is the point.** Nucleation is harder than growth in the
real thing — a dissolved salt will happily deposit onto an existing crystal at a concentration that
would never start one — so the scan looks for a *high* threshold while surface growth uses a lower
one. The consequence is exactly what is wanted: the expensive path fires rarely and only where the
water has genuinely run away, and once it has fired, the cheap path takes over and does all the
work of growing the patch out.

**On a cadence.** Nothing here needs to happen every tick. Rock is the slowest thing in the world
and a `mineral_interval` alongside `fluid_interval` makes that explicit rather than implicit in a
small rate constant — a rate of one-sixteenth applied every step and a rate of one applied every
sixteenth are not the same thing when the concentration between them is being driven by cells, and
the second is the one that reads as geology.

**Into empty squares only.** A square holding a cell does not solidify. Refusing beats evicting or
killing it: it avoids a class of bugs, and it reads correctly — living tissue keeps its own ground
open, which is why a mat resists cementation and why the edge of a reef is where the cells are.

### The water displaced when a square closes

**This is the part that has to be exact.** Only the mineral becomes solid; the sugar and oxygen
and everything else dissolved in that square has to go somewhere, and "nowhere" is the leak this
design exists to close. It is pushed into the neighbouring open squares, bounded by what they will
take — and if it cannot all be placed, **the square does not close this tick**. Conservation is
then exact by construction rather than by accounting.

### Colour from composition

`art::BARRIER_RGB` is one constant today. A mineral square's colour is instead the blend of its
held chemicals' own `colour` entries, weighted by amount — so a silica reef comes out pale
blue-grey and a phosphate outcrop yellow-brown, and the picture says what the rock is made of
without a legend. Rock with no stock keeps the constant.

`paint_barriers` already writes RGBA per square, so this is a parameter rather than a new path.

### Ragged, without blurring the wall

`paint_barriers`'s own note is that a barrier is **nearest**-sampled on purpose: a wall is blocked
or not, and interpolating it invents half a wall, which is a value the simulation never held. That
argument does not weaken for mineral walls, so the raggedness must not come from softening the
texture.

It comes from *grains* instead: a scatter of small mineral sprites along the wall's exposed edges,
drawn with the machinery the carrion flakes already use — a pure function of `(square, index)`,
tinted from the composition, sized and turned per grain. The wall stays a hard-edged truth about
which squares are solid, and what breaks the outline is loose material lying against it, which is
what the edge of a real mineral bed looks like.

### Not built

Design only. The pieces it rests on all exist: `set_blocked` and its eviction, the edge masks, the
per-chemical `colour` in the table, the sprite pool, and a `total_matter` that already counts four
compartments and needs a fifth.

### Built, and two things found afterwards

The above shipped. Two faults in it were found by looking at a slide, and one is fixed.

**Fixed: a wall that grew was a wall only half the engine believed in.** `weather` closes a square
through `World::place_barrier`, which is the *deferred* setter — the same one the drawing tool
uses, which leaves `Substrate::rebuild_edge_masks` to the caller because the rebuild walks the
whole slide and one weathering step can close hundreds of squares at once. `World::set_barriers`
makes that call and `weather` did not. So `blocked` was set, the light regime shadowed the square
and `add_chem` refused it — while the `open_x`/`open_y` masks still said the edge was open, so the
fluid fluxed straight through the rock, and `has_barriers` still said the slide had no walls at
all, so the renderer was never asked to draw one. What you saw was a black hole in the picture
with the chemistry piling up behind it: the signature of a drawing bug, out of a solver contract.
Held by `a_wall_that_grew_closes_the_masks_and_shows_on_the_slide` in `tests/mineral_walls.rs`.

**Open: a nucleated grain is over the wall line and never becomes a wall.** The law above is that
a square is rock when it holds more than `wall_threshold` of solid, and nothing sets a flag. The
code only asks the question on the *deposition* path — inside the `else` branch that fires when a
square holds no solid of that plane — so it is asked exactly once per square, at the moment the
first grain lands on it, and never again.

Two consequences, and they compound. A square that already holds solid takes the dissolve branch
forever after, so surface growth can add **one step's worth** and no more; a reef spreads sideways
and cannot thicken. And nucleation takes the *whole* excess over saturation rather than a fraction
of it, which is routinely several times the threshold, and pushes nothing onto `became_rock` —
deliberately, per its own note, "a grain, not a wall".

Measured on a 32×32 slide seeded uniformly at 400 units of a mineral whose saturation is 8, run to
40,000 ticks: **1,024 squares of 1,024 hold solid, every one of them at twice `wall_threshold`,
and not one is blocked.** The world is entirely paved in rock that is not rock. So "walls that grow
from minerals falling out of solution" produces solid and no walls at any concentration a slide
actually reaches.

The fix is a design decision rather than a repair, which is why it is written down here instead of
made. Asking the question of every square that holds solid honours the stated law, and turns that
measurement into a slide that is 100% wall on the first weathering step — which is not a world.
Making nucleation deposit a grain-sized amount keeps "a grain, not a wall" true by arithmetic and
leaves surface growth to do the rest, but surface growth is capped at one deposit per square, so
that needs the cap lifted too. Both dials are in `MineralRates`.

## 11. The carbonate system: store the buffer, read the pH

Design only. Nothing below is built.

§8 gave the world a nitrogen cycle and §10 gave it rock. This is the third: a **carbonate
buffer**, a pH that follows from it, calcium as the fourth mineral, and two catalogue entries that
make the swing matter to something alive.

The framing that decided its shape is the aquarium, not the ocean model. A fish tank is simulated
by a handful of coupled cycles — nitrogen, carbon, KH, calcium — and it behaves like a tank
because of them; it does not need speciation constants to do it. This is the same bargain SPEC
makes everywhere else, and §11 is written to take the cheapest mechanism that produces the
behaviour, not the most faithful one.

### Why pH is not a plane

**pH is not matter.** It does not conserve, so a `chem` plane holding it would be a field the M1
gate cannot check and a hole in the one invariant this project will not trade — §8 already refused
a nitrogen-importing port for the same reason and paid a whole chemical instead.

The aquarium says how to have it anyway, because a tank keeps the two apart already: you **measure
KH**, which is matter in the water, and you **read pH**, which is a number derived from it. So:

> **Store the buffer. Derive the pH.**

The buffer is a chemical, conserved like everything else. The pH is a function of two planes at a
square, computed where it is needed and stored nowhere — which means hard rule 7 is untouched,
there is nothing to serialise, and there is no way for a saved world to come back with a pH that
disagrees with the chemistry that produced it.

### The driver already exists

Nothing has to be built to make biology move this, which is the strongest argument for the whole
section. The metabolic chemistry is already:

```
photosynthesis:  2 CO2 + light -> sugar + O2
respiration:     sugar + O2   -> energy + 2 CO2
```

CO₂ is produced by every respiring cell and consumed by every photosynthesising one, and §7
measured how hard it swings: on a 64² slide seeded at 400 units, **26,667 of CO₂ consumed against
13,333 of sugar and 13,333 of oxygen produced** — the pool very nearly emptied by the population
and then refilled as respiration caught up. That is already a biology-driven acid-base swing with
nothing reading it.

### Two chemicals

| | index | saturation | diffusion | advection | structural | why |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| calcium | 17 | `Q10 * 24` | `Q10/24` | `Q10/2` | yes | mobile enough to arrive, slow enough that a reef is a place |
| carbonate | 18 | `Q10 * 24` | `Q10/12` | `Q10` | no | the buffer has to be well mixed or it is not a buffer |

Both join `chem::SOLID_CHEMICALS`, taking `SOLID_COUNT` to four. Calcite is not a fifth species:
it is a square holding **both** planes as solid, and `Substrate::solid_total_at` already sums
across planes for exactly this reason — §10 records that judging a wall one plane at a time opened
walls that were plainly solid, and "a reef made of two minerals is a reef".

Carbonate carries no `energy_yield`: it is not something to be burnt
or burnt, it is the water's capacity to absorb an insult.

The saturations are the least-supported numbers here and are flagged as such below.

### pH, derived

Integer, bounded, and monotone in the ratio that matters:

```
r  = ((carbonate - co2) * Q10) / (carbonate + co2)      // i64 intermediate; [-Q10, +Q10]
pH = 7*Q10 + 7*r                                        // [0, 14*Q10]
```

Both planes empty at a square gives `pH = 7*Q10` by definition rather than by division — neutral
water is the honest reading for water with no carbon chemistry in it.

**It is linear in a bounded ratio where the real thing is logarithmic, and that is deliberate.**
Every mechanism downstream needs monotone, bounded and cheap; none of them needs the decade. A
`log2` over fixed point would be three times the arithmetic to move a threshold that is going to
be swept empirically anyway. If a sweep later shows the linear version compresses the interesting
range, the function is one place and can be changed without touching anything that reads it.

Computed on demand from the two planes, never stored. `sense_chemical` already reads a square and
its neighbours to produce a value and two gradients, so a pH gradient costs the same walk twice
and no state.

### What the buffer does, for free

The whole point of a ratio: **a square thick with carbonate barely moves when respiration dumps
CO₂ into it; a square scoured of carbonate swings hard.** That is KH, and it is emergent from the
arithmetic rather than a mechanism anybody wrote. A slide with a deep carbonate pool is a stable
world; one that has had its carbonate locked into reef is a world where a crowd can crash its own
pH — and get it back only by dissolving the reef.

Over a day/night cycle that gives a **diel swing** with no new machinery: the mat photosynthesises,
CO₂ falls, pH rises, calcite precipitates; at night respiration takes over, pH falls, calcite
dissolves and returns its carbonate to the water. A carbonate cycle beside the carbon and nitrogen
ones, driven entirely by what the cells are already doing.

### Calcite: precipitation and dissolution are a mechanism, not a table edit

§10's weathering law is one `saturation` per chemical, compared per plane. Calcite cannot use it:
it is a function of **three** quantities at a square — calcium, carbonate and pH — and §8 has the
precedent written down already, from denitrification: *"an oxidant-gated reversion is not
`decay_to` plus `decay_rate` ... it is a function of two chemicals at a square, so it is a
mechanism and not a table edit."*

So `World::weather` gains a calcite arm beside the existing per-plane one:

* **precipitates** where `calcium * carbonate` exceeds a solubility product **and** pH is above a
  threshold, taking both out of the water in equal measure and into the two solid planes;
* **dissolves** where pH is below it, returning both — at a rate driven by how far below, which is
  the same deficit shape the existing law uses and keeps the two recognisably one law;
* **closes the square** by the same derived rule everything else uses: `solid_total_at` over the
  threshold is rock, under it is a crust.

The reef is then the buffer's reservoir, which is the behaviour a real tank has and the reason
this is worth building: acid dissolves the substrate, the substrate raises the buffer, the buffer
resists further acid. Negative feedback that nobody wrote as a feedback.

**This inherits §10's open fault.** The promotion-to-rock test currently only runs on the
deposition path, so a square that gains solid any other way sits above the wall line and never
closes. Calcite must not be built on top of that; §10's "Built, and two things found afterwards"
has to be settled first, or the calcite arm gets the same bug on day one.

### The calcite shell, at slot 31

`Shell = 15` and `Reserved31` is empty — and that is not a free slot, it is *the* slot. The
catalogue's upper half is laid out so bit 4 of a type operand means "the same job done a different
way", so a single copy error moves a lineage between siblings; the layout comment says a
`Reserved` entry up there means "this organ has no variant yet, which is a meaningful reservation
rather than filler". A calcite test at 31 is one mutation from a silica test at 15.

**It has to be a sibling and not an option on the existing entry.** `OrganelleSpec::build_trace`
is a fixed `[i32; CHEM_COUNT]` and every non-zero entry is *required*: the affordability gate
refuses if any is short, and all are charged and refunded together. Putting calcium beside silicon
in the shell's trace makes a shell needing **both**, not either. The two alternatives are worse: a
recipe selected by a control word puts a branch in the gate, the charge and all four refund sites
— that is a branch in the conservation path; and a substitutable trace needs the organelle to
*remember which mineral it was made of* so the refund can return it, which is new per-slot state
and hard rule 7 on top.

Priced against its sibling rather than against the catalogue, since that is the comparison a
mutation is actually making:

| | silica shell (15) | calcite shell (31) | why |
| --- | ---: | ---: | --- |
| `build_matter` | `q10(13)` | `q10(11)` | limestone is cheaper to lay down than glass |
| `build_energy` | `q10(20)` | `q10(14)` | and much cheaper to precipitate |
| `build_ticks` | 28 | 18 | armour you can raise in a hurry |
| `upkeep` | `q10(8)/96` | `q10(8)/96` | same: a wall is cheap to keep |
| `teardown_recovery` | `Q10/8` | `Q10/8` | mineral put down does not come back up |
| `build_trace` | silicon `q10(6)` | calcium `q10(4)` + carbonate `q10(4)` | |

The trade is the point. Silica is dear, slow and **pH-indifferent**; calcite is cheap, quick and
**dissolves in acid** — so a calcite-armoured cell in a crowded respiring mat is paying for its
neighbours' CO₂, and the same cell in bright open water is armoured for nearly nothing. Neither
dominates, which is the test of whether a sibling was worth a slot.

Dissolution acts on the organelle, not just on the rock: below the threshold a calcite shell loses
coverage over time and its silicon returns — mechanically the same as `TEAR`'s partial recovery,
which already exists, gated on pH rather than on a genome's instruction.

### The pH sensor, at slot 23

`Chemosensor = 7` pairs with the empty `Reserved23`. Same argument, same slot logic: one bit-flip
retunes a lineage from tasting a chemical to tasting acidity.

It reads like its sibling — `index % 3` gives the value and the two gradients — and reports pH in
`Q10` rather than divided down to whole units, following the photosensor's glow readings, whose
note gives the reason: a reading rounded to integers loses everything a gradient is made of.

Priced as the `cheap` default with `nitrogen_trace(618)`, exactly as the other three sensors are.
**And it wants the same caveat §8 attached to that number**: the sensors are the cost of perceiving
at all, taxing them is a distinct pressure from taxing metabolism, and a fourth sensor is a fourth
call on the same sixteen slots. If behaviour changes when this lands, this entry has to be ruled in
or out separately.

Without it the swing would select on lineages that cannot act on it, which is a pressure with no
strategy behind it. With it, "swim away from the acid" and "build armour only where the water is
sweet" are both reachable by mutation.

### What the cell's interior needs, which is nothing

**No new per-cell state, and the reason is that there is nothing to add.** `CellArena::interior` is
a flat `Vec<i32>` sliced `CHEM_COUNT` per cell, and the snapshot writes it as a slice and checks
its length on the way back in. Calcium and carbonate get interior slots the moment `CHEM_COUNT`
goes to nineteen — no new field, no new serialisation, and hard rule 7 satisfied by the mechanism
that is already there. Old snapshots fail the length check, which is exactly what the stamp is for.

Interior pH is the same derived function applied to the interior array instead of the substrate
planes. Stored nowhere, there too.

The membrane's self-sensor already reads internal chemistry through `MembraneReading::Chemical`, so
a cell can read its own calcium and carbonate for free. An interior *pH* reading would be a new
`MembraneReading`, appended after `Crowding` — the pattern `Badge` and `Crowding` both followed,
and for the reason their notes give: appending renumbers nothing.

### Which pH acts on a shell, and the fork nobody has taken

This section assumed above that the *water's* pH wears a calcite test. That is not obviously right,
because of where the carbon actually goes:

> Respiration writes its waste to `interior[p.waste]`, and photosynthesis consumes it from
> `interior[p.waste]`. The exchange happens **inside the cell**; CO₂ reaches the water only through
> `EMIT` or through leakage.

So a cell's internal CO₂ already swings with its own metabolism, with no transport at all. A
respiring cell acidifies itself and a photosynthesising one sweetens itself, today, unread. Which
means interior pH is a real per-cell number the moment carbonate exists, and a shell built from the
interior and sitting on the membrane has at least as good a claim to be judged against it.

The two give quite different worlds. Against the **water's** pH, calcite armour is a bet on the
neighbourhood: cheap in bright open water, expensive in a crowded respiring mat, and the cost is
something a cell's neighbours impose on it. Against the **cell's own** pH, calcite armour is a bet
on its own metabolism: a hard-breathing predator dissolves its own shell, and armour and aerobic
throughput become rival in a way nothing else in the catalogue makes them.

**And there is a known hazard on the interior side.** `metabolism.rs` records that with free
interior decay, *retaining* waste was an advantage — it decayed into carbon dioxide right where
photosynthesis needed it, and "the strain that dutifully excreted its waste lost every time for
having given away its own food supply". Interior CO₂ is therefore already something lineages are
selected to hoard, and hanging a mechanism on interior pH steps directly into that dynamic. It may
be the more interesting world; it is certainly the one with more ways to go wrong, and it should be
measured rather than assumed.

Not settled here. Both are one line in the same place, so this is a thing to try both ways.

### The membrane index space moves, and that is what the stamp is for

Worth writing down because it is the concrete cost of ISA 12 and is easy to miss:
`MembraneReading::decode` places `Badge` at `5 + CHEM_COUNT` and `Crowding` at `6 + CHEM_COUNT`.
They are at **22 and 23** today — not the 21 and 22 the enum discriminants suggest, which are
labels from when the table held sixteen. Adding two chemicals moves them to **24 and 25**, so every
genome that reads its own badge or its own crowding at a hard-coded index reads something else
afterwards.

That is not a bug; it is the reason hard rule 8 exists, and archived genomes replay under the
version they evolved in. But it means the bump is load-bearing rather than ceremonial, and the
doc comment on `MembraneReading::Chemical` — "`5..=20`" — wants correcting to `5..=(4 + CHEM_COUNT)`
in the same commit, since it has been stale by one since dinitrogen landed.

Noticed while checking that: `decode` reduces its operand `% (7 + CHEM_COUNT)` and `chemical_of`
reduces `% (5 + CHEM_COUNT)`, so past the first period the two disagree about which chemical an
index names. Both are total and deterministic, so no rule is broken and nothing is unsafe — but the
index space is not the clean repeat the doc describes, and a genome walking it does not find what
the comment says it finds. Its own fix, on its own stamp.

### What this costs

Measured on the development machine, performance-core pool, ancestor grown to ~11–12k cells, still
water, ten planes carrying matter:

| grid | whole tick | fluid step | fluid share | one more plane |
| --- | ---: | ---: | ---: | ---: |
| 256² | 5.73 ms | 0.39 ms | 7% | +0.04 ms (0.7% of a tick) |
| 500² | 9.58 ms | 0.90 ms | 9% | +0.09 ms (0.9% of a tick) |

Two always-present planes is therefore **about 1.5–1.9% of a tick**, and pH itself is free because
it is derived from planes already being walked.

**§8's cost table has been misread and this is why.** Its +8%/+15% figures are shares of the *fluid
step*, measured at 512² with every plane carrying matter and the water moving — the M1 gate's
workload, chosen deliberately as the worst case, where the fluid essentially *is* the tick. On a
populated slide the cells dominate and the fluid is under a tenth of it. Both numbers are right and
they are not the same number.

Flow matters more than plane count: advection roughly doubles to triples the step (500², eleven
planes: 1.03 ms still against 2.46 ms flowing), and a plane with advection on costs about 2.5× one
with only diffusion. `fluid_interval` divides all of it.

### What is deliberately not modelled

Written down because the fishbowl argument is only worth anything if the line is somewhere:

* **No DIC speciation.** One carbonate pool, not CO₂/H₂CO₃/HCO₃⁻/CO₃²⁻ in equilibrium.
* **No alkalinity-versus-DIC distinction.** The buffer is one number.
* **No temperature, no salinity, no ionic strength.** Nothing here has a temperature.
* **No log scale**, per above.
* **No aragonite/calcite polymorphs**, no magnesium.
* **pH does not directly harm a cell.** It acts through calcite and through what a genome chooses
  to do about it. A direct metabolic penalty is listed as open below rather than assumed — a
  mechanism that kills is a big lever and wants its own measurement.

### The decisions that are open

1. **Every number above.** The saturations, the solubility product, the pH thresholds and the
   precipitation rate are written from stoichiometry and shape, not from a sweep. §6's lesson is
   that a requirement is not a binding constraint and the one you did not measure alone is the one
   that was two orders out.
2. **CO₂ is doing double duty.** It is already the carbon source and already the thing §7 says
   runs out; making it the acid couples two pressures through one pool. That is correct chemistry
   and it is also a real design coupling, and it means a change to photosynthesis is now also a
   change to pH.
3. **Whether pH should have a direct metabolic cost.** Left out above. If it goes in, it wants to
   be its own experiment.
4. **Whether the reef's carbonate should be reachable by a cell.** A lysosome or exoenzyme that
   can dissolve calcite directly would make bioerosion a strategy. Not proposed here; noted so it
   is not stumbled into.

### Acceptance tests

* `tests/carbonate.rs::total_carbon_is_constant_across_water_reef_and_body` — over 200 ticks, the
  M1 discipline applied to the new compartments.
* `..::a_crowd_acidifies_its_own_water_and_a_mat_sweetens_it` — pH falls under respiration and
  rises under photosynthesis, on the same slide, from the existing pathways alone.
* `..::carbonate_buffers_the_swing` — the same CO₂ insult moves pH less in high-carbonate water
  than in low, monotonically. **This is the test the section exists for.**
* `..::calcite_precipitates_above_the_line_and_dissolves_below_it`, and conserves both ways.
* `..::a_reef_dissolving_raises_the_buffer` — the negative feedback, end to end.
* `..::a_calcite_shell_wears_in_acid_and_a_silica_shell_does_not` — the pair, as §10's rock and
  bedrock are tested as a pair, because either half alone passes on a re-skin.
* `..::a_genome_can_follow_a_ph_gradient` — the sensor reaches a behaviour, not just a number.
* And the guard that has now been earned twice: the mm-app world test must hold every scenario to
  seeding calcium and carbonate, and a shipped genome must *eat* them. §8 records both halves of
  that lesson — a world test cannot catch a gate, a gate cannot catch an empty world, and neither
  catches a cell that never goes shopping.

### Built, and four things the design got wrong

All of it, at ISA 12 and 13. The shape held; four of the specifics did not, and each was found by
a test rather than by reading.

**1. The saturations were the wrong instrument.** §11 gave calcium and carbonate a `saturation` of
24 units each, which would have put them under the *per-plane* weathering law as well as under the
calcite law — two laws governing one pair, which is how they come to disagree. Both are zero now:
`World::weather` returns early on a ceiling of zero, so the generic arm skips them entirely and
the pair is governed once, on the product and the pH. It also frees the seeding, which mattered
more than expected — see 3.

**2. The pH scale was linear and had to be, but for a different reason than the one given.** §11
argued for linear-in-a-bounded-ratio on cost grounds. The stronger argument turned up in testing:
at the operating point — pools comparable, which is how a world is seeded — the swing for an
insult `d` goes as `K·d / 2P`, so doubling the buffer halves the swing. **That is the buffering,
and it is a property of the ratio rather than something implemented.** A log form would have given
a swing independent of the buffer entirely, which is the opposite of what was wanted.

The way to get this wrong is written into `carbonate_buffers_the_swing`, because the first version
of that test got it wrong: raising carbonate against a *fixed* CO₂ walks the operating point up
the curve rather than adding capacity, and the curve is least sensitive at its ends. A world pinned
at pH 2 does not move much because there is nowhere left to go, which is saturation and not
buffering.

**3. Calcium is not a free parameter, and this is the number that will trip people up.** Carbonate
is pinned by the pH anchor — matched to a world's dissolved CO₂ so the slide reads exactly seven
and every move from there is something the cells did. Calcite then precipitates on the *product*,
so calcium is the only thing left that decides whether a world sits above or below the line. Each
world seeds twice the amount that puts `sqrt(calcium × carbonate)` exactly at
`calcite_saturation`: near enough to equilibrium that the pH decides the direction, far enough
over that a lit mat has something to lay down. On the shipped scenarios, whose CO₂ is 400 units,
that is 3.4 units of calcium; on the fresh slide, whose CO₂ is 40, it is 34.

**A consequence worth stating: calcium is scarce on the 400-CO₂ worlds.** That falls out of the
anchor rather than being chosen, and whether it makes calcite armour too dear to reach on exactly
the worlds people run is a question for a sweep, not for an argument.

**4. A wall cannot be judged against itself, and this cost two goes.** A blocked square holds
nothing: `ph_at` reads it neutral and its dissolved minerals read zero. Both are the right answers
to "what is in this rock" and the wrong inputs to "is this rock dissolving". Read from the square
itself a reef could never see the acid eating it *and* always looked maximally thirsty — so every
reef wore away at full rate whatever it stood in, including water saturated in exactly what it is
made of. A wall is now judged against **one** open neighbour, the same one for both readings: the
one with the least mineral in it, which is what the rock is actually giving itself up to.

The first fix did only the pH half, and the test passed anyway on the thirst term. That is what
`a_reef_in_saturated_but_sour_water_still_wears` is for — it holds the water at the calcite line
so thirst is nought and acid is the only thing left that can move the reef.

**And the rate is measured rather than matched.** `calcite_rate` started at a sixteenth, matching
`dissolve`, and a lit slide laid down reef over a tenth of itself in two thousand ticks — the
mechanism working, at a speed that reads as the world turning to stone rather than as geology. A
hundred and twenty-eighth is §10's "thousands of ticks" for the pair.

### Versioning

Two landings, two stamps (hard rule 8):

* **ISA 12** — calcium and carbonate exist, calcium is a rock, seeded everywhere, genomes taught to
  eat them. `chem_index` reduces `% CHEM_COUNT`, so every genome operand at or above 17 changes
  meaning; archived genomes replay under 11.
* **ISA 13** — the calcite shell at 31 and the pH sensor at 23. The derived pH, the buffer and the
  calcite weathering arm need no stamp of their own: nothing a genome can observe changes until
  the catalogue does.
