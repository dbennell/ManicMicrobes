# Manic Microbes — Milestones

Each milestone is a vertical slice with an explicit definition of done. A milestone is
complete when **every acceptance test passes and the performance gate is met**, not when the
code looks finished. Acceptance tests live in `tests/` and run in CI forever after; a later
milestone may not break an earlier milestone's tests.

Read `docs/SPEC.md` before starting any milestone. Where this plan and the spec disagree,
the spec wins.

Ordering note: the microscope lands at **M4**, before phylogeny and before multicellularity.
This is deliberate. The project is a fishtank as much as an instrument, and evolutionary
work is much harder to steer when you cannot see what the population is doing. Every
milestone from M2 onward must remain visually inspectable.

---

## The current working target

**50,000 cells at 30fps, interactive, in `mm-app`.** One number, end to end: the slide open,
the panels drawn, the world running at 1×.

This **supersedes every per-milestone performance gate below** until it is met and reviewed.
The gates are not deleted — they are the ship targets and they stay written down — but a
milestone is not held open on one while the working target is in force. They are marked
*(ship target, suspended)* where that applies.

Why one number instead of seven: the gates were written per-milestone and measure different
things on different axes — headless ticks/second here, rendered frames/second there — and the
figure a person actually feels is neither of them. It is whether the window responds while
fifty thousand cells are alive in it. Tuning to seven abstractions while the thing in front of
you stutters is how a benchmark suite ends up green on a program nobody can use.

The working target decomposes into two halves that must be measured separately, because they
are owned by different milestones and they fail differently:

| half | requirement at 50,000 cells | owner |
| --- | --- | --- |
| simulation | ≥ 30 ticks/second, headless, 8 cores | M9 |
| render | ≥ 30 frames/second at whole-slide zoom | M10 |

They can only be measured separately once the simulation is off the render thread, which it is
not: `advance_simulation` and `redraw` are chained in the same `Update` schedule, so at 1× the
tick rate *is* the frame rate and a slow tick is indistinguishable from a slow frame. Splitting
them is the first deliverable of M10 and is a prerequisite for believing any figure here.

Review this section when both halves are met, or when the shape of the work changes enough
that one number stops being the honest summary.

---

## M0 — Core VM and toolchain

**Goal:** a total, deterministic virtual machine and the assembler to write for it. No
world, no cells, no rendering.

**Deliverables**
- Cargo workspace; `mm-core` with no Bevy dependency and no floats.
- `Genome` type: `Arc`-shared, content-hashed, interned, copy-on-write.
- VM: 64 opcodes per SPEC §5.1, circular stacks, wrapping addressing, saturating arithmetic.
- Template scanning, complementary jump search, `EXPRESS` promoter binding by Hamming
  distance.
- `mm-asm`: assembler with labels compiling to templates, named promoters compiling to bit
  patterns via stable hash, disassembler, source maps.
- Deterministic hash-based randomness (SPEC §11).
- Rolling world-state hash function (trivial at this stage; the interface matters).

**Acceptance tests**
1. **Totality fuzz.** 10,000,000 random byte arrays of length 1–4096, each executed for
    100,000 instructions from randomised initial VM state. Zero panics, zero hangs, zero
    aborts. This test is the foundation of everything and must never be weakened.
2. **Determinism.** Identical seed and program produce identical VM state after 1,000,000
    instructions, across 3 runs and across debug and release builds.
3. **Round-trip.** Assemble → disassemble → reassemble is byte-identical for every genome in
    `genomes/`.
4. **Degenerate encoding.** For all `b` in 0..=255, opcode dispatch equals `b % 64`.
5. **Saturation and wrapping.** Property test: arithmetic never wraps; all index operations
    are in range for arbitrary inputs.

**Performance gate:** ≥ 50M instructions/second, single core, release build.

**Out of scope:** anything with a position.

---

## M1 — Substrate, fluid and chemistry

**Goal:** a world that conserves matter exactly and accounts for energy exactly, with
nothing alive in it.

**Deliverables**
- Grid substrate with per-square: 16 chemical quantities, light, velocity, `blocked` flag.
- Flux-based integer diffusion and donor-cell upwind advection (SPEC §7.4).
- Prescribed velocity field with configurable currents; local impulse injection API.
- Light regimes: uniform, day/night, directional gradient, point source, slow decline.
- Data-driven `ChemicalDef` table loaded from scenario `.ron`.
- Global accounting: per-species totals, `energy_in`, `energy_out`, `energy_stored`.
- Rayon parallelisation with checkerboard phasing.

