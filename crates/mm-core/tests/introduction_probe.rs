//! Why a predator dropped into an established population dies out.
//!
//! Ignored — a probe, run it on purpose.

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

/// Mass percentiles of a cohort, in whole units.
fn masses(world: &World, cohorts: &[Cohort], label: &str) -> Option<(i32, i32, i32, u32)> {
    let census = world.census(cohorts);
    let reading = census.cohort(label)?;
    if reading.cells == 0 {
        return Some((0, 0, 0, 0));
    }
    let root = reading.root;
    let mut m: Vec<i32> = world
        .cells()
        .iter()
        .filter(|i| {
            mm_core::census::root_of(world.archive(), world.cells().species[*i]) == root
        })
        .map(|i| world.cells().mass[i] / Q10_ONE)
        .collect();
    m.sort_unstable();
    let p = |q: usize| m[(m.len() - 1) * q / 100];
    Some((p(10), p(50), p(90), reading.cells))
}

#[test]
#[ignore = "a probe; run it on purpose"]
fn what_happens_when_a_predator_arrives_late() {
    for predator in ["genomes/engulfer.mm", "genomes/stalker.mm"] {
        let scenario = Scenario::from_ron(
            &std::fs::read_to_string(root("scenarios/predator_introduction.ron")).expect("scenario"),
        )
        .expect("parse");
        let mut world = World::new(scenario).expect("world");

        // Let the ancestors fill the slide first, which is the thing being asked about.
        let mut cohorts = world.place_community(&[("ancestor", &asm("genomes/ancestor.mm"), 24)], Placement::Spread);
        world.run(8_000);
        let settled = world.cells().len();

        // Then drop the predator in.
        let mut later = world.place_community(&[("predator", &asm(predator), 12)], Placement::Spread);
        cohorts.append(&mut later);
        eprintln!("\n=== {predator} into {settled} settled ancestors ===");

        let mut last = world.report();
        for step in 1..=10 {
            world.run(2_000);
            let now = world.report();
            let (pa, pp) = (
                masses(&world, &cohorts, "ancestor"),
                masses(&world, &cohorts, "predator"),
            );
            let killed = now.ecology.engulfed as i64 - last.ecology.engulfed as i64;
            let wounded = now.ecology.wounded as i64 - last.ecology.wounded as i64;
            if let (Some(a), Some(p)) = (pa, pp) {
                eprintln!(
                    "t={:>6}  ancestor n={:<5} mass p10/50/90 {:>3}/{:>3}/{:>3}   \
                     predator n={:<4} mass {:>3}/{:>3}/{:>3}   engulfed {killed:<4} wounded {wounded}",
                    world.tick_count(), a.3, a.0, a.1, a.2, p.3, p.0, p.1, p.2
                );
                if p.3 == 0 {
                    eprintln!("  extinct after {} ticks", step * 2_000);
                    break;
                }
                // Can it even qualify? Engulfment needs mass >= 2x the victim's bulk.
                eprintln!(
                    "          predator p50 {} vs 2x ancestor p10 {} -> {}",
                    p.1,
                    a.0 * 2,
                    if p.1 >= a.0 * 2 { "can swallow the smallest tenth" } else { "CANNOT SWALLOW ANYTHING" }
                );
            }
            last = now;
        }
    }
}

/// What the newcomer is actually short of in its first thousand ticks.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_the_newcomer_runs_out_of() {
    let carbon = 4usize;
    for settle in [0u64, 8_000] {
        let scenario = Scenario::from_ron(
            &std::fs::read_to_string(root("scenarios/predator_introduction.ron")).expect("scenario"),
        )
        .expect("parse");
        let mut world = World::new(scenario).expect("world");
        let mut cohorts =
            world.place_community(&[("ancestor", &asm("genomes/ancestor.mm"), 24)], Placement::Spread);
        world.run(settle);

        // What is left in the water for a newcomer to build a body out of?
        let free: i64 = world.substrate().total_chem()[carbon];
        eprintln!(
            "\n=== introduced at t={settle} into {} ancestors; free carbon in the water {} units ===",
            world.cells().len(),
            free / Q10_ONE as i64
        );

        let mut later =
            world.place_community(&[("predator", &asm("genomes/engulfer.mm"), 12)], Placement::Spread);
        cohorts.append(&mut later);

        for step in 1..=8 {
            world.run(250);
            let census = world.census(&cohorts);
            let Some(r) = census.cohort("predator") else { break };
            if r.cells == 0 {
                eprintln!("  t+{}: extinct", step * 250);
                break;
            }
            let root = r.root;
            let mine: Vec<usize> = world
                .cells()
                .iter()
                .filter(|i| {
                    mm_core::census::root_of(world.archive(), world.cells().species[*i]) == root
                })
                .collect();
            let n = mine.len().max(1) as i64;
            let energy: i64 = mine.iter().map(|i| world.cells().energy[*i] as i64).sum::<i64>() / n;
            let mass: i64 = mine.iter().map(|i| world.cells().mass[*i] as i64).sum::<i64>() / n;
            let built: i64 = mine
                .iter()
                .map(|i| world.cells().slots(*i).iter().filter(|o| o.is_active()).count() as i64)
                .sum::<i64>()
                / n;
            eprintln!(
                "  t+{:<4} n={:<3} mean energy {:<5} mass {:<4} organelles finished {built}/6",
                step * 250,
                r.cells,
                energy / Q10_ONE as i64,
                mass / Q10_ONE as i64,
            );
        }
    }
}

