//! A detector for the artefact itself, rather than for any theory about it.
//!
//! Everything up to here tested *hypotheses* — the seam-slot cap, seam membership, the swell
//! solve, the radius staircase, a stale index — and each was measured on a settled pack, which
//! is the one condition the artefact is reported *not* to appear in. That is how five
//! explanations came to be ruled out without the thing ever being seen.
//!
//! So: define it precisely and look for it.
//!
//! **Two cells are drawn overlapping with no shared wall when their drawn outlines reach past
//! each other and neither has a seam pointing at the other.** A seam is what cuts an outline
//! back; with no seam on either side both are drawn as full swollen discs in that direction, and
//! one is laid over the other. That is exactly the report: a rogue point extending over a
//! neighbour, as though the cell could not see it was there.
//!
//! The drawn extent in a free direction is `radius * PACKING * area_swell`, which is what the
//! shader draws to between facets.

use mm_app::slide::Slide;
use mm_core::{Scenario, World};

/// Must match `slide::PACKING`, which is private.
const PACKING: f32 = 1.15;

fn world_with(jitter: i32, ticks: u64) -> Slide {
    let genome = {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm");
        let src = std::fs::read_to_string(path).expect("genome");
        mm_asm::assemble(&src).expect("assembles").bytes
    };
    let mut world = World::new(Scenario {
        jitter,
        ..Scenario::stress(64, 64)
    })
    .expect("world");
    world.place_founders(&genome, 16);
    world.run(ticks);
    let mut slide = Slide::new(Scenario::stress(8, 8)).expect("slide");
    slide.set_world(world);
    // Everything in view and at the packed level of detail, or `squash` is not built at all and
    // every pair would look like it had no wall.
    slide.set_camera(32.0, 32.0, 64.0, 64.0);
    slide.set_zoom(64.0);
    slide
}

/// Pairs drawn overlapping with no wall between them, and how deep the overlap goes.
fn offenders(slide: &Slide) -> (usize, usize, f32) {
    let frame = slide.frame();
    let cells = &frame.cells;
    let mut pairs = 0usize;
    let mut worst = 0.0f32;
    let mut touching = 0usize;
    let mut worst_cases: Vec<(f32, usize, usize, f32, f32, f32)> = Vec::new();
    let mut bearings: Vec<(f32, f32)> = Vec::new();
    let mut first_case: Option<String> = None;
    for (a, i) in cells.iter().enumerate() {
        for j in cells.iter().skip(a + 1) {
            let (dx, dy) = (j.x - i.x, j.y - i.y);
            let d = (dx * dx + dy * dy).sqrt();
            if d <= 0.0001 {
                continue;
            }
            let ri = i.radius * PACKING * i.area_swell;
            let rj = j.radius * PACKING * j.area_swell;
            if d >= ri + rj {
                continue;
            }
            touching += 1;
            // Does either one have a seam pointing at the other? Matched by direction, which is
            // all a `Squash` carries.
            let (ux, uy) = (dx / d, dy / d);
            let i_sees = i.squash.iter().any(|s| s.nx * ux + s.ny * uy > 0.999);
            let j_sees = j.squash.iter().any(|s| s.nx * -ux + s.ny * -uy > 0.999);
            if !(i_sees && j_sees) {
                pairs += 1;
                worst = worst.max((ri + rj - d) / (ri + rj));
                // What is actually missing: a seam that is not there at all, or one that is
                // there and pointing somewhere else.
                let best_i = i
                    .squash
                    .iter()
                    .map(|s| s.nx * ux + s.ny * uy)
                    .fold(-1.0f32, f32::max);
                let best_j = j
                    .squash
                    .iter()
                    .map(|s| s.nx * -ux + s.ny * -uy)
                    .fold(-1.0f32, f32::max);
                // Which way the overlap points, from the cell that is missing the seam towards
                // the one it is laid over. Reported because the artefact is observed to run one
                // way — "eighty percent down, twenty side to side, none up" — and a direction
                // that strong is structural, not statistical.
                if !i_sees {
                    bearings.push((ux, uy));
                }
                if !j_sees {
                    bearings.push((-ux, -uy));
                }
                if first_case.is_none() {
                    // Everything about one pair, so the reason can be read rather than guessed.
                    // `PACKING_PERMILLE` admits a contact when `d < 1.5 * (ri + rj)` using
                    // `mm-core`'s radii, which are not the radii anything is *drawn* at.
                    let core_i = i.radius; // the drawn bare radius
                    let core_j = j.radius;
                    first_case = Some(format!(
                        "d {d:.4}  bare {core_i:.4}/{core_j:.4}  swell {:.3}/{:.3}  \
                         drawn {ri:.4}/{rj:.4}  sum {:.4}  admit-at {:.4}  seams {}/{}  \
                         sees {}/{}",
                        i.area_swell,
                        j.area_swell,
                        ri + rj,
                        1.5 * (core_i + core_j),
                        i.squash.len(),
                        j.squash.len(),
                        i_sees,
                        j_sees,
                    ));
                }
                worst_cases.push((
                    (ri + rj - d) / (ri + rj),
                    i.squash.len(),
                    j.squash.len(),
                    best_i,
                    best_j,
                    d / (ri + rj),
                ));
            }
        }
    }
    // World +y is *down* on screen: `to_screen` negates it.
    let down = bearings.iter().filter(|(_, y)| *y > 0.3).count();
    let up = bearings.iter().filter(|(_, y)| *y < -0.3).count();
    let side = bearings.len() - down - up;
    eprintln!(
        "    the overlap points: down {down}, up {up}, sideways {side}  (of {})",
        bearings.len()
    );
    if let Some(first) = first_case {
        eprintln!("    one offender in full: {first}");
    }
    worst_cases.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (depth, ni, nj, bi, bj, gap) in worst_cases.iter().take(5) {
        eprintln!(
            "    overlap {:.0}%  seams {ni}/{nj}  best alignment {bi:+.3}/{bj:+.3}  \
             d/(ri+rj) {gap:.3}",
            depth * 100.0
        );
    }
    (pairs, touching, worst)
}

