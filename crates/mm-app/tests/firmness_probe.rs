//! What the foam-to-marbles slider actually does to a picture, in numbers.
//!
//! Run with `--release --ignored --nocapture`.
//!
//! # Why this is not a simulation test
//!
//! Whether a packed crowd should be drawn as a tessellation or as a heap of separate round bodies
//! is a question about the *picture*, and it had been getting answered through the simulation: a
//! genome, a scenario, a carrying capacity, twenty thousand ticks, and then an argument about
//! whether the result looked right. Every one of those is a variable that has nothing to do with
//! the question, and `docs/OVERLAPS.md` exists because that is exactly how the overlapping-cells
//! bug cost three days.
//!
//! So this asks it of `phantom::Bench` instead — cells no simulation made, on a lattice, whose
//! seams are all-pairs arithmetic on a frame number. `Bench::firmness` is the slider. Nothing
//! here knows what a genome is, and **what drives firmness on a real slide is deliberately not
//! decided here**; that is the next experiment, and it is a separate one.
//!
//! # What is measured
//!
//! `Drawn::outline` is the fragment shader's own outline in Rust, so the area a cell actually
//! covers on screen can be integrated without a GPU. Two numbers come out of it:
//!
//! * **swell** — how much larger than its true radius a cell is drawn, which is the knob's direct
//!   effect. 1.000 is untouched.
//! * **out of round** — the spread of a cell's own outline radius over its largest. Zero is a
//!   circle; a regular hexagon reads about 0.13 and a square about 0.29. **This is the measure
//!   the question is about**, and the first version of this probe used area instead and learned
//!   nothing, because `area_swell` is area-preserving by construction: switching it off changes
//!   what a cell covers by about a tenth and says nothing about whether it is a polygon.
//!
//! # What it found
//!
//! **The slider works and cannot reach marbles.** On a raft at the spacing the physics actually
//! drives a pack to — exactly touching:
//!
//! ```text
//!   firmness    swell   area kept   out of round
//!       0.00    1.221        100%          0.387
//!       0.25    1.166         98%          0.368
//!       0.50    1.111         95%          0.347
//!       0.75    1.055         93%          0.324
//!       1.00    1.000         91%          0.298
//! ```
//!
//! Swell spans its whole range, linearly, exactly as asked. Out-of-round moves 0.387 to 0.298 —
//! and **a square is 0.29**. So the far end of the slider is still a polygon; it is a *smaller*
//! polygon with gaps around it, not a round cell.
//!
//! The reason is structural rather than a matter of tuning. `area_swell` decides how much a cell
//! **inflates**; the seams decide how much it is **cut**, and firmness does not touch them. Turn
//! the swell off entirely and every seam plane is still exactly where it was, so the outline is
//! the same polygon drawn at 91% of the area.
//!
//! Two things would reach round, and both have a catch worth knowing before either is built.
//!
//! **Cut less deeply.** A firm cell's seam plane moves out towards its own circle. The catch is
//! that both cells of a pair must agree on their shared wall — SPEC §6.4 — so a firm cell next to
//! a soft one cannot simply refuse the cut without the two being drawn overlapping, which is the
//! fault `docs/OVERLAPS.md` is about. It only works if the *physics* keeps firm cells further
//! apart, which is what `MetabolicRates::rigidity_gain` does. Picture and simulation have to move
//! together here, and that is a real constraint rather than an implementation detail.
//!
//! **Inflate less to begin with.** `slide::PACKING` is 1.15: every cell is drawn 15% larger than
//! it physically is, before any swell. That is why the bottom block below still reads 0.209 at
//! spacing 1.10, where nothing physically touches at all — the drawn cells overlap even though
//! the real ones do not. A firm cell with a smaller `PACKING` would be cut less by arithmetic
//! rather than by a new rule, and needs no agreement between cells because each already computes
//! the pair's seam from both radii.
//!
//! The second is the cheaper experiment and does not need the physics to move. It is the one to
//! try next.
//!
//! # Built, and where it has to stop
//!
//! Firmness now shrinks the drawn radius from `slide::PACKING` towards one, and the far end
//! reaches 0.167 out of round where the swell alone stopped at 0.298. The obvious next question
//! is what happens past one, and the answer is worth having in a table rather than an opinion:
//!
//! ```text
//!   firmness   out of round   drawn apart while physically touching
//!       0.00          0.387     0%
//!       1.00          0.167     0%
//!       1.25          0.134    19%   worst 0.064 squares of daylight
//!       1.50          0.102    30%   worst 0.132
//!       2.00          0.043    65%   worst 0.268
//! ```
//!
//! **One is exactly the last honest setting, and that is not a coincidence** — it is where the
//! drawn radius equals the radius `mm-core` says the cell has. Past it the cells keep getting
//! rounder, all the way to 0.043 which is a sphere for practical purposes, and they do it by
//! being drawn smaller than they are: at two, two thirds of the pairs the *physics* has pressed
//! together are drawn with clear air between them.
//!
//! That is the mirror image of the fault `docs/OVERLAPS.md` was written about. Drawing a pair
//! apart when the simulation has them in contact is exactly as much of a lie as drawing them
//! through each other, and it is a worse kind of lie for being flattering — the picture looks
//! *better*, and every crowding, collision and packing result read off it is wrong.
//!
//! So `slide::packing_for` clamps at one and `phantom::Bench::packing` does not. The bench is
//! where a question like this gets answered; the renderer is where the answer gets enforced.

