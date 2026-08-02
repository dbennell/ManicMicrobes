# Manic Microbes — Technical Specification

**Status:** normative. Where this document and an implementation disagree, this document wins.
**ISA version:** 1
**Target:** Rust (stable), Bevy front-end, headless core.

---

## 0. What this is

An artificial-life simulator in the lineage of Tierra, Avida and DarwinBots. Cells carry a
genome of byte-encoded instructions which they execute in a per-cell virtual machine. The
genome builds and controls physical machinery (organelles); it does not directly express
behaviour. Cells live on a 2D substrate carrying a fluid with 16 diffusing chemical species
and an incident light field. Matter is exactly conserved. Energy enters as light and leaves
as heat. Cells eat, move, sense, attack, join, differentiate and divide. Nothing is scored
by a fitness function; selection is a consequence of the physics.

Two framings coexist and must both be served:

- **Petri mode** — open-ended evolution from a seeded or minimal ancestor. Mutation on.
  The interesting output is the phylogeny and the story it tells.
- **Arena mode** — hand-authored cells competing. Mutation off or clamped. The interesting
  output is whose code wins.

These are the same engine under different configuration. Neither may compromise the other.

There is a third, equally load-bearing goal: **the thing must be beautiful and legible to
people who do not care about artificial life.** It is a fishtank as much as an instrument.
The microscope view, the species wiki and the timeline of first occurrences are not polish;
they are the product.

---

## 1. Repository layout

```
manic-microbes/
  Cargo.toml                  # workspace
  CLAUDE.md                   # working agreement — read before any change
  crates/
    mm-core/                  # simulation. NO Bevy. NO floats. NO wall-clock. NO global RNG.
    mm-asm/                   # assembler, disassembler, source maps
    mm-cli/                   # headless runner, batch sweeps, metric export
    mm-app/                   # Bevy front-end: microscope, editor, wiki, tools
  docs/
    SPEC.md                   # this file
    MILESTONES.md
  scenarios/                  # .ron scenario configs
  genomes/                    # .mm assembly sources
  tests/
```

`mm-core` must compile and run with `--no-default-features` and zero Bevy dependency. This
is enforced by CI. It exists so the simulation can run headless at 1000x realtime for
parameter sweeps, and so the renderer can never hold the simulation hostage to a frame
budget.

---

## 2. Invariants

These hold at every tick, are asserted in debug builds, and are covered by tests.

**I1 — Determinism.** Given the same scenario, seed and input event log, two runs produce
bit-identical world state at every tick, on any platform, at any thread count. Verified by
comparing a rolling state hash.

**I2 — No floats in `mm-core`.** All simulation arithmetic is integer or fixed-point. `f32`
and `f64` are forbidden in the crate; enforced by a lint/grep test. Floats exist only in
`mm-app` for rendering.

**I3 — Totality.** No byte sequence, in any register/stack/memory state, can panic, hang or
abort the VM. Every opcode is defined for every input. Out-of-range addressing wraps.
Division by zero yields zero. Stack underflow yields zero. The worst a program can do is
waste energy. Covered by a fuzz/property test over random byte arrays.

**I4 — Matter conservation.** The sum of each chemical species over (fluid + all cell
interiors + all cell structural mass + all corpses) is invariant to the exact integer.
Asserted every N ticks in debug; a test runs 1,000,000 ticks and requires zero drift.

*Except through a balanced reaction.* §7.2's metabolism turns substrate into waste and back
again, so a per-species total that never moved would make the matter loop impossible — the two
statements as originally written contradicted each other. The resolution keeps the invariant
in its exact per-species form rather than weakening it to a sum:

- **Total matter, summed over all sixteen species, is invariant. No mechanism may move it.**
- A per-species total may change **only** through a reaction that reports itself to the
  ledger, and every reaction is stoichiometrically balanced: the units leaving one species
  equal the units arriving in another.

The ledger's per-species claim therefore stays exact, and an unreported transmutation shows up
as drift in exactly the same way a leak would. That is the point of routing conversions through
the ledger rather than letting metabolism adjust two arrays and hope: a reaction that forgets to
account for itself is indistinguishable from a conservation bug, and both should fail the same
test.

**I5 — Energy accounting.** `energy_in (light) == energy_out (heat) + delta(stored energy)`,
exactly, in integer units. Energy is *not* conserved — it degrades. It is *accounted*.

"Stored" has to be defined precisely enough to recompute from the world, or the identity is a
ledger agreeing with itself. It is the sum of two things:

- the energy each living cell holds, and
- the **latent energy of the substrate chemical**, wherever it is — inside a cell or dissolved
  in the fluid. A unit of sugar is energy that has not been spent yet. Counting only cell
  energy would make photosynthesis look like it created energy from nothing and respiration
  look like it destroyed it.

Both conversions are lossy on purpose. Photosynthesis banks less than the light it catches and
respiration recovers less than the substrate holds; the difference is dissipated as heat, and
that difference is the free-energy dissipation rate §13 exists to plot. Without it a cell would
be a battery rather than a dissipative structure.

**I6 — Schedule independence.** Simulation results do not depend on thread count,
work-stealing order, or iteration order of any hash map. Randomness is derived by hashing
`(seed, tick, cell_id, purpose)`, never from a sequential stream.

**I7 — Serialisable state.** Complete world state round-trips through serialisation with
bit-identical resumption. This is a hard requirement now even though networking is deferred,
because retrofitting it is infeasible.

---

## 3. Numeric conventions

- Cell-visible values are `i16`. Arithmetic is performed in `i32` and **saturates** on store
  to `i16`.
- **Addressing wraps; magnitudes saturate.** Organelle slot, chemical index, register index,
  RAM address, genome offset and junction index are all reduced modulo their range.
  Arithmetic results clamp to `[-32768, 32767]`.

  *Rationale: wrapping arithmetic puts a cliff in the fitness landscape — a one-bit mutation
  flips a cell from "very fast forward" to "very fast reverse". Saturation keeps the landscape
  continuous and climbable. Wrapping addresses keep every index legal, which serves totality.*

- Fluid quantities, cell mass and energy are `i32` in fixed-point with an implied scale of
  1024 (`Q22.10`). Conversion to cell-visible `i16` saturates.
- Positions are `i32` fixed-point, scale 256, in substrate-cell units.
- A **shift count is an address**, not a magnitude: `SHL` and `SHR` reduce it modulo 16
  (taking the low four bits of the operand's raw representation, so a negative count is
  legal and large). The shifted *value* saturates like any other magnitude, and `SHR` is
  arithmetic, preserving sign.

---

## 4. The genome

### 4.1 Physical representation

A genome is a flat `[u8]`. It is physically resident in the cell's **nucleus** organelles;
total nucleus capacity bounds genome length, and nucleus capacity carries a continuous
upkeep cost in energy and structural matter. A cell whose genome exceeds its nucleus
capacity is truncated at the next division. Genome bloat is therefore selected against by
physics rather than by a rule.

Genomes are interned and shared: `Arc<Genome>`, deduplicated by content hash. In a 200k
population with clonal lineages, the number of distinct genomes is typically in the low
thousands. Mutation triggers copy-on-write.

A genome may not exceed **65,536 bytes**. `IP`, `PA`, `PB` and call-stack entries are `u16`
so that per-cell fixed state stays inside the 512-byte budget of §6.1, and a genome longer
than they can address would be unrepresentable rather than merely large. Nucleus capacity
bounds length far below this in any running simulation; the limit exists so that the
representation has no undefined region, and construction rejects an over-long genome rather
than truncating it silently.