**Acceptance tests**
1. **Exact matter conservation.** 1,000,000 ticks with aggressive stirring, steep initial
   gradients and barriers. Per-species totals drift by exactly zero. Not "within epsilon" —
   zero.
2. **Energy accounting.** `energy_in == energy_out + Δenergy_stored` exactly, every tick,
   over 1,000,000 ticks.
3. **Schedule independence.** Identical state hash at 100,000 ticks with 1, 2, 4 and 16
   rayon threads.
4. **Barriers are impermeable.** No chemical crosses a `blocked` square, ever.
5. **Serialisation round-trip.** Save at tick 50,000, reload, run to 100,000; state hash
   matches an uninterrupted run.

**Performance gate:** 512×512 grid, 16 chemicals, ≥ 500 fluid steps/second on 8 cores.

---

## M2 — Cells, metabolism, division, mutation

**Goal:** the first thing that is alive. A hand-written ancestor sustains a population
indefinitely.

**Deliverables**
- SoA cell arena with generational slot-map IDs. Not the Bevy ECS.
- Organelle slots and the catalogue entries needed for a metabolising, dividing cell:
  membrane, nucleus, mitochondrion, chloroplast, vacuole, pump.
- `EAT`, `EMIT`, membrane permeability and passive transport.
- The metabolic loop of SPEC §7.2, closing matter through photosynthesis.
- `BUD` / `COPYB` / `SPLIT`; energy and matter cost of division; daughter inherits half.
- Death, corpse deposition, decay.
- Mutation: per-byte copy error under evolvable nucleus fidelity; structural operators at
  `SPLIT` including duplication.
- Intent-based tick order (SPEC §12) with deterministic conflict resolution.
- `mm-cli` headless runner with NDJSON metric export.
- **A minimal but honest viewer** in `mm-app`: cells as coloured dots, one chemical overlay,
  pan and zoom, population plot. Deliberately unstyled; it exists so no later milestone is
  developed blind.

**Acceptance tests**
1. **Population persistence.** A hand-written ancestor sustains population > 0 for 1,000,000
   ticks across 10 seeds, with mutation on.
2. **Conservation under life.** M1's exact matter conservation still holds with 10,000 cells
   eating, building, dividing and dying.
3. **Selection works.** Seed two ancestors differing only in metabolic efficiency; the more
   efficient one reaches > 90% of the population within 100,000 ticks in ≥ 9 of 10 seeds.
4. **Mutation-rate evolution.** Under a stable environment, mean nucleus copy fidelity rises
   measurably over 1,000,000 ticks. Under a fluctuating environment, it does not rise as
   fast. (Directional test, not a fixed threshold.)
5. **Determinism with life.** Identical state hash at 500,000 ticks across thread counts.

**Performance gate:** 50,000 cells at ≥ 60 ticks/second headless on 8 cores. *(Ship target,
suspended — see the working target above, which asks for 30.)*

---

## M3 — Sensing and motility

**Goal:** cells that go somewhere for a reason, and the first genuinely open-ended
evolutionary result.

**Deliverables**
- Chemosensor, photosensor, touch sensor, oscillator.
- Cilium thrust with mount angle, signed power, drag, and impulse injection into the fluid.
- Brownian jitter, collision resolution.
- Trophic accounting: energy income attributed to light vs ingestion.

**Acceptance tests**
1. **Chemotaxis evolves.** Starting from an ancestor with a chemosensor and cilia but no
   code linking them, and a patchy food distribution, mean cell-to-food distance falls
   significantly below a motile-but-blind control within 2,000,000 ticks, in ≥ 6 of 10
   seeds. This is the first real evolution test and the most important one in the plan.
2. **Arena determinism.** Two hand-written cells in a fixed scenario with mutation off
   produce identical outcomes across 100 runs.
3. **Momentum sanity.** Cilia impulses into the fluid do not create net momentum from
   nothing beyond the configured drag budget.

**Performance gate:** 50,000 cells with sensors and cilia at ≥ 45 ticks/second on 8 cores.
*(Ship target, suspended — see the working target above, which asks for 30.)*

---

## M4 — The microscope

**Goal:** the thing people actually want to look at. This is a product milestone, not a
graphics chore.

**Deliverables**
- Slide-plate presentation: circular vignette, depth-of-field falloff from the focal plane,
  faint edge chromatic aberration, drifting dust motes.
- Continuous zoom, whole-slide to single-cell, with LOD tiers: instanced points → organelle-
  resolved sprites → full membrane, organelle and junction rendering.
- Chemical field overlays in each chemical's configured colour, individually toggleable, with
  a legend. Light as a warm luminance layer.
