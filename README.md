# Manic Microbes

An artificial-life simulator in the lineage of Tierra, Avida and DarwinBots, and a piece of
consumer software about looking at one.

Cells carry a genome of byte-encoded instructions and execute it in a per-cell virtual machine.
The genome builds and controls physical machinery — organelles — rather than expressing
behaviour directly. They live on a 2D substrate carrying a fluid with sixteen diffusing chemical
species and an incident light field. Matter is exactly conserved. Energy enters as light and
leaves as heat. Cells eat, move, sense, attack, join, differentiate and divide.

**There is no fitness function anywhere in the codebase, and there must never be one.** Nothing
is scored. Selection is a consequence of the physics, and that constraint is enforced by tests
rather than by good intentions.

The microscope, the species wiki and the timeline of first occurrences are not decoration around
the simulation — they are half the point. It is meant to be a fishtank as much as an instrument.

Two framings share one engine, and neither may compromise the other:

- **Petri mode** — open-ended evolution from a seeded or minimal ancestor, mutation on. The
  interesting output is the phylogeny and the story it tells.
- **Arena mode** — hand-authored cells competing, mutation off or clamped. The interesting
  output is whose code wins.

---

## Building and running

Rust stable, edition 2021, MSRV 1.95. No system dependencies beyond a graphics stack for the
front-end: Bevy comes in with `default-features = false` and neither audio nor gamepad support
enabled, so there is no ALSA or udev package to install first. On Linux the `x11` feature is on
and `wayland` is not, so a Wayland session needs XWayland — which almost every distribution
already ships.

```sh
cargo build --workspace
cargo fast          # ~500 tests in seconds; see .cargo/config.toml for what it skips and why
```

From a clean clone the first build is the slow one — almost all of it is Bevy, and it leaves
about 2 GB in `target/`. `mm-cli` is much the quicker of the two, because `mm-core` and `mm-asm`
have almost no dependencies between them.

**`cargo test --workspace` takes the better part of an hour** — it is four evolutionary
acceptance runs across ten seeds each, and it is not what you want between two edits. `cargo
fast` is the one to use; `cargo full` is the hour.

### The microscope

```sh
cargo run -p mm-app --features render --release
```

`--features render` is required and is not a convenience flag. Bevy is an *optional* dependency
of `mm-app` so that the simulation/render boundary in `slide.rs` stays testable on a machine with
no display at all — the guarantee is that rendering cannot reach the simulation, and that is only
checkable if the crate compiles without a renderer.

Release, because a debug build of the fluid solver is not worth watching.

### Headless

`mm-cli` is why `mm-core` has no Bevy in it: the simulation runs at a thousand times realtime for
parameter sweeps, and the renderer can never hold it to a frame budget.

A scenario is a recipe rather than a state: it seeds the water, the light and the chemistry, but
it puts no cells on the slide. **Every `run` needs a `--genome` to seed with**, or it simulates
sterile water and reports a population of zero for as long as you let it.

```sh
# the first thing to run: an ancestor in the default soup, invariants checked at every sample
cargo run -p mm-cli --release -- run scenarios/soup.ron --genome genomes/ancestor.mm \
    --ticks 50000 --check

# run a scenario, sampling metrics as NDJSON
cargo run -p mm-cli --release -- run scenarios/soup.ron --genome genomes/ancestor.mm \
    --ticks 1000000 --metrics out.ndjson

# the same recipe once per parameter value
cargo run -p mm-cli --release -- sweep scenarios/soup.ron --genome genomes/ancestor.mm \
    --param mutation --range 1..64

# the state hash, for determinism checks — must match what the microscope reaches
cargo run -p mm-cli --release -- hash scenarios/soup.ron --genome genomes/ancestor.mm \
    --ticks 100000

# the species archive from a long run
cargo run -p mm-cli --release -- run scenarios/soup.ron --genome genomes/ancestor.mm \
    --archive species.ndjson

# arena mode: two authored genomes, one slide
cargo run -p mm-cli --release -- match genomes/hunter.mm genomes/sponge.mm --ticks 20000
```

`mm-cli --help` prints the full flag list. `--check` verifies the invariants at every sample and
exits non-zero if one breaks, so it is usable directly from CI.

### Tests and gates

```sh
cargo fast                                   # the inner loop, after every change
cargo slow                                   # the four evolutionary runs, before a commit
cargo full                                   # everything, as CI runs it — the better part of an hour
cargo test -p mm-core                        # the simulation only
cargo test --release --test totality_fuzz    # the long fuzz; release only
cargo bench --workspace                      # criterion gates
```

The three aliases are defined in `.cargo/config.toml`, which also says what `fast` leaves out and
why. `full` is `cargo test --release --workspace`.

`cargo fast` and `cargo build-include` are also what CI runs on `main` and on every pull request,
and what the `pre-push` hook runs before anything leaves the machine. Hooks are not installed by
cloning, so in a fresh clone:

```sh
git config core.hooksPath .githooks
```