### 4.2 Degenerate encoding

The opcode is `byte % 64`. Four distinct byte values map to each of the 64 opcodes.

This is deliberate and mirrors codon degeneracy: a large fraction of point mutations are
**synonymous**, producing no phenotypic change. Synonymous sites accumulate neutral
variation, which is both a smoother search landscape and a molecular clock the phylogeny
layer can exploit.

### 4.3 Templates

Several instructions are followed by a **template**: the maximal run of `NOP0`/`NOP1`
instructions immediately following, up to 8. A template has a value (bits read
least-significant-first, `NOP0`=0, `NOP1`=1) and a length.

A template is the **maximal** run, which has a consequence worth stating outright: letters
written immediately after one become part of it. A jump followed by a label fuses eight
letters into a single template, and the jump then searches for the complement of something
nobody wrote. This is correct behaviour and it is also the easiest mistake to make in
hand-written assembly, so `mm-asm` refuses to assemble a source where an explicitly written
template of fewer than 8 letters is extended by the letters after it. Template runs do not
wrap past the end of the genome; a run at the end is truncated there.

A zero-length template suppresses its host instruction's *search or bind* — the jump does
not jump, `EXPRESS` does not bind — but the instruction's documented **stack effect still
happens**. `JMPZ` consumes its operand whether or not it jumps.

*Rationale: the alternative, that a zero-length template suppresses the instruction
entirely, would make `JMPZ` sometimes consume its operand and sometimes not, so deleting a
single `NOP` would silently change the stack balance of everything downstream. That is a
cliff in exactly the place the rest of §3 works to avoid one. `IMM` pushing 0 is this same
rule rather than an exception to it: the stack effect happens, with the value the empty
template denotes.*

Templates serve three purposes:

1. **Numeric literals** — `IMM <template>` pushes the value. Appending a `NOP` is a small
   numeric change, so literals are incrementally mutable.
2. **Jump targets** — `JMPF`/`JMPB`/`CALL` search for the *complementary* pattern
   (`NOP0`↔`NOP1`) of the same length, scanning outward from the instruction pointer up to
   `template_search_range` (default 512 bytes). Not found ⇒ no-op.

   An offset matches when the template run starting there is **at least** as long as the
   query and its first `len` letters are the complement; execution resumes at the offset
   just past those letters. The scan wraps around the genome, and probes at most
   `min(template_search_range, genome length)` offsets — so a genome shorter than the search
   range is scanned exactly once round rather than repeatedly, and a backward jump in a
   short genome can legitimately reach a target that lies physically ahead of it.
3. **Gene promoters** — see below.

Complementary matching is base-pairing, and it is the single most important mechanic for
evolvability: genomes become position-independent, so duplication, translocation and
insertion produce working variants rather than rubble, and a damaged template finds a
slightly wrong target instead of crashing.

### 4.4 Gene blocks and promoters

A gene block begins with `GENE <template>`. The template is the gene's **promoter**.
`GENE` executes as a no-op if reached by fall-through.

`EXPRESS <template>` performs an associative lookup: among all `GENE` headers in the genome,
find the promoter with minimum Hamming distance to the given pattern (patterns of unequal
length are compared over the shorter, with each missing bit counted as a half-mismatch,
rounded up). If the minimum distance exceeds `promoter_bind_threshold` (default 2), `EXPRESS`
is a no-op. Ties resolve to the lowest genome offset. On success it behaves as `CALL`,
entering the gene body — the offset just past the `GENE` header and its promoter.

Distance is computed in halves to keep the rounding unambiguous: with `d` full mismatches
over the shared prefix and `m` letters present in only the longer pattern, the distance is
`d + ceil(m / 2)`. A promoter is scanned once per genome at construction, not per
`EXPRESS`.

This is transcription-factor binding, and it replaces symbolic `#tag` addressing. It fixes
the fragility directly: deleting a gene does not orphan its callers, they bind the next-best
match. Duplicating a gene and mutating its promoter yields a paralog expressed under
different conditions — which is the actual mechanism by which biological novelty arises.

The assembler still lets the author write `#hunt`; it compiles the name to a bit pattern via
a stable hash rather than to a symbol-table entry.

---

## 5. The virtual machine

Per cell:

| Component      | Size | Notes |
|----------------|------|-------|
| Data stack     | 16   | `i16`, **circular** — no underflow or overflow failure |
| Call stack     | 8    | `u16` return offsets, circular |
| Registers      | 16   | `i16`, addressed `idx % 16` |
| Scratch RAM    | 64   | `i16` words, addressed `addr % 64` |
| `PA`, `PB`     | —    | source and destination pointers for copying |
| `LN`           | —    | copy-length counter |
| `IP`           | —    | instruction pointer, wraps modulo genome length |

Circular stacks are the totality mechanism: popping an empty stack yields 0; pushing a full
stack overwrites the oldest entry. There is no fault state.

`LN` is unsigned. `SETLN` clamps a negative operand to 0, and `COPYB` saturates it at 0
rather than wrapping — otherwise a copy loop that ran one byte too far would run 65,536
more.

**Execution budget.** A cell executes up to `instr_per_tick` instructions per tick (default
16), each costing energy. `HALT` yields the remainder of the budget and refunds a fraction
of its cost — sleeping is cheap, which makes dormancy an evolvable strategy.

### 5.1 Instruction set (64 opcodes)

`<t>` marks a template-consuming instruction. Stack effects are written `( before -- after )`
with the top of stack rightmost.

**0x00–0x0F — Templates, literals, stack, memory**

| Op | Name | Effect |
|----|------|--------|
| 00 | `NOP0` | template letter 0; no-op when executed |
| 01 | `NOP1` | template letter 1; no-op when executed |
| 02 | `IMM <t>` | `( -- v )` push template value |
| 03 | `ZERO` | `( -- 0 )` |
| 04 | `ONE` | `( -- 1 )` |
| 05 | `DUP` | `( a -- a a )` |
| 06 | `DROP` | `( a -- )` |
| 07 | `SWAP` | `( a b -- b a )` |
| 08 | `OVER` | `( a b -- a b a )` |
| 09 | `ROT` | `( a b c -- b c a )` |
| 0A | `LOAD` | `( addr -- v )` from scratch RAM |
| 0B | `STORE` | `( v addr -- )` to scratch RAM |
| 0C | `RLOAD` | `( idx -- v )` from register |
| 0D | `RSTORE` | `( v idx -- )` to register |
| 0E | `RAND` | `( -- v )` deterministic; see §11 |
| 0F | `RESERVED_0` | no-op |

**0x10–0x1F — Arithmetic and logic** (all saturating; `DIV`/`MOD` by zero yield 0)

| Op | Name | | Op | Name |
|----|------|-|----|------|
| 10 | `ADD` | | 18 | `MAX` |
| 11 | `SUB` | | 19 | `SHL` |
| 12 | `MUL` | | 1A | `SHR` |
| 13 | `DIV` | | 1B | `AND` |
| 14 | `MOD` | | 1C | `OR` |
| 15 | `NEG` | | 1D | `XOR` |
| 16 | `ABS` | | 1E | `NOT` |
| 17 | `MIN` | | 1F | `CMP` → `sign(a-b)` ∈ {-1,0,1} |