- Cell inspector: live stack, registers, RAM, organelle slots, internal chemistry, energy,
  age, species.
- Live metric plots: population, dissipation rate, trophic composition, entropy measures.
- Simulation speed control including step, and a "run headless as fast as possible" mode
  that detaches rendering from the tick rate.

**Acceptance tests**
1. **Rendering cannot affect simulation.** State hash at 100,000 ticks is identical whether
   run through `mm-app` at 60fps or `mm-cli` headless.
2. **Frame budget.** 100,000 visible cells render at ≥ 60fps at whole-slide zoom on a
   mid-range discrete GPU. *(Ship target, suspended — the working target asks for 50,000 at
   30fps, and M10 owns reaching it.)*
3. **Decoupling.** Dropping the render to 5fps does not change tick output or ordering.
4. **Zero Bevy in core.** CI check: `mm-core` builds with Bevy absent from the dependency
   graph.

**Out of scope:** editor, tweezers, barrier drawing — those are M6.

---

## M5 — Phylogeny, speciation and the wiki

**Goal:** the simulation starts telling stories about itself.

**Deliverables**
- Parentage tracking, true tree construction, scheduled pruning of extinct branches.
- 64-bit SimHash genome fingerprints, inherited free on unmutated division.
- Species forking on fingerprint distance from founder; genus and family grouping for
  display.
- Latinate binomial name generation with trait-biased epithets.
- Species records: founding tick, parent, population curve, peak, extinction, inferred cause,
  behavioural description from organelle loadout and runtime statistics, founder genome.
- First-occurrence detectors for the full list in SPEC §10.6; mass-extinction detection.
- Wiki and phylogenetic tree UI; scrubbable annotated world timeline.
- NDJSON export of the whole species archive for offline analysis.

**Acceptance tests**
1. **Tree correctness.** For a 100,000-cell run, every cell's ancestry chain terminates at a
   founder; no cycles; no orphans.
2. **Storage bound.** 10,000,000 ticks at 100,000 cells produces < 1GB of archive. Verifies
   that per-individual records are not being retained.
3. **Speciation stability.** Species count does not oscillate — no lineage flips between two
   species assignments more than once per 10,000 ticks under a stable environment.
4. **Detector correctness.** In a scripted scenario where a known event occurs at a known
   tick, the corresponding first-occurrence detector fires within 100 ticks and not before.
5. **Fingerprint sanity.** SimHash distance correlates with true edit distance at Spearman
   ρ > 0.8 over a sampled corpus. If it does not, upgrade to MinHash per SPEC §10.2.

**Performance gate:** phylogeny and metrics cost < 5% of tick time at 100,000 cells.

---

## M6 — Editor, debugger and laboratory tools

**Goal:** the coder-vs-coder half of the product. Arena mode becomes real.

**Deliverables**
- `bevy_egui` editor: `.mm` syntax highlighting, assembler diagnostics with source positions,
  disassembly of any live genome with source map where available.
- Debugger: breakpoints, single-step, run-to-tick, watch panes over stack, registers, RAM,
  organelles and junctions, on a selected live cell.
- Live genome injection into a selected cell.
- Tweezers: select, isolate, relocate, copy genome to editor, transplant to a fresh slide.
- Barrier drawing and erasing on the substrate grid.
- Slide save/load; genome import/export as a shareable single file including ISA version.
- Arena scenario type: fixed seed, mutation off, N cells per side, defined win conditions,
  reproducible match reports.

**Acceptance tests**
1. **Match reproducibility.** An arena match replays identically from its saved scenario and
   seed, 100 times, on 2 different machines.
2. **Debugger non-interference.** Stepping and breakpoints do not change the state hash of a
   run relative to running it uninterrupted.
3. **Portability.** A genome exported on one machine loads and behaves identically on
   another.
4. **ISA guard.** Loading a genome stamped with a different ISA version produces a clear
   warning and refuses to run it silently.

---

## M7 — Junctions, structure and multicellularity

**Goal:** organisms rather than cells. This is the least settled part of the design; expect
the parameters here to need the most tuning.

**Deliverables**
- Junction ports; soft and hard junctions; `JOIN`, `LEAVE`, `JXFER`, `JLEN`, `SETKEY`,
  `INJECT`.
- The binding-key mechanic per SPEC §8.2, with `join_forced_penalty` and binary probe
  results by default.
- PBD distance constraints, 2–3 Gauss–Seidel iterations, mass-weighting, stiffness, breaking
  strain. **No fluid coupling.**
