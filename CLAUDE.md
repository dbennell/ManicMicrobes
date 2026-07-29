# CLAUDE.md — working agreement

Read this before making any change. Read `docs/SPEC.md` before implementing anything;
it is normative. Read `docs/MILESTONES.md` to find the current definition of done.

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
cargo run -p mm-cli -- sweep scenarios/soup.ron --param mutation_rate --range 1..64
cargo run -p mm-cli -- hash scenarios/soup.ron --ticks 100000   # determinism check
cargo run -p mm-cli -- run scenarios/soup.ron --archive species.ndjson   # the species archive
cargo run -p mm-app --features render --release   # the microscope
# `--features render` is required: Bevy is an optional dependency so that the
# simulation/render wall in slide.rs stays testable without a graphics stack.
```

## Layout

```
crates/mm-core/   simulation. No Bevy, no floats, no wall-clock, no global RNG.
crates/mm-asm/    assembler, disassembler, source maps.
crates/mm-cli/    headless runner, parameter sweeps, metric export.
crates/mm-app/    Bevy front-end: microscope, editor, wiki, tools.
docs/SPEC.md      normative specification.
docs/MILESTONES.md  delivery plan and acceptance tests.
scenarios/        .ron scenario configs.
genomes/          .mm assembly sources.
```
