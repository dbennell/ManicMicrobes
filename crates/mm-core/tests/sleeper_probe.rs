//! What `genomes/sleeper.mm` actually does, and the three probes it took to find out.
//!
//! All ignored — run them on purpose. They are kept because the first two answers were wrong and
//! the way they were wrong is instructive: a gate that looks like it is failing to pay may not be
//! running at all, and a gate that is running may be reading a number its own genome has already
//! rewritten.

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::chem::CARBON_DIOXIDE;
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::{MutationRates, Organelle, OrganelleType, Scenario, World};

fn root(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn assembled(genome: &str) -> Vec<u8> {
    let source = std::fs::read_to_string(root(genome)).expect("genome");
    mm_asm::assemble(&source).expect("assemble").bytes
}

/// Does the gate work at all, in isolation?
///
/// The one that settled it. A cell with a body and nothing to fix shuts its chloroplast by tick
/// 10 and keeps it shut, so any claim that the gate "never fires" in a world is a claim about the
/// world rather than about the gene — which is what sent the next probe looking in the right
/// place.
#[test]
#[ignore = "a probe; run it on purpose"]
fn the_gate_shuts_a_chloroplast_that_has_nothing_to_fix() {
    let mut world = World::new(Scenario {
        seed: 7,
        width: 16,
        height: 16,
        ..Scenario::default()
    })
    .expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    let genome = world.genomes().intern(assembled("genomes/sleeper.mm")).expect("intern");
    let id = world.spawn_cell(CellSeed {
        x: pos(8),
        y: pos(8),
        mass: q10(30),
        energy: q10(400),
        membrane: 24,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    // The body the genome would build. Installed directly, because a bare slide has no chemistry
    // to build one out of — the first run of this probe reported "the cell builds nothing", which
    // was true and had nothing to do with the gate.
    let i = world.cells().index(id).expect("alive");
    world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 48);
    world.cells_mut().slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
    world.cells_mut().slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);

    let mut shut_by = None;
    for tick in 0..60 {
        world.run(1);
        let Some(i) = world.cells().index(id) else { break };
        world.cells_mut().interior_mut(i)[CARBON_DIOXIDE] = 0;
        let control = world.cells().slots(i)[3].control[0];
        if control == 0 && shut_by.is_none() {
            shut_by = Some(tick);
        }
    }
    eprintln!("chloroplast shut at tick {shut_by:?}");
    assert!(shut_by.is_some(), "the gate never fired with nothing to fix");
}

/// How often the gate fires across a whole population, and the gene-order trap.
///
/// **Measure the population, not a cell.** The first version followed `cells().iter().next()`
/// after five thousand ticks and reported 0 firings — a statement about one arbitrary descendant
/// of a mutating lineage, not about the genome.
///
/// What it found once it was looking at everybody: with `EXPRESS #feed` ahead of `EXPRESS #sun`,
/// a third of cell-ticks sat below the gate's threshold and the gate fired on **none** of them,
/// because `#feed` tops the cell up immediately before `#sun` reads it. The cells measured as low
/// were low at *end* of tick, having spent their carbon on photosynthesis after the gate had
/// already looked. The gene was testing a condition its own genome prevented. Swapping the two
/// `EXPRESS` lines takes it from 0% to 33%.
#[test]
#[ignore = "a probe; run it on purpose"]
fn how_often_the_gate_fires() {
    for (file, mutate) in [
        ("scenarios/the_thicket.ron", false),
        ("scenarios/the_thicket.ron", true),
        ("scenarios/the_lean_water.ron", false),
        ("scenarios/the_lean_water.ron", true),
    ] {
        let scenario = Scenario::from_ron(&std::fs::read_to_string(root(file)).expect("scenario"))
            .expect("parse");
        let mut world = World::new(scenario).expect("world");
        if !mutate {
            let biology = BiologyConfig {
                mutation: MutationRates::none(),
                ..world.biology().clone()
            };
            world.set_biology(biology);
        }
        world.place_founders(&assembled("genomes/sleeper.mm"), 16);
        world.run(5_000);

        let (mut shut, mut ticks, mut low) = (0u64, 0u64, 0u64);
        for _ in 0..200 {
            world.run(1);
            for i in world.cells().iter() {
                let Some(c) = world
                    .cells()
                    .slots(i)
                    .iter()
                    .find(|o| o.kind == OrganelleType::Chloroplast && o.is_active())
                else {
                    continue;
                };
                ticks += 1;
                if c.control[0] == 0 {
                    shut += 1;
                }
                if world.cells().interior(i)[CARBON_DIOXIDE] < 4 * Q10_ONE {
                    low += 1;
                }
            }
        }
        let pct = |n: u64| if ticks > 0 { n * 100 / ticks } else { 0 };
        eprintln!(
            "{file} mutation {}: {ticks} cell-ticks, {}% below the threshold, {}% shut",
            if mutate { "on" } else { "off" },
            pct(low),
            pct(shut),
        );
    }
}

/// Does idling pay? Five seeds, because a stochastic result stated on one is not a result.
#[test]
#[ignore = "a probe; run it on purpose"]
fn does_the_gate_pay() {
    for file in ["scenarios/the_thicket.ron", "scenarios/the_lean_water.ron"] {
        let mut wins = 0;
        for seed in [1u64, 2, 3, 4, 5] {
            let run = |genome: &str| {
                let mut scenario =
                    Scenario::from_ron(&std::fs::read_to_string(root(file)).expect("scenario"))
                        .expect("parse");
                scenario.seed = seed;
                let mut world = World::new(scenario).expect("world");
                world.place_founders(&assembled(genome), 16);
                world.run(20_000);
                world.cells().len()
            };
            let (a, s) = (run("genomes/ancestor.mm"), run("genomes/sleeper.mm"));
            if s > a {
                wins += 1;
            }
            eprintln!("{file} seed {seed}: ancestor {a}, sleeper {s}");
        }
        eprintln!("  -> sleeper wins {wins} of 5 seeds");
    }
}