Benchmarks are gates, not information: a change that regresses a performance gate is not done,
however correct it is.

---

## Where to look

| document | what it is |
| --- | --- |
| [`CLAUDE.md`](CLAUDE.md) | the working agreement. **Read this before changing anything.** The hard rules, and what each one is protecting. |
| [`docs/SPEC.md`](docs/SPEC.md) | normative specification of the simulation. Where it and an implementation disagree, it wins. |
| [`docs/MILESTONES.md`](docs/MILESTONES.md) | the delivery plan and, more usefully, the acceptance test for each milestone. Definition of done. |
| [`docs/UI.md`](docs/UI.md) | normative for the front-end the way SPEC.md is for the core: the layout, the palette and type scale, and a running record of which interface decisions were later reversed and why. |
| [`docs/CHEMISTRY.md`](docs/CHEMISTRY.md) | the metabolic pathways, and the investigation that produced them. |
| [`docs/FEEDING.md`](docs/FEEDING.md) | the ways of making a living the engine has and the ones it does not, the control-word ledger, and whether sixteen organelle types is enough. |
| [`docs/NEURONS.md`](docs/NEURONS.md) | the guide to building a nervous system out of junctions: what already works, the one reading that blocks it, and the order to do it in. |
| [`docs/STIFFNESS.md`](docs/STIFFNESS.md) | why every cell is exactly as stiff as every other, what turgor is currently charged for, and where the squish is actually drawn. |
| [`docs/OVERLAPS.md`](docs/OVERLAPS.md) | **read before chasing anything wrong with the picture.** Cells drawn over one another was hunted through the physics for days and was twice in the shader's wiring. |

The prose in this repository is load-bearing. Most non-obvious decisions carry the reasoning and
the failure that produced them in a comment beside the code, on the principle that a rule without
its reason gets deleted by the next person who finds it inconvenient.

### Layout

```
crates/mm-core/   simulation. No Bevy, no floats, no wall-clock, no global RNG.
crates/mm-asm/    assembler, disassembler, source maps.
crates/mm-cli/    headless runner, parameter sweeps, metric export.
crates/mm-app/    Bevy front-end: microscope, editor, wiki, tools.
docs/             specification, milestones, interface, investigations.
tools/            scripts that turn a screenshot into numbers.
scenarios/        .ron scenario configs — a scenario is a recipe, not a state.
genomes/          .mm assembly sources.
```

---

## The hard rules, in brief

Each of these is enforced by a test. `CLAUDE.md` has the full statement and the reasoning.

1. **No Bevy in `mm-core`.** It must build with Bevy absent from its dependency graph.
2. **No floats in `mm-core`.** All simulation arithmetic is integer or fixed-point. Floats exist
   only in `mm-app`, for rendering.
3. **No panics in the VM.** Any byte sequence must be a legal program. No indexing that can go
   out of range, no arithmetic that can overflow.
4. **Addressing wraps, magnitudes saturate.** Never the other way round.
5. **No sequential RNG.** Randomness is `hash(seed, tick, cell_id, purpose)`.
6. **No iteration-order dependence.** Outcomes never depend on hash iteration, rayon scheduling
   or thread count.
7. **State round-trips.** Any new field serialises and resumes bit-identically.
8. **The ISA version is stamped.** Archived genomes are replayed under the version they evolved
   in. Currently 5.

Evolutionary results are not deterministic, but the tests are: an acceptance test that asserts an
evolutionary outcome is specified as "in at least N of 10 seeds", with the seeds recorded. When
one of those fails, the finding is *which parameter is starving the result* — not a licence to
tune until it passes.

---

## Debugging the picture

If something looks wrong on screen, the data comes off the table in one run rather than one
hypothesis at a time. `shaderbench` draws cells that no simulation made, through the same shader
and vertex layout the microscope uses:

```sh
cargo run -p mm-app --bin shaderbench --features render --release
```

`cell.wgsl` hot-reloads. `tools/check_outline.py` turns two photographs of one frame, over
different backgrounds, into the coverage the shader actually produced against what it was told to
draw. See `CLAUDE.md` for the exact incantations.

The front-end can photograph itself: `MM_SHOT_VIEW` arranges a panel, window, sheet or menu and
`MM_SHOT` writes a PNG and quits. A claim about the picture is settled by a photograph.

---

## Status

Pre-release; no published crates, no stable file formats. M10 — the instrument — is the milestone
in progress, and M9 (scale and hardening) deliberately follows it: the working target's render
half is entirely M10's, and profiling M9 against a figure that is currently the frame rate in
disguise would tune the wrong thing.

## Licence

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([`LICENSE-MIT`](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option. Apache-2.0 carries an explicit patent grant; MIT is short and — unlike
Apache-2.0 — compatible with GPLv2, so a GPLv2 project can still use this. Dual gets both.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 licence, shall be dual licensed as above, without
any additional terms or conditions.

The name "Manic Microbes" and the project logo are not covered by either licence and remain the
property of David Bennell. A licence cannot stop a fork being renamed; this is the part that can.
