//! Why `genomes/stalker.mm` cannot make a living, measured before anything is changed.
//!
//! Ignored — probes, run them on purpose.

use mm_core::census::Cohort;
use mm_core::fixed::Q10_ONE;
use mm_core::{Placement, Scenario, World};

fn root(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

fn asm(genome: &str) -> Vec<u8> {
    let source = std::fs::read_to_string(root(genome)).expect("genome");
    mm_asm::assemble(&source).expect("assemble").bytes
}

/// Alone on a well-fed slide with nothing to compete with and nothing to hunt.
///
/// The question this separates: is the body unaffordable, or is the *hunt* unprofitable? A cell
/// that cannot pay for itself in an empty world with free light has a body problem, and no
/// predation yield will rescue it.
#[test]
#[ignore = "a probe; run it on purpose"]
fn can_it_pay_for_itself_with_nobody_else_on_the_slide() {
    for genome in ["genomes/ancestor.mm", "genomes/stalker.mm", "genomes/engulfer.mm"] {
        let scenario = Scenario::from_ron(
            &std::fs::read_to_string(root("scenarios/predator_introduction.ron")).expect("scenario"),
        )
        .expect("parse");
        let mut world = World::new(scenario).expect("world");
        let bytes = asm(genome);
        eprintln!("\n=== {genome} ({} bytes), alone ===", bytes.len());
        world.place_founders(&bytes, 8);

        for step in 1..=10 {
            world.run(1_000);
            let n = world.cells().len();
            if n == 0 {
                eprintln!("  t={:<6} extinct", step * 1_000);
                break;
            }
            let live: Vec<usize> = world.cells().iter().collect();
            let cnt = live.len() as i64;
            let energy: i64 = live.iter().map(|i| world.cells().energy[*i] as i64).sum::<i64>() / cnt;
            let mass: i64 = live.iter().map(|i| world.cells().mass[*i] as i64).sum::<i64>() / cnt;
            let organelles: i64 = live
                .iter()
                .map(|i| world.cells().slots(*i).iter().filter(|o| o.is_active()).count() as i64)
                .sum::<i64>()
                / cnt;
            let upkeep: i64 = live
                .iter()
                .map(|i| {
                    world.biology().metabolism.catalogue.upkeep(world.cells().slots(*i)) as i64
                })
                .sum::<i64>()
                / cnt;
            eprintln!(
                "  t={:<6} n={n:<5} mean energy {:<5} mass {:<4} organelles {organelles} upkeep {upkeep} Q10/tick",
                step * 1_000,
                energy / Q10_ONE as i64,
                mass / Q10_ONE as i64,
            );
        }
    }
}

/// Head to head on the same slide, so the comparison is competition rather than solitude.
#[test]
#[ignore = "a probe; run it on purpose"]
fn against_the_ancestor_from_the_same_start() {
    let scenario = Scenario::from_ron(
        &std::fs::read_to_string(root("scenarios/predator_introduction.ron")).expect("scenario"),
    )
    .expect("parse");
    let mut world = World::new(scenario).expect("world");
    let cohorts: Vec<Cohort> = world.place_community(
        &[
            ("ancestor", &asm("genomes/ancestor.mm"), 12),
            ("stalker", &asm("genomes/stalker.mm"), 12),
        ],
        Placement::Spread,
    );
    let mut wounds = 0u64;
    for step in 1..=10 {
        for _ in 0..1_000 {
            world.run(1);
            wounds += world.report().ecology.wounded as u64;
        }
        let census = world.census(&cohorts);
        eprintln!(
            "  t={:<6} ancestor {:<5} stalker {:<5} wounds {wounds}",
            step * 1_000,
            census.cohort("ancestor").map_or(0, |r| r.cells),
            census.cohort("stalker").map_or(0, |r| r.cells),
        );
    }
}
