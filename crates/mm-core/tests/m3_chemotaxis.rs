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

/// Mean distance to food for cells scattered at random over the whole slide.
///
/// The null. Added after the first full run came back with numbers that looked like a result
/// and were not: the sighted line averaged 13.49 against this baseline's 13.13, which is to
/// say it was sitting exactly where cells with no sensor at all would sit. Without the null
/// printed next to them, "sighted 13.49, blind 14.32" reads as chemotaxis, and it is not.
///
/// Computed rather than hard-coded, so it stays true if the patches or the slide move.
fn scattered_baseline() -> f64 {
    let mut total = 0f64;
    for x in 0..WIDTH as i32 {
        for y in 0..HEIGHT as i32 {
            total += PATCHES
                .iter()
                .map(|(px, py)| {
                    let dx = (x - *px as i32) as f64;
                    let dy = (y - *py as i32) as f64;
                    (dx * dx + dy * dy).sqrt()
                })
                .fold(f64::INFINITY, f64::min);
        }
    }
    total / (WIDTH as f64 * HEIGHT as f64)
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

    let null = scattered_baseline();
    eprintln!(
        "cells scattered at random would score {null:.2}; sitting in the patches would score \
         about {:.0}. A line that has not learnt to steer scores the first.",
        PATCH_RADIUS as f64
    );

    let mut wins = 0;
    let mut sighted_total = 0f64;
    let mut blind_total = 0f64;
    let mut counted = 0f64;
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
                sighted_total += sd;
                blind_total += bd;
                counted += 1.0;
            }
            (a, b) => eprintln!(
                "seed {seed}: extinct — sighted {}, blind {}",
                a.map_or("dead".to_string(), |x| format!("{:.2}", x.0)),
                b.map_or("dead".to_string(), |x| format!("{:.2}", x.0)),
            ),
        }
    }
    if counted > 0.0 {
        let (sm, bm) = (sighted_total / counted, blind_total / counted);
        eprintln!(
            "means: sighted {sm:.2}, blind {bm:.2}, random scatter {null:.2} — sighted is {:.0}% \
             of the way from random to the patches",
            (null - sm) / (null - PATCH_RADIUS as f64) * 100.0
        );
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

#[test]
fn there_is_a_gradient_to_climb_where_the_cells_actually_are() {
    // Written to answer the third question in the failure note below, and it is the one that
    // decides whether the whole experiment is measuring anything: a chemosensor that reads
    // zero in both directions cannot be selected for, however long the run is.
    //
    // Checks the food field itself, after letting diffusion do whatever it is going to do, at
    // the place the founders start — between the patches, which is the whole point of starting
    // them there.
    let mut world = World::new(patchy(1)).unwrap();
    // Long enough for diffusion to have smeared the patch edges as far as it is going to
    // matter within a founder's lifetime.
    world.run(2_000);

    let food = world.substrate().chem_plane(FOOD);
    let at = |x: i32, y: i32| -> i64 {
        let i = (y.clamp(0, HEIGHT as i32 - 1) as usize) * WIDTH as usize
            + x.clamp(0, WIDTH as i32 - 1) as usize;
        food.get(i).copied().unwrap_or(0) as i64
    };

    // Walk the line from the centre of the slide towards one patch and report what a cell
    // standing at each point would have to work with. A gradient is readable if the difference
    // across a cell's own sensing span is bigger than the noise it is reading through.
    let (px, py) = (PATCHES[0].0 as i32, PATCHES[0].1 as i32);
    let (cx, cy) = (WIDTH as i32 / 2, HEIGHT as i32 / 2);
    let mut readable = 0;
    let mut steps = 0;
    eprintln!("food along the line from the centre ({cx},{cy}) to patch 0 ({px},{py}):");
    for k in 0..=10 {
        let x = cx + (px - cx) * k / 10;
        let y = cy + (py - cy) * k / 10;
        // What a chemosensor compares: this square against its neighbour one step nearer.
        let here = at(x, y);
        let nearer = at(x + (px - cx).signum(), y + (py - cy).signum());
        let delta = nearer - here;
        steps += 1;
        if delta.abs() > 0 {
            readable += 1;
        }
        eprintln!(
            "  ({x:>2},{y:>2})  food {:>8}  step towards patch {delta:>+8}{}",
            here,
            if delta > 0 {
                "  uphill"
            } else if delta < 0 {
                "  downhill"
            } else {
                "  FLAT"
            }
        );
    }
    assert!(
        readable * 2 >= steps,
        "only {readable} of {steps} points on the way to a patch have any gradient at all; a \
         chemosensor is reading zero across most of the slide, so chemotaxis has nothing to be \
         selected on and the acceptance experiment cannot succeed however long it runs"
    );
}

#[test]
fn a_chemosensor_can_actually_read_the_gradient_it_is_standing_in() {
    // The companion to `there_is_a_gradient_to_climb_where_the_cells_actually_are`: that one
    // checks the gradient exists in the water, this one checks it survives the trip through
    // the sensor and into a genome's stack.
    //
    // It did not, and that is what starved the acceptance test below. A gradient of a few
    // hundred `Q10` units per square was being divided by 1024 along with the concentration
    // and arriving as a literal zero — at the centre of the slide, which is where the founders
    // start. See `sensing::GRADIENT_GAIN`.
    let mut world = World::new(patchy(1)).unwrap();
    world.run(2_000);

    // Standing between the patches, looking towards patch 0 at (12,12): both components
    // should be negative, since the patch is up and to the left of the centre.
    let reading = mm_core::sensing::sense_chemical(world.substrate(), FOOD, 24, 24);
    let mut sensor = Organelle::finished(OrganelleType::Chemosensor, 60);
    sensor.control[0] = FOOD as i16;

    let ctx = mm_core::sensing::SensorContext {
        substrate: world.substrate(),
        x: 24,
        y: 24,
        tick: 0,
        cell_key: 0,
        touch: mm_core::sensing::TouchReading::default(),
    };
    let gx = mm_core::sensing::read_sensor(&sensor, 1, ctx).expect("a chemosensor reads");
    let gy = mm_core::sensing::read_sensor(&sensor, 2, ctx).expect("a chemosensor reads");

    eprintln!(
        "at (24,24): raw gradient ({}, {}) -> genome sees ({gx}, {gy})",
        reading.gradient_x, reading.gradient_y
    );
    assert!(
        reading.gradient_x < 0 && reading.gradient_y < 0,
        "the raw field does not slope towards the patch at all: {reading:?}"
    );
    assert!(
        gx.abs() >= 8 && gy.abs() >= 8,
        "a genome standing in a real gradient sees ({gx}, {gy}); anything this close to zero \
         cannot be steered on, and chemotaxis cannot evolve from a signal that is not there"
    );
    assert!(
        gx < 0 && gy < 0,
        "the sensor points ({gx}, {gy}) — away from the food it is supposed to find"
    );
}

#[test]
#[ignore = "runs a live population for 25,000 ticks; --release --ignored"]
fn the_patches_still_exist_once_a_population_has_been_eating_them() {
    // The third question, and the one that decides whether the acceptance test below can be
    // won at all.
    //
    // `mean_distance_to_food` measures distance to where the food was *seeded*, deliberately,
    // so that a lineage cannot score well by eating its patch flat and destroying the
    // gradient. That is the right choice only while the patches are still there. If a
    // population strips them early, the test spends the rest of the run asking cells to
    // congregate on bare water — and a cell that did so would be selected against, not for.
    //
    // Reports rather than asserts a threshold: what the right contrast is depends on a
    // judgement about the scenario, and the number is what that judgement needs.
    let bytes = assemble("drifter.mm");
    let mut world = World::new(patchy(1)).unwrap();
    world.set_biology(BiologyConfig {
        mutation: MutationRates::default(),
        ..BiologyConfig::default()
    });
    for k in 0..16u32 {
        let genome = world.genomes().intern(bytes.clone()).expect("genome");
        let id = world.spawn_cell(CellSeed {
            x: pos((8 + (k % 4) * 16) as i32),
            y: pos((8 + (k / 4) * 16) as i32),
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
            let cells = world.cells_mut();
            cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
            cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 40);
            cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 50);
            cells.interior_mut(i)[FOOD] = q10(40);
            cells.interior_mut(i)[14] = q10(40);
        }
    }
    world.adopt_current_contents_as_baseline();

    let sample = |world: &World| -> (i64, i64) {
        let food = world.substrate().chem_plane(FOOD);
        let at = |x: u32, y: u32| food[(y as usize) * WIDTH as usize + x as usize] as i64;
        let in_patches: i64 = PATCHES.iter().map(|(x, y)| at(*x, *y)).sum::<i64>() / 4;
        // The middle of the slide: as far from every patch as it is possible to be.
        let between = at(WIDTH / 2, HEIGHT / 2);
        (in_patches, between)
    };

    eprintln!("   tick    cells   food in patches   food between   contrast");
    for step in 0..=10 {
        if step > 0 {
            world.run(2_500);
        }
        let (patch, between) = sample(&world);
        eprintln!(
            "{:>7}  {:>7}  {:>16}  {:>13}  {:>7.2}x",
            world.tick_count(),
            world.cells().len(),
            patch,
            between,
            patch as f64 / between.max(1) as f64
        );
        if world.cells().is_empty() {
            break;
        }
    }
    eprintln!(
        "\nA contrast near 1.00 means the patches are gone and the slide is uniform: there is \
         no longer anywhere better to be, and `mean_distance_to_food` is measuring loyalty to \
         a memory."
    );
}