**0x20–0x2F — Control flow and replication machinery**

| Op | Name | Effect |
|----|------|--------|
| 20 | `JMPF <t>` | jump forward to complement |
| 21 | `JMPB <t>` | jump backward to complement |
| 22 | `JMPZ <t>` | `( a -- )` jump forward if `a == 0` |
| 23 | `JMPNZ <t>` | `( a -- )` jump forward if `a != 0` |
| 24 | `CALL <t>` | jump forward to complement, push return |
| 25 | `RET` | pop return |
| 26 | `GENE <t>` | promoter marker; no-op when executed |
| 27 | `EXPRESS <t>` | associative call by promoter match |
| 28 | `SKIPZ` | `( a -- )` if `a == 0`, skip next instruction and its template |
| 29 | `SETPA` | `( v -- )` set source pointer |
| 2A | `SETPB` | `( v -- )` set destination pointer |
| 2B | `SETLN` | `( v -- )` set copy counter |
| 2C | `GLEN` | `( -- n )` push own genome length |
| 2D | `LOOPLN <t>` | if `LN != 0`, jump backward to complement |
| 2E | `HALT` | yield remaining instruction budget this tick |
| 2F | `RESERVED_1` | no-op |

**0x30–0x3F — Body and world**

| Op | Name | Effect |
|----|------|--------|
| 30 | `BUILD` | `( param type slot -- )` begin constructing an organelle |
| 31 | `TEAR` | `( slot -- )` dismantle, recovering part of its matter |
| 32 | `OSET` | `( v idx slot -- )` write organelle control input |
| 33 | `OGET` | `( idx slot -- v )` read organelle output |
| 34 | `OTYPE` | `( slot -- type )` |
| 35 | `EAT` | `( amount chem -- got )` ingest from local fluid |
| 36 | `EMIT` | `( amount chem -- sent )` excrete to local fluid |
| 37 | `BUD` | `( size -- ok )` allocate daughter genome buffer; sets `PB = 0` |
| 38 | `COPYB` | copy byte `PA`→daughter`[PB]`; `PA++`, `PB++`, `LN--` |
| 39 | `SPLIT` | finalise division |
| 3A | `JOIN` | `( key kind handle -- ok )` attempt junction |
| 3B | `LEAVE` | `( jidx -- )` dissolve junction |
| 3C | `JXFER` | `( amount what jidx -- moved )` transfer over soft junction |
| 3D | `JLEN` | `( v jidx -- )` set junction rest-length offset (muscle) |
| 3E | `SETKEY` | `( v -- )` set own receptor key, `v & 0x7F` |
| 3F | `INJECT` | `( jidx -- ok )` write byte `PA`→target nucleus`[PB]`; `PA++`, `PB++`, `LN--` |

`INJECT` advances the same pointers as `COPYB`, and for the same reason §8.3 gives: reading
and writing genome bytes is one interface whether the target is self or a neighbour, so the
copy loop over a soft junction is written exactly like the copy loop into a daughter buffer.
Anything else would make horizontal transfer a special case, and it must not be one.

The `0x30`–`0x3F` block acts on a body and a substrate. Their **stack effects are part of
the ISA and happen regardless** of whether the world can satisfy them — a genome's stack
discipline must not depend on what its environment supports, or the same bytes would balance
differently in a cell with a chloroplast and a cell without one.

Cell introspection (mass, energy, age, radius, position hint, internal chemical levels) is
read via `OGET` on slot 0, the membrane — the membrane is the self-sensor. This keeps the
opcode table at 64 without losing capability.

### 5.2 The replication loop

`COPYB` plus `LOOPLN` makes the inner copy loop two instructions. A minimal viable
replicator is roughly:

```
        GENE  #replicate
        GLEN  SETLN            ; length = own genome length
        GLEN  BUD  DROP        ; allocate daughter buffer
        ZERO  SETPA
        ZERO  SETPB
  loop: COPYB
        LOOPLN loop
        SPLIT
```

~10 instructions. With degenerate encoding, per-position probability of the correct opcode
under uniform random bytes is 1/64. Combined with overlapping reading frames across a
population of 10⁴–10⁵ cells, this is a searchable target — but the primary intended path is
seeding a hand-written ancestor, with de-novo emergence as a scenario option rather than a
gate on progress.

---

## 6. Cells

### 6.1 Layout

Cells live in a struct-of-arrays arena in an `mm-core` resource, **not** in the Bevy ECS.
200k entities with birth and death every tick means constant archetype churn. IDs are
stable generational keys from a slot map. `mm-app` holds a single entity owning a render
buffer.

Budget: ≤ 512 bytes per cell for fixed state, excluding the shared genome.

Per-cell fixed state: position, velocity, mass, energy, age, VM state (stack/registers/RAM/
pointers/IP), 16 organelle slots, internal chemical vector (16 × `i32`), receptor key,
species id, parent id, birth tick, genome `Arc`, junction list head, daughter buffer handle.

### 6.2 Organelles

16 slots, addressed `slot % 16`. Slot 0 is always `MEMBRANE` and cannot be torn down or
retyped. A 4-bit slot operand means a mutation to an organelle reference is a small local
perturbation rather than a 1-in-100 lottery.

The catalogue is fixed at 16 entries; unimplemented entries are `RESERVED` and behave as
no-ops. New organelle types are added by filling a `RESERVED` slot, which preserves the
meaning of every previously defined type in archived genomes.

| # | Type | Control inputs (`OSET`) | Outputs (`OGET`) |
|---|------|-------------------------|------------------|
| 0 | `MEMBRANE` | permeability, investment | mass, energy, age, radius, internal chem[c], damage |
| 1 | `NUCLEUS` | copy fidelity | capacity, used |
| 2 | `MITOCHONDRION` | throttle | rate, substrate available |
| 3 | `CHLOROPLAST` | throttle | rate, local light |
| 4 | `VACUOLE` | — | capacity, contents |
| 5 | `PUMP` | chemical, signed rate | achieved rate |
| 6 | `CILIUM` | signed power | achieved thrust, load |
| 7 | `CHEMOSENSOR` | chemical | concentration, gradient dx, gradient dy |
| 8 | `PHOTOSENSOR` | — | intensity, direction dx, dy |
| 9 | `TOUCHSENSOR` | — | contact count, handle, contact mass, contact kind |
| 10 | `JUNCTION_PORT` | — | junction count, handle by index |
| 11 | `LYSOSOME` | throttle | digestion rate |
| 12 | `SPIKE` | signed extension | contact damage dealt |
| 13 | `OSCILLATOR` | period | phase |
| 14 | `RESERVED_A` | — | — |
| 15 | `RESERVED_B` | — | — |

Organelles have a `param` (0–255) set at `BUILD` time, scaling capability and cost. They
take time to construct, consuming structural chemicals and energy across multiple ticks; a
partially built organelle is inert.

### 6.3 Differentiation

There is **no cell-type enum.** "Skin", "muscle" and "neuron" are descriptive labels applied
by the analysis layer to cells whose expressed organelle loadouts fit known patterns. A
muscle cell is one that modulates `JLEN` on its hard junctions. A neuron is one whose
organelles are cheap junction ports and whose genome transforms signals arriving via
`JXFER`. Skin is high membrane investment and little else.

Differentiation arises because gene expression is gated on internal chemical state, and
internal chemical state varies across a cluster because neighbours pump chemicals into each
other. That is morphogen gradients, and it gives evo-devo for free. Do not shortcut it with
a type field.

