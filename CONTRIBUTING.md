# Contributing

Patches are welcome. This file is longer than the usual one because the project is strict about
a small number of things and vague about the rest, and it is only fair to say which is which
before you spend an evening on something that will be turned down.

## Before you write anything

Read [`CLAUDE.md`](CLAUDE.md). It is the working agreement — the hard rules and, more usefully,
what each one is protecting. Read [`docs/SPEC.md`](docs/SPEC.md) before implementing simulation
behaviour; where it and the code disagree, the spec wins and the code is the bug. If the picture
is what you are chasing, read [`docs/OVERLAPS.md`](docs/OVERLAPS.md) first: cells drawn over one
another was hunted through the physics for days and was twice in the shader's wiring.

**Open an issue before anything larger than a fix.** Milestones are worked one at a time and
[`docs/MILESTONES.md`](docs/MILESTONES.md) says which one is in progress; a well-built feature
from three milestones ahead is still a merge conflict with a plan. An issue costs you nothing and
may save you the evening.

## Setting up

```sh
git clone https://github.com/dbennell/ManicMicrobes
cd ManicMicrobes
git config core.hooksPath .githooks   # hooks are never installed by cloning
cargo fast                            # ~500 tests in seconds — confirms the toolchain
```

Rust stable, MSRV 1.95. The first build from a clean clone is the slow one; almost all of it is
Bevy, and it leaves about 2 GB in `target/`.

## The loop

| when | run |
| --- | --- |
| after every change | `cargo fast` — every unit test, the totality fuzz, the hard-rule guards, the determinism suite |
| after a change to `mm-app` | build it and look at it: `cargo run -p mm-app --features render --release` |
| after a change to core physics, chemistry or the VM | `cargo slow` — the four evolutionary acceptance runs |
| before pushing | the `pre-push` hook does it for you: `cargo fast` and `cargo build-include` |

`cargo test --workspace` is the better part of an hour and is not the inner loop. `.cargo/config.toml`
says what each alias covers and why they all run in release.

CI runs `cargo fast` and `cargo build-include` on every pull request, from a clean checkout with
`--locked`. If you are contributing for the first time, GitHub holds that run until a maintainer
approves it — that is a platform default, not a comment on your patch.

## What a good pull request looks like

- **One concern per commit.** A commit that adds an opcode and refactors the fluid solver is two
  commits.
- **Tests land with the feature, in the same PR.** Every milestone's acceptance tests are
  permanent CI, not scaffolding — earlier ones must keep passing.
- **Never weaken a test to make it pass.** The totality fuzz and the exact matter-conservation
  test especially: "within epsilon" is not conservation. If one of those fails, the code is wrong.
- **Say what the change is for, not only what it does.** The prose in this repository is
  load-bearing — most non-obvious decisions carry the reasoning and the failure that produced
  them in a comment beside the code, on the principle that a rule without its reason gets deleted
  by the next person who finds it inconvenient. A new non-obvious decision should arrive the same
  way.
- **If you had to decide something the spec did not cover, flag it in the PR description** rather
  than picking whichever reading was easier to implement and moving on. If implementation showed
  the spec cannot work as written, say so and update it in the same PR.
- **New state serialises in the same commit that adds it.** Any field in world state must
  round-trip bit-identically; extend the serialisation and its test together or the change is
  incomplete.

## What will be turned down

Not because the idea is bad — several of these are the obvious next thing to reach for — but
because the project is defined by not having them.

- **A fitness function, or anything that scores a cell.** There is none anywhere in the codebase
  and there must never be one. Selection is a consequence of the physics. This is the project.
- **Floats or Bevy in `mm-core`.** Both are enforced by tests. Simulation arithmetic is integer or
  fixed-point; floats exist in `mm-app`, for rendering.
- **A panic reachable from executing a genome.** Any byte sequence must be a legal program — no
  `unwrap`, no indexing that can go out of range, no arithmetic that can overflow in release.
- **A cell-type enum.** Skin, muscle and neuron are labels the analysis layer infers from
  organelle loadouts. Differentiation emerges from expression gated on internal chemical state or
  it does not happen.
- **Special cases for viruses, colonies or organisms.** A parasite is a cell that writes to
  another cell's nucleus; an organism is a connected component. A flag for one of these means the
  mechanism is wrong.
- **Coupling junctions to the fluid.** Position-based distance constraints only — no torque, no
  angular dynamics, no fluid backpressure. This was decided deliberately for performance.
- **Renumbering the organelle catalogue.** New types fill a `RESERVED` slot. Archived genomes are
  replayed under the ISA version they evolved in, and renumbering silently reinterprets them.
- **Tuning a parameter until an evolutionary acceptance test passes.** Those tests are "in at
  least N of 10 seeds" with the seeds recorded. When one fails, the finding is *which parameter
  is starving the result* — mutation rate, energy economics, instruction budget, template search
  range. Report that. It is the interesting output of the project and it is worth more than a
  green tick.
- **A performance regression past a criterion gate**, however correct the change is. Benchmarks
  are gates, not information.

## Reporting a bug

The simulation is deterministic — randomness is `hash(seed, tick, cell_id, purpose)`, there is no
sequential generator and no wall-clock anywhere in `mm-core`. So a report can be exactly
reproducible, and one that is gets fixed far faster:

```sh
cargo run -p mm-cli --release -- run scenarios/soup.ron --genome genomes/ancestor.mm \
    --ticks 50000 --check
cargo run -p mm-cli --release -- hash scenarios/soup.ron --genome genomes/ancestor.mm \
    --ticks 100000
```

Include the scenario, the genome, the seed and the tick it goes wrong at, and the state hash if
you have it. If two machines disagree on that hash, that is its own bug and a serious one.

For anything wrong with the picture, include a photograph — the front-end can take its own, with
`MM_SHOT_VIEW` to arrange a panel and `MM_SHOT` to write a PNG and quit — and say whether
`shaderbench` reproduces it. It draws cells no simulation made, through the same shader and
vertex layout, so it settles whether the fault is in the data or in the drawing before anyone
argues about which.

## Licence

Dual MIT / Apache-2.0, at the user's option. Unless you state otherwise, a contribution submitted
for inclusion is dual licensed the same way, without additional terms. See the licence section of
the [README](README.md#licence) — including the part about the name and the logo, which are not
covered by either licence.