/// The milestone's headline result.
///
/// # It has not passed. Here is exactly how it failed.
///
/// Run at **200,000 ticks** — a tenth of the budget the milestone allows — over all ten seeds,
/// on 2026-07-28:
///
/// ```text
/// seed  1: sighted 16.04 (1857 cells), blind 13.60 (6441 cells)
/// seed  2: sighted 11.58 (15533),      blind 13.67 (4641)     <-- closer
/// seed  3: sighted 13.33 (6368),       blind 14.31 (1169)
/// seed  4: sighted 13.77 (3216),       blind 14.50 (12008)
/// seed  5: sighted 13.86 (5648),       blind 14.63 (169)
/// seed  6: sighted 14.39 (2150),       blind 15.32 (1520)
/// seed  7: sighted 13.54 (1445),       blind 16.11 (1011)     <-- closer
/// seed  8: sighted 11.87 (4897),       blind 13.30 (6556)     <-- closer
/// seed  9: sighted 13.35 (7998),       blind 13.68 (7357)
/// seed 10: sighted 13.21 (6420),       blind 14.12 (2175)
///
/// sighted mean 13.49    blind mean 14.32    random scatter 13.13
/// ```
///
/// The sighted line is closer in nine seeds of ten, which looks like a result and is not one.
/// **13.49 against a random-scatter baseline of 13.13 means the sighted population is sitting
/// where cells with no sensor at all would sit.** Chemotaxis would put it near the patches, at
/// four to six. Nothing has learnt to steer. What the nine-of-ten actually measures is the
/// blind line being pushed *further out than random* by swimming it cannot aim, while the
/// sighted line stays at the null — a difference in how badly each line is failing.
///
/// That is why [`scattered_baseline`] now prints alongside the result. Without it these
/// numbers read as evolution.
///
/// The `< 0.9` margin in [`chemotaxis_run`] is the milestone's "significantly below", and it
/// is **not to be relaxed**. Three of ten seeds clear it; six are needed. Lowering it to 0.95
/// would report success for a population that has not moved towards its food at all.
///
/// # Two causes were found. The first is fixed. The second makes this scenario unwinnable.
///
/// **One — the sensor could not read the gradient.** Fixed; see `sensing::GRADIENT_GAIN` and
/// `a_chemosensor_can_actually_read_the_gradient_it_is_standing_in`. Both the concentration
/// and the gradient went through the same divide-by-1024, and a gradient is two or three
/// orders of magnitude smaller than an amount, so it arrived as a literal zero at the centre
/// of the slide. Gradients now have their own scale and read about -192 where they read 0.
///
/// **Two — the patches do not survive contact with a population, and this is fatal to the
/// scenario as written.** `the_patches_still_exist_once_a_population_has_been_eating_them`
/// measures the food at the patch centres against the middle of the slide, with the drifter
/// line living on it:
///
/// ```text
///    tick    cells   food in patches   food between   contrast
///       0       16           1454080          20480     71.00x
///    2500    15368              3483           5520      0.63x
///    5000    14887              2302           1418      1.62x
///   10000    13037               571            532      1.07x
///   25000    22638               391            277      1.41x
/// ```
///
/// The patches are gone inside 2,500 ticks — a hundredth of this run and an eight-hundredth
/// of the 2,000,000 the milestone allows. After that the slide is uniform, and the contrast
/// spends as much time *below* one as above it: the patch sites are repeatedly **poorer** than
/// open water, because that is where the cells are and they are eating.
///
/// So for better than 98% of the run there is nowhere better to be. `mean_distance_to_food`
/// measures distance to where food was *seeded* — chosen deliberately, so that a lineage
/// cannot score well by eating its patch flat — and the consequence is that the test spends
/// almost all of its length asking cells to congregate on bare water. A cell that did so
/// would be selected *against*. Chemotaxis is not being starved of time here; it is being
/// asked for and then punished.
///
/// That also explains the numbers above. Founders start at mean distance 11.98, closer than
/// random scatter's 13.13. Both lines end near 14.2 — worse than random — because the
/// survivors are the ones that left the stripped patch sites. The nine-of-ten was never a
/// signal.
///
/// # What this needs, and why it is not done here
///
/// The patches have to be a *source*, not a stock: food replenished at the patch centres so
/// the gradient persists for the length of the run. That is a change to what the scenario
/// means — a standing gradient is a different world from a depleting one, with different
/// carrying capacity and different selection — and `Seeding` has no notion of a source, so it
/// is a mechanism to add rather than a constant to tweak. CLAUDE.md says to flag a design
/// decision like that rather than pick whichever reading is easier, so it is flagged.
///
/// The `< 0.9` margin in [`chemotaxis_run`] is the milestone's "significantly below" and is
/// **not to be relaxed**. Nothing above is a reason to lower it; all of it is a reason the
/// scenario cannot currently reach it.
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
