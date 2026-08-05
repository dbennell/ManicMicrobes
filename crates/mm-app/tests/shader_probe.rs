//! What the cell shader is given, and what its own field does with it, as numbers.
//!
//! The companion to `src/bin/shaderbench.rs`: the same phantom cells, the same seams, measured
//! instead of looked at. It needs no graphics stack, so it runs in ordinary CI and on a machine
//! with no display — which is the difference between a property that is checked and a property
//! somebody remembers checking.
//!
//! Run the tables with:
//!
//! ```text
//! cargo test -p mm-app --test shader_probe -- --ignored --nocapture
//! ```
//!
//! # What it can and cannot say
//!
//! The outline here is `phantom::Drawn::outline`, a copy of `cell.wgsl`'s field in Rust. It sees
//! everything the shader computes from its inputs — the taper, the swell, the wobble, the seam
//! planes — and nothing about how those inputs reach the GPU or how they are sampled. So:
//!
//! * a fault it finds is in the data or in the field's arithmetic, and is real;
//! * a fault it does not find may still be in the attribute packing, the interpolation across the
//!   quad, or the pixel grid — and for those, the window is the instrument.

use mm_app::phantom::{self, Bench, Layout, Motion};

/// The zoom the bench window opens at, in pixels per substrate square. Only used to report a
/// distance in the units somebody looking at the screen is in.
const ZOOM: f32 = 70.0;

/// Every number one scene has to offer, gathered over a run of frames.
struct Run {
    frames: u64,
    /// Frames on which at least one pair was drawn overlapping with nothing between them.
    bad_frames: usize,
    /// Pairs, summed over frames.
    events: usize,
    /// The deepest overlap seen, as a fraction of the smaller cell's drawn radius.
    worst: f32,
    /// The furthest one cell was ever drawn past where its neighbour's outline ends, in squares.
    cross: f32,
    /// The worst daylight ever left between two cells pressed together, in squares.
    gap: f32,
    /// The most any cell's swell moved in one frame, as a fraction.
    swell_jump: f32,
    /// Cell-frames where the swell moved by more than one percent.
    resized: usize,
    /// Cell-frames where the seam count changed.
    churned: usize,
    /// The most any cell's outline moved in one frame, as a fraction of its own radius.
    outline_jump: f32,
    max_seams: usize,
}

fn run(bench: &Bench, frames: u64) -> Run {
    let mut r = Run {
        frames,
        bad_frames: 0,
        events: 0,
        worst: 0.0,
        cross: 0.0,
        gap: 0.0,
        swell_jump: 0.0,
        resized: 0,
        churned: 0,
        outline_jump: 0.0,
        max_seams: 0,
    };
    let mut previous = bench.frame(0);
    for f in 1..=frames {
        let now = bench.frame(f);
        let report = phantom::inspect(&now);
        if report.no_wall > 0 {
            r.bad_frames += 1;
            r.events += report.no_wall;
            r.worst = r.worst.max(report.worst);
        }
        r.cross = r.cross.max(report.wall_cross);
        r.gap = r.gap.max(report.wall_gap);
        r.max_seams = r.max_seams.max(report.max_seams);
        let flicker = phantom::flicker(&previous, &now);
        r.swell_jump = r.swell_jump.max(flicker.worst_swell);
        r.resized += flicker.resizing;
        r.churned += flicker.churned;
        r.outline_jump = r.outline_jump.max(flicker.worst_outline);
        previous = now;
    }
    r
}

fn header(what: &str) {
    println!(
        "\n{what}\n{:<26} {:>7} {:>6} {:>7} {:>9} {:>9} {:>8} {:>8} {:>8} {:>9}",
        "", "frames", "bad", "pairs", "worst over", "crossing", "gap", "Δswell", "resized",
        "Δoutline",
    );
}

fn row(label: &str, r: &Run) {
    println!(
        "{label:<26} {:>7} {:>6} {:>7} {:>8.1}% {:>9.5} {:>8.4} {:>7.1}% {:>8} {:>8.1}%",
        r.frames,
        r.bad_frames,
        r.events,
        100.0 * r.worst,
        r.cross,
        r.gap,
        100.0 * r.swell_jump,
        r.resized,
        100.0 * r.outline_jump,
    );
}

/// The scene the artefact was reported on: nine cells, mixed sizes, pressed to touching.
fn nine() -> Bench {
    Bench {
        layout: Layout::Nine,
        ..Bench::default()
    }
}