/// Do the newcomers ever actually eat? Accumulated per tick, because `World::report` is the
/// *last tick's* counters and diffing it across a two-thousand-tick window measures one tick.
#[test]
#[ignore = "a probe; run it on purpose"]
fn do_they_ever_eat() {
    for settle in [0u64, 8_000] {
        let scenario = Scenario::from_ron(
            &std::fs::read_to_string(root("scenarios/predator_introduction.ron")).expect("scenario"),
        )
        .expect("parse");
        let mut world = World::new(scenario).expect("world");
        let mut cohorts =
            world.place_community(&[("ancestor", &asm("genomes/ancestor.mm"), 24)], Placement::Spread);
        world.run(settle);
        let mut later =
            world.place_community(&[("predator", &asm("genomes/engulfer.mm"), 12)], Placement::Spread);
        cohorts.append(&mut later);

        let (mut engulfed, mut swallowed) = (0u64, 0i64);
        for step in 1..=8 {
            for _ in 0..250 {
                world.run(1);
                let r = world.report();
                engulfed += r.ecology.engulfed as u64;
                swallowed += r.ecology.swallowed;
                // How many predators are over their own divide weight of 140?
            }
            let census = world.census(&cohorts);
            let Some(r) = census.cohort("predator") else { break };
            if r.cells == 0 {
                eprintln!("  t+{}: extinct, {engulfed} meals in total", step * 250);
                break;
            }
            let root = r.root;
            let over = world
                .cells()
                .iter()
                .filter(|i| {
                    mm_core::census::root_of(world.archive(), world.cells().species[*i]) == root
                })
                .filter(|i| world.cells().mass[*i] >= 140 * Q10_ONE)
                .count() as u32;
            eprintln!(
                "  t+{:<4} n={:<3} meals so far {engulfed:<4} flesh {:<6} over divide weight: {over}",
                step * 250,
                r.cells,
                swallowed / Q10_ONE as i64,
            );
        }
        eprintln!("  introduced at t={settle}: {engulfed} meals total\n");
    }
}

/// The survivors, one by one: are they able to eat, and is there anything next to them?
#[test]
#[ignore = "a probe; run it on purpose"]
fn why_the_survivors_stop_eating() {
    let scenario = Scenario::from_ron(
        &std::fs::read_to_string(root("scenarios/predator_introduction.ron")).expect("scenario"),
    )
    .expect("parse");
    let mut world = World::new(scenario).expect("world");
    let mut cohorts =
        world.place_community(&[("ancestor", &asm("genomes/ancestor.mm"), 24)], Placement::Spread);
    world.run(8_000);
    let mut later =
        world.place_community(&[("predator", &asm("genomes/engulfer.mm"), 12)], Placement::Spread);
    cohorts.append(&mut later);
    world.run(1_500);

    let census = world.census(&cohorts);
    let Some(r) = census.cohort("predator") else { return };
    let root = r.root;
    let mine: Vec<usize> = world
        .cells()
        .iter()
        .filter(|i| mm_core::census::root_of(world.archive(), world.cells().species[*i]) == root)
        .collect();
    eprintln!("{} survivors at t=9500", mine.len());
    for i in mine {
        let mass = world.cells().mass[i] / Q10_ONE;
        let appetite = world
            .cells()
            .slots(i)
            .iter()
            .filter(|o| o.kind == mm_core::OrganelleType::Vacuole && o.is_active())
            .map(|o| o.control[1])
            .max()
            .unwrap_or(-1);
        // Everything within a couple of squares, and the smallest of it.
        let (x, y) = (world.cells().x[i], world.cells().y[i]);
        let mut near = 0;
        let mut smallest = i32::MAX;
        for j in world.cells().iter() {
            if j == i {
                continue;
            }
            let d = ((world.cells().x[j] - x) as i64).pow(2) + ((world.cells().y[j] - y) as i64).pow(2);
            if d < (4 * mm_core::fixed::POS_ONE as i64).pow(2) {
                near += 1;
                smallest = smallest.min(world.cells().mass[j] / Q10_ONE);
            }
        }
        eprintln!(
            "  mass {mass:<4} appetite {appetite:<5} neighbours within 4 squares: {near:<4} \
             smallest {} -> {}",
            if smallest == i32::MAX { -1 } else { smallest },
            if appetite <= 0 {
                "MOUTH SHUT"
            } else if smallest == i32::MAX {
                "nothing near"
            } else if mass >= smallest * 2 {
                "could swallow the smallest"
            } else {
                "TOO SMALL TO SWALLOW ANY OF THEM"
            }
        );
    }
}

