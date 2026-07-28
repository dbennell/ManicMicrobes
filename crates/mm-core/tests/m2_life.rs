//! M2 acceptance tests — cells, metabolism, division, mutation.
//!
//! > The first thing that is alive. A hand-written ancestor sustains a population
//! > indefinitely.

mod common;

use std::path::Path;

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
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

/// A slide with light, food, and no flow: the simplest world an autotroph can live in.
fn petri(seed: u64) -> Scenario {
    Scenario {
        name: "petri".to_string(),
        seed,
        // The same size as `scenarios/soup.ron`. A smaller slide does not sustain a
        // population now that respiration's byproduct is a real constraint, and a test whose
        // world dies of its own dimensions measures the dimensions.
        width: 64,
        height: 64,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        current: CurrentField::Still,
        seeding: vec![
            Seeding::Uniform {
                chemical: 11,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 14,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 4,
                per_square: q10(400),
            },
        ],
        ..Scenario::default()
    }
}

fn seed_ancestors(world: &mut World, bytes: &[u8], n: u32) {
    for k in 0..n {
        let genome = world
            .genomes()
            .intern(bytes.to_vec())
            .expect("ancestor genome");
        let x = 6 + (k % 6) * 9;
        let y = 6 + (k / 6) * 9;
        let id = world.spawn_cell(CellSeed {
            x: pos(x as i32),
            y: pos(y as i32),
            mass: q10(30),
            energy: q10(400),
            membrane: 24,
            key: 11,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        // Hand it the organelles its build gene would otherwise take many ticks to afford,
        // so the test measures whether it can *live*, not whether it can bootstrap.
        if let Some(i) = world.cells_mut().index(id) {
            world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
            world.cells_mut().slots_mut(i)[2] =
                Organelle::finished(OrganelleType::Mitochondrion, 50);
            world.cells_mut().slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
            world.cells_mut().interior_mut(i)[11] = q10(40);
            world.cells_mut().interior_mut(i)[14] = q10(40);
        }
    }
    // Filling a cytoplasm by hand creates matter, which is what scenario setup is for. Said
    // out loud here rather than left for the conservation check to trip over.
    world.adopt_current_contents_as_baseline();
}

#[test]
fn a_cell_runs_its_genome_and_touches_the_world() {
    let bytes = assemble("ancestor.mm");
    let mut world = World::new(petri(1)).unwrap();
    seed_ancestors(&mut world, &bytes, 1);
    assert_eq!(world.cells().len(), 1);

    let before = world.total_matter();
    world.run(200);
    world
        .check_matter()
        .expect("a living cell broke matter conservation");
    assert_eq!(
        world.total_matter().iter().sum::<i64>(),
        before.iter().sum::<i64>(),
        "total matter moved"
    );
}

#[test]
fn matter_is_conserved_with_life_in_the_world() {
    // M2 acceptance 2: M1's exact conservation still holds with cells eating, building,
    // dividing and dying.
    let bytes = assemble("ancestor.mm");
    let mut world = World::new(petri(7)).unwrap();
    world.set_biology(BiologyConfig {
        mutation: MutationRates::default(),
        ..BiologyConfig::default()
    });
    seed_ancestors(&mut world, &bytes, 12);

    let grand_before: i64 = world.total_matter().iter().sum();
    for tick in 0..2_000u64 {
        world.step();
        if tick % 100 == 0 {
            world
                .check_matter()
                .unwrap_or_else(|e| panic!("at tick {tick}: {e}"));
            assert_eq!(
                world.total_matter().iter().sum::<i64>(),
                grand_before,
                "total matter moved by tick {tick}"
            );
            assert!(!world.substrate().any_negative());
        }
    }
}

#[test]
fn a_population_is_deterministic() {
    // I1 with life in the world.
    let bytes = assemble("ancestor.mm");
    let mut hashes = Vec::new();
    for _ in 0..3 {
        let mut world = World::new(petri(3)).unwrap();
        seed_ancestors(&mut world, &bytes, 6);
        world.run(1_000);
        hashes.push((world.state_hash(), world.cells().len()));
    }
    assert_eq!(hashes[0], hashes[1]);
    assert_eq!(hashes[1], hashes[2]);
}

#[test]
fn cells_die_when_they_cannot_pay_their_upkeep() {
    // The floor on the cost of being alive. A cell in the dark with no substrate must not
    // persist forever.
    let mut scenario = petri(5);
    scenario.light = LightRegime::Uniform { intensity: 0 };
    scenario.seeding.clear();
    let bytes = assemble("ancestor.mm");
    let mut world = World::new(scenario).unwrap();
    seed_ancestors(&mut world, &bytes, 4);
    let before = world.total_matter();

    world.run(20_000);
    assert_eq!(world.cells().len(), 0, "cells lived on nothing");
    world.check_matter().expect("corpses lost matter");
    assert_eq!(
        world.total_matter().iter().sum::<i64>(),
        before.iter().sum::<i64>()
    );
}

/// M2 acceptance 1: a hand-written ancestor sustains a population indefinitely.
///
/// > population > 0 for 1,000,000 ticks across 10 seeds, with mutation on.
fn persistence_run(ticks: u64, seeds: &[u64]) {
    let bytes = assemble("ancestor.mm");
    let mut survived = Vec::new();
    for seed in seeds {
        let mut world = World::new(petri(*seed)).unwrap();
        world.set_biology(BiologyConfig {
            mutation: MutationRates::default(),
            ..BiologyConfig::default()
        });
        seed_ancestors(&mut world, &bytes, 12);

        let mut extinct_at = None;
        let mut peak = 0u32;
        let check = (ticks / 200).max(1);
        for tick in 0..ticks {
            world.step();
            if tick % check == 0 {
                let n = world.cells().len() as u32;
                peak = peak.max(n);
                if n == 0 {
                    extinct_at = Some(tick);
                    break;
                }
            }
        }
        let final_n = world.cells().len();
        eprintln!(
            "seed {seed}: peak {peak}, final {final_n}{}",
            match extinct_at {
                Some(t) => format!(", EXTINCT at tick {t}"),
                None => String::new(),
            }
        );
        survived.push(extinct_at.is_none() && final_n > 0);

        // Whatever happened to the population, the world's books must still balance.
        world
            .check_matter()
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    }
    let n = survived.iter().filter(|s| **s).count();
    assert_eq!(
        n,
        seeds.len(),
        "the ancestor went extinct in {} of {} seeds",
        seeds.len() - n,
        seeds.len()
    );
}

#[test]
fn population_persistence_guard() {
    // Long enough to see the population grow, level off and hold, but not so long that a
    // debug-build `cargo test` spends ten minutes on it. The full run is the ignored one.
    let ticks = if cfg!(debug_assertions) {
        1_500
    } else {
        20_000
    };
    persistence_run(ticks, &[1, 2, 3]);
}

#[test]
#[ignore = "1,000,000 ticks across 10 seeds; run with --release --ignored"]
fn acceptance_population_persistence() {
    persistence_run(
        common::env_usize("MM_M2_TICKS", 1_000_000) as u64,
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
    );
}

#[test]
fn the_population_is_limited_by_matter_rather_than_growing_forever() {
    // The closed loop's signature: growth stops because the slide runs out of the waste that
    // photosynthesis needs, not because anything told it to. A population that grew without
    // bound would mean matter was being created somewhere.
    let bytes = assemble("ancestor.mm");
    let mut world = World::new(petri(21)).unwrap();
    seed_ancestors(&mut world, &bytes, 12);
    let (a, b) = if cfg!(debug_assertions) {
        (500, 2_000)
    } else {
        (3_000, 20_000)
    };
    world.run(a);
    let early = world.cells().len();
    world.run(b);
    let late = world.cells().len();

    assert!(early > 12, "the population never grew: {early}");
    assert!(late > 0, "the population died out");
    assert!(
        late < early * 20,
        "growth did not level off: {early} -> {late}"
    );
    world.check_matter().expect("books must balance either way");
}

#[test]
fn a_living_world_survives_a_snapshot() {
    // Hard rule 7 with a population in the world: a save taken mid-run must restore a world
    // whose *future* matches, not merely one whose fields look similar.
    let bytes = assemble("ancestor.mm");
    let mut world = World::new(petri(31)).unwrap();
    seed_ancestors(&mut world, &bytes, 8);
    world.run(1_500);
    assert!(world.cells().len() > 8, "nothing had happened yet");

    let saved = mm_core::Snapshot::write(&world).expect("write");
    let mut resumed = mm_core::Snapshot::read(&saved).expect("read");
    assert_eq!(resumed.cells().len(), world.cells().len());
    assert_eq!(resumed.state_hash(), world.state_hash());

    for tick in 0..500 {
        world.step();
        resumed.step();
        assert_eq!(
            resumed.state_hash(),
            world.state_hash(),
            "diverged {tick} ticks after resuming"
        );
    }
}

/// M2 acceptance 3: selection works.
///
/// > Seed two ancestors differing only in metabolic efficiency; the more efficient one
/// > reaches > 90% of the population within 100,000 ticks in >= 9 of 10 seeds.
///
/// The two strains are `ancestor.mm` and `ancestor_sloppy.mm`, which differ by four
/// instructions: one excretes the reactive byproduct respiration exhales, and one does not.
/// That is metabolic efficiency in the sense that matters — what a cell does with its own
/// exhaust — rather than organelle size, which buys capacity and costs upkeep in the same
/// breath and is therefore not efficiency at all.
///
/// Mutation is off, so the strains stay distinguishable and the only thing that can move
/// their ratio is which one leaves more descendants. Nothing scores them. The tidy strain
/// wins because the poison it declines to carry damages a membrane, and there is no fitness
/// function anywhere in this project.
fn selection_run(ticks: u64, seeds: &[u64]) -> usize {
    let tidy = assemble("ancestor.mm");
    let sloppy = assemble("ancestor_sloppy.mm");
    let mut wins = 0;
    for seed in seeds {
        let mut world = World::new(petri(*seed)).unwrap();
        world.set_biology(BiologyConfig {
            mutation: MutationRates::none(),
            ..BiologyConfig::default()
        });

        // Alternating, so neither gets the better half of the slide.
        for k in 0..16u32 {
            let is_tidy = k % 2 == 0;
            let bytes = if is_tidy { &tidy } else { &sloppy };
            let genome = world.genomes().intern(bytes.clone()).unwrap();
            let id = world.spawn_cell(CellSeed {
                x: pos(6 + (k % 8) as i32 * 7),
                y: pos(6 + (k / 8) as i32 * 7),
                mass: q10(30),
                energy: q10(400),
                membrane: 24,
                key: 11,
                species: u32::from(is_tidy),
                parent: CellId::NONE,
                birth_tick: 0,
                genome,
            });
            if let Some(i) = world.cells_mut().index(id) {
                world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
                world.cells_mut().slots_mut(i)[2] =
                    Organelle::finished(OrganelleType::Mitochondrion, 50);
                world.cells_mut().slots_mut(i)[3] =
                    Organelle::finished(OrganelleType::Chloroplast, 60);
                world.cells_mut().interior_mut(i)[11] = q10(40);
                world.cells_mut().interior_mut(i)[14] = q10(40);
            }
        }
        world.adopt_current_contents_as_baseline();

        world.run(ticks);
        let total = world.cells().len();
        let tidy_now = world
            .cells()
            .iter()
            .filter(|i| world.cells().species[*i] == 1)
            .count();
        let share = if total == 0 {
            0.0
        } else {
            tidy_now as f64 / total as f64
        };
        eprintln!(
            "seed {seed}: {tidy_now}/{total} tidy ({:.0}%)",
            share * 100.0
        );
        if share > 0.9 {
            wins += 1;
        }
        world.check_matter().expect("books balance");
    }
    wins
}

#[test]
fn selection_guard() {
    let ticks = if cfg!(debug_assertions) {
        2_000
    } else {
        20_000
    };
    let seeds = [1u64, 2, 3];
    let wins = selection_run(ticks, &seeds);
    assert!(
        wins >= 2,
        "the tidy strain won in only {wins} of {} seeds",
        seeds.len()
    );
}

#[test]
#[ignore = "100,000 ticks across 10 seeds; run with --release --ignored"]
fn acceptance_selection_works() {
    let seeds = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let wins = selection_run(common::env_usize("MM_M2_TICKS", 100_000) as u64, &seeds);
    assert!(
        wins >= 9,
        "the more efficient strain reached >90% in only {wins} of 10 seeds"
    );
}

// ---------------------------------------------------------------------------------------
// Acceptance 4 — mutation-rate evolution.
//
// > Under a stable environment, mean nucleus copy fidelity rises measurably over 1,000,000
// > ticks. Under a fluctuating environment, it does not rise as fast.
//
// This needs an ancestor that *expresses* fidelity: `ancestor.mm` never touches the nucleus
// control, so its fidelity is whatever the organelle was built with and no mutation can move
// it. `mutator.mm` sets it every tick from a genome immediate, which is what turns a constant
// into a trait. See the comment at the top of that file.

/// Mean nucleus copy fidelity across the living population, `Q10`, or `None` if all dead.
fn mean_fidelity(world: &World) -> Option<i64> {
    let cells = world.cells();
    let mut total = 0i64;
    let mut n = 0i64;
    for i in cells.iter() {
        total += i64::from(mm_core::biology::nucleus_fidelity(cells, i));
        n += 1;
    }
    (n > 0).then(|| total / n)
}

/// Run the mutator ancestor and report (starting fidelity, ending fidelity).
fn fidelity_run(ticks: u64, seed: u64, fluctuating: bool) -> Option<(i64, i64)> {
    let bytes = assemble("mutator.mm");
    let mut scenario = petri(seed);
    if fluctuating {
        // The same total light over a cycle as the stable world receives, delivered in
        // alternating gluts and famines. Matching the mean matters: a fluctuating world that
        // is also a darker world would be measuring the darkness.
        scenario.light = LightRegime::DayNight {
            period_ticks: 2_000,
            day: mm_core::Q10_ONE * 2,
            night: 0,
        };
    }
    let mut world = World::new(scenario).unwrap();
    world.set_biology(BiologyConfig {
        mutation: MutationRates::default(),
        ..BiologyConfig::default()
    });
    seed_ancestors(&mut world, &bytes, 12);

    // Measured after a settling period rather than at tick zero: every seeded cell starts on
    // the same immediate, so tick-zero fidelity is a property of the genome file and not of
    // anything the population has done.
    let settle = (ticks / 20).max(1);
    let mut start = None;
    for tick in 0..ticks {
        world.step();
        if tick == settle {
            start = mean_fidelity(&world);
        }
        if world.cells().is_empty() {
            return None;
        }
    }
    Some((start?, mean_fidelity(&world)?))
}

fn fidelity_report(ticks: u64, seeds: &[u64]) -> (i64, i64) {
    let mut stable_drift = 0i64;
    let mut fluctuating_drift = 0i64;
    let mut counted = 0i64;
    for seed in seeds {
        let (Some(stable), Some(fluctuating)) = (
            fidelity_run(ticks, *seed, false),
            fidelity_run(ticks, *seed, true),
        ) else {
            eprintln!("seed {seed}: extinct, not counted");
            continue;
        };
        let s = stable.1 - stable.0;
        let f = fluctuating.1 - fluctuating.0;
        eprintln!(
            "seed {seed}: stable {} -> {} ({s:+}), fluctuating {} -> {} ({f:+})",
            stable.0, stable.1, fluctuating.0, fluctuating.1
        );
        stable_drift += s;
        fluctuating_drift += f;
        counted += 1;
    }
    assert!(counted > 0, "every seed went extinct; nothing was measured");
    (stable_drift / counted, fluctuating_drift / counted)
}

#[test]
fn fidelity_is_a_trait_the_genome_controls() {
    // The premise of acceptance 4, checked directly rather than inferred from a long run: the
    // mutator ancestor's fidelity is what its genome says, and the plain ancestor's is not.
    let mut world = World::new(petri(1)).unwrap();
    seed_ancestors(&mut world, &assemble("mutator.mm"), 1);
    for _ in 0..8 {
        world.step();
    }
    assert_eq!(
        mean_fidelity(&world),
        Some(512),
        "the mutator ancestor did not set its nucleus fidelity to the 512 its genome asks for"
    );
}

#[test]
#[ignore = "1,000,000 ticks, stable and fluctuating, across 10 seeds; --release --ignored"]
fn acceptance_mutation_rate_evolves() {
    let seeds = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let ticks = common::env_usize("MM_M2_TICKS", 1_000_000) as u64;
    let (stable, fluctuating) = fidelity_report(ticks, &seeds);
    eprintln!("mean drift: stable {stable:+}, fluctuating {fluctuating:+}");
    assert!(
        stable > 0,
        "fidelity fell by {} in a stable world, where preserving a working genome should pay",
        -stable
    );
    assert!(
        stable > fluctuating,
        "fidelity rose at least as fast in a fluctuating world ({fluctuating:+}) as in a \
         stable one ({stable:+}); the environment is not reaching selection"
    );
}

// ---------------------------------------------------------------------------------------
// Acceptance 5 — determinism with life.
//
// > Identical state hash at 500,000 ticks across thread counts.
//
// The execute phase runs cells on however many threads rayon offers (see
// `biology::execute`). This is the test that says the number of threads is not an input to
// the simulation.

fn hash_after(ticks: u64, threads: usize) -> u64 {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("thread pool");
    pool.install(|| {
        let bytes = assemble("ancestor.mm");
        let mut world = World::new(petri(7)).unwrap();
        world.set_biology(BiologyConfig {
            mutation: MutationRates::default(),
            ..BiologyConfig::default()
        });
        seed_ancestors(&mut world, &bytes, 12);
        for _ in 0..ticks {
            world.step();
        }
        assert!(
            !world.cells().is_empty(),
            "the population died, so the hash compares two empty worlds"
        );
        world.state_hash()
    })
}

fn thread_count_run(ticks: u64) {
    // 1 is below `biology::PARALLEL_THRESHOLD`'s effect and 16 is well above it, so this
    // compares the serial path against the parallel one and not just two parallel runs.
    let one = hash_after(ticks, 1);
    let many: Vec<(usize, u64)> = [2usize, 3, 8, 16]
        .into_iter()
        .map(|t| (t, hash_after(ticks, t)))
        .collect();
    for (threads, hash) in &many {
        assert_eq!(
            *hash, one,
            "{threads} threads gave {hash:#018x} where 1 thread gave {one:#018x}"
        );
    }
    eprintln!("{ticks} ticks: {one:#018x} on 1, 2, 3, 8 and 16 threads");
}

#[test]
fn thread_count_is_not_an_input() {
    // Long enough for the arena to grow past the parallel threshold and for births, deaths
    // and mutations to have happened many times over.
    thread_count_run(if cfg!(debug_assertions) { 400 } else { 4_000 });
}

#[test]
#[ignore = "500,000 ticks at five thread counts; run with --release --ignored"]
fn acceptance_determinism_with_life() {
    thread_count_run(common::env_usize("MM_M2_TICKS", 500_000) as u64);
}