/// Does *movement alone* do it?
///
/// This is the question the whole bench exists for, and it is asked here with the data known
/// good: all-pairs seams, no cap, no churn, in reach. Drift and orbit are rigid — every distance
/// between every pair is preserved exactly — so if the numbers move under those, something is
/// wrong that no simulation could be responsible for.
#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn what_movement_alone_does() {
    header("nine cells, data correct by construction, 600 frames of each motion");
    for motion in Motion::ALL {
        let bench = Bench {
            motion,
            ..nine()
        };
        row(motion.label(), &run(&bench, 600));
    }

    header("and the same at ten times the amplitude");
    for motion in Motion::ALL {
        let bench = Bench {
            motion,
            amplitude: 0.5,
            ..nine()
        };
        row(motion.label(), &run(&bench, 600));
    }
}

/// Each suspected fault, injected on its own, against the same scene with none.
///
/// The point is the comparison: a fault that produces nothing here is not what the slide is
/// suffering from, however plausible it sounds, and one that reproduces the reported behaviour is
/// worth the next day. Every row is the same cells and the same motion; only the fault differs.
#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn what_each_injected_fault_does() {
    let base = Bench {
        motion: Motion::Jitter,
        ..nine()
    };
    header("nine cells, jittering, 600 frames, one fault at a time");
    row("none", &run(&base, 600));
    row(
        "seam cap 6",
        &run(&Bench { cap: 6, ..base }, 600),
    );
    row(
        "seam cap 4",
        &run(&Bench { cap: 4, ..base }, 600),
    );
    row(
        "reach 1.0 (outlines)",
        &run(&Bench { reach: 1.0, ..base }, 600),
    );
    row(
        "reach 0.9 (short)",
        &run(&Bench { reach: 0.9, ..base }, 600),
    );
    row(
        "churn 0.6/cell/frame",
        &run(&Bench { churn: 0.6, ..base }, 600),
    );
    row(
        "radius staircase",
        &run(
            &Bench {
                staircase: true,
                ..base
            },
            600,
        ),
    );
    row(
        "no area swell",
        &run(&Bench { swell: false, ..base }, 600),
    );

    header("and on a raft of 37, where a cell has more neighbours than slots");
    let raft = Bench {
        layout: Layout::Raft,
        motion: Motion::Jitter,
        ..Bench::default()
    };
    row("none", &run(&raft, 600));
    row("seam cap 6", &run(&Bench { cap: 6, ..raft }, 600));
    row("churn 0.6", &run(&Bench { churn: 0.6, ..raft }, 600));
}

/// How much of the picture the swell moves for how little cause.
///
/// `slide::SWELL_GAIN` records the measurement that started this: on a settled pack, 14% of cells
/// changed size by more than a percent one tick apart and the worst by eleven, with their seam
/// sets unchanged. Here the cause is known exactly, so the sensitivity can be read off against it.
#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn how_sensitive_the_swell_is() {
    println!("\nnine cells, jittering, 600 frames: how far a cell must move to resize");
    println!(
        "{:>12} {:>10} {:>12} {:>12}",
        "amplitude", "in pixels", "worst Δswell", "resized"
    );
    for amplitude in [0.0f32, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1] {
        let bench = Bench {
            motion: Motion::Jitter,
            amplitude,
            ..nine()
        };
        let r = run(&bench, 600);
        // At the bench's default zoom, which is what the window opens at.
        println!(
            "{amplitude:>12.3} {:>10.2} {:>11.1}% {:>12}",
            amplitude * ZOOM,
            100.0 * r.swell_jump,
            r.resized
        );
    }
}

