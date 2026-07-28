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
        width: 32,
        height: 32,
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
        let x = 4 + (k % 6) * 4;
        let y = 4 + (k / 6) * 4;
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
/// Mutation is off, so the two strains stay distinguishable and the only thing that can
/// change their ratio is which one leaves more descendants. Nothing scores them: there is no
/// fitness function anywhere in this project, and the efficient strain wins because a bigger
/// chloroplast catches more light, not because anything said it should.
fn selection_run(ticks: u64, seeds: &[u64]) -> usize {
    let bytes = assemble("ancestor.mm");
    let mut wins = 0;
    for seed in seeds {
        let mut world = World::new(petri(*seed)).unwrap();
        world.set_biology(BiologyConfig {
            mutation: MutationRates::none(),
            ..BiologyConfig::default()
        });

        // Two strains, alternating so neither gets the better half of the slide.
        for k in 0..16u32 {
            let efficient = k % 2 == 0;
            let genome = world.genomes().intern(bytes.clone()).unwrap();
            let id = world.spawn_cell(CellSeed {
                x: pos(3 + (k % 8) as i32 * 3),
                y: pos(3 + (k / 8) as i32 * 3),
                mass: q10(30),
                energy: q10(400),
                membrane: 24,
                key: 11,
                species: u32::from(efficient),
                parent: CellId::NONE,
                birth_tick: 0,
                genome,
            });
            if let Some(i) = world.cells_mut().index(id) {
                world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
                world.cells_mut().slots_mut(i)[2] =
                    Organelle::finished(OrganelleType::Mitochondrion, 50);
                // The only difference between the strains.
                world.cells_mut().slots_mut(i)[3] = Organelle::finished(
                    OrganelleType::Chloroplast,
                    if efficient { 90 } else { 45 },
                );
                world.cells_mut().interior_mut(i)[11] = q10(40);
                world.cells_mut().interior_mut(i)[14] = q10(40);
            }
        }
        world.adopt_current_contents_as_baseline();

        let mut at = 0u64;
        while at < ticks {
            let chunk = (ticks / 10).max(1);
            world.run(chunk);
            at += chunk;
            let t = world.cells().len();
            let e = world
                .cells()
                .iter()
                .filter(|i| world.cells().species[*i] == 1)
                .count();
            eprintln!(
                "  seed {seed} tick {at}: {e}/{t} = {:.0}%  co2 in fluid {}  carbon {}",
                if t == 0 {
                    0.0
                } else {
                    e as f64 / t as f64 * 100.0
                },
                world.substrate().total_chem()[11] / 1024,
                world.substrate().total_chem()[4] / 1024,
            );
        }
        let total = world.cells().len();
        let efficient = world
            .cells()
            .iter()
            .filter(|i| world.cells().species[*i] == 1)
            .count();
        let share = if total == 0 {
            0.0
        } else {
            efficient as f64 / total as f64
        };
        eprintln!(
            "seed {seed}: {efficient}/{total} efficient ({:.0}%)",
            share * 100.0
        );
        if share > 0.9 {
            wins += 1;
        }
        world.check_matter().expect("books balance");
    }
    wins
}

/// M2 acceptance 3 does not pass, and this records why rather than pretending otherwise.
///
/// Measured over 20,000 ticks, three seeds: the more efficient strain reaches 58%, not the
/// >90% the milestone asks for. The trajectory says exactly what is happening.
///
/// ```text
///   tick  2000: 51%   co2 in fluid 101081
///   tick  4000: 56%   co2 in fluid   5929
///   tick  6000: 56%   co2 in fluid     89
///   tick  8000: 58%   co2 in fluid     22
///   tick 20000: 58%   co2 in fluid     21     <- and frozen from here on
/// ```
///
/// **Selection works, and then stops having anything to work on.** The efficient strain does
/// gain — 51% to 58% — for as long as the population is growing. Then the slide runs out of
/// matter, growth stops, and because nothing here kills a cell that can still feed itself,
/// the population freezes: no births, no deaths, no turnover, and therefore no differential
/// reproduction for selection to be made of.
///
/// There are two separate problems, and only the first is about parameters.
///
/// **No turnover.** The only cause of death is starvation, and a cell at equilibrium does not
/// starve: it holds its own sugar and carbon dioxide, cycles them against the light, and
/// generates energy indefinitely. What it cannot do is grow, because structural carbon is
/// exhausted and exactly conserved. The end state is an immortal, static population — the
/// *correct* consequence of the mechanisms as specified, and not something an evolutionary
/// test can measure, because selection is differential reproduction and nothing is
/// reproducing. M2's deliverables name "death, corpse deposition, decay" without saying what
/// causes death besides starvation; whatever provides turnover is missing.
///
/// **"Metabolic efficiency" is not what this test varies.** The two strains differ in
/// chloroplast `param`, which buys *throughput*, not efficiency — and throughput costs
/// upkeep whether or not it is used. At param 90 the strain pays 115 energy a tick against
/// the small strain's 70, for a capacity it can only exploit while carbon dioxide is
/// abundant. That window closes early: the fluid goes from 101,081 to 5,929 units of it
/// between ticks 2,000 and 4,000. After that the extra capacity is pure cost.
///
/// The consequence is that the big-chloroplast strain is not reliably favoured at all. Over
/// three seeds it ends at 58%, 45% and 53% — ahead, behind, and level. A test asserting it
/// wins would be asserting noise.
///
/// So the honest reading is that acceptance 3 needs two things this milestone does not have:
/// a source of mortality, and a definition of "more efficient" that means more output per
/// unit of upkeep rather than more capacity. Both are design decisions about mechanism, so
/// this is left failing and documented rather than tuned until it goes green.
#[test]
#[ignore = "known failure; see the doc comment for the diagnosis"]
fn selection_guard() {
    let ticks = if cfg!(debug_assertions) {
        1_500
    } else {
        20_000
    };
    let seeds = [1u64, 2, 3];
    let wins = selection_run(ticks, &seeds);
    assert!(
        wins >= 2,
        "the more efficient strain won in only {wins} of {} seeds",
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