- Incremental union-find over hard junctions for connected components.
- Chemical transfer between joined cells, enabling expression gating on internal state, and
  therefore differentiation.
- Rendering of junctions, clusters and cluster-level selection.
- Cluster-aware detectors and wiki entries.

**Acceptance tests**
1. **Cheap clonal assembly.** Clonal cells sharing a receptor key form clusters at
   `join_base_cost`; non-clonal join attempts cost the full forced penalty. Verified
   directly against the energy ledger.
2. **Colony locomotion is emergent.** A hand-written 8-cell cluster with cilia on one member
   translates coherently, with no code in the engine that moves clusters as a unit.
3. **Muscle works.** A hand-written cluster modulating `JLEN` periodically produces measurable
   shape change and net displacement.
4. **Parasitism is possible and costly.** A hand-written parasite with the correct key
   infects successfully; the same parasite with a wrong key succeeds only after paying the
   penalty, and dies if under-resourced.
5. **Constraint cost.** Junction solve is < 5% of tick time at 50,000 junctions.
6. **Differentiation emerges.** In a long open-ended run, at least one cluster of size ≥ 8
   with two or more distinct organelle loadouts appears in ≥ 3 of 10 seeds. If this fails,
   the failure is diagnostic, not a blocker — record which parameter starved it.

**Performance gate:** 150,000 cells with 50,000 junctions at ≥ 30 ticks/second on 8 cores.
*(Ship target, suspended — the working target asks for 30 ticks/second at 50,000.)*

---

## M8 — Ecology, predation and scenarios

**Goal:** an ecosystem worth watching for hours.

**Deliverables**
- Spike organelle, membrane damage, cytoplasm release on rupture, lysosome digestion of
  corpses.
- Light regimes as authored scenario events: day/night, seasonal cycles, slow decline,
  vent-only worlds.
- A curated scenario library: "primordial soup", "photosynthesis or die", "predator
  introduction", "the long dusk", "archipelago" (barrier-fragmented substrate for allopatric
  speciation).
- Trophic-level analysis and food-web display.
- Balancing pass across energy costs, mutation rates and junction costs.

**Acceptance tests**
1. **Allopatric speciation.** In the archipelago scenario, populations in barrier-separated
   regions diverge into distinct species significantly faster than in a connected control.
2. **Trophic structure.** In the predator scenario, a stable predator–prey oscillation
   persists for > 1,000,000 ticks in ≥ 5 of 10 seeds.
3. **Extinction and recovery.** In "the long dusk", the population crashes and either
   recovers with a measurable shift in trophic composition or goes extinct — and the timeline
   correctly reports which.
4. **No degenerate optimum.** No scenario in the library collapses to a single strategy
   within 100,000 ticks. If one does, it is a balancing bug, not a result.

---

## M9 — Scale and hardening

**Goal:** the numbers in the spec, on real hardware, sustained.

**Deliverables**
- Memory-layout optimisation to the ≤ 512 bytes/cell fixed-state budget.
- Spatial hashing and broad-phase collision tuning.
- Genome interning statistics and cache-pressure profiling.
- Optional GPU compute path for the fluid, behind a trait, with a CPU mirror for sampling —
  gated on producing identical results to the CPU path.
- Long-run soak testing and archive compaction.
- Criterion benchmarks in CI with regression thresholds.

**Acceptance tests**
1. **Target scale.** 200,000 cells at ≥ 30 ticks/second headless on 8 cores. *(Ship target.
   The working target's simulation half — 50,000 cells at ≥ 30 ticks/second — is the interim
   figure and is the one M9 is held to first.)*
2. **Memory.** < 200MB resident for the cell arena at 200,000 cells.
3. **Soak.** 100,000,000 ticks without leak, drift, archive bloat or conservation violation.
4. **GPU parity.** If the GPU fluid path is enabled, its state hash matches the CPU path
   exactly. If exact parity is unachievable, the GPU path ships disabled by default and is
   excluded from any run whose results are recorded.
5. **No regression.** Every acceptance test from M0–M8 still passes.

---

## M10 — The instrument

**Goal:** the application around the microscope. A shell you can drive, parameters you can
author, a genome you can read, a tree of life you can look at, and cells that look like cells.

Designed in full in `docs/UI.md`, which is normative for this milestone the way `SPEC.md` is
for the simulation.

**Ordering:** M10 comes *before* M9, despite the number. Two reasons. The working target's
render half is entirely M10's, and its simulation half cannot be measured at all until M10.1
puts the simulation on its own thread — profiling M9 against a figure that is currently the
frame rate in disguise would tune the wrong thing. And the balancing work the simulation needs
is done by watching, which is what this milestone is for.

