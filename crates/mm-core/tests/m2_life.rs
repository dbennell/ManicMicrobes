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
    persistence_run(20_000, &[1, 2, 3]);
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
    world.run(3_000);
    let early = world.cells().len();
    world.run(20_000);
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