### 6.4 Contact, and why cells compress

A cell occupies space, and its radius is derived from its mass. Two cells whose radii overlap
are pushed apart along the line between their centres, by position projection, over the same
2–3 Gauss–Seidel iterations §8.4 specifies for junctions.

Contact is **not** a non-penetration constraint, and this is the part that is easy to get
wrong by assuming otherwise. Cells are allowed to overlap. The response has two regimes:

- **Soft**, through most of the overlap: a weak restoring push, so a pair under load rests
  visibly compressed and how deeply it is compressed is a reading of how hard it is pressed.
- **Stiff**, past a **core** at a fixed fraction of the touching distance: sixteen times the
  soft response, so compression effectively stops there regardless of load.

A crowd therefore gets harder to squash the more it is squashed, and the depth it settles at
is set by geometry rather than by whatever the load happens to be, or by the solver's budget.

Three consequences that are normative rather than incidental:

1. **The resting overlap is the tissue, not an error.** The renderer draws the flat wall
   between two cells by cutting each at the plane where their outlines cross, which exists
   only where they overlap. Drive overlap to zero and there is no wall to draw — and
   non-overlapping circles cannot tile a plane, so a crowd solved to convergence is a bag of
   marbles with holes between them, however good the shader is.
2. **The core fraction is shared with the renderer.** `mm_core::neighbours::CORE_PERMILLE` and
   `mm_app::slide::MIN_FACE` are the same fraction expressing the same idea — every cell keeps
   an incompressible core of that fraction of its own radius — as a floor on centre distance
   in the physics and as a floor on where a cell may be cut in the renderer. They must be
   changed together. A cell drawn with a core it does not physically have is a cell drawn
   overlapping.

   They are not, however, the same constraint, and the renderer's clamp is not redundant. For
   two cells of equal radius the two coincide exactly: at the distance where the cores touch,
   the plane through the crossing outlines falls precisely at the core of each. For unequal
   radii it does not — a cell twice its neighbour's radius has that plane past the smaller
   cell's *centre* while the cores are still apart. Respecting the core in the physics does
   not make a cell safe from being cut away, so both floors are load-bearing.
3. **Being crushed means being driven past the core**, not being in contact. Crowding damage
   (§ecology) is charged on core penetration only. Charging for ordinary resting compression
   would make a crowd lethal and a tissue impossible to hold together.

Every correction is clamped to a fraction of the cell's own radius, **per contact rather than
as a pooled per-tick budget.** A pool is spent in slot order, so a cell with eight neighbours
resolves the first few contacts and silently skips the rest: the surface of a pack behaves and
its interior collapses into itself. That clamp also bounds the stiff regime, so there is a
load above which the core loses and cells pass through one another; that ceiling is asserted
by test rather than left to be discovered.

---

## 7. Chemistry, matter and energy

### 7.1 Chemicals

16 species, index `c % 16`, each described by a data-driven entry in the scenario:

```rust
struct ChemicalDef {
    name: String,
    diffusion: i32,        // fixed-point rate
    toxicity: i32,         // membrane damage per unit above threshold
    energy_yield: i32,     // released when oxidised by a mitochondrion
    structural: bool,      // usable as build material
    colour: [u8; 3],       // for the false-colour overlay
}
```

Default table: 4 inert signalling species, 4 structural monomers, 3 energy substrates,
2 metabolic wastes, 1 toxin, 1 inert filler, and carrion.

**Carrion is chemical 15** (M8). A corpse is not an object with a decay timer: when a cell
dies, part of its structural mass is deposited on the square it died on as chemical 15,
which diffuses very slowly, decays into ordinary waste on its own, and is conserved exactly
because it is conserved by the same machinery as everything else in the fluid. It is not
`structural`, so it cannot be built from directly — a cell has to digest it with a lysosome
first, which is the whole of what makes scavenging a distinct trade.

### 7.2 The matter loop

Matter is conserved exactly, which means the world would otherwise run down into an
all-waste equilibrium and die. Closing the loop requires a primary producer pathway:

- **Mitochondrion:** `substrate + O → energy + waste`
- **Chloroplast:** `waste + light → substrate`

Light is therefore the only thing that keeps the biosphere from equilibrating. This is not
incidental — it is the entropy story the whole simulation exists to display. A cell is a
dissipative structure: it maintains local order by consuming a gradient and exporting
disorder.

**A world offers several such pathways, not one** (added at M10.3; ISA version 2). Each names
its own substrate, oxidant, waste and reactive byproduct, and each must close on its own. An
organelle that runs a reaction — mitochondrion, chloroplast, lysosome — chooses which one by
its **`control[1]`**, reduced modulo the number of pathways so that every value a genome can
write names a real reaction.

This is the difference between one way of making a living and several. A mitochondrion set to
pathway 1 can burn only pathway 1's substrate, so a lineage must either pair its own
chloroplast onto the same reaction or eat something that makes what it burns — which is
cross-feeding, and the first mechanism here that turns one lineage's waste into another's food
by evolution rather than by construction. Pathways share an oxidant and a waste in the default
set, so they are alternatives competing for one pool rather than disjoint worlds.

Pathway 0 is the reaction the engine ran on from M2 to M9, and a fresh organelle's `control[1]`
is zero, so a genome that says nothing about pathways behaves exactly as it always did.

The reasoning, and the measurements that produced it, are in `docs/CHEMISTRY.md`.

Dead cells become corpses: their structural mass and internal chemicals persist as a
localised deposit that lysosomes can digest and that decays into the fluid over time.

### 7.3 Energy and light

Energy is a separate scalar, not a chemical. It enters the world only via chloroplasts
absorbing light and leaves only as heat (metabolic inefficiency, movement drag,
maintenance). `energy_in`, `energy_out` and `energy_stored` are tracked globally to the
exact integer.

The user-facing scenario knobs are the **shape of the gradient**, not open-vs-closed:

- uniform illumination
- day/night cycle (period configurable)
- directional gradient — bright at one edge, dark at the other
- hydrothermal vent — a chemical energy source at a point, no light
- slow decline — declining flux over millions of ticks, forcing adaptation or extinction
- seasons — a day/night cycle whose noon itself rises and falls over a much longer year
  (added at M8: two timescales, which a single cycle cannot express, so that the strategy
  that pays in summer is not the one that pays in winter and nothing can settle)

These generate mass extinctions and radiations, which are the events the wiki timeline
exists to report.

### 7.4 Fluid

The substrate is a uniform grid. Each grid square holds 16 chemical quantities, a light
value, a velocity vector, and a `blocked` flag for user-drawn barriers.

**Scope decision for v1:** diffusion plus advection by a velocity field, no pressure
projection, no incompressibility solve.

- **Diffusion** is flux-based and symmetric: the integer amount subtracted from A is exactly
  the amount added to B. This makes conservation exact by construction rather than by
  correction.
- **Advection** is donor-cell upwind, also flux-based, therefore also exactly conservative.
- **Velocity** comes from a slowly varying prescribed field (user-configurable currents,
  e.g. rotational stirring) plus local impulses injected by cilia, decaying over time.

Full Navier–Stokes with projection is a later upgrade behind the same interface. It is
explicitly not required for interesting behaviour, and it costs determinism and speed.

The fluid runs at `fluid_hz`, decoupled from and typically lower than the cell tick rate,
parallelised with rayon over row bands.