/// How much of the swell is the area integral's own sampling.
///
/// A rigid rotation cannot change any distance between any two cells, so every seam plane and
/// every face is exactly where it was. But `slide::area_swell` measures the clipped area along
/// `SWELL_RAYS` directions fixed in the *world*, and turning the neighbourhood past those rays
/// changes the quadrature — so a cell whose neighbours merely swing around it is drawn a
/// different size. Nothing upstream moved; the number did.
///
/// Reported as a percentage of the cell's radius and as pixels at the bench's own zoom, because
/// a tenth of a percent and a tenth of a pixel are different findings.
#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn how_much_of_the_swell_is_quadrature() {
    println!("\na rigid turn, which changes no distance at all: what the swell does anyway");
    println!(
        "{:<10} {:>10} {:>14} {:>14} {:>12}",
        "layout", "cells", "worst range", "worst Δ/frame", "at 110 px/sq"
    );
    for layout in [Layout::Pair, Layout::Nine, Layout::Raft] {
        let bench = Bench {
            layout,
            motion: Motion::Orbit,
            // A quarter of a radian per unit of time, and 0.02 of that a frame: a full turn takes
            // about 1250 frames, so every ray is swept past slowly.
            amplitude: 0.25,
            speed: 0.02,
            ..Bench::default()
        };
        let turn = 1257u64;
        let first = bench.frame(0);
        let mut lo = vec![f32::MAX; first.len()];
        let mut hi = vec![0.0f32; first.len()];
        let mut worst_step = 0.0f32;
        let mut previous = first;
        for f in 1..=turn {
            let now = bench.frame(f);
            for (k, c) in now.iter().enumerate() {
                lo[k] = lo[k].min(c.swell);
                hi[k] = hi[k].max(c.swell);
                worst_step = worst_step.max((c.swell - previous[k].swell).abs() / previous[k].swell);
            }
            previous = now;
        }
        let range = lo
            .iter()
            .zip(hi.iter())
            .map(|(a, b)| (b - a) / a)
            .fold(0.0f32, f32::max);
        // The bench's default radius, in pixels, times the range.
        println!(
            "{:<10} {:>10} {:>13.2}% {:>13.3}% {:>10.2} px",
            layout.label(),
            lo.len(),
            100.0 * range,
            100.0 * worst_step,
            range * 0.9 * 1.15 * ZOOM,
        );
    }
}

/// How many of a cell's seam slots are spent on neighbours that never cut it?
///
/// The reach is deliberately generous — `slide::PACKING_PERMILLE` admits a contact at 1.75 times
/// the two *physical* radii, which is 1.52 times the two drawn ones — because a pair that is not
/// overlapping yet costs one half-plane test that clips nothing, and a pair that arrives late
/// costs the picture. But a slot is not free: `mm_core::CONTACTS_PER_CELL` and
/// `cellmesh::SQUASH_PER_CELL` are both twelve, and a seam that cuts nothing still occupies one.
///
/// So this counts, per cell, how many admitted seams have their plane at or beyond the cell's own
/// drawn outline — a seam that cannot touch it whatever direction is asked. If that number is
/// large the reach is spending slots on pairs nobody needs to worry about, and the deepest
/// contacts are competing with them for room.
#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn how_many_seams_are_doing_nothing() {
    println!("\nseams admitted against seams that actually cut, uncapped, over 200 frames");
    println!(
        "{:<10} {:>6} {:>9} {:>9} {:>9} {:>9} {:>10}",
        "layout", "reach", "cells", "seams", "idle", "idle %", "worst cell"
    );
    for layout in Layout::ALL {
        for reach in [1.0f32, 1.25, 1.52, 1.75] {
            let bench = Bench {
                layout,
                motion: Motion::Jitter,
                reach,
                // Uncapped, so what is counted is what the reach admits rather than what
                // survived a truncation.
                cap: 64,
                ..Bench::default()
            };
            let (mut cells, mut seams, mut idle, mut worst) = (0usize, 0usize, 0usize, 0usize);
            let (mut most, mut most_cutting) = (0usize, 0usize);
            for f in 0..200u64 {
                for c in bench.frame(f) {
                    cells += 1;
                    seams += c.seams.len();
                    most = most.max(c.seams.len());
                    // The plane sits at `face` of the swollen radius. At or past one it is outside
                    // the outline in its own direction and cuts nothing anywhere.
                    let doing_nothing = c.seams.iter().filter(|s| s.face >= 1.0).count();
                    most_cutting = most_cutting.max(c.seams.len() - doing_nothing);
                    idle += doing_nothing;
                    worst = worst.max(doing_nothing);
                }
            }
            println!(
                "{:<10} {reach:>6.2} {:>9} {:>9} {:>9} {:>8.0}% {:>10}   \
                 (most seams {most}, most that cut {most_cutting})",
                layout.label(),
                cells,
                seams,
                idle,
                100.0 * idle as f32 / seams.max(1) as f32,
                worst,
            );
        }
    }
    println!(
        "\n  A cell has {} slots. Anything in the 'most seams' column above that is a cell whose \
         seams were truncated on the way to the shader.",
        mm_app::cellmesh::SQUASH_PER_CELL
    );
}

