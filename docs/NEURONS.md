# Every cell is already a neuron, and none of them can find each other

A guide rather than an investigation, though it is written out of one. `genomes/reflex.mm` is a
nerve net built from pieces that were all already here, and `reflex_probe` is what happened when
it was run. This says what to build next, in what order, and what not to build at all.

Short version: **the synapse works, the shipped chemistry makes it pointless, and one missing
`OGET` reading makes it impossible to aim.** Nothing on the list below needs a catalogue slot or
an opcode, and the largest item is a scenario file.

---

## 1. What is already here

Worth stating plainly, because the instinct is to add a neuron and the instinct is wrong.

**A cell is a better neuron than a neuron.** It has persistent state (RAM and registers), inputs
(five sensor types, four junctions, sixteen interior chemicals), a program, and outputs. A
McCulloch–Pitts unit is a weighted sum and a threshold; a cell can compute anything the VM can.

**The synapse exists and the spec already calls it that.** `junction.rs` opens with "It is the
conjugation channel, the synapse and the infection route, all the same mechanism." `JXFER` moves
a chemical or energy to the cell on the other end of a junction, in one tick, at a cost per unit
moved. `reflex_probe::the_synapse_moves_matter_exactly` asserts it: four units a tick, exactly
conserved.

**Summation is free, and nobody designed it.** Several presynaptic cells pushing into the same
chemical arrive in one interior pool, so the receiving cell reads a number that is already the
weighted sum. **The cytoplasm is the dendritic integrator.** The weight is how much each sender
chooses to push — one genome immediate, so a single bit flip retunes a synapse without breaking
the program.