/// The stalker's version of the same question: does it ever land a hit?
#[test]
#[ignore = "a probe; run it on purpose"]
fn does_the_stalker_ever_connect() {
    for settle in [0u64, 8_000] {
        let scenario = Scenario::from_ron(
            &std::fs::read_to_string(root("scenarios/predator_introduction.ron")).expect("scenario"),
        )
        .expect("parse");
        let mut world = World::new(scenario).expect("world");
        let mut cohorts =
            world.place_community(&[("ancestor", &asm("genomes/ancestor.mm"), 24)], Placement::Spread);
        world.run(settle);
        let mut later =
            world.place_community(&[("predator", &asm("genomes/stalker.mm"), 12)], Placement::Spread);
        cohorts.append(&mut later);

        let (mut wounded, mut damage, mut bled) = (0u64, 0i64, 0i64);
        for step in 1..=6 {
            for _ in 0..500 {
                world.run(1);
                let r = world.report();
                wounded += r.ecology.wounded as u64;
                damage += r.ecology.damage_dealt;
                bled += r.ecology.bled;
            }
            let census = world.census(&cohorts);
            let n = census.cohort("predator").map_or(0, |r| r.cells);
            eprintln!(
                "  settle {settle} t+{:<5} n={n:<3} wounds {wounded:<5} damage {:<7} bled {}",
                step * 500,
                damage / Q10_ONE as i64,
                bled / Q10_ONE as i64
            );
            if n == 0 {
                break;
            }
        }
        eprintln!();
    }
}

/// `bleed_rate` is 0 by default, so a wound yields nothing. Does turning it on feed the stalker?
#[test]
#[ignore = "a probe; run it on purpose"]
fn does_a_leaking_wound_feed_the_stalker() {
    for bleed in [0i32, Q10_ONE / 64, Q10_ONE / 8] {
        let scenario = Scenario::from_ron(
            &std::fs::read_to_string(root("scenarios/predator_introduction.ron")).expect("scenario"),
        )
        .expect("parse");
        let mut world = World::new(scenario).expect("world");
        let mut biology = world.biology().clone();
        biology.ecology.bleed_rate = bleed;
        world.set_biology(biology);

        let mut cohorts =
            world.place_community(&[("ancestor", &asm("genomes/ancestor.mm"), 24)], Placement::Spread);
        world.run(8_000);
        let mut later =
            world.place_community(&[("predator", &asm("genomes/stalker.mm"), 12)], Placement::Spread);
        cohorts.append(&mut later);

        let mut bled = 0i64;
        let mut trace = Vec::new();
        for step in 1..=6 {
            for _ in 0..500 {
                world.run(1);
                bled += world.report().ecology.bled;
            }
            let census = world.census(&cohorts);
            trace.push(format!(
                "t+{}: stalkers {} ancestors {}",
                step * 500,
                census.cohort("predator").map_or(0, |r| r.cells),
                census.cohort("ancestor").map_or(0, |r| r.cells),
            ));
        }
        eprintln!("bleed_rate {bleed}: bled {} units", bled / Q10_ONE as i64);
        for t in &trace {
            eprintln!("    {t}");
        }
    }
}