use mm_app::phantom::{Bench, Layout, Motion};

/// The area a drawn cell's outline encloses, by integrating its own radius function.
///
/// `½∫ρ(θ)²dθ`, which is exact for a star-shaped outline about the centre — and the clipped
/// shape is star-shaped about the centre, because every seam plane contains it.
fn outline_area(cell: &mm_app::phantom::Drawn) -> f32 {
    const RAYS: usize = 720;
    let step = std::f32::consts::TAU / RAYS as f32;
    let mut area = 0.0;
    for k in 0..RAYS {
        let r = cell.outline(k as f32 * step, false);
        area += 0.5 * r * r * step;
    }
    area
}

/// How far from round a drawn cell is: the spread of its own radius, over its largest.
///
/// Zero is a circle. A regular hexagon inscribed the same way reads about 0.13, a square about
/// 0.29. **This is the measure the question is actually about** — "squished into a polygon"
/// against "retains its individual shape" is a statement about the outline's shape, and area is
/// not, because `area_swell` is area-preserving by construction and barely moves when it is
/// switched off.
fn out_of_round(cell: &mm_app::phantom::Drawn) -> f32 {
    const RAYS: usize = 720;
    let step = std::f32::consts::TAU / RAYS as f32;
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for k in 0..RAYS {
        let r = cell.outline(k as f32 * step, false);
        lo = lo.min(r);
        hi = hi.max(r);
    }
    if hi <= 0.0 {
        return 0.0;
    }
    (hi - lo) / hi
}

fn bench(firmness: f32, spacing: f32) -> Bench {
    Bench {
        layout: Layout::Raft,
        spacing,
        // A lattice of identical cells never asks an awkward question, and neither does a
        // perfectly regular one. Some of both, fixed per cell, as the module asks for.
        spread: 0.3,
        dither: 0.1,
        motion: Motion::Still,
        amplitude: 0.0,
        speed: 0.0,
        firmness,
        ..Bench::default()
    }
}

/// Of the pairs the *physics* has in contact, how many are drawn with daylight between them.
///
/// The bench's own `wall_gap` cannot answer this and is not meant to: it asks whether two cells
/// whose *drawn* circles overlap meet properly along their shared wall, which is a question about
/// the seam. This asks whether the drawn circles overlap **at all** for a pair that is physically
/// touching — which is the fault a shrinking radius introduces, and the mirror image of the
/// overlapping `docs/OVERLAPS.md` was written about. Drawing a pair apart when the simulation has
/// them pressed together is exactly as much of a lie as drawing them through each other.
fn false_gaps(firmness: f32, spacing: f32) -> (f32, f32) {
    let b = bench(firmness, spacing);
    let blobs = b.blobs(0);
    let drawn = b.draw(&blobs, 0);
    let (mut touching, mut lying, mut worst) = (0u32, 0u32, 0.0f32);
    for i in 0..drawn.len() {
        for j in (i + 1)..drawn.len() {
            let (a, c) = (&drawn[i], &drawn[j]);
            let d = ((a.blob.x - c.blob.x).powi(2) + (a.blob.y - c.blob.y).powi(2)).sqrt();
            // Physically in contact: the radii `mm-core` would report, not the drawn ones.
            if d >= a.blob.r + c.blob.r {
                continue;
            }
            touching += 1;
            let daylight = d - (a.bare + c.bare);
            if daylight > 0.0 {
                lying += 1;
                worst = worst.max(daylight);
            }
        }
    }
    (
        if touching == 0 {
            0.0
        } else {
            lying as f32 * 100.0 / touching as f32
        },
        worst,
    )
}

