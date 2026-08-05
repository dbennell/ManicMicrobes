# The overlapping cells

Cells on a packed slide were drawn over one another with no boundary between them: a neighbour's
lobe crossing a shared wall, cells shaped like kite shields and fish scales instead of tiling,
overlaps appearing and vanishing as the slide moved. It never happened on a still picture and any
movement at all brought it out.

It ran from the first packed slide to the fix, and it was hunted almost entirely in the wrong
place. This is the record of what it actually was, what it was not, and what was built to tell the
difference — because the expensive part was never the fix. Both fixes are one line.

---

## What it was

**Two faults, both in how the fragment shader was wired up, neither in any of the geometry.**

### 1. The material never asked for alpha blending

`Material2d::alpha_mode` defaults to `AlphaMode2d::Opaque`, which is a blend state of *replace*:
the fragment's alpha is written to the target and never composited. `CellMaterial` did not
override it. So `cell.wgsl` computed an antialiased edge one pixel wide at any magnification —
the stated reason for evaluating the field per pixel at all — and it was discarded. What decided
the drawn shape instead was the `discard` at the bottom of the fade, `alpha <= 0.001`, which is a
hard aliased edge at the *outer* end of the ramp rather than its middle.

Measured on the bench, a plain circle with the wobble switched off, against the radius the data
asked for:

| | median | p10 | p90 | the edge |
|---|---|---|---|---|
| `Opaque` | **+1.85 px** | +1.39 | +2.19 | 1.000 → 0.144 → 0.000, one pixel, hard |
| `Blend` | **−0.00 px** | −0.01 | +0.01 | 0.870, 0.739, **0.500**, 0.271, 0.128 |

The same +1.8 px at 40 px per square and at 110, because it is not a radius error: it is the width
of the fade, added to every cell whatever size it is drawn.

Two consequences. Every cell was drawn about two pixels past its own outline, so both cells of a
pair overran their shared wall and the wall became a four-pixel band that both claimed and neither
owned, settled by draw order. And the edge could not be antialiased, so it crawled: a hard edge on
a body moving by a fraction of a pixel flips whole pixels, where an antialiased one slides.

Fixed in `cellpipe::CellMaterial::alpha_mode`.

### 2. The seam directions are bit patterns, and they were being interpolated

This is the one that produced the egregious overlaps — the kite shields and the fish scales.

`squash_dir` does not carry a number. It carries two 16-bit snorms packed into the bits of an
`f32`, which the fragment shader takes apart with `bitcast`. The *bit pattern* is the payload, so
any arithmetic on it at all is destructive — and a vertex output is interpolated across the
triangle by default.

All four vertices of a quad hold the same value, which looks like it makes interpolation a no-op.
On hardware that interpolates as `a + (b−a)u + (c−a)v` it is. On hardware that interpolates as
`a·w₀ + b·w₁ + c·w₂` it is not: the weights sum to one only to within rounding, so the result
comes back an ulp or two off. One ulp is the bottom bit of the mantissa, the bottom bit of the
mantissa is the bottom bit of the packed pattern, and that is the low half of `nx`. **A seam
normal arrives pointing somewhere else entirely, and does so differently at every pixel, because
the interpolation weights differ at every pixel.**

What that draws is a cell whose flat sides come and go across its own face — round where the
normals landed wrong, walled where they happened to survive — so two neighbours have no agreed
boundary and one is drawn over the other. It moves when the cell moves, because the weights move
with it.

On fifteen cells, still, with the alpha fix already in:

| | worst overlap | over 1 px | over 4 px |
|---|---|---|---|
| interpolated | **29.33 px** | 62% | 41% |
| `@interpolate(flat)` | **0.82 px** | 0% | 0% |

Fixed in `cell.wgsl`'s `Output` struct: every per-cell attribute is `@interpolate(flat)`, and only
`uv` — the quad corner the field is evaluated against — is interpolated.

### Why it needed movement, and why it needed a crowd

Both faults are position-dependent in screen space and neither touches the simulation. That is why
three days of measurements inside `mm-core` and `slide.rs` found nothing: every one of them was
looking at data that was already correct. The bench's geometry probe now asserts exactly that —
given all-pairs seams, **no cell is ever drawn over another, in any arrangement, under any
motion**, and it has never once failed.

The second fault also needs neighbours to show its worst: a corrupted normal only matters where a
seam was supposed to cut. On a square lattice it reads as round cells overlapping; on a **hex**
lattice, where six seams meet at sixty degrees, it produces the kite-shield and fish-scale shapes
that were reported from the beginning. `phantom::Layout::Hex` exists because that arrangement was
the one that reproduced them.

---

## What it was not

Every one of these was proposed, tested and found not to be the cause. They are recorded so that
nobody spends another day on them.

| Mechanism | Measurement |
|---|---|
| The area-preserving swell resizing cells for no reason | A rigid turn changes no distance at all; the swell moves **0.12% of a radius, 0.14 px**, from the ray quadrature. Real, far too small. |
| The seam cap truncating real seams | At most **9** seams ever cut a cell, against 12 slots. Capping at 6 on nine or fifteen cells produces nothing. |
| The `mm-core` radius staircase | Injected: no overlapping pairs at all, only a 3.4% swell jump. |
| Iteration order in the neighbour search | Ruled out earlier by direct tally; the bench reproduces the artefact with no neighbour index at all. |
| The contact set churning | Injected at the measured rate (0.6 per cell per tick) it *does* produce overlaps — but the artefact is present with all-pairs seams and no churn, so churn is at most an aggravator. |
| A reach that falls short | Same: injectable, produces overlaps, but not necessary for the artefact. |

