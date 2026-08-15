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

**Where that matter is, is not established and should not be guessed at**, any more than §6's
light column should have been. The candidate is the decay chain's time constant — corpse →
carrion → detritus → carbon, half-life near seven hundred ticks at the detritus stage — so a big
standing population means a big death flux with a lot of matter in transit, and an overshoot
large enough locks up more carbon than the survivors can replace. That is a hypothesis with a
curve consistent with it and no measurement behind it. The measurement it wants is a per-chemical
total over time, which `mm_core::metrics::Sample` does not carry; that is the job this leaves
behind, and it is now the second time this section has needed a number it cannot see.

The caveat this time, stated so it can be held against the next revision: **40 is one seed, flat
across the last 20,000 ticks of 120,000 — which is exactly what 100 looked like at 60,000.** It
should be run to 200,000 on three seeds before anyone calls it settled. The honest claim today is
that 40 rings on a period longer than any yet measured, not that it does not ring.