This paragraph used to specify checkerboard phasing as the way fluxes avoid racing. M1
measured it: checkerboard is about twice as slow as the scheme that shipped, and phasing an
isotropic kernel makes it anisotropic. What ships instead computes every flux into its own
plane and then applies all of them, which needs no phasing at all — a flux is a pure function
of the two squares either side of it, so nothing races when nothing is being written yet.
Row bands are still how the work is divided. See the module docs in `fluid.rs` for the two
alternatives that were tried and dropped.

---

## 8. Junctions, and the binding key

### 8.1 Kinds

- **Soft** — a transfer channel. Moves chemicals, energy and genome bytes. No positional
  constraint; breaks beyond `soft_max_range`. This is the conjugation/synapse/infection
  channel.
- **Hard** — structural. Carries a distance constraint. This is multicellularity.

### 8.2 The binding key — consent with a back door

Every cell has a 7-bit **receptor key** (0–127), set by `SETKEY`, default inherited from the
parent's genome. `JOIN` supplies a key along with the target handle.

- Key matches the target's receptor key → cost is `join_base_cost`. Effectively free.
- Key does not match → cost is `join_base_cost + join_forced_penalty × target_membrane_investment`.

The junction still forms if the aggressor can pay. Consent is economic, not absolute.

This mechanic does a remarkable amount of work for its size:

1. **Clones share a key**, so self-assembly of a multicellular colony is nearly free. The
   bootstrap problem for multicellularity — needing two genomes to cooperate before either
   has a reason to — dissolves, because clonal cells cooperate by default.
2. **Kin recognition falls out.** A cell can join its relatives cheaply and strangers dearly
   without any explicit relatedness calculation.
3. **Parasitism is possible but must be paid for.** A parasite either brute-forces the key
   space (128 attempts, each costing energy and time) or evolves to specialise on a
   prevalent host key.
4. **Host defence is a real trade-off.** Mutating your key escapes a specialised parasite —
   but it also disconnects you from your own colony and your own offspring. This is a
   genuine Red Queen dynamic with a cost on both sides, and it is the kind of thing that
   produces a story worth writing on a wiki page.

**Probe semantics.** By default, a failed `JOIN` returns only success/failure — one bit. It
must not return Hamming distance to the true key, because that makes the key hill-climbable
in about seven probes and parasitism becomes trivial. `probe_leaks_distance` exists as a
scenario knob for anyone who wants to watch that happen deliberately.

### 8.3 Nucleus access is symmetric

Reading and writing genome bytes uses the same interface whether the target is self or a
neighbour: `INJECT` takes a junction index, with a reserved index meaning "self". A soft
junction is required for a non-self target.

Consequently **viruses are emergent, not implemented.** A cell that forms a soft junction
and writes bytes into a neighbour's nucleus is a parasite; nothing in the engine knows the
word. Self-modifying code and other-modifying code are the same mechanism.

A cell being written to continues executing. The instruction pointer wraps modulo genome
length, so there is no invalid state.

### 8.4 Physics

Hard junctions are **position-based dynamics** distance constraints, solved with 2–3
Gauss–Seidel iterations per tick, mass-weighted, with a stiffness parameter and a breaking
strain.

Junctions do **not** couple to the fluid. There is no torque, no angular dynamics, no lever
arms, no fluid backpressure. A cluster does not paddle.

What this does buy, for roughly 1–2% of frame cost at 50k junctions:

- Connected cells stay connected in space, so a junction means something spatially.
- Cilia on one cell push that cell; the constraints drag the rest. **Colony locomotion is
  emergent** with no rigid-body solver.
- Muscle becomes a coherent strategy: `JLEN` modulates rest length within
  `±muscle_range`, giving contraction, peristalsis and shape change.

A union-find pass maintains connected components over hard junctions, updated incrementally
on join and dissolve. "Which cells constitute one organism" is needed by the phylogeny
layer, the wiki, selection rendering and the tweezers regardless.

---

## 9. Mutation

Mutation happens at two points, which differ in kind.

**Per-byte copy error, during `COPYB`.** Probability is a function of the nucleus organelle's
copy-fidelity control input and the energy spent on that byte. High fidelity costs more
energy per byte copied. This makes **mutation rate genetically encoded and physically
costly**, so mutator alleles can evolve, and the observed fidelity of a lineage becomes a
measurable, plottable trait.

**Structural mutation, at `SPLIT`**, at scenario-configured base rates:

| Operator | Effect |
|----------|--------|
| Point | substitute one byte |
| Insertion | insert a random byte |
| Deletion | remove a byte |
| **Duplication** | copy a segment (biased toward gene-block boundaries) |
| Inversion | reverse a segment |
| Translocation | move a segment |

Duplication is first-class and must not be omitted: duplication-and-divergence is the
principal engine of novelty in biology, and combined with promoter binding it produces
paralogs — the same gene expressed under different conditions. Horizontal transfer via
`INJECT` is the second such engine.

---

## 10. Phylogeny, speciation and the wiki

### 10.1 The tree is free

Real biologists infer phylogeny from extant sequences because they have no record of who
descended from whom. **We have a perfect record.** Every cell stores its parent id and birth
tick. The true tree requires no inference.

Genetic distance is needed only for **naming** — deciding when a lineage has diverged enough
to be called a new species.

### 10.2 Distance

Each genome carries a 64-bit **SimHash** over 4-byte k-mers, 8 bytes per genome, computed
once at mutation (an unmutated daughter inherits the parent's fingerprint, so the common
case is free). Distance is Hamming distance over the fingerprint.

If precision proves insufficient in practice, upgrade to a 32×`u16` MinHash sketch with
Jaccard similarity — same interface, 64 bytes per genome.

### 10.3 Speciation

A species record holds a founder genome and fingerprint. A newborn whose fingerprint
distance from its species founder exceeds `speciation_threshold` founds a new species,
parented to the old one. Species above a deeper distance threshold are grouped into genera,
and genera into families, purely for display.

Dead branches are pruned on a schedule; the overwhelming majority of the tree is dead ends
and keeping it all is a storage leak.

**Do not store per-individual birth records.** At 200k cells with fast turnover this is
millions of events per minute. Store species-level aggregates continuously, and full genomes
only for species founders plus periodic population snapshots.

### 10.4 Names

Auto-generated Linnaean binomials from Latinate syllable tables, seeded by lineage hash,
with the specific epithet biased by dominant traits (`rapidus` for high cilium investment,
`vorax` for high predation rate, `lucens` for chloroplast dominance).

### 10.5 The wiki

A generated page per species, retained after extinction:

- founding tick, parent species, phylogenetic position
- population curve, peak population and when
- extinction tick and an inferred cause (outcompeted by X, starved during light decline,
  parasitised by Y)
- a behavioural description derived from expressed organelles plus runtime statistics
- the founder genome, viewable and loadable into the editor

> *Cilius rapidus — fast chemotroph, 3 cilia, follows chemical 7. Diverged from* C. tardus
> *at tick 1.2M. Peak population 14,000 at tick 3.4M. Extinct at 5.1M, outcompeted by its
> own descendant* C. velox.

### 10.6 First-occurrence detectors

A world-level event log recording the first time each milestone is observed, with tick,
species and location. This is the newspaper, and it is the single largest contributor to the
simulation feeling like a story rather than a screensaver.