#[test]
fn overlapping_pairs_with_no_wall_between_them() {
    eprintln!("\npairs drawn overlapping with no seam on both sides:");
    eprintln!(
        "{:>10}  {:>6}  {:>9}  {:>9}  {:>8}",
        "jitter", "cells", "touching", "no wall", "worst"
    );
    let mut counts = Vec::new();
    for jitter in [0, 24, 96] {
        let slide = world_with(jitter, 4000);
        let n = slide.frame().cells.len();
        let (bad, touching, worst) = offenders(&slide);
        eprintln!(
            "{jitter:>10}  {n:>6}  {touching:>9}  {bad:>9}  {:>7.1}%",
            worst * 100.0
        );
        counts.push(bad);
    }
    // What the detector found, so the next person does not have to rediscover it:
    //
    //   jitter   cells  touching  no wall     the artefact roughly doubles once anything moves,
    //        0     230       664       41     which is the reported behaviour exactly
    //       24     304       995      111
    //       96     422      1345      186
    //
    // And every offending pair has the same shape: one side's seam is perfectly aligned
    // (+1.000) and the other side's best is 8 to 23 degrees off — the *other* side does not have
    // that neighbour at all. The side that is missing it is, in every one of the worst cases,
    // the side sitting at exactly `CONTACTS_PER_CELL` seams. It ran out of slots.
    //
    // Raising `mm_core::CONTACTS_PER_CELL` alone, jitter 24: 111 -> 65 (16) -> 42 (20) -> 31
    // (24). Which is also why raising it has been tried before and appeared to do nothing:
    // `cellmesh::SQUASH_PER_CELL` is a *second* cap of twelve, and the extra contacts never
    // reach the shader. Both have to move.
    // Was "a jiggling pack has more of these than a still one", which is what it measured while
    // the artefact was there — 41 / 111 / 186 as the jitter rose. With both causes fixed the
    // counts are 4 / 4 / 3 and the dependence on movement is gone, which is the result and no
    // longer an ordering. So the guard is on the level instead.
    for (n, bad) in counts.iter().enumerate() {
        assert!(
            *bad <= 8,
            "{bad} pairs are drawn overlapping with no wall between them at jitter step {n}. \
             Both causes have to stay fixed: the contact reach has to cover what the renderer \
             actually draws to, and the seam cap has to be high enough that nobody hits it — \
             widening one alone trades one failure for the other."
        );
    }
}