/// Mean swell, the fraction of the lattice covered, and how far from round the outlines are.
fn measure(firmness: f32, spacing: f32) -> (f32, f32, f32) {
    let b = bench(firmness, spacing);
    let blobs = b.blobs(0);
    let drawn = b.draw(&blobs, 0);
    let n = drawn.len().max(1) as f32;
    let swell = drawn.iter().map(|c| c.swell).sum::<f32>() / n;

    // The area the lattice gives each cell, which is what "no gaps" would mean. Taken from the
    // bounding box of the centres rather than assumed, so a change to the layout cannot make this
    // quietly wrong.
    let (mut lo_x, mut hi_x, mut lo_y, mut hi_y) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for c in &drawn {
        lo_x = lo_x.min(c.blob.x);
        hi_x = hi_x.max(c.blob.x);
        lo_y = lo_y.min(c.blob.y);
        hi_y = hi_y.max(c.blob.y);
    }
    let span = ((hi_x - lo_x) * (hi_y - lo_y)).max(f32::EPSILON);
    let covered: f32 = drawn.iter().map(outline_area).sum();
    // Cells on the rim of the raft hang outside the box the centres span, so the raw ratio runs
    // over one. Normalised against the softest case, which *is* the no-gaps picture by
    // construction — so this reads as "how much of the tessellation's coverage is left".
    let round = drawn.iter().map(out_of_round).sum::<f32>() / n;
    (swell, covered / span, round)
}

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn what_firmness_does_to_the_picture() {
    println!(
        "\nFIRMNESS  a raft of 37 phantom cells, still, all-pairs seams, no faults injected.\n\
         `swell` is how much larger than its true radius a cell is drawn.\n\
         `fill` is outline area over the area the lattice gives them, against the softest case."
    );
    for spacing in [0.95f32, 1.0, 1.1] {
        println!(
            "\n  spacing {spacing:.2}  {}",
            match spacing {
                s if s < 1.0 => "(genuinely interpenetrating)",
                s if s < 1.05 => "(exactly touching — what the physics drives a pack to)",
                _ => "(loose: nothing quite touches)",
            }
        );
        println!(
            "  firmness    swell   area kept   out of round   drawn apart while touching"
        );
        let (_, soft_fill, _) = measure(0.0, spacing);
        for firmness in [0.0f32, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0] {
            let (swell, fill, round) = measure(firmness, spacing);
            let (lying, worst) = false_gaps(firmness, spacing);
            println!(
                "  {firmness:>8.2}   {swell:>6.3}   {:>8.0}%   {round:>12.3}   {lying:>10.0}%                  (worst {worst:.3} sq)",
                fill * 100.0 / soft_fill.max(f32::EPSILON),
            );
        }
    }
    println!(
        "\n  To see it rather than read it:\n    \
         cargo run -p mm-app --bin shaderbench --features render --release\n  \
         and drag `firmness` in the panel, or MM_BENCH_FIRMNESS=1 for the far end."
    );
}

#[test]
fn the_renderer_never_draws_a_cell_smaller_than_it_is() {
    // The guard on the finding above. `Bench::packing` is deliberately open past one so that the
    // question "what would that look like" can be asked; `slide::packing_for` is not, because the
    // answer is that two thirds of touching pairs get drawn apart and every packing result read
    // off the picture becomes wrong.
    for f in [-1.0f32, 0.0, 0.5, 1.0, 1.5, 2.0, 100.0] {
        let p = mm_app::slide::packing_for(f);
        assert!(
            (1.0..=mm_app::slide::PACKING).contains(&p),
            "packing_for({f}) = {p}, outside 1.0..={}",
            mm_app::slide::PACKING
        );
    }
    // And the honest end really is honest: at firmness one, nothing physically touching is drawn
    // apart, at every spacing a pack can settle at.
    for spacing in [0.9f32, 0.95, 1.0] {
        let (lying, worst) = false_gaps(1.0, spacing);
        assert_eq!(
            lying, 0.0,
            "at spacing {spacing}, {lying}% of touching pairs drawn apart by up to {worst}"
        );
    }
}