Minimum set: first endogenous replication; first chemotaxis; first phototaxis; first
motility; first predation; first successful `INJECT` into a non-self genome; first soft
junction; first hard junction; first cluster of size ≥ 4, ≥ 16, ≥ 64; first differentiated
cluster (two distinct organelle loadouts within one component); first signal relay through a
junction chain of length ≥ 3; first key-mismatch forced junction; first dormancy; first
lineage to exceed N generations; each mass extinction (population drop > 50% within a
window).

---

## 11. Randomness

There is no RNG stream. Every random value is derived by hashing:

```
value = mix(seed, tick, cell_id, purpose_tag, index)
```

using a fast integer hash (splitmix64 or PCG). Purpose tags separate uses (`RAND` opcode,
mutation site selection, mutation operator choice, Brownian jitter) so that consuming a
random number in one system cannot perturb another.

`index` distinguishes repeated draws for the same purpose within one tick — the *n*th `RAND`
a cell executes, the *n*th mutation site considered. Without it every draw in a tick would
return the same number. For `RAND` the index is a per-cell counter, part of VM state and
therefore part of what serialisation must round-trip (I7); it is derived from state rather
than from a stream, so it costs nothing in schedule independence.

This is what makes I1 and I6 hold under rayon: no cell's randomness depends on when it was
scheduled relative to any other cell.

---

## 12. Tick order

Each tick, in fixed order:

1. **Sense** — parallel, read-only over world state. Populate sensor outputs.
2. **Execute** — parallel over cells. Each cell runs its instruction budget and emits an
   `Intent` list (move impulse, eat, emit, build, tear, join, leave, transfer, inject,
   split, attack). No cell writes shared state.
3. **Resolve** — deterministic application of intents, sorted by cell id, with spatial
   partitioning where safe. Contested resources (a chemical two cells both ate) are
   allocated in cell-id order. Conflicts must never be resolved by iteration order of a
   hash map.
4. **Physics** — Brownian jitter, cilia thrust, drag, integration, junction constraint
   solve, collision resolution.
5. **Fluid** — at `fluid_hz`: diffusion, advection, light propagation, decay.
6. **Bookkeeping** — deaths, corpse deposition, births finalised, species assignment,
   metrics accumulation, event detection, state hash update.

---

## 13. Instrumentation

The entropy story requires it to be measurable, not merely narrated. Exported continuously
by `mm-cli` as newline-delimited JSON, and rendered as live plots in `mm-app`:

- **Free-energy dissipation rate** — energy converted to heat per tick, globally and per
  capita. This is the most direct statement of "life as a dissipative structure".
- **Spatial Shannon entropy of the chemical field** — per species and aggregate, computed on
  a coarsened grid. Expect it to *fall* where life organises matter, at the cost of raised
  dissipation.
- **Genomic information** — mean genome length, mean pairwise fingerprint distance,
  population-level Shannon entropy over species abundances.
- **Organisational complexity** — count of distinct organelle configurations present;
  distribution of connected-component sizes.
- **Ecology** — population, births, deaths, mean age, mean energy, trophic composition
  (fraction of matter income from light vs from carrion, plus the guild census).

  Amended at M8. This originally said "energy income from light vs predation vs scavenging",
  which describes an engine with a direct predation-to-predator flow. There is no such flow
  and there deliberately is not one: a spike does damage, damage kills, death makes carrion,
  and a lysosome digests carrion. So predation is measured as damage dealt and as the carrion
  it produces, and the only route by which a kill reaches anything living is scavenging —
  which a predator has to acquire separately if it wants to eat what it killed. Reporting a
  "predation income" would mean inventing a number for a mechanism the engine does not have.

---

## 14. The front-end

`mm-app`, Bevy. This is a fishtank and must be beautiful.

**Microscope.** The substrate is presented as a slide plate under a microscope: circular
vignette, subtle depth-of-field falling off from the focal plane, faint chromatic aberration
at the edge of the field, dust motes. Continuous zoom from whole-slide down to a single
cell, with LOD: instanced dots at far zoom, organelle-resolved sprites near, full membrane
and junction rendering at maximum zoom. Chemical fields render as toggleable false-colour
overlays using each chemical's configured colour; light as a warm luminance layer.

**Tools.** Tweezers to pick out and isolate cells, drop them onto a fresh slide, or copy
their genome to the editor. Barrier drawing on the substrate grid (blocks cells and fluid).
Slide save/load for whole simulations. Cell inspector showing live registers, stack,
organelle slots, internal chemistry, junctions and current species.

**Editor.** Embedded via `bevy_egui`: `.mm` assembly with syntax highlighting, assembler
diagnostics with source positions, disassembly of any live cell's genome with a source map
where available, breakpoints, single-step, watch panes over stack/registers/RAM/organelles,
and live injection of an edited genome into a selected cell.

**Wiki and tree.** Navigable phylogenetic tree with population-over-time ribbons, species
pages as described in §10.5, and a scrubbable world timeline annotated with
first-occurrence events and mass extinctions.

---

## 15. Deferred, but not precluded

Networking — teleporter pads at the slide edge that ship cells between running simulations,
for a larger shared substrate or a competitive battleground — is **not in scope**.

It remains possible only because I1 (determinism) and I7 (serialisable state) are enforced
from the first commit. Both are non-negotiable for that reason alone.

---

## 16. Configuration

Scenarios are `.ron` files with the full parameter set: substrate dimensions, chemical
table, light regime, fluid rates, energy costs, mutation rates, `instr_per_tick`,
`promoter_bind_threshold`, `template_search_range`, junction costs and `join_forced_penalty`,
`speciation_threshold`, seed, initial population and their genomes.

The ISA version is stamped into every save file, scenario and archived genome. Changing the
opcode table changes the meaning of every stored genome, so archived species must be
replayed under the ISA version they evolved in.

---

## 17. Planned: what sets the carrying capacity

Normative once implemented; recorded here first so the pieces are designed together rather
than bolted on one at a time. Everything in this section exists to answer one question — what
stops a slide filling up — with something more interesting than *area*.

At the time of writing, area is the whole answer. A slide of a given size holds a given number
of cells, every square is as good as every other, and the only thing a lineage can do about a
full world is wait for someone to die. That is a poor substrate for open-endedness: it selects
for growth rate and nothing else, because there is no second axis on which to be better.

### 17.1 Barriers as habitat, not decoration

Substrate squares already carry a `blocked` flag (§7.4) and the fluid already respects it —
flux across a blocked edge is zero, which is what makes barriers conserve matter for free.
What is missing is any reason to have them.

Barriers break a slide into rooms joined by channels. That does three things a uniform slide
cannot:

- **It reduces usable area without reducing the slide**, which is a carrying-capacity dial
  that does not change the scale of anything else.
- **It creates isolation.** Two rooms joined by a narrow channel are two populations with
  limited gene flow, which is the classic condition for allopatric speciation. The phylogeny
  (§10) already records what happens; it has never been given a world where it can happen
  properly.
- **It creates edges.** A cell against a wall has fewer neighbours pressing on it than one in
  open water, so `split_pressure` (§6) makes the perimeter of a room a better place to live
  than its middle. That is a spatial gradient in fitness that nobody had to write down.

Scenarios describe barriers as shapes rather than as bitmaps: rectangles, channels, and a
sparse-noise "reef" generator. A bitmap is a serialisation detail, not a way to author a world.

### 17.2 Light and nutrient as separate limiters

Today light is the only real input and it is uniform. Two changes:

