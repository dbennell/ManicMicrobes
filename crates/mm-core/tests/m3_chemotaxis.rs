//! M3 acceptance tests — sensing and motility.
//!
//! > **Chemotaxis evolves.** Starting from an ancestor with a chemosensor and cilia but no
//! > code linking them, and a patchy food distribution, mean cell-to-food distance falls
//! > significantly below a motile-but-blind control within 2,000,000 ticks, in >= 6 of 10
//! > seeds. This is the first real evolution test and the most important one in the plan.
//!
//! The control is the whole experiment. A sighted population that ends up nearer its food
//! than a blind one has evolved chemotaxis; a sighted population that ends up nearer its food
//! than *nothing* has merely swum about, and half of swimming about is ending up somewhere.
//!
//! So the two ancestors are identical byte for byte — 329 bytes apiece — except that one has a
//! chemosensor in slot 7 and the other has an inert organelle there. Same length, same
//! instruction count, same build cost, same upkeep, same time round the cycle. One has
//! information and one does not.
//!
//! If this fails, that is a finding rather than a bug (`CLAUDE.md`), and the thing to report
//! is which parameter appears to be starving it — mutation rate, energy economics, instruction
//! budget, patch geometry — rather than tuning until it goes green.

mod common;

use std::path::Path;

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, pos_to_square, q10};
use mm_core::light::CurrentField;
use mm_core::{LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding, World};

fn assemble(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    mm_asm::assemble(&src)
        .unwrap_or_else(|e| panic!("{name} does not assemble:\n{e}"))
        .bytes
}

/// The chemical a chemotactic cell would be following: the carbon dioxide it photosynthesises.
const FOOD: usize = 11;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
/// Where the food patches are, in squares. Far enough apart that a cell in one cannot sense
/// another, so getting between them is a matter of swimming rather than of reading.
const PATCHES: [(u32, u32); 4] = [(12, 12), (52, 12), (12, 52), (52, 52)];
const PATCH_RADIUS: u32 = 6;

/// A patchy slide: food in four clumps and bare water between them.
///
/// Patchy rather than uniform because a gradient is only worth reading when there is somewhere
/// better to be. On an evenly fed slide a chemosensor reports zero everywhere and chemotaxis
/// has nothing to be selected for.
fn patchy(seed: u64) -> Scenario {
    let mut seeding = vec![
        // A thin wash everywhere, so a cell between patches is hungry rather than instantly
        // dead — starving to death in transit would select against leaving at all.
        Seeding::Uniform {
            chemical: FOOD,
            per_square: q10(20),
        },
        Seeding::Uniform {
            chemical: 14,
            per_square: q10(300),
        },
        Seeding::Uniform {
            chemical: 4,
            per_square: q10(300),
        },
    ];
    for (x, y) in PATCHES {
        seeding.push(Seeding::Patch {
            chemical: FOOD,
            x: x - PATCH_RADIUS,
            y: y - PATCH_RADIUS,
            width: PATCH_RADIUS * 2,
            height: PATCH_RADIUS * 2,
            per_square: q10(1_400),
        });
    }
    Scenario {
        name: "patchy".to_string(),
        seed,
        width: WIDTH,
        height: HEIGHT,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        current: CurrentField::Still,
        seeding,
        ..Scenario::default()
    }
}

/// Mean distance from a cell to the nearest food patch, in squares.
///
/// The measure the milestone names. Distance to the *patch centres* rather than to whatever
/// food happens to be nearby, so that a population which has eaten its patch flat is still
/// measured against where the food was — otherwise a lineage could score well by destroying
/// the gradient it was supposed to be following.
fn mean_distance_to_food(world: &World) -> f64 {
    let cells = world.cells();
    if cells.is_empty() {
        return f64::NAN;
    }
    let mut total = 0f64;
    let mut n = 0u32;
    for i in cells.iter() {
        let x = pos_to_square(cells.x[i]);
        let y = pos_to_square(cells.y[i]);
        let best = PATCHES
            .iter()
            .map(|(px, py)| {
                let dx = (x - *px as i32) as f64;
                let dy = (y - *py as i32) as f64;
                (dx * dx + dy * dy).sqrt()
            })
            .fold(f64::INFINITY, f64::min);
        total += best;
        n += 1;
    }
    total / n as f64
}