/// Correct data draws no overlapping pairs, under any motion.
///
/// The permanent version of the bench's central claim, and a genuine regression test for the seam
/// geometry that needs no world to run: if a change to `slide::seam_between` or
/// `slide::area_swell` ever stops two cells agreeing about their shared wall, this fails here
/// rather than three days later in a screenshot.
#[test]
fn cells_given_correct_seams_are_never_drawn_over_each_other() {
    for motion in Motion::ALL {
        for layout in Layout::ALL {
            let bench = Bench {
                motion,
                layout,
                ..Bench::default()
            };
            let r = run(&bench, 120);
            assert_eq!(
                r.events,
                0,
                "{} cells {}: {} pairs drawn overlapping with no wall between them, worst {:.1}% \
                 of a radius, on {} of {} frames — with all-pairs seams, no cap and nothing \
                 dropped. The data cannot be blamed for this one.",
                layout.label(),
                motion.label(),
                r.events,
                100.0 * r.worst,
                r.bad_frames,
                r.frames,
            );
            assert!(
                r.cross < 2e-3,
                "{} cells {}: a cell is drawn {:.5} squares past where its neighbour's outline \
                 ends. The seam is one plane both of them computed from the same two centres and \
                 the same two radii, so neither may cross it.",
                layout.label(),
                motion.label(),
                r.cross,
            );
        }
    }
}

/// A rigid motion changes nothing the shader is given, to within the area integral's sampling.
///
/// Drift moves every cell by one offset and orbit turns them all about the same point, so every
/// distance between every pair is preserved and every seam plane keeps its face exactly. That is
/// what makes those two motions the sharp instrument they are: anything that changes on screen
/// under them changed *after* this point — in the shader, in the attribute packing, or in the
/// pixel grid.
///
/// The one thing that does move is the swell, and by a measured amount:
/// `how_much_of_the_swell_is_quadrature` reports **0.12% of a radius over a whole turn, 0.14 of a
/// pixel** at the bench's zoom, because `area_swell` samples the clipped area along rays fixed in
/// the world and a turning neighbourhood sweeps past them. Real, and far too small to be the
/// artefact — which is worth knowing, since "the swell resizes cells for no reason" was a live
/// hypothesis. The tolerance here is that measurement with room, not a number picked to pass.
#[test]
fn a_rigid_motion_leaves_every_number_where_it_was() {
    for motion in [Motion::Drift, Motion::Orbit] {
        let bench = Bench {
            motion,
            amplitude: 0.25,
            ..nine()
        };
        let first = bench.frame(0);
        for f in [1u64, 7, 61, 300, 1201] {
            let now = bench.frame(f);
            for (a, b) in first.iter().zip(now.iter()) {
                assert_eq!(
                    a.seams.len(),
                    b.seams.len(),
                    "{}: cell {} gained or lost a seam under a rigid motion at frame {f}",
                    motion.label(),
                    a.blob.id,
                );
                // The faces are what the two cells of a pair have to agree on, and they depend on
                // nothing but the two centres and the two radii — neither of which a rigid motion
                // touches.
                //
                // Measured *times the swell*, which is the face as a fraction of the unswollen
                // radius and therefore the plane itself. The stored face is that divided by the
                // swell, so it inherits the swell's quadrature wobble: it moved by 1.5 parts in ten
                // thousand here, all of it the area integral and none of it the plane. Worth
                // keeping straight, because the number the shader reads is the stored one.
                for (p, q) in a.seams.iter().zip(b.seams.iter()) {
                    let (plane, was) = (q.face * b.swell, p.face * a.swell);
                    assert!(
                        (plane - was).abs() / was.abs().max(1e-3) < 1e-5,
                        "{}: a seam plane moved from {was:.7} to {plane:.7} under a rigid motion \
                         at frame {f}",
                        motion.label(),
                    );
                }
                assert!(
                    (a.swell - b.swell).abs() / a.swell < 5e-3,
                    "{}: the swell moved from {:.5} to {:.5} at frame {f}, which is more than the \
                     0.12% the ray quadrature accounts for",
                    motion.label(),
                    a.swell,
                    b.swell,
                );
            }
        }
    }
}