The two that *are* real aggravators — churn and reach — are worth revisiting only now that the
shader is honest, because until now they were being blamed for something else's work.

---

## The instrument

Built because every experiment before it had to run a world to get a picture, which meant every
experiment carried the whole simulation with it.

- **`mm_app::phantom`** — cells no simulation made. Positions and radii are arithmetic on a frame
  number; seams are computed **all-pairs**, using the production `slide::seam_between` and
  `slide::area_swell`. The data is correct by construction, and each suspected upstream fault is a
  knob that can be injected instead: `cap`, `reach`, `churn`, `staircase`, `swell`.
  Six arrangements (`pair`, `nine`, `fifteen`, `hex`, `scatter`, `raft`), five motions, and a
  `dither` that jogs every cell off the lattice so that no two distances, normals or sub-pixel
  phases repeat — a clean lattice is where an off-by-one hides.
- **`src/bin/shaderbench.rs`** — those cells in a window, through the same shader, material and
  vertex layout the microscope uses (`cellpipe`, which exists so the two cannot drift). Sliders for
  every knob, a live readout, and an overlay that draws the outline the *data* says each cell has
  over the one the shader drew. `cell.wgsl` hot-reloads on save.
- **`tests/shader_probe.rs`** — the same phantom measured headlessly, in the style of
  `packing_probe.rs`. Runs in ordinary CI with no graphics stack.
- **`tools/check_outline.py`** — where the shader put a cell's edge against where it was told to,
  recovering coverage exactly by photographing one frame over two backgrounds and solving each
  pixel for its alpha.
- **`tools/check_walls.py`** — the same for the wall *between* two cells, which is where an overlap
  actually looks like something. Attributes each pixel by fitting it as a blend of the two cells'
  colours.

### Rigid motion is the sharp instrument

`Drift` and `Orbit` move every cell by the same offset or turn them about one point, so every
distance between every pair is preserved and every seam plane keeps its face exactly. Anything that
changes on screen under those changed *after* the data — in the shader, in the packing, or in the
sampling. That single distinction is what split the problem in half, and it took four hours where
the preceding three days took three days.

---

## Traps in measuring this

Three of the artefacts found along the way were in the measuring tools, not in the renderer, and
each was convincing:

1. **Comparing colours in sRGB when the blend is linear.** The shader only ever scales a cell's
   colour, so hue is preserved — in *linear* light. The PNG is sRGB, and a scale in linear is not a
   scale in sRGB, so a pixel deep inside a cell classified as some other cell entirely.
2. **Measuring past the end of a wall.** Two circles cross at a chord, but in a packed sheet the
   *shared* edge is shorter than that chord: a third cell arrives and takes over. Sampling at ±0.8
   of the chord compared the boundary between two other cells against this pair's wall and reported
   30 px faults that were not there. Every one of the worst readings on a raft was this.
3. **A dump and a photograph that did not pair up.** The screenshot is saved by an observer a few
   frames after it is asked for; a run can write its geometry and not its picture. Measuring three
   dumps against two pictures pairs the wrong frames and reports the mismatch as a fault. The tool
   now refuses.

The general lesson is the one `packing_probe.rs` already states: describing a screenshot is not a
measurement, and neither is a measurement whose own conventions have not been calibrated. Both
tools are now checked against a case with a known answer — a plain quad, whose edges are where its
vertices are — before being believed.

---

## Still open

- **The seam reach spends slots on pairs that cannot touch.** At `slide::PACKING_PERMILLE` (1750,
  which is 1.52× the drawn radii), **35–55% of admitted seams have their plane outside the cell
  entirely** and can never cut it. On a raft a cell reaches 15–18 admitted seams against 12 slots,
  while at most 9 ever cut. The deepest-first sort in `main::squash_of` is currently what stops
  that mattering. At a reach of 1.25 — just enough to cover `PACKING × MAX_SWELL` — idle seams drop
  to 2–10% and no cell exceeds 9. Run
  `cargo test -p mm-app --test shader_probe -- --ignored how_many_seams_are_doing_nothing`.
- **`cellmesh::seed_of` defeats its own intent.** Its doc says the seed is kept small because the
  shader hashes it as an `f32` and a large integer loses its low bits. It shifts to leave values up
  to 262144, and `hash11` begins `fract(p * 0.1031)` — at that size one ulp is ~0.002 and the hash
  is chaotic in it: seed 162013.9 hashes to 0.8202, one ulp along to 0.6385. Silhouettes are drawn
  from far fewer distinct shapes than intended and are not reproducible across machines. Not a
  flicker — a seed is fixed for a cell's life.
- **Two unchased outliers** in the pixel sweep: 6.3 px on hex under jitter, 29.6 px on scatter
  under jitter, against medians of 0.3 and 0.5 px. Every earlier outlier of that size turned out to
  be trap 2 above, and these have not been confirmed either way.

---

## Running it

```
cargo run -p mm-app --bin shaderbench --features render --release
cargo test -p mm-app --test shader_probe -- --ignored --nocapture --test-threads=1

# what the shader drew, against what it was told to draw
MM_BENCH_LAYOUT=hex MM_BENCH_SERIES=4 MM_BENCH_PANEL=0 \
  MM_BENCH_DUMP=/tmp/f.txt MM_BENCH_SHOT=/tmp/f.png ./target/release/shaderbench
tools/check_walls.py /tmp/f.txt /tmp/f.png
```

Every knob has an environment variable and a frame is a pure function of its number, so a scene
photographed today and one photographed next week are pictures of the same thing.