**There are two signalling routes, not one.** Wired (`JXFER`, directed, one tick) and broadcast
(`EMIT` into the square, read by a neighbour's chemosensor, carried by diffusion). A primitive
nervous system probably wants both. §2 is about when the first beats the second.

---

## 2. A nervous system needs a transmitter that does not diffuse

The first measurement, and the one that decides whether any of this is worth doing.
`reflex_probe::where_the_wire_starts_to_pay` sweeps `signal_a`'s diffusion down by halves and
times how long the far end of a three-cell chain takes to reach half thrust:

| `signal_a` diffusion | wired | control | what the wire bought |
| --- | ---: | ---: | --- |
| **Q10/4 — what ships** | 21 | 21 | **nothing** |
| Q10/8 | 29 | 37 | 8 ticks |
| Q10/16 | 29 | 53 | 24 ticks |
| **Q10/32 — as slow as detritus** | 29 | 101 | **72 ticks** |
| Q10/64 | 29 | 165 | 136 ticks |
| Q10/128 | 29 | 213 | 184 ticks |
| Q10/256 | 29 | 277 | 248 ticks |
| Q10/512 | 29 | 341 | 312 ticks |
| Q10/1024 | 29 | never | the response itself |
| none | 29 | never | the response itself |

> **The wired column is flat.** Twenty-nine ticks at every rate from `Q10/8` to zero, while the
> diffusive column scales as roughly `1/D` and then stops arriving at all.

That is the entire case for a nervous system, in one table: *conduction time is independent of
the chemistry and diffusion time is not.* And the shipped table sits on the one rate where the
difference is zero — `signal_a` diffuses at `Q10/4`, which is `MAX_DIFFUSION`, the fastest the
engine allows. Three cells two squares apart are one puddle on the timescale a genome runs at.

The same matter enters the chain in both arms down to `Q10/512`, so this is not a dosing
artefact. Below that the stimulus stops spreading far enough for the head cell to take up as
much, and the totals part company — which is the floor on how slow a transmitter can usefully be.

**Nothing has to be invented.** Detritus already sits at `Q10/32`, and a transmitter that slow is
worth 72 ticks. What is missing is a scenario that authors one.

---

## 3. The one thing that is actually blocking

`reflex_probe::whether_the_genome_can_wire_itself` takes the harness away. With hard junctions
the genome **does** build a connected chain unaided — `X... XX.. X...`, one link on each end cell
and two on the middle — and it conducts nothing at all.

`resolve_join` gives each end its **lowest free slot**. So the inbound junction lands in slot 0,
which is the first slot the genome transmits into, and every cell sends the signal back the way
it came before it sends it onward. The middle cell empties towards the head. The net is connected
and electrically sterile.

> A genome cannot choose a junction slot, cannot read which slot a junction landed in, and cannot
> tell a link it made from a link that was made on it. **So it cannot build a directed arc, and
> an undirected one does not conduct.**

This is not a subtle problem and it is not expensive to fix. `JunctionPort` is catalogue slot 10.
It is built, priced, carried — and **read by nothing**: it has no arm in `biology::oget`, falls
through to `sensing::read_sensor`, which returns `None` for it, so `oget` answers zero for every
index. It is also not required in order to `JOIN`; the four junction slots are per-cell state
independent of the loadout.

The same silence breaks a shipped genome. `parasite.mm`'s `#infect` reads exactly this port and
branches on it — `JMPNZ connected` — so its second state has never been reachable and it has
never injected anything into a host. `JOIN`'s own comment says "the genome finds out by looking".
There is nothing to look at.

### The design

`control[0]` on the port selects which junction slot it reports on, reduced modulo
`JUNCTIONS_PER_CELL` so every value a genome can write names a real slot (hard rule 4). The
reading index selects what:

| index | reading |
| ---: | --- |
| 0 | how many junctions this cell holds — what `parasite.mm` already assumes |
| 1 | whether the selected slot is occupied |
| 2 | its kind: 0 soft, 1 hard |
| 3 | **whether this cell initiated it**, or was joined by the other end |
| 4 | the far cell's **badge** |
| 5 | its current length, so `JLEN` has feedback |

Two of those are load-bearing and the rest are conveniences.

**Reading 3 is the minimum fix.** A junction is symmetric in physics, but the *act* of joining is
not: one cell reached out and the other was reached. That is a historical fact the engine already
knows at join time and immediately throws away, and it is the difference between an axon and a
dendrite. Recording it costs one bit on `Junction` — which is `{kind, other, rest}` and has
padding to spare — plus the snapshot field that hard rule 7 requires in the same commit.

**Reading 4 is what makes a real network possible**, and it is the one that keeps the design rule
intact. The engine must never know what a friend is (SPEC §8.2), so it reports a number and says
nothing about what the number means. A lineage that differentiates its badge along a body axis
can then have each cell transmit only into junctions whose far end wears a higher badge — a
gradient rule written entirely in the genome, with no engine concept of direction anywhere.

**No catalogue slot. No opcode.** It fills in an organelle that already exists and currently lies.

---

## 4. Two geometry defects found on the way

Both were found by `reflex.mm` failing to assemble itself, and both are older and wider than
nervous systems.

### A cell can feel much further than it can join

`neighbours::feel` counts a contact at `rj + 3·ri`. `junction::reach` is `2·ri + 1`. For equal
radii those are `4r` and `2r + 1`, which agree only at **r = 0.5 squares** and diverge linearly
above it. Every cell in every run is above it.

`junction::reach`'s own documentation says:

> Its own radius plus the target's, plus a margin — the same "touching" test the touch sensor
> uses, **so a genome that can feel a neighbour can join it.**

Measured at the size these genomes settle at — radius 1.50 squares — a cell feels out to 6.00,
joins out to 4.00, and its nearest neighbour sits at 4.28. It feels a neighbour it cannot join,
which is precisely what `#wire` was written against.

**Recommendation: make `reach` match `feel`, and measure what it costs.** The comment states the
intent and the intent is right; the arithmetic drifted from it. Widening join reach to `rj + 3·ri`
lets cells join from further away, which touches every M7 result, so it wants its own run rather
than being folded into this work.

### A soft junction cannot span two grown cells

`soft_max_range` is an absolute **3 squares**. Two tangent cells of radius 1.5 are 3.00 apart
before any drift at all, and separation plus jitter holds them further. So the channel SPEC §8.1
calls the synapse breaks the tick it is formed, between any two adults.

`reflex.mm` uses **hard** junctions instead, and gets away with it because `resolve_transfer`
never checks the kind — a strut carries signal. That is lucky rather than designed, and it is the
only reason a nerve net is expressible at all.

**Recommendation: make `soft_max_range` relative**, as hard junctions' `breaking_strain` already
is — it is measured against the rest length rather than being an absolute. A soft junction that
breaks at `rest + slack` scales with the cells on either end, and a soft junction between two
newborns and one between two adults stop being different mechanisms.

---

## 5. Three constraints to design around rather than fix

**Conduction velocity is a function of birth order.** Resolve applies every intent in one pass in
slot order, so a chain whose cells sit in ascending slot order propagates end to end in a *single
tick*, and the same chain in descending order takes one tick a hop. In the zero-diffusion run the
middle cell reads zero transmitter and zero thrust throughout while the tail behind it climbs to
full — it conducts without ever being excited. Nothing about this is non-deterministic (slot
order is id order; I6 is intact), but it is not a property anybody chose. **Any latency
measurement on a network has to control for slot order**, or it is measuring the arena.

**Sixteen instructions a tick, and that is a feature.** A synapse is five instructions — `IMM`,
`IMM`, slot, `JXFER`, `DROP` — or 31% of a tick, and `reflex.mm` walks its whole program every
eight ticks. No cell can be both a competent metabolist and a competent neuron in that budget. So
the instruction budget is a pressure *towards* differentiation, which is the thing M7 acceptance
6 has never managed to demonstrate. Raising `instr_per_tick` is a scenario edit — it lives in
`Scenario::vm` and is inside the state hash — and it should be tried before it is assumed to be
the problem.

**Four junction slots, shared between structure and signal.** A directed chain of three already
spends two on its middle cell, and a cell with two inputs and two outputs is full before it has a
body to hold together. Raising `JUNCTIONS_PER_CELL` is per-cell state (hard rule 7) and doubles
the junction solve against M7's 5%-of-tick gate. Do not raise it until §3 lands and a genome can
actually use the four it has.

---

## 6. The obstacle nobody was looking for

A specialised cell is a cell that has stopped dividing, and **a cell that has stopped dividing
dies of turgor.**

`reflex_probe::an_undivided_cell_dies_of_turgor` measured it, and it took three runs of the main
experiment to notice because the probe was settling for 1,500 ticks and reading a body in the last
quarter of its life. On the ancestor's diet, `reflex.mm` accumulates solute linearly to seventeen
interior capacities against a turgor threshold of four, the quadratic charge takes hold around
twelve, and the cell is dead by tick 1,800. The ancestor is on the same curve — thirteen
capacities and climbing at tick 2,000 — and survives only because dividing sheds solute.

The genome was fixed dietarily: eat a quarter as much carbon, and excrete the oxidant that
photosynthesis *makes* and the ancestor eats anyway. That holds it at one capacity and 2,107
energy indefinitely. But the general problem stands:

> **Terminal differentiation is currently lethal.** Any cell type that gives up division has to
> discover an excretion strategy first, and nothing rewards it for doing so.

This is `docs/STIFFNESS.md` §3 arriving from a direction it did not anticipate — the quadratic
turgor charge is the largest thing in a settled cell's budget, it buys nothing, and here it is
also a barrier to multicellularity. The two documents want reading together.

---

## 7. What not to build

**A neuron organelle.** It would be redundant with the VM — every cell already has a program that
is strictly more capable than an integrate-and-fire unit — and it would be the cell-type enum by
the back door, which CLAUDE.md names in exactly these words: "Skin, muscle and neuron are labels
the analysis layer infers from organelle loadouts."

**Any engine notion of a network.** An organism is a connected component and a nervous system
should be nothing more than what some connected components happen to do. If a nerve net needs a
`network` flag, the mechanism is wrong.

**A `JXFER` that can pull.** It is push-only, and that is what makes the binding key coherent: a
channel that could take would turn a forced join from an intrusion into a robbery. The engine's
answer to a suctorian is a parasite that `INJECT`s code making its host donate, which is better
design than a suction opcode and is available today.

**Fan-out primitives on the last opcode.** `Reserved1` is the only free opcode there will ever
be. A broadcast-to-all-junctions instruction saves nine instructions of a four-way fan-out, which
is real but is not what is blocking anything. Spend it on something that cannot be done at all.

---

## 8. The order to do it in

1. **Junction port readings** (§3). The blocker. One bit on `Junction`, six readings on an
   organelle that already exists, the snapshot field rule 7 asks for in the same commit. An
   organelle-catalogue semantics change, so ISA 5 → 6 — and `docs/FEEDING.md` §6 and
   `docs/STIFFNESS.md` §7 both want a bump too, so **make it one bump**.

2. **Fix `parasite.mm`** in the same pass, since it has been branching on this reading since M7
   and has never once reached its second state. Then re-run `predator_probe`'s arena match, whose
   conclusion about infection being slower than division was drawn against a parasite that was
   not infecting.

3. **A slow-transmitter scenario** (§2). `Q10/32` is a defensible starting value — as slow as
   detritus, worth 72 ticks — and the floor is around `Q10/512`, below which the stimulus stops
   spreading far enough to be taken up. Author it as a scenario rather than changing the default
   table: `signal_a`'s rate is what every existing run has had, and this is the first thing that
   would notice a change.

4. **Re-run `reflex_probe` end to end.** With 1 and 3 in place, `whether_the_genome_can_wire_itself`
   should conduct. If it does not, the finding is which of §5's three constraints bit — and that
   is a better outcome than a guess.

5. **Then, and only then, the geometry** (§4): `reach` to match `feel`, and `soft_max_range` to be
   relative. Both touch every M7 result and both deserve their own runs.

6. **Leave `JUNCTIONS_PER_CELL` alone** until something has used four properly.

---

## 9. What this did not settle

- **Whether a nerve net ever pays for itself.** Everything above is about whether one is
  *expressible*. Nothing here shows a lineage that is better off for having one, and in a uniform
  soup with a dissolved commons the optimal behaviour is to sit still and photosynthesise. A brain
  is pure cost in a world with nothing to compute — which is `docs/FEEDING.md` §8's argument and
  SPEC §17.9's, arrived at independently. **Neurons need the ecology before they need the
  mechanism.**
- **Whether differentiation happens at all.** M7 acceptance 6 is `#[ignore]`d and diagnostic by
  its own milestone's words. `reflex.mm` is deliberately a nerve net rather than a brain — every
  cell does the same three things — precisely so that the signalling result does not depend on it.
- **Whether badge-gradient wiring works.** §3's reading 4 is designed and unbuilt, and the
  differentiation it depends on is the previous point.
- **What the promoter collision costs.** `#wire` and `#grow` in `reflex.mm` hash exactly two apart
  at a bind threshold of two. Exact references still bind their own gene, so it costs nothing
  today and something the first time a promoter mutates. Recorded as a test rather than designed
  around; a genome that survives its own promoters drifting is the interesting case.