/// The detector finds the faults it is pointed at.
///
/// A test that reports zero is only worth something if it can report anything else. Each of these
/// is a fault the real path has actually had — a cap that truncates, a reach that falls short, a
/// contact set that churns — and each must show up as overlapping pairs, or the two tests above
/// are measuring nothing.
#[test]
fn the_detector_is_sensitive_to_the_faults_it_is_for() {
    let base = Bench {
        motion: Motion::Jitter,
        layout: Layout::Raft,
        ..Bench::default()
    };
    for (what, bench) in [
        ("a seam cap of four", Bench { cap: 4, ..base }),
        ("a reach that stops short", Bench { reach: 0.9, ..base }),
        ("a churning contact set", Bench { churn: 0.6, ..base }),
    ] {
        let r = run(&bench, 120);
        assert!(
            r.events > 0,
            "{what} produced no overlapping pairs at all, so the detector is not measuring what \
             it claims to"
        );
    }
}

/// The closed-form swell agrees with the bisection it replaced.
///
/// `slide::area_swell` used to find its scale by sixteen steps of bisection over `[1, MAX_SWELL]`,
/// summing all `SWELL_RAYS` rays at each step. It now inverts the piecewise quadratic directly.
/// That is the same function evaluated a different way, so it has to give the same answer — and
/// where it differs it must be because the *bisection* was the approximate one: sixteen steps over
/// a range of a quarter leaves about 4·10⁻⁶ of slack.
///
/// Checked against a reference bisection written out here, over every arrangement the phantom
/// has, at several spacings, which is thousands of real seam sets rather than invented ones.
#[test]
fn the_closed_form_swell_agrees_with_the_bisection_it_replaced() {
    /// What `area_swell` used to do, kept as the thing the new one is measured against.
    fn by_bisection(radius: f32, want_radius: f32, seams: &[mm_app::slide::Squash]) -> f32 {
        const RAYS: usize = 64;
        const MAX_SWELL: f32 = 1.25;
        if seams.is_empty() || radius <= 0.0 {
            return 1.0;
        }
        let mut reach = [f32::INFINITY; RAYS];
        for (j, r) in reach.iter_mut().enumerate() {
            let theta = std::f32::consts::TAU * j as f32 / RAYS as f32;
            let (sy, sx) = theta.sin_cos();
            for s in seams {
                let along = sx * s.nx + sy * s.ny;
                if along > 1e-4 {
                    *r = r.min((s.face * radius) / along);
                }
            }
        }
        let target = std::f32::consts::PI * want_radius * want_radius;
        let area_at = |scale: f32| -> f32 {
            let r = radius * scale;
            let sum: f32 = reach.iter().map(|reach| reach.min(r) * reach.min(r)).sum();
            0.5 * sum * std::f32::consts::TAU / RAYS as f32
        };
        if area_at(MAX_SWELL) < target {
            return MAX_SWELL;
        }
        let (mut lo, mut hi) = (1.0f32, MAX_SWELL);
        for _ in 0..16 {
            let mid = 0.5 * (lo + hi);
            if area_at(mid) < target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    let mut compared = 0usize;
    let mut worst = 0.0f32;
    for layout in Layout::ALL {
        for spacing in [0.7f32, 0.85, 1.0, 1.15, 1.3] {
            let bench = Bench {
                layout,
                spacing,
                motion: Motion::Jitter,
                ..Bench::default()
            };
            for f in 0..12u64 {
                for cell in bench.frame(f) {
                    // The seams as `squash_of` has them at this point: fractions of the
                    // *unswollen* radius, before the divide by the swell.
                    let raw: Vec<mm_app::slide::Squash> = cell
                        .seams
                        .iter()
                        .map(|s| mm_app::slide::Squash {
                            nx: s.nx,
                            ny: s.ny,
                            face: s.face * cell.swell,
                        })
                        .collect();
                    let closed = mm_app::slide::area_swell(cell.bare, cell.bare, &raw);
                    let bisected = by_bisection(cell.bare, cell.bare, &raw);
                    worst = worst.max((closed - bisected).abs());
                    compared += 1;
                }
            }
        }
    }
    assert!(compared > 1000, "only {compared} cells compared");
    // The bisection's own slack is 0.25 / 2^16 ≈ 3.8e-6, and the closed form should land inside
    // it. Ten times that is a tolerance the difference cannot hide in and float noise can.
    assert!(
        worst < 4e-5,
        "the closed form and the bisection disagree by {worst:.3e} over {compared} cells, which \
         is more than the bisection's own convergence slack"
    );
    eprintln!("  {compared} cells: worst disagreement {worst:.3e} (bisection's own slack 3.8e-6)");
}