- **Light becomes spatially structured** — the regimes of §7.3 already allow it, and nothing
  uses the gradient ones by default. A slide where the top half is lit and the bottom is not is
  a slide with two trades, not one.
- **Nutrient influx becomes a scenario input in its own right**, injected at points or edges
  rather than seeded uniformly at tick zero. A world whose matter arrives continuously at a
  *place* is a world with somewhere worth being, and matter that arrives must also be able to
  leave, or the ledger's conservation becomes a slow flood.

The two limiters must be genuinely independent, or they collapse into one. A cell that has
light but no carbon and a cell that has carbon but no light should both be stuck, and stuck in
ways that select for different answers — one for motility, one for storage.

### 17.3 The vent scenario

No light anywhere. Heat and reduced chemistry pumped in at one or more points on the floor,
with a prescribed flow carrying the plume. Everything alive is downstream of a vent, and the
plume is the habitat.

This is the sharpest test of whether the chemistry is really data-driven. There is no
photosynthesis to fall back on, so the whole food web has to start from a chemical gradient —
chemosynthesis is a pathway (§7.2) and nothing in the engine should need to know it is special.
If the vent scenario requires an engine change, the chemistry was not general.

Flow matters more here than anywhere else: the plume's reach is a scenario parameter, and it
decides how much of the slide is habitable. That makes `CurrentField` a carrying-capacity
control rather than set dressing.

### 17.4 Particulates

Solid matter suspended in a square: it does not diffuse the way a dissolved chemical does, it
breaks down slowly into the dissolved form, and it can be **ingested directly** by a cell
standing in it.

The precedent is already here and works: carrion (§7.2, `ecology::CARRION`) is a chemical, not
an object, with a very low diffusion rate and a slow decay into ordinary waste. Particulates
generalise exactly that, and for the same reason — the substrate already diffuses, decays and
conserves every chemical exactly, so anything expressed as a chemical inherits all of it, and
inherits it correctly.

What is new is **direct ingestion**. A cell currently eats by absorbing from the fluid in its
own square, which means everything edible must first dissolve. Particulate feeding is a second
route: slower per unit, available only where the particulate physically is, and not shared with
every other cell in the square the way a dissolved pool is. That difference is the point. A
dissolved resource is a commons; a particulate is a thing you can be standing on.

`ChemicalDef` grows a `particulate: bool` — or more precisely a settling rate and a breakdown
rate, since "solid" is a behaviour and not a category.

### 17.5 Lysis: turning cells into particulate

A cell can already wound another with a spike, and a cell that dies becomes carrion. What it
cannot do is *break another cell down* — attack the body rather than the membrane, and get
matter out of it directly.

Lysis is that: an organelle that converts an adjacent cell's structural mass into particulate
in the square, whether that cell is alive or dead. Two consequences worth stating plainly:

- **It shortens the food chain.** Today a predator kills, waits for the corpse to become
  carrion, and then digests it with a lysosome — three steps and two lossy conversions. Lysis
  makes flesh into food in one, which changes the economics of predation enough that it may
  become worth doing.
- **It applies to the living.** A cell being lysed is losing mass while alive, which is a
  pressure with no current equivalent: not damage, which is repairable, but *theft*.

Efficiency must stay below one, as digestion's already is (§7.2). A corpse must never be worth
more than the cell that made it, or the food web runs backwards.

### 17.6 The sessile filter feeder, which is the point

Take the pieces together rather than one at a time. Barriers make channels; a prescribed flow
pushes water through them; particulates ride that flow because they are matter in the fluid and
the fluid advects. Now a cell anchored in a channel has food arriving past it without having to
go anywhere, and a cell that can hold position and intercept particulate outcompetes one that
swims about looking for a dissolved pool.

That is a sponge, and nothing in the engine will know it. It needs junctions to anchor with
(§8), a flow to sit in, a particulate to catch, and a wall to sit against — four features none
of which mention filter feeding. It is the sharpest available test of the design rule in
CLAUDE.md that there are no special-cased organisms: if a sessile filter feeder needs a
`sessile` flag, the mechanism was wrong.

It is also a strategy that cannot exist under the current limiter. In a uniform slide with a
dissolved commons there is no advantage to being still, so nothing is ever still. Motility is
currently free of any opportunity cost, and a world where staying put can win is a world with
somewhere new for evolution to go.

### 17.7 The energy sink, without which none of it works

A cluster that cannot move or grow is a closed system with matter and energy still arriving. It
needs somewhere for them to go, and at the time of writing there is nowhere: stored energy has
no ceiling, upkeep is a per-cell floor plus a per-organelle charge, and neither scales with what
a cell is holding or how hard it is being squeezed.

*Written before it was measured, and half of it is wrong — there is a sink, and it is large, and
it does not help. See "The tax that is already being paid" below, which is the finding that
matters most in this section.*

This was found by trying to do without it. Two mechanisms were built and measured and both
failed for the same reason:

- **Refusing to grow** bounds occupancy but cannot reduce it. A pack that overshot while there
  was room stays overshot — twenty thousand ticks with births off left the worst pair at 92.3%
  against a 95% floor, and occupancy fixed at 116%. Converged, and too dense.
- **Shrinking under pressure** removes area, and made things worse. Cells gave mass back to
  their cytoplasm, the room that freed was taken by new cells, and the slide went from 85 cells
  at 116% to 121 cells at 122%, with the worst pair falling from 86% to 82%. Size became
  population. Nothing left the system, so nothing improved.

Both are the same lesson: with no sink, every mechanism that relieves crowding converts it into
another form of crowding.

#### The tax that is already being paid

The paragraph above says there is nowhere for energy to go. That is wrong, and it was worth
finding out before building somewhere.

`Ledger::dissipate` is called on every conversion the engine runs. Photosynthesis banks half of
what it absorbs — `photosynthesis_efficiency` is `Q10_ONE / 2` — and respiration banks three
quarters of what it releases. Upkeep, repair, building, dividing, junction transfer, cilium
thrust and death all dissipate too. Free energy leaves this world as heat on every conversion,
exactly as §7.3 says it does, and `gradient_probe::energy_budget` measures the rate: over four
thousand ticks of a growth slide, `energy_out` is **72.75%** of `energy_in`.

It changes nothing. On the same run, with the population flat at 88–90 cells from tick 1800, the
median cell's holdings go on climbing — 1,170 energy units at tick 1800, 4,304 at tick 4000 —
and stored energy across the world reaches 447 million with net income still positive.

The reason is structural, and it is the reason no rate of tax will do. Holdings obey
`dS/dt = (1 − τ)·I − U`, where `τ` is the fraction taxed and both income `I` and upkeep `U`
scale with population and throughput. Raising `τ` scales how fast `S` climbs. It does not put a
fixed point in `S`, because nothing in the expression depends on `S`. **Only a term that scales
with holdings can bound holdings**, and there is no such term anywhere in the engine: upkeep is
`metabolic_floor` plus a per-organelle charge, and both are constants of a cell's loadout.

Expressed as a buffer, the pathology is stark. `gradient_probe::energy_budget` reports what each
cell holds divided by what it spends per tick, which is how many ticks it could coast for:

