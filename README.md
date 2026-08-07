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
front-end.

```sh
cargo build --workspace
cargo test  --workspace
```

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

```sh
# run a scenario, sampling metrics as NDJSON
cargo run -p mm-cli --release -- run scenarios/soup.ron --ticks 1000000 --metrics out.ndjson

# the same recipe once per parameter value
cargo run -p mm-cli --release -- sweep scenarios/soup.ron --param mutation --range 1..64

# the state hash, for determinism checks — must match what the microscope reaches
cargo run -p mm-cli --release -- hash scenarios/soup.ron --ticks 100000

# the species archive from a long run
cargo run -p mm-cli --release -- run scenarios/soup.ron --archive species.ndjson

# arena mode: two authored genomes, one slide
cargo run -p mm-cli --release -- match genomes/hunter.mm genomes/sponge.mm --ticks 20000
```

`mm-cli --help` prints the full flag list. `--check` verifies the invariants at every sample and
exits non-zero if one breaks, so it is usable directly from CI.

### Tests and gates

```sh
cargo test --workspace                       # everything
cargo test -p mm-core                        # the simulation only
cargo test --release --test totality_fuzz    # the long fuzz; release only
cargo bench --workspace                      # criterion gates
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

MIT OR Apache-2.0.