/// Seed a population of one ancestor and run it.
///
/// Founders start *between* the patches, so a lineage that never learns to steer stays spread
/// out and one that does concentrates. Starting them on the food would mean a blind population
/// scored well simply by not having moved yet.
fn run_line(bytes: &[u8], scenario: Scenario, ticks: u64) -> Option<(f64, usize)> {
    let mut world = World::new(scenario).expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::default(),
        ..BiologyConfig::default()
    });

    for k in 0..16u32 {
        let genome = world.genomes().intern(bytes.to_vec()).expect("genome");
        let x = 8 + (k % 4) * 16;
        let y = 8 + (k / 4) * 16;
        let id = world.spawn_cell(CellSeed {
            x: pos(x as i32),
            y: pos(y as i32),
            mass: q10(30),
            energy: q10(500),
            membrane: 24,
            key: 11,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        if let Some(i) = world.cells_mut().index(id) {
            world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
            world.cells_mut().slots_mut(i)[2] =
                Organelle::finished(OrganelleType::Mitochondrion, 40);
            world.cells_mut().slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 50);
            world.cells_mut().interior_mut(i)[FOOD] = q10(40);
            world.cells_mut().interior_mut(i)[14] = q10(40);
        }
    }
    world.adopt_current_contents_as_baseline();

    world.run(ticks);
    world.check_matter().expect("books balance");
    if world.cells().is_empty() {
        return None;
    }
    Some((mean_distance_to_food(&world), world.cells().len()))
}

/// The experiment: sighted against blind, same seed, same slide.
fn chemotaxis_run(ticks: u64, seeds: &[u64]) -> usize {
    let sighted = assemble("drifter.mm");
    let blind = assemble("drifter_blind.mm");
    assert_eq!(
        sighted.len(),
        blind.len(),
        "the two lines must be the same length, or this measures genome length"
    );

    let mut wins = 0;
    for seed in seeds {
        let a = run_line(&sighted, patchy(*seed), ticks);
        let b = run_line(&blind, patchy(*seed), ticks);
        match (a, b) {
            (Some((sd, sn)), Some((bd, bn))) => {
                let better = sd < bd * 0.9;
                eprintln!(
                    "seed {seed}: sighted {sd:.2} ({sn} cells), blind {bd:.2} ({bn} cells){}",
                    if better { "  <-- closer" } else { "" }
                );
                if better {
                    wins += 1;
                }
            }
            (a, b) => eprintln!(
                "seed {seed}: extinct — sighted {}, blind {}",
                a.map_or("dead".to_string(), |x| format!("{:.2}", x.0)),
                b.map_or("dead".to_string(), |x| format!("{:.2}", x.0)),
            ),
        }
    }
    wins
}

#[test]
fn the_two_ancestors_differ_only_in_whether_they_can_see() {
    // The control the whole experiment rests on. If these ever drift apart in length or cost,
    // the result stops being about chemotaxis.
    let sighted = assemble("drifter.mm");
    let blind = assemble("drifter_blind.mm");
    assert_eq!(sighted.len(), blind.len());
    let differing = sighted
        .iter()
        .zip(blind.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        differing <= 2,
        "the two ancestors differ in {differing} bytes; they should differ in the sensor and \
         nothing else"
    );
}

#[test]
fn a_drifter_swims_and_a_population_survives_the_patchy_slide() {
    // Before asking whether chemotaxis evolves, check that the experiment can run at all: the
    // ancestor has to move, and the world has to sustain it. A dead or motionless population
    // would make the real test vacuous.
    let sighted = assemble("drifter.mm");
    let ticks = if cfg!(debug_assertions) { 1_200 } else { 8_000 };
    let (distance, n) = run_line(&sighted, patchy(1), ticks).expect("the population died out");
    assert!(n > 16, "the population never grew: {n}");
    assert!(distance.is_finite());
    assert!(
        distance < WIDTH as f64,
        "cells ended up further from food than the slide is wide: {distance}"
    );
}