| phase | tick | pop | min | p10 | p50 | p90 |
|---|---|---|---|---|---|---|
| founding | 400 | 11 | 37 | 44 | 189 | 497 |
| expanding | 800 | 43 | **1** | 12 | 290 | 1,077 |
| expanding | 1200 | 76 | **2** | 99 | 632 | 1,959 |
| converged | 2400 | 88 | 2,393 | 3,644 | 4,901 | 6,416 |
| converged | 4000 | 90 | 4,860 | 7,269 | 9,582 | 11,916 |

During the founding race the poorest cell alive is one tick from starving. In the converged pack
the poorest cell alive holds nearly five thousand ticks of upkeep. That factor of a few thousand
is the room a storage charge has to work in, and it is why a charge is safe where the cap of
experiment 1 was fatal: a rule calibrated to bite at a thousand ticks of buffer is invisible to
every cell in the top half of this table and inescapable to every cell in the bottom half.

#### Pressure cannot carry a sink, and the band recorded here was wrong

The band below was measured at `761ab7c`, before `38ff420` fixed the deep overlaps. The slide it
was taken from was 142 cells at 237% occupancy with a fifth of its pairs interpenetrating and a
worst pair 3% of the way to touching. That is not an overfilled pack, it is a pack the solver
was failing, and p50 6.1 / p90 8.1 was reading the failure. Re-measured on HEAD by
`gradient_probe::pressure_band`, with the same scenarios:

| slide | n | p10 | p50 | p90 | max |
|---|---|---|---|---|---|
| packing bench, settled (tiles perfectly) | 220 | 2.00 | 3.96 | 5.00 | 6.00 |
| growth 16, dividing (116% occupancy) | 85 | 2.00 | 3.11 | 4.80 | 5.88 |
| growth 16, converged | 84 | 2.00 | 3.45 | 5.30 | 6.00 |
| growth 32, 306 cells | 306 | 2.82 | 4.00 | 5.46 | 6.49 |

**The overfilled slide reads lower than the healthy bench.** There is no band between them —
they are the same distribution. This is not a calibration that needs redoing; it is what
`pressure` measures. Each contact contributes `clamp((want − d) / (want − core), 0, Q10_ONE)`,
which saturates at one unit the moment a pair reaches its core, and the sum over contacts is
therefore close to a count of neighbours that are touching at all. It tops out near six because
six is how many neighbours fit around a disc. It cannot tell a crowd resting on each other from
a crowd crushed into each other, because both saturate — and the bench beats the overfilled
slide on it for the mundane reason that its cells are smaller and so each has more neighbours.

Upkeep that scales with pressure is therefore struck off, and the reasoning that recommended it
is struck off with it. Charged against the numbers above, it would bill the healthy pack more
than the overfilled one. It cannot be rescued by moving the threshold. It could be rescued by
redefining `pressure` to something unsaturating — total core penetration in `POS`, say, or the
`crowding` sum that already exists beside it — but that is a change to the division and growth
gates that currently read it, and those gates work.

**Upkeep that scales with what is stored** is what survives. Holding matter should cost,
continuously, or a full cytoplasm is a free battery. It is also the only proposal that connects
the quantity with no bound to the sink that does exist: matter is conserved and can never leave,
but energy leaves as heat on every conversion, so a rule that makes holding matter *cost energy*
drains a conserved pool into an unconserved one. That is the thermodynamics the rest of the
engine already runs on.

This is the tax that makes a vacuole worth building: at present the interior cap is
per-chemical, unenforced on conversion, and total free solute is not bounded by anything at all.
`gradient_probe::solute_and_shade` measures it as a multiple of the per-chemical cap — 2.3× at
the founding, 9.7× at convergence on the 16 slide and 11.6× on the 32, peaking at 28× — and not
one cell in any run has ever built a vacuole. Storage as a *strategy* requires storage to be
rationed and expensive; today it is neither.

Rupture — damage rising steeply with pressure until a cell bursts — belongs after these, not
instead of them. It is a ceiling, and a ceiling on top of an economy with no sink only decides
who dies of a problem nobody could have avoided. It also cannot be built on `pressure` as
`pressure` is currently defined, for the reason above.

### 17.8 The pack has no inside

§17.7 is about the economy. This is about the geometry, and it is the other half of why a dense
pack has no structure: a cell wedged in the middle of one lives in exactly the same medium as a
cell on the rind, to the last digit.

In a real biofilm or tumour, nutrients diffuse in from the surface and waste diffuses out, both
distance-limited, so past a couple of hundred microns there is an anoxic acidic core. The pack
does not rupture, it stratifies: growing rind, dormant middle, dead core. That structure is what
gives motility a reason to exist and what makes the interior of a crowd a bad place to be.

Three measurements say it cannot happen here.

**Nothing in the fluid knows the population exists.** `fluid::step` takes a `Substrate`, a rate
table and a scratch buffer, and no `CellArena`. `blocked` is set by scenario barriers and the
user's drawing tool and by nothing else. There is no porosity term, no occupancy-weighted flux,
no volume exclusion. `gradient_probe::tracer_through_the_pack` releases an identical tracer spike
into the middle of a 306-cell pack and into the same square of the same slide with every cell
removed, and reads the radial profile at eight radii over 120 ticks: the two are **bit-identical
at every ring and every timestep**. A hundred and seventy-one cells with five or more contacts
each retard transport by exactly nothing.

**Light is not rival either.** The light plane is prescribed from a closed form every fluid step
and re-prescribed the next; a chloroplast reads the value at its centre square and does not
decrement it, so nothing shades anything but a barrier, and absorption is not competed for.

**So no gradient forms**, and `gradient_probe::rind_to_core` confirms it directly. On a 32 slide
grown to 306 cells, ring by ring out from the pack's centroid, the oxidant varies by 0.02% and
the structural monomer by 0.3%; the largest variation in any species is the waste at 30%, and it
peaks in the *middle* rings rather than the core, against a pool three orders of magnitude
larger. Grouping cells by how many neighbours they touch rather than by radius gives the same
answer and adds a worse one:

| contacts | n | mass | energy | fill | pressure |
|---|---|---|---|---|---|
| 0–2 (rind) | 2 | 15.9 | 2,893 | 633% | 2.00 |
| 3–4 | 133 | 35.3 | 3,394 | 823% | 3.28 |
| 5–6 | 158 | 62.7 | 3,988 | 1,105% | 4.67 |
| 7+ (core) | 13 | 77.3 | 4,237 | 1,295% | 5.45 |

**Being buried is currently the best place to be.** A cell in the core is heavier, richer and
fuller than one on the rind. Some of that is a cell being large *causing* it to have more
contacts rather than the reverse, but the direction is the point: there is no penalty anywhere in
it, and the local chemistry columns of that same table are flat.

The cause is a rate mismatch, not a missing mechanism. Diffusion rates run from `Q10_ONE/64` to
`Q10_ONE/4` per fluid step, and the fluid steps every tick by default, so the tracer above is
substantially spread across seven squares within roughly a hundred ticks. Metabolism changes a
pack over thousands. On the timescale a cell lives at, the fluid is a well-stirred bath. Any
mechanism that wants stratification has to close that gap: make transport slow relative to
consumption, and the honest way is to let occupancy impede it, which is the porosity term the
solver does not have.

### 17.9 What this is all for

### 17.8 What this is all for

Each of these is a carrying-capacity term, and that is deliberate. Area is a limit that selects
for one thing. Light, nutrient influx, barriers, particulate availability and predation
pressure are five limits that select for five things, and a world with five limits has room for
strategies that are better in different places — which is the minimum condition for the
radiations the timeline (§10) exists to report.
