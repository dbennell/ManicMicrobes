# Sixteen chemicals, and why that was never the constraint

An investigation prompted by a fair question — *we picked 16 because it sounded about right; is
it still?* — and the answer it produced, which was not the one the question expected.

Short version: **16 is fine, and roughly ten of them are scenery.** Adding chemicals cannot add
complexity, because the mechanism that would use them does not exist. What is scarce is
*reactions*, not *species*. The rest of this document is the evidence, and the design of the
change that follows from it.

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
oxidant: 14,    // brine — "an inert filler standing in for dissolved oxygen"
waste: 11,      // carbon_dioxide
byproduct: 14,  // the same filler back again
structural: 4,  // carbon
reactive: 13,   // peroxide
```

Plus carrion at 15, named in `ecology.rs`. Note that `oxidant` and `byproduct` are the *same
index*, and `OrganelleCatalogue::closes` actually requires them to be — `byproduct` is a
vestigial name for the thing photosynthesis produces alongside the substrate, which is the
oxidant.

That leaves ten:

| | | |
| --- | --- | --- |
| `signal_a`–`signal_d` | 0–3 | **Genuinely useful.** `EMIT`, `EAT` and the chemosensor all work on them and the engine ascribes them no meaning, which is exactly right for a communication channel that evolution is supposed to invent a use for. |
| `nitrogen`, `phosphorus`, `silicon` | 5–7 | Flagged `structural: true` in the table — but only index 4 is *the* structural chemical. Nothing can be built out of them. |
| `lipid`, `sulphide` | 9–10 | Carry `energy_yield` of 1536 and 768. But a mitochondrion burns `chemistry.substrate`, which is one index. **Nothing burns them.** |
| `ammonia` | 12 | Filler. |

So the table already describes a richer world than the engine implements. Lipid is a food no
organelle can eat; phosphorus is a building material nothing is built from. A seventeenth
chemical would join them.

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
