# CLAUDE.md — working agreement

Read this before making any change. Read `docs/SPEC.md` before implementing anything;
it is normative. Read `docs/MILESTONES.md` to find the current definition of done.

**Before chasing anything wrong with the picture, read `docs/OVERLAPS.md`.** Cells drawn over one
another was hunted through the physics for days and was twice in the shader's wiring — and the
bench that settled it (`cargo run -p mm-app --bin shaderbench --features render`) draws cells no
simulation made, so the data can be taken off the table in one run instead of one per hypothesis.

## What this project is

Manic Microbes: an artificial-life simulator where cells execute byte-encoded genomes in a
per-cell VM, on a 2D substrate with conserved matter, diffusing chemistry and an energy
gradient. Evolution is open-ended — there is no fitness function anywhere in the codebase,
and there must never be one. Selection is a consequence of the physics.

It is equally a piece of consumer software. The microscope, the species wiki and the
timeline are the product, not decoration.

---

## Hard rules

These are not stylistic preferences. Violating any of them breaks the project's foundations,
and each is enforced by a test.

1. **No Bevy in `mm-core`.** The simulation crate must build with Bevy absent from its
   dependency graph. No `bevy_*` imports, no ECS types, no `Component` derives.
2. **No floats in `mm-core`.** `f32` and `f64` are forbidden. All simulation arithmetic is
   integer or fixed-point. Floats exist only in `mm-app`, for rendering.
3. **No panics in the VM.** No `unwrap`, `expect`, `panic!`, `assert!` (outside `#[cfg(test)]`),
   array indexing that can go out of range, or arithmetic that can overflow in release, on any
   path reachable from executing a genome. Use `wrapping_*`, `saturating_*` and `%` explicitly.
   Any byte sequence must be a legal program.
4. **Addressing wraps, magnitudes saturate.** Indices reduce modulo their range. Arithmetic
   clamps to `i16` bounds. Never the other way round.
5. **No sequential RNG.** Randomness is `hash(seed, tick, cell_id, purpose)`. No `rand`
   thread-local, no stateful generator, no `SystemTime`, no `Instant` anywhere in `mm-core`.
6. **No iteration-order dependence.** Never let simulation outcomes depend on `HashMap` or
   `HashSet` iteration order, rayon scheduling, or thread count. Use `BTreeMap` or sort
   explicitly by stable ID where order matters.
7. **State must round-trip.** Any new field in world state must serialise and deserialise with
   bit-identical resumption. If you add state, extend the serialisation and its test in the
   same commit.
8. **Stamp the ISA version.** Any change to the opcode table, template semantics or organelle
   catalogue is an ISA version bump, and archived genomes must be replayed under the version
   they evolved in.

## Design rules

- **No cell-type enum.** Skin, muscle and neuron are labels the analysis layer infers from
  organelle loadouts. Differentiation must emerge from expression gated on internal chemical
  state.
- **No special-cased viruses, colonies or organisms.** A parasite is a cell that writes to
  another cell's nucleus. An organism is a connected component. If you find yourself adding a
  flag for one of these, the mechanism is wrong.
- **New organelle types fill a `RESERVED` catalogue slot.** Never renumber existing types.
- **Junctions never couple to the fluid.** Position-based distance constraints only. No
  torque, no angular dynamics, no fluid backpressure. This was decided deliberately for
  performance; do not helpfully add it.
- **Duplication is a required mutation operator.** Do not ship a mutation set without it.
- **Store species-level aggregates, not per-individual records.** Full genomes only for
  species founders and periodic snapshots.

---

## Working method

**Work one milestone at a time.** Do not start M(n+1) work while M(n) has a failing
acceptance test. Do not implement things from later milestones because they seem easy.

**Every milestone lands with its acceptance tests**, in the same PR as the feature. Tests
from earlier milestones must keep passing; they are permanent CI, not scaffolding.

**Benchmarks are gates, not information.** Criterion benchmarks run in CI with regression
thresholds. A change that regresses a performance gate is not done, however correct it is.

**Small commits, one concern each.** A commit that both adds an opcode and refactors the
fluid solver is two commits.

**When the spec is ambiguous, ask.** Do not resolve a design question by picking whichever
reading is easier to implement and moving on. If a decision has to be made mid-task, make it,
implement it, and flag it explicitly in the PR description as a decision that needs review.

**When the spec is wrong, say so.** It was written before the code existed. If implementation
reveals that a mechanism cannot work as described, stop and explain the problem rather than
silently implementing something adjacent. Then update the spec in the same PR.

**Never weaken a test to make it pass.** In particular, the totality fuzz test (M0) and the
exact matter-conservation test (M1) are load-bearing. "Within epsilon" is not conservation.
If one of these fails, the code is wrong.