**Deliverables**
- **M10.1 — shell and input.** Menu bar; docked layout with a known central viewport rect;
  pointer and keyboard routed by which region owns them. Simulation moved onto its own thread
  with a double-buffered frame handoff.
- **M10.2 — configuration.** `BiologyConfig`, `MetabolicRates`, `EcologyConfig` and
  `JunctionConfig` hoisted into `Scenario`, so every parameter is authorable and persistent.
  Parameter editor; scenario open/save; slide open/save wired to `mm-core::snapshot`;
  view-only settings kept in a separate file. Mid-run parameter changes recorded as
  timestamped interventions in world state, so a slide still replays exactly.
- **M10.3 — several ways to make a living.** *(Taken out of order, ahead of the genome view,
  because it is an ISA change and the ISA bump was free while no saved run carried a stamp.)*
  `MetabolicChemistry` becomes a set of pathways rather than one reaction; an organelle picks
  which it runs by its `control[1]`. Investigated and designed in `docs/CHEMISTRY.md`,
  normative in SPEC §7.2. ISA version 1 → 2.
- **M10.3b — the genome view.** Source and reading modes. Template operands resolved to what
  they mean: immediates in decimal, jumps and calls to their target offset, promoters to their
  gene. The `%` form remains the only thing the editor round-trips.
- **M10.4 — the ecology pane.** The phylogenetic tree drawn as a tree; the food web drawn as a
  graph; the timeline full width and scrubbable.
- **M10.5 — the look.** Chemical fields as one uploaded texture rather than a quarter of a
  million sprites. Cells as a single instanced draw with a signed-distance shader: rounded
  shading, per-cell irregularity, membrane ring, in-shader depth of field.
- **M10.6 — the chrome.** The half of the window that is not the slide. A palette and a type
  scale in `theme.rs` with no egui in it; every number in monospace; the three-column row
  grammar replacing the labelled-slab bars; section headers; the drawer's work-area-plus-context
  shape, which the toolbox does not have; menus with a shortcut column and a live speed control;
  a parameter editor that says which field you changed. Designed in `docs/UI.md` §8.

  **This deliverable does not open the renderer.** `cellpipe.rs`, `cellmesh.rs`, `cell.wgsl`,
  `art.rs`, `optics.rs`, `phantom.rs`, `slide.rs` and `main.rs`'s `redraw` are finished work —
  see `docs/OVERLAPS.md` for what some of it cost — and UI.md §8.1 makes the boundary explicit.
  A chrome change that appears to need one of those files is a chrome change that is wrong.

**Acceptance tests**
1. **Input goes where it is pointed.** `ui::route` is a pure function of pointer position and
   layout; a pointer inside any panel rect never returns the slide. Tested without a graphics
   stack, as `slide.rs` is.
2. **Configuration round-trips.** Every field of `Scenario` survives RON write→read and
   snapshot write→read. A run with recorded interventions replays to the same state hash as
   the run that produced them.
3. **The reading view is not the source view.** The text the editor hands back still
   reassembles to the original bytes, for every genome in `genomes/` and for random byte
   strings. M0's round-trip test, extended to whatever the genome pane emits.
4. **Frame budget.** 50,000 cells at ≥ 30fps at whole-slide zoom.
5. **Decoupling, for real this time.** The tick rate is unchanged whether the render runs at
   60fps or 5fps, and the state hash at 100,000 ticks matches `mm-cli`. M4 tests 1 and 3,
   which could not fail while the two shared a thread.
6. **The theme is data, and the data is checked.** `theme.rs` builds with egui absent, as
   `ui.rs` does. Its tests assert what a palette can actually get wrong: no two text roles
   share a size; the accent colour is returned for selection and state and for nothing else;
   and every role/ground pair that occurs in the interface clears a contrast floor, so a
   near-black theme cannot ship with a label nobody can read.
7. **The chrome did not touch the renderer.** `shader_probe`, `packing_probe`, `nine_cells`,
   `overlap_detector`, `swell_probe`, `gradient_probe` and `frame_cost` pass unmodified, and
   the M10.6 diff contains none of the files UI.md §8.1 lists. The picture is finished; this
   milestone is the window around it.
8. **No regression.** Every acceptance test from M0–M8 still passes.

---

## Deferred

**Networking and teleporter pads.** Not in scope. Kept possible solely by enforcing
determinism (I1) and serialisable state (I7) from M0 onward. Do not compromise either for
short-term convenience at any milestone.