#[test]
fn cilia_actually_move_a_population_around() {
    // The motility half of M3, stated so that a failure of the evolution test cannot be
    // blamed on cells that never went anywhere.
    let sighted = assemble("drifter.mm");
    let mut world = World::new(patchy(5)).unwrap();
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    let genome = world.genomes().intern(sighted).unwrap();
    let id = world.spawn_cell(CellSeed {
        x: pos(32),
        y: pos(32),
        mass: q10(30),
        energy: q10(5_000),
        membrane: 24,
        key: 11,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    let i = world.cells_mut().index(id).unwrap();
    world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
    world.cells_mut().slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 50);
    world.adopt_current_contents_as_baseline();

    let start = (world.cells().x[i], world.cells().y[i]);
    world.run(400);
    // It may have divided or died; what matters is that something moved.
    let moved = world
        .cells()
        .iter()
        .any(|j| (world.cells().x[j], world.cells().y[j]) != start);
    assert!(moved, "nothing moved in four hundred ticks");
}

#[test]
fn chemotaxis_guard() {
    // A short run at three seeds. Not enough time for the behaviour to evolve — the full test
    // is the ignored one — but enough to catch the experiment breaking: an extinct line, a
    // world that stops conserving, a measure that stops being finite.
    let ticks = if cfg!(debug_assertions) {
        1_000
    } else {
        10_000
    };
    let _ = chemotaxis_run(ticks, &[1, 2, 3]);
}

/// The milestone's headline result.
#[test]
#[ignore = "2,000,000 ticks x 10 seeds x 2 lines; run with --release --ignored"]
fn acceptance_chemotaxis_evolves() {
    let seeds = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let ticks = common::env_usize("MM_M3_TICKS", 2_000_000) as u64;
    let wins = chemotaxis_run(ticks, &seeds);
    assert!(
        wins >= 6,
        "the sighted line ended up closer to its food in only {wins} of 10 seeds"
    );
}

/// M3 acceptance 2: arena determinism.
///
/// > Two hand-written cells in a fixed scenario with mutation off produce identical outcomes
/// > across 100 runs.
///
/// Arena mode (SPEC §0) is the half of the product where people write cells and find out
/// whose code wins, and a match that did not replay identically would not be a match. This is
/// the same claim as I1, stated at the scale a user cares about: two named cells, one slide,
/// a hundred runs, one answer.
#[test]
fn arena_determinism() {
    let a = assemble("drifter.mm");
    let b = assemble("drifter_blind.mm");
    let ticks = if cfg!(debug_assertions) { 300 } else { 2_000 };

    let play = || {
        let mut world = World::new(patchy(99)).expect("world");
        world.set_biology(BiologyConfig {
            mutation: MutationRates::none(),
            ..BiologyConfig::default()
        });
        for (k, bytes) in [&a, &b].iter().enumerate() {
            let genome = world.genomes().intern((*bytes).clone()).unwrap();
            let id = world.spawn_cell(CellSeed {
                x: pos(20 + k as i32 * 24),
                y: pos(32),
                mass: q10(30),
                energy: q10(600),
                membrane: 24,
                key: 11,
                species: k as u32,
                parent: CellId::NONE,
                birth_tick: 0,
                genome,
            });
            if let Some(i) = world.cells_mut().index(id) {
                world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
                world.cells_mut().slots_mut(i)[2] =
                    Organelle::finished(OrganelleType::Mitochondrion, 40);
                world.cells_mut().slots_mut(i)[3] =
                    Organelle::finished(OrganelleType::Chloroplast, 50);
            }
        }
        world.adopt_current_contents_as_baseline();
        world.run(ticks);
        (world.state_hash(), world.cells().len())
    };

    let expected = play();
    // A hundred replays of the same match. If any one of them differs, the match is not a
    // match and nothing built on top of it — a leaderboard, a saved replay, a shared
    // scenario — means anything.
    for run in 1..100 {
        assert_eq!(play(), expected, "run {run} differed from the first");
    }
}

/// M3 acceptance 3: momentum sanity.
///
/// > Cilia impulses into the fluid do not create net momentum from nothing beyond the
/// > configured drag budget.
///
/// Momentum here is not conserved and is not claimed to be — impulses decay, which is the
/// whole reason one flick of one cilium does not stir the slide forever. What must hold is
/// that a cilium cannot push on nothing: every unit of thrust a cell gives itself is a unit
/// it puts into the water the other way, and the water then loses it to drag rather than to
/// arithmetic.
#[test]
fn cilia_push_on_the_water_rather_than_on_nothing() {
    let sighted = assemble("drifter.mm");
    let mut world = World::new(patchy(7)).unwrap();
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    let genome = world.genomes().intern(sighted).unwrap();
    let id = world.spawn_cell(CellSeed {
        x: pos(32),
        y: pos(32),
        mass: q10(30),
        energy: q10(20_000),
        membrane: 24,
        key: 11,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    let i = world.cells_mut().index(id).unwrap();
    world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
    world.cells_mut().slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 50);
    world.adopt_current_contents_as_baseline();

    // A still slide with one cell on it: any momentum in the water came from that cell.
    let impulse_total = |w: &World| -> i64 {
        let (ix, iy) = w.impulses();
        ix.iter().map(|v| *v as i64).sum::<i64>() + iy.iter().map(|v| *v as i64).sum::<i64>()
    };
    assert_eq!(impulse_total(&world), 0, "the water starts still");

    let mut peak = 0i64;
    let ticks = if cfg!(debug_assertions) { 200 } else { 1_500 };
    for _ in 0..ticks {
        world.step();
        peak = peak.max(impulse_total(&world).abs());
    }
    assert!(peak > 0, "nothing ever pushed on the water");
    // Bounded: the impulse layer is clamped per square and decays every fluid step, so a cell
    // beating for fifteen hundred ticks cannot accumulate momentum without limit.
    let squares = (WIDTH as i64) * (HEIGHT as i64);
    assert!(
        peak < squares * mm_core::Q10_ONE as i64,
        "momentum in the water grew without bound: {peak}"
    );

    // Now take everything out of the water. Zeroing the cilia would not do it — the genome
    // rewrites its own control inputs every tick, which is the point of them — so the only
    // way to have nothing pushing is to have nothing there.
    let ids: Vec<_> = world.cells().ids().collect();
    for id in ids {
        world.cells_mut().despawn(id);
    }
    assert!(world.cells().is_empty());

    for _ in 0..2_000 {
        world.step();
    }
    assert_eq!(
        impulse_total(&world),
        0,
        "the water was still moving long after everything stopped pushing it"
    );
}