---

## Evolutionary results are not deterministic, but tests must be

Acceptance tests that assert an evolutionary outcome — chemotaxis evolving, differentiation
appearing — are specified as "in at least N of 10 seeds", with fixed seeds recorded in the
test. This makes a stochastic result into a reproducible test.

If such a test fails, **that is a finding, not just a bug.** Report which parameter appears to
be starving the result (mutation rate, energy economics, instruction budget, template search
range) rather than tuning until it passes and moving on. The interesting output of this
project is knowing which parameters matter.

---

## Commands

```
cargo test --workspace                       # all tests
cargo test -p mm-core                        # core only
cargo test --release --test totality_fuzz    # the long fuzz; release only
cargo bench --workspace                      # criterion gates
cargo run -p mm-cli -- run scenarios/soup.ron --ticks 1000000 --metrics out.ndjson
cargo run -p mm-cli -- sweep scenarios/soup.ron --param mutation --range 1..64  # or duplication, fluid, light
cargo run -p mm-cli -- hash scenarios/soup.ron --ticks 100000   # determinism check
cargo run -p mm-cli -- run scenarios/soup.ron --ruleset rival_light   # same world, other rules
cargo run -p mm-cli -- run scenarios/soup.ron --archive species.ndjson   # the species archive
cargo run -p mm-cli -- match genomes/a.mm genomes/b.mm --ticks 20000     # an arena match
cargo run -p mm-app --features render --release   # the microscope
# `--features render` is required: Bevy is an optional dependency so that the
# simulation/render wall in slide.rs stays testable without a graphics stack.

# The cell shader with no simulation behind it: cells from `mm_app::phantom`, drawn through
# the same shader and vertex layout the microscope uses, so that a fault in the picture can be
# blamed on the shader or on the data and not argued about. `cell.wgsl` hot-reloads.
cargo run -p mm-app --bin shaderbench --features render --release   # --bin, not the default
cargo test -p mm-app --test shader_probe -- --ignored --nocapture --test-threads=1  # its numbers

# And what the shader actually put on the screen, against what it was told to draw. Two
# photographs of one frame over different backgrounds give the coverage exactly; see the script.
MM_BENCH_AT=30 MM_BENCH_PANEL=0 MM_BENCH_DUMP=/tmp/f.txt MM_BENCH_SHOT=/tmp/dark.png \
  ./target/release/shaderbench
MM_BENCH_AT=30 MM_BENCH_PANEL=0 MM_BENCH_BG=1,1,1 MM_BENCH_SHOT=/tmp/light.png \
  ./target/release/shaderbench
tools/check_outline.py /tmp/f.txt /tmp/dark.png /tmp/light.png
```

## Layout

```
crates/mm-core/   simulation. No Bevy, no floats, no wall-clock, no global RNG.
crates/mm-asm/    assembler, disassembler, source maps.
crates/mm-cli/    headless runner, parameter sweeps, metric export.
crates/mm-app/    Bevy front-end: microscope, editor, wiki, tools.
docs/SPEC.md      normative specification.
docs/MILESTONES.md  delivery plan and acceptance tests.
docs/UI.md        normative for the front-end, as SPEC.md is for the core.
docs/CHEMISTRY.md the metabolic pathways, and the investigation behind them.
docs/FEEDING.md   the ways of making a living, the control-word ledger, and the catalogue budget.
docs/ECONOMY.md   what a cell earns and pays, why the autotroph wins everything, and the rebalance.
                  Its harness is `mm_core::balance` + `tests/balance.rs`; run it before and after
                  any change to a price, a rate or a light regime.
docs/NEURONS.md   nervous systems from junctions: what works, what blocks it, what not to build.
docs/STIFFNESS.md the contact model, turgor, and which of the squish is physics and which is drawn.
docs/OVERLAPS.md  the overlapping cells: what it was, what it was not, and the bench.
README.md         what this is and where to look. Written for somebody arriving.
tools/            scripts that turn a screenshot into numbers.
scenarios/        .ron scenario configs — the *worlds*: size, light, current, chemistry seeding,
                  barriers, and `inhabitants` (which genomes start where, via `mm_core::Placement`
                  — Spread, At, Grid, Hex or Scatter, all of them barrier-aware).
rulesets/         .ron named parameter sets — the *rules*: what a cell may do, as dotted-path
                  diffs. A scenario says `ruleset: "name"` to inherit one and may override it
                  inline. Resolved at load, stored resolved, name kept only as provenance —
                  see `mm_core::ruleset` for why that is what keeps hard rule 7 intact.
genomes/          .mm assembly sources.
```
