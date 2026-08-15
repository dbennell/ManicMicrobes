//! What every way of making a living actually earns, side by side, in one table.
//!
//! `predator_probe` asked why one genome stopped reproducing and `hunting_probe` asked whether
//! a signature can be steered by. Both are questions about one strategy. This is the question
//! underneath them: **given the catalogue as priced today, what is the income and the bill of
//! each thing a cell can be**, and is there any loadout other than the sit-still autotroph whose
//! income exceeds its bill by enough to pay for a daughter?
//!
//! Run with
//! `cargo test -p mm-core --test economy_probe -- --ignored --nocapture --test-threads=1`.
//!
//! `#[ignore]`, like every other probe in the tree. A probe answers a question once.
//!
//! Mutation is off in every measurement here. The question is what a loadout is worth, and a
//! population that drifts off its founder's body is answering a different one.

mod common;

use std::path::Path;

use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::organelle::{OrganelleType, SLOT_COUNT};
use mm_core::{
    CellId, LightRegime, MutationRates, NullHost, Scenario, VmConfig, World,
};

/// Every genome that builds a body. The four that do not — `arithmetic`, `expression`, `scan`,
/// `replicator`, `dormant` — are ISA demonstrations rather than organisms.
const ORGANISMS: [&str; 16] = [
    "ancestor.mm",
    "ancestor_sloppy.mm",
    "drifter.mm",
    "drifter_blind.mm",
    "hoarder.mm",
    "hunter.mm",
    "marble.mm",
    "mutator.mm",
    "oscillator.mm",
    "parasite.mm",
    "predator.mm",
    "reflex.mm",
    "scavenger.mm",
    "sentinel.mm",
    "sponge.mm",
    "stalker.mm",
];

fn assemble(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    mm_asm::assemble(&src)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .bytes
}

/// A lit, still, well-fed dish, with mutation off.
fn dish() -> Scenario {
    Scenario {
        name: "economy".to_string(),
        seed: 0xEC0_10,
        width: 64,
        height: 64,
        light: LightRegime::Uniform {
            intensity: Q10_ONE,
        },
        current: mm_core::CurrentField::Still,
        jitter: 0,
        seeding: vec![
            mm_core::Seeding::Uniform {
                chemical: 11,
                per_square: q10(400),
            },
            mm_core::Seeding::Uniform {
                chemical: 14,
                per_square: q10(400),
            },
            mm_core::Seeding::Uniform {
                chemical: 4,
                per_square: q10(400),
            },
            // The minerals every recipe in the catalogue is costed in, at the
            // Redfield proportion of the carbon above. Nothing produces them.
            mm_core::Seeding::Uniform {
                chemical: 5,
                per_square: (q10(400)) * 16 / 106,
            },
            mm_core::Seeding::Uniform {
                chemical: 6,
                per_square: (q10(400)) / 53,
            },
        ],
        biology: mm_core::BiologyConfig {
            mutation: MutationRates::none(),
            ..Default::default()
        },
        ..Scenario::default()
    }
}

/// The loadout as a one-line inventory.
fn loadout(world: &World, i: usize) -> String {
    let mut parts = Vec::new();
    for slot in 0..SLOT_COUNT {
        let o = world.cells().slots(i)[slot];
        if o.is_present() {
            parts.push(format!("{}({})", o.kind.name(), o.param));
        }
    }
    parts.join(" ")
}

/// Everything a loadout costs and everything it can convert, from the catalogue alone.
struct Books {
    /// Organelle upkeep plus the metabolic floor, `Q10` energy a tick.
    upkeep: i32,
    /// What the mitochondria could burn a tick if substrate were free, `Q10` matter.
    respiration: i32,
    /// What the chloroplasts could fix a tick in full light, `Q10` matter.
    photosynthesis: i32,
    /// The energy respiration at capacity would recover, `Q10` a tick.
    gross: i64,
    /// What the cilia cost at the power they are set to, `Q10` a tick.
    thrust: i32,
    /// What an extended spike costs a tick, `Q10`.
    spike: i32,
}

fn books(world: &World, i: usize) -> Books {
    let bio = world.biology();
    let rates = &bio.metabolism.rates;
    let cat = &bio.metabolism.catalogue;
    let chem = world.scenario().chemicals.clone();

    let mut respiration = 0i32;
    let mut photosynthesis = 0i32;
    let mut gross = 0i64;
    for o in world.cells().slots(i) {
        if !o.is_active() {
            continue;
        }
        let size = rates.throughput_per_param.saturating_mul(o.param as i32);
        let capacity = mm_core::fixed::q10_scale(size, o.throttle());
        match o.kind {
            OrganelleType::Mitochondrion => {
                respiration += capacity;
                let p = cat.metabolism.pathway(o.control[1]);
                let latent = chem.get(p.substrate).energy_yield.max(rates.latent_per_substrate);
                let released = (capacity as i64 * latent as i64) / Q10_ONE as i64;
                gross += (released * rates.respiration_efficiency as i64) / Q10_ONE as i64;
            }
            OrganelleType::Chloroplast => photosynthesis += capacity,
            _ => {}
        }
    }

    let mut thrust = 0i32;
    for o in world.cells().slots(i) {
        let t = mm_core::sensing::cilium_thrust(o);
        thrust += mm_core::fixed::q10_scale(t.abs(), mm_core::sensing::THRUST_ENERGY);
    }
    let extension = mm_core::ecology::spike_extension(world.cells(), i);
    let spike = mm_core::fixed::q10_scale(bio.ecology.spike_upkeep, extension);

    Books {
        upkeep: cat.upkeep(world.cells().slots(i)) + rates.metabolic_floor,
        respiration,
        photosynthesis,
        gross,
        thrust,
        spike,
    }
}

/// One founder, alone, long enough to have built itself.
fn grown(name: &str, ticks: u64) -> (World, CellId) {
    let (mut world, id) = founded(name);
    world.run(ticks);
    (world, id)
}

/// One founder, seeded exactly as `place_founders` seeds them, so these numbers and the
/// acceptance tests' numbers are about the same cell.
fn founded(name: &str) -> (World, CellId) {
    let bytes = assemble(name);
    let mut world = World::new(dish()).expect("world");
    world.place_founders_at(&bytes, 1, Some((32, 32)));
    let id = world.cells().iter().next().map_or(CellId::NONE, |i| {
        world.cells().id_at(i)
    });
    (world, id)
}

/// The books of every shipped organism, in one table.
///
/// The column that matters is `net`: gross respiratory income less everything the body costs to
/// carry and to drive. A loadout whose net is a large positive number is one that can afford a
/// daughter often; a loadout near zero is solvent and sterile.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_every_shipped_loadout_earns_and_pays() {
    eprintln!("\none founder, alone in a lit dish, 900 ticks, mutation off.");
    eprintln!("`Q10` energy per tick — 1024 is one energy unit.\n");
    eprintln!(
        "{:>18} {:>6} {:>6} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}  {}",
        "genome", "fix", "burn", "gross", "upkeep", "thrust", "spike", "net", "held", "loadout"
    );
    for name in ORGANISMS {
        let (world, id) = grown(name, 900);
        let Some(i) = world.cells().index(id) else {
            eprintln!("{name:>18}  died before it was measured");
            continue;
        };
        let b = books(&world, i);
        let net = b.gross - (b.upkeep + b.thrust + b.spike) as i64;
        eprintln!(
            "{:>18} {:>6} {:>6} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}  {}",
            name,
            b.photosynthesis,
            b.respiration,
            b.gross,
            b.upkeep,
            b.thrust,
            b.spike,
            net,
            world.cells().energy[i] / Q10_ONE,
            loadout(&world, i),
        );
    }
    eprintln!(
        "\nfor scale: one division costs division_energy {} plus {} a byte to copy.",
        q10(20),
        Q10_ONE / 64
    );
}

/// How long a genome takes to come round to the same instruction again.
///
/// The instruction budget is sixteen a tick and the instruction pointer persists across ticks,
/// so a genome's whole body of code is a loop whose period is `instructions / 16` ticks. That
/// period is the cell's reaction time: a sensor reading taken at the top of the loop is acted on
/// once per cycle and not again until the next.
///
/// Against it, the distance a cell covers in one cycle at the power its own cilia are set to. A
/// cell that moves further than it can see between decisions is not steering, it is guessing.
#[test]
#[ignore = "a probe; run it on purpose"]
fn how_long_a_genome_takes_to_come_round() {
    eprintln!("\nthe control loop, against the motion it is supposed to control.\n");
    eprintln!(
        "{:>18} {:>7} {:>7} {:>8} {:>10} {:>10}",
        "genome", "bytes", "instrs", "ticks", "squares/t", "squares/cyc"
    );
    for name in ORGANISMS {
        let bytes = assemble(name);
        let pool = mm_core::GenomePool::new();
        let genome = pool.intern(bytes.clone()).expect("intern");
        let cfg = VmConfig::default();
        let ctx = mm_core::RandCtx::new(1, 0, 0);
        let mut vm = mm_core::vm::Vm::new();
        let mut host = NullHost::default();

        // One instruction at a time until the pointer comes back to where it began, which is a
        // whole pass of the genome including every branch it actually takes.
        let mut instrs = 0u32;
        for _ in 0..100_000 {
            vm.run(&genome, &cfg, &ctx, &mut host, 1);
            instrs += 1;
            if vm.ip == 0 {
                break;
            }
        }

        // The speed the founder's own cilia give it, from the grown cell rather than the source.
        let (world, id) = grown(name, 900);
        let speed = world.cells().index(id).map_or(0, |i| {
            let mut t = 0i32;
            for o in world.cells().slots(i) {
                t = t.saturating_add(mm_core::sensing::cilium_thrust(o).abs());
            }
            t
        });
        let ticks = instrs as f64 / cfg.instr_per_tick as f64;
        eprintln!(
            "{:>18} {:>7} {:>7} {:>8.1} {:>10.3} {:>10.2}",
            name,
            bytes.len(),
            instrs,
            ticks,
            speed as f64 / Q10_ONE as f64,
            ticks * speed as f64 / Q10_ONE as f64,
        );
    }
    eprintln!(
        "\nfor scale: a sensor reaches {} square and a signature reaches {}.",
        mm_core::sensing::SENSOR_RANGE,
        mm_core::ecology::EcologyConfig::default().em_range
    );
}

/// What a kill is worth, followed all the way from a body to the energy it becomes.
///
/// Every step of the chain is a configured fraction, so this is arithmetic rather than a run —
/// but the arithmetic has never been written down in one place, and the last step is the one that
/// decides the whole question: the mitochondrion cap.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_a_corpse_is_worth() {
    let bio = mm_core::BiologyConfig::default();
    let eco = &bio.ecology;
    let rates = &bio.metabolism.rates;

    // A prey cell the size the shipped ancestors settle at.
    for prey_mass in [q10(30), q10(60), q10(120)] {
        let carrion = mm_core::fixed::q10_scale(prey_mass, eco.carrion_fraction);
        let recovered = mm_core::fixed::q10_scale(carrion, eco.digestion_efficiency);
        let energy = (recovered as i64 * 1024) / Q10_ONE as i64; // sugar at yield 1024
        let usable = (energy * rates.respiration_efficiency as i64) / Q10_ONE as i64;
        // What a mitochondrion at param 50 can put through in a tick.
        let per_tick = rates.throughput_per_param * 50;
        eprintln!(
            "prey mass {:>4}  carrion {:>4}  sugar {:>4}  energy {:>6}  \
             = {:>5.1} ticks of one mitochondrion, {:>4.1}% of a division",
            prey_mass / Q10_ONE,
            carrion / Q10_ONE,
            recovered / Q10_ONE,
            usable,
            usable as f64 / per_tick as f64,
            100.0 * usable as f64 / bio.division_energy as f64,
        );
    }

    eprintln!(
        "\nand what the killing costs: a spike at param 80 is {} to build, {} a tick to carry,\n\
         and {} a tick more to hold out at full extension.",
        bio.metabolism
            .catalogue
            .spec(OrganelleType::Spike)
            .matter_cost(80)
            / Q10_ONE,
        bio.metabolism
            .catalogue
            .spec(OrganelleType::Spike)
            .upkeep_cost(80),
        mm_core::fixed::q10_scale(eco.spike_upkeep, q10(80)),
    );
    eprintln!(
        "a membrane at param 24 tolerates {} damage; a spike at full extension deals {} a tick,\n\
         so a kill takes {} ticks of contact.",
        q10(24) / Q10_ONE,
        mm_core::fixed::q10_scale(eco.spike_damage, q10(80)) as f64 / Q10_ONE as f64,
        q10(24) / mm_core::fixed::q10_scale(eco.spike_damage, q10(80)).max(1),
    );
}

/// Which of the shipped organisms can actually found a lineage, and what kills the ones that
/// cannot.
///
/// **The `cells` column is not a population measurement and must not be read as one.** This is one
/// founder, one seed: a lineage either takes off or it does not, and the variance across seeds
/// swamps any difference between genomes. Measured the hard way — `ancestor.mm` and `mutator.mm`
/// have identical `#divide` genes and, with mutation off, differ only in length, and in one run
/// one of them came back with four cells and the other with six hundred and twenty-two.
///
/// What it *is* good for is the per-cell columns: how a founder dies, how much solute it is
/// carrying when it does, and whether it was poisoned or swollen. Those are properties of one cell
/// and need no population to be visible.
///
/// For anything about which strategy wins, use the panel in `tests/balance.rs` — sixteen founders
/// a side, three seeds, median. That is what it is for.
///
/// `m8_ecology::the_shipped_organisms_reproduce` asks for four cells from one founder and says
/// pass or fail. This asks the next question: how long does a founder last, how many daughters
/// does it get, and which of the three ways out did it take — starved, poisoned, or crushed.
#[test]
#[ignore = "a probe; run it on purpose"]
fn which_founders_survive_being_alone() {
    eprintln!("\none founder, alone, 6000 ticks, mutation off.\n");
    eprintln!(
        "{:>18} {:>7} {:>7} {:>8} {:>8} {:>9} {:>9}",
        "genome", "cells", "births", "died at", "energy", "solute", "damage"
    );
    for name in ORGANISMS {
        let (mut world, id) = founded(name);

        let mut births = 0u64;
        let mut died_at: i64 = -1;
        // The last state the founder was seen in, so a death can be attributed.
        let (mut e, mut s, mut d) = (0i32, 0i64, 0i32);
        for tick in 0..6000u64 {
            world.step();
            births = world.births_total();
            if let Some(i) = world.cells().index(id) {
                e = world.cells().energy[i];
                s = mm_core::biology::osmotic_load(world.cells(), i);
                d = world.cells().damage[i];
            } else if died_at < 0 {
                died_at = tick as i64;
            }
            if world.cells().len() == 0 {
                break;
            }
        }
        eprintln!(
            "{:>18} {:>7} {:>7} {:>8} {:>8} {:>9} {:>9}",
            name,
            world.cells().len(),
            births,
            if died_at < 0 {
                "alive".to_string()
            } else {
                died_at.to_string()
            },
            e / Q10_ONE,
            s / Q10_ONE as i64,
            d / Q10_ONE,
        );
    }
    eprintln!(
        "\nthe founder's membrane tolerates {} damage and the osmotic threshold is {}.",
        24,
        mm_core::MetabolicRates::default().osmotic_threshold / Q10_ONE
    );
}

/// What light is worth, and where autotrophy stops being free.
///
/// The chloroplast's rate is `throughput_per_param * param * light`, so intensity multiplies
/// income directly. Every scenario in the library runs at full intensity, where the ancestor
/// earns five times what it spends. This is the sweep nobody has run: how dim does the slide have
/// to be before sitting still is a living rather than a windfall.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_light_is_worth() {
    let bytes = assemble("ancestor.mm");
    eprintln!("\nsixteen founders, mutation off, population at each of four horizons.");
    eprintln!(
        "the horizons are the point: a dimmer slide is not only smaller, it converges slower,\n\
         and a sweep taken at one tick count reads the second as the first.\n"
    );
    eprintln!(
        "{:>10} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "intensity", "12k", "25k", "50k", "100k", "med E"
    );
    for intensity in [Q10_ONE, Q10_ONE * 3 / 4, Q10_ONE / 2, Q10_ONE / 4, Q10_ONE / 8] {
        let mut world = World::new(Scenario {
            light: LightRegime::Uniform { intensity },
            ..dish()
        })
        .expect("world");
        world.place_founders(&bytes, 16);
        let mut row = Vec::new();
        let mut last = 0u64;
        for horizon in [12_000u64, 25_000, 50_000, 100_000] {
            world.run(horizon - last);
            last = horizon;
            row.push(world.cells().len());
        }
        let mut energies: Vec<i32> = world
            .cells()
            .iter()
            .map(|i| world.cells().energy[i])
            .collect();
        energies.sort_unstable();
        eprintln!(
            "{:>10} {:>9} {:>9} {:>9} {:>9} {:>9}",
            intensity,
            row[0],
            row[1],
            row[2],
            row[3],
            energies.get(energies.len() / 2).copied().unwrap_or(0) / Q10_ONE,
        );
    }
}

/// Two lineages on one slide, sharing a carrying capacity.
///
/// `mm-cli match` does this with arena rules; this does it in a plain world so a single
/// parameter can be varied around it.
fn share(
    bio: mm_core::BiologyConfig,
    light: i32,
    left: &str,
    right: &str,
    ticks: u64,
) -> (usize, usize) {
    let l = assemble(left);
    let r = assemble(right);
    let mut world = World::new(Scenario {
        light: LightRegime::Uniform { intensity: light },
        biology: bio,
        ..dish()
    })
    .expect("world");

    // Sixteen of each, spread over its own half of the slide rather than piled on one square.
    // The pile is not a neutral choice: sixteen sessile founders one square apart are already
    // over `split_pressure`, so they cannot divide, cannot shed solute, and die of turgor —
    // which measures the placement rather than the strategy. A swimmer escapes the pile and a
    // sitter does not, so a pile silently hands the match to whichever side has cilia.
    let mut plant = |world: &mut World, bytes: &[u8], x0: u32| -> u32 {
        for k in 0..16u32 {
            world.place_founders_at(bytes, 1, Some((x0 + (k % 4) * 8, 6 + (k / 4) * 16)));
        }
        // Founders of one genome share one species (`Phylogeny::found`), so any of them names it.
        world
            .cells()
            .iter()
            .map(|i| world.cells().species[i])
            .max()
            .unwrap_or(0)
    };
    let left_founder = plant(&mut world, &l, 4);
    let right_founder = plant(&mut world, &r, 36);
    assert_ne!(
        left_founder, right_founder,
        "both sides were filed under one species, so nothing here can be told apart"
    );

    world.run(ticks);

    // Attributed by **ancestry**, not by genome bytes. A daughter's genome is not always its
    // parent's: `BUD` allocates the size the genome asked for and `COPY` fills it a byte at a
    // time over many ticks, so a lineage that splits before it has finished copying breeds true
    // to its own descendants and not to its founder. Counting exact byte matches finds only the
    // cells that happened to finish, which was five per cent of a slide.
    let (mut a, mut b) = (0usize, 0usize);
    for i in world.cells().iter() {
        let species = world.cells().species[i];
        let line = world.archive().ancestry(species);
        if line.contains(&left_founder) {
            a += 1;
        } else if line.contains(&right_founder) {
            b += 1;
        }
    }
    (a, b)
}

/// The control for [`which_dial_lets_a_hunter_live`]: does the harness itself pick a winner?
///
/// Two lineages on one slide is a fair test only if the two halves of the slide are worth the
/// same and the order of placement does not matter. Both are worth checking rather than
/// assuming, because either one being false would make every row of that table an artefact.
#[test]
#[ignore = "a probe; run it on purpose"]
fn the_two_halves_of_the_slide_are_worth_the_same() {
    let base = mm_core::BiologyConfig {
        mutation: MutationRates::none(),
        ..Default::default()
    };
    for (l, r) in [
        ("ancestor.mm", "mutator.mm"),
        ("mutator.mm", "ancestor.mm"),
        ("stalker.mm", "ancestor.mm"),
        ("ancestor.mm", "stalker.mm"),
    ] {
        let (a, b) = share(base.clone(), Q10_ONE, l, r, 20_000);
        eprintln!("{l:>14} {a:>6}   against {r:>14} {b:>6}");
    }
}

/// Which single dial lets anything other than the sit-still autotroph hold a share of the slide.
///
/// Every strategy in `genomes/` loses to `ancestor.mm` head to head, and loses in order of
/// upkeep. This tries the obvious levers one at a time — never two — because the value of the
/// exercise is knowing which one moves the answer, not producing a world in which the hunter
/// happens to win.
#[test]
#[ignore = "a probe; run it on purpose"]
fn which_dial_lets_a_hunter_live() {
    let base = mm_core::BiologyConfig {
        mutation: MutationRates::none(),
        ..Default::default()
    };
    // Eighty thousand, not twenty. `what_light_is_worth` found that a dim slide takes tens of
    // thousands of ticks to leave its lag phase — at half intensity the ancestor sits at eighty
    // cells until tick 25,000 and reaches a thousand by 100,000 — so a sweep read at twenty
    // thousand reports the lag and calls it the ceiling.
    let ticks = 80_000;

    let variants: Vec<(&str, mm_core::BiologyConfig, i32)> = vec![
        ("as shipped", base.clone(), Q10_ONE),
        (
            "turgor off",
            mm_core::BiologyConfig {
                metabolism: mm_core::Metabolism {
                    rates: mm_core::MetabolicRates {
                        osmotic_upkeep: 0,
                        ..base.metabolism.rates
                    },
                    ..base.metabolism.clone()
                },
                ..base.clone()
            },
            Q10_ONE,
        ),
        (
            "spike 8x cheaper to hold",
            mm_core::BiologyConfig {
                ecology: mm_core::ecology::EcologyConfig {
                    spike_upkeep: Q10_ONE / 512,
                    ..base.ecology
                },
                ..base.clone()
            },
            Q10_ONE,
        ),
        (
            "a corpse loses nothing",
            mm_core::BiologyConfig {
                ecology: mm_core::ecology::EcologyConfig {
                    carrion_fraction: Q10_ONE,
                    digestion_efficiency: Q10_ONE,
                    digestion_rate: Q10_ONE,
                    ..base.ecology
                },
                ..base.clone()
            },
            Q10_ONE,
        ),
        (
            "light is rival",
            mm_core::BiologyConfig {
                metabolism: mm_core::Metabolism {
                    rates: mm_core::MetabolicRates {
                        light_occlusion: 128,
                        rigidity_gain: 16384,
                        ..base.metabolism.rates
                    },
                    ..base.metabolism.clone()
                },
                ..base.clone()
            },
            Q10_ONE,
        ),
        ("half light", base.clone(), Q10_ONE / 2),
        (
            "half light, turgor off",
            mm_core::BiologyConfig {
                metabolism: mm_core::Metabolism {
                    rates: mm_core::MetabolicRates {
                        osmotic_upkeep: 0,
                        ..base.metabolism.rates
                    },
                    ..base.metabolism.clone()
                },
                ..base.clone()
            },
            Q10_ONE / 2,
        ),
    ];

    eprintln!("\nsixteen founders each, 80000 ticks, mutation off. one parameter at a time.\n");
    eprintln!(
        "{:>26} {:>9} {:>9} {:>9} {:>9}",
        "variant", "stalker", "ancestor", "sponge", "ancestor"
    );
    for (label, bio, light) in variants {
        let (hunter, prey) = share(bio.clone(), light, "stalker.mm", "ancestor.mm", ticks);
        let (filter, prey2) = share(bio, light, "sponge.mm", "ancestor.mm", ticks);
        eprintln!("{label:>26} {hunter:>9} {prey:>9} {filter:>9} {prey2:>9}");
    }
}

/// Whether the osmotic charge is what turns a dimmer slide into an empty one.
///
/// `what_light_is_worth` finds a cliff rather than a slope: full light holds a thousand cells and
/// half light holds eighty. A 40% cut in income should not cost 92% of a population, so something
/// is amplifying it. The candidate is turgor, because it is quadratic in a quantity that climbs
/// linearly whenever a cell is not dividing — so a cell that slows down is charged the square of
/// how long it has been slow.
#[test]
#[ignore = "a probe; run it on purpose"]
fn whether_turgor_is_what_makes_the_cliff() {
    let bytes = assemble("ancestor.mm");
    eprintln!("\nsixteen founders, 12000 ticks, mutation off.\n");
    eprintln!("{:>10} {:>12} {:>12}", "intensity", "as shipped", "turgor off");
    for intensity in [Q10_ONE, Q10_ONE * 3 / 4, Q10_ONE / 2, Q10_ONE / 4, Q10_ONE / 8] {
        let mut row = Vec::new();
        for upkeep in [mm_core::MetabolicRates::default().osmotic_upkeep, 0] {
            let mut base = mm_core::BiologyConfig {
                mutation: MutationRates::none(),
                ..Default::default()
            };
            base.metabolism.rates.osmotic_upkeep = upkeep;
            let mut world = World::new(Scenario {
                light: LightRegime::Uniform { intensity },
                biology: base,
                ..dish()
            })
            .expect("world");
            world.place_founders(&bytes, 16);
            world.run(12_000);
            row.push(world.cells().len());
        }
        eprintln!("{:>10} {:>12} {:>12}", intensity, row[0], row[1]);
    }
}

/// What a saturated population has run out of.
///
/// Runs the ancestor to its ceiling and then asks where the world's structural carbon is, how
/// much energy the population is throwing away, and how many divisions were refused for want of
/// room rather than for want of means.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_the_population_runs_out_of() {
    let bytes = assemble("ancestor.mm");
    let mut world = World::new(dish()).expect("world");
    world.place_founders(&bytes, 16);
    world.adopt_current_contents_as_baseline();

    eprintln!(
        "\n{:>7} {:>7} {:>9} {:>9} {:>9} {:>8} {:>8} {:>8}",
        "tick", "cells", "C in body", "C in cyto", "C in water", "refused", "med E", "med load"
    );
    for step in 0..12 {
        world.run(2000);
        let cells = world.cells();
        let mut in_body = 0i64;
        let mut in_cyto = 0i64;
        let mut energies: Vec<i32> = Vec::new();
        let mut loads: Vec<i64> = Vec::new();
        for i in cells.iter() {
            in_body += cells.mass[i] as i64;
            in_cyto += cells.interior(i)[4] as i64;
            energies.push(cells.energy[i]);
            loads.push(mm_core::biology::osmotic_load(cells, i));
        }
        energies.sort_unstable();
        loads.sort_unstable();
        let med = |v: &Vec<i32>| v.get(v.len() / 2).copied().unwrap_or(0);
        let medl = |v: &Vec<i64>| v.get(v.len() / 2).copied().unwrap_or(0);
        let in_water = world.substrate().total_chem()[4];
        eprintln!(
            "{:>7} {:>7} {:>9} {:>9} {:>9} {:>8} {:>8} {:>8}",
            (step + 1) * 2000,
            world.cells().len(),
            in_body / Q10_ONE as i64,
            in_cyto / Q10_ONE as i64,
            in_water / Q10_ONE as i64,
            world.report().biology.failed_splits,
            med(&energies) / Q10_ONE,
            medl(&loads) / Q10_ONE as i64,
        );
    }
    eprintln!(
        "\nthe osmotic threshold is {} and the energy reserve is {}.",
        mm_core::MetabolicRates::default().osmotic_threshold / Q10_ONE,
        mm_core::MetabolicRates::default().energy_reserve / Q10_ONE,
    );
}

/// Whether a sponge on the wall earns anything a sitting autotroph does not.
///
/// `sponge.mm` on `the_drift.ron` ends at 59 cells against `ancestor.mm`'s 60 — but that is not
/// a fair test and should not be quoted as one. `place_founders` spreads founders over the whole
/// slide, and a holdfast grips nothing unless the body overlaps a barrier, so most of the sponges
/// in that run never anchored, never refused the drift, never had any slip past them and
/// therefore never filtered. This puts both lineages *on the wall*, which is the only place the
/// mechanism can work at all.
///
/// The barriers in that file run along y = 32 and y = 62, so a founder placed at y = 31 is
/// resting on the upper one.
#[test]
#[ignore = "a probe; run it on purpose"]
fn whether_a_sponge_on_the_wall_earns_anything() {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/the_drift.ron"),
    )
    .expect("the_drift.ron");
    let mut base = Scenario::from_ron(&text).expect("parses");
    base.biology.mutation = MutationRates::none();

    eprintln!("\nboth lineages seeded along the upper barrier of `the_drift`, 30000 ticks.\n");
    eprintln!(
        "{:>14} {:>7} {:>9} {:>9} {:>10}",
        "genome", "cells", "med E", "filtered", "detritus"
    );
    for name in ["sponge.mm", "ancestor.mm", "hoarder.mm"] {
        let bytes = assemble(name);
        let mut world = World::new(base.clone()).expect("world");
        // Along the wall, inside the channel, spread out so they are not each other's problem.
        for k in 0..16u32 {
            world.place_founders_at(&bytes, 1, Some((8 + k * 5, 31)));
        }
        let mut filtered = 0i64;
        for _ in 0..30_000 {
            world.step();
            filtered += world.report().ecology.filtered;
        }
        let mut energies: Vec<i32> = world
            .cells()
            .iter()
            .map(|i| world.cells().energy[i])
            .collect();
        energies.sort_unstable();
        eprintln!(
            "{:>14} {:>7} {:>9} {:>9} {:>10}",
            name,
            world.cells().len(),
            energies.get(energies.len() / 2).copied().unwrap_or(0) / Q10_ONE,
            filtered / Q10_ONE as i64,
            world.substrate().total_chem()[mm_core::ecology::DETRITUS] / Q10_ONE as i64,
        );
    }
}

/// Does the armed lineage ever actually draw?
///
/// `which_dial_lets_a_hunter_live` returns **bit-identical** populations with `spike_upkeep` at
/// its default and at an eighth of it. Over eighty thousand ticks that can only happen if the
/// cost was never charged, which would mean the spike was never extended — so this asks the
/// question directly rather than inferring it, because "the flagship predator never draws its
/// weapon" is too large a claim to make from a coincidence.
#[test]
#[ignore = "a probe; run it on purpose"]
fn whether_the_armed_lineages_ever_draw() {
    for name in ["stalker.mm", "sentinel.mm", "predator.mm", "hunter.mm"] {
        let hunter = assemble(name);
        let prey = assemble("ancestor.mm");
        let mut world = World::new(dish()).expect("world");
        for k in 0..8u32 {
            world.place_founders_at(&hunter, 1, Some((10 + k * 6, 20)));
            world.place_founders_at(&prey, 1, Some((10 + k * 6, 40)));
        }

        let (mut drawn, mut ticks_seen, mut wounds) = (0u64, 0u64, 0u64);
        let mut badges = std::collections::BTreeSet::new();
        for _ in 0..8_000u64 {
            world.step();
            wounds += u64::from(world.report().ecology.wounded);
            let mut any = 0u64;
            for i in world.cells().iter() {
                badges.insert(world.cells().badge[i]);
                if mm_core::ecology::spike_extension(world.cells(), i) > 0 {
                    any += 1;
                }
            }
            if any > 0 {
                ticks_seen += 1;
            }
            drawn += any;
        }
        eprintln!(
            "{name:>14}  cell-ticks with a spike out: {drawn:>8}   ticks on which any was out: \
             {ticks_seen:>6}/8000   wounds dealt: {wounds:>8}   badges worn: {badges:?}"
        );
    }
}

/// What a *fat* corpse is worth, against the lean one `what_a_corpse_is_worth` follows.
///
/// `apply_deaths` turns half the body into carrion and returns the **whole cytoplasm** to the
/// square, before any of predation's arithmetic applies. So a cell that has stored matter against
/// the dark is worth something quite different to whoever kills it than a cell that has not — and
/// by a route that skips `carrion_fraction`, `digestion_efficiency` and the lysosome entirely,
/// because dissolved sugar is taken with `EAT`, which is free and instant.
///
/// `docs/ECONOMY.md` §10 leans on this, so it is measured rather than reasoned about.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_a_fat_corpse_is_worth() {
    let bio = mm_core::BiologyConfig::default();
    let rates = &bio.metabolism.rates;

    for stored in [0, q10(50), q10(200)] {
        let mut world = World::new(dish()).expect("world");
        // A victim, with a body and a cytoplasm, and nothing else — no genome behaviour, so that
        // this measures the deposit and not a genome's reaction to being killed.
        let g = world.genomes().intern(vec![0x2Eu8]).expect("intern");
        let victim = world.spawn_cell(mm_core::CellSeed {
            x: pos(20),
            y: pos(20),
            mass: q10(60),
            energy: q10(1_000),
            membrane: 24,
            key: 11,
            badge: 0,
            species: 0,
            parent: mm_core::CellId::NONE,
            birth_tick: 0,
            genome: g,
        });
        if let Some(i) = world.cells_mut().index(victim) {
            world.cells_mut().interior_mut(i)[8] = stored;
        }
        world.adopt_current_contents_as_baseline();

        let before_sugar = world.substrate().chem_at(8, 20, 20);
        let before_carrion = world.substrate().chem_at(mm_core::ecology::CARRION, 20, 20);
        world.kill_cell(victim);
        world.step();
        let sugar = world.substrate().chem_at(8, 20, 20) - before_sugar;
        let carrion = world.substrate().chem_at(mm_core::ecology::CARRION, 20, 20) - before_carrion;

        // What each is worth to a killer standing on the square, in energy.
        //
        // Sugar: `EAT` takes it whole, bounded by one interior capacity, and a mitochondrion
        // recovers `respiration_efficiency` of its latent energy.
        let takeable = sugar.min(mm_core::biology::BASE_INTERIOR_CAPACITY);
        let from_sugar = (takeable as i64 * 1024 / Q10_ONE as i64)
            * rates.respiration_efficiency as i64
            / Q10_ONE as i64;
        // Carrion: digested at `digestion_efficiency` into substrate, then burnt.
        let from_carrion = ((carrion as i64
            * bio.ecology.digestion_efficiency as i64
            / Q10_ONE as i64)
            * 1024
            / Q10_ONE as i64)
            * rates.respiration_efficiency as i64
            / Q10_ONE as i64;

        eprintln!(
            "victim holding {:>4} sugar:  deposit {:>4} sugar + {:>4} carrion  ->  \
             {:>7} Q10 by EAT (capped at one capacity), {:>7} Q10 by lysosome  \
             = {:>4}% and {:>4}% of a division",
            stored / Q10_ONE,
            sugar / Q10_ONE,
            carrion / Q10_ONE,
            from_sugar,
            from_carrion,
            100 * from_sugar / bio.division_energy as i64,
            100 * from_carrion / bio.division_energy as i64,
        );
    }
    eprintln!(
        "\nand what a corpse does *not* carry: `apply_deaths` dissipates the victim's banked \
         energy as heat.\nStoring against the dark has to be stored as matter, and matter is what \
         a killer gets."
    );
}

/// Which chemical a swollen cell is actually swollen with.
///
/// `docs/ECONOMY.md` §6 blamed the shipped `#feed` genes for eating on a timer, and teaching them
/// to read before eating turned out to cost most of the population without fixing the solute. So
/// the assumption was wrong somewhere, and this is the measurement that was skipped: not how much
/// solute a founder holds, but *what it is made of*.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_a_swollen_cell_is_swollen_with() {
    let names = mm_core::ChemTable::spec_default();
    for genome in ["ancestor.mm", "hoarder.mm", "sponge.mm"] {
        let (mut world, id) = founded(genome);
        eprintln!("\n{genome}");
        eprintln!(
            "{:>7} {:>8} {:>7}   {}",
            "tick", "solute", "energy", "the three largest holdings"
        );
        for step in 0..6 {
            world.run(1_000);
            let Some(i) = world.cells().index(id) else {
                eprintln!("{:>7}  founder dead", (step + 1) * 1000);
                break;
            };
            let interior = world.cells().interior(i);
            let mut held: Vec<(usize, i32)> = interior
                .iter()
                .enumerate()
                .map(|(c, v)| (c, *v / Q10_ONE))
                .filter(|(_, v)| *v > 0)
                .collect();
            held.sort_by_key(|(_, v)| -v);
            let top: Vec<String> = held
                .iter()
                .take(3)
                .map(|(c, v)| format!("{} {v}", names.get(*c).name))
                .collect();
            eprintln!(
                "{:>7} {:>8} {:>7}   {}",
                (step + 1) * 1000,
                mm_core::biology::osmotic_load(world.cells(), i) / Q10_ONE as i64,
                world.cells().energy[i] / Q10_ONE,
                top.join(",  ")
            );
        }
    }
    eprintln!(
        "\nthe osmotic threshold is {} units, summed over all sixteen chemicals.",
        mm_core::MetabolicRates::default().osmotic_threshold / Q10_ONE
    );
}

/// Do organelles cost too much to be worth building?
///
/// The question is the right shape and the premise wants checking first, because "they never get
/// built" is an impression and this is the measurement that would confirm or kill it. **Mutation
/// is on here, unlike everywhere else in this file**, and it has to be: the question is not what
/// a hand-written loadout is worth, it is what evolution converges on when it is free to choose.
///
/// `upkeep` scales every entry in the catalogue at once — both `upkeep` and `upkeep_per_param`,
/// for all sixteen types — so the sweep asks one thing and not sixteen. What to watch is not the
/// population, which is bounded by space (§3), but the **census**: how many organelles a typical
/// cell carries, and which types appear at all. A world where cheaper machinery buys richer
/// bodies is a world where the price was the thing stopping them.
#[test]
#[ignore = "a probe; run it on purpose. Mutation is on"]
fn do_organelles_cost_too_much_to_be_worth_building() {
    let bytes = assemble("ancestor.mm");
    eprintln!("\nsixteen founders on the soup, mutation ON, 40,000 ticks.\n");
    eprintln!(
        "{:>8} {:>7} {:>9} {:>9}   {}",
        "upkeep", "cells", "organs", "loadouts", "types carried by 1% or more of the population"
    );

    for percent in [25i64, 50, 100, 200] {
        let mut bio = mm_core::BiologyConfig::default();
        let mut specs = *bio.metabolism.catalogue.specs();
        for spec in specs.iter_mut() {
            spec.upkeep = ((spec.upkeep as i64 * percent) / 100) as i32;
            spec.upkeep_per_param = ((spec.upkeep_per_param as i64 * percent) / 100) as i32;
        }
        bio.metabolism.catalogue.set_specs(specs);

        let mut world = World::new(Scenario {
            biology: bio,
            ..dish()
        })
        .expect("world");
        // The dish is mutation-off by construction; this question needs it on.
        let mut with_mutation = world.biology().clone();
        with_mutation.mutation = mm_core::MutationRates::default();
        world.set_biology(with_mutation);
        world.place_founders(&bytes, 16);
        world.run(40_000);

        let cells = world.cells();
        let n = cells.len().max(1);
        let mut organs = 0usize;
        let mut census = [0usize; mm_core::organelle::SLOT_COUNT];
        let mut loadouts = std::collections::BTreeSet::new();
        for i in cells.iter() {
            let mut kinds = Vec::new();
            for o in cells.slots(i) {
                if o.is_active() {
                    organs += 1;
                    census[(o.kind as u8 as usize) % mm_core::organelle::SLOT_COUNT] += 1;
                    if !kinds.contains(&o.kind) {
                        kinds.push(o.kind);
                    }
                }
            }
            kinds.sort();
            loadouts.insert(kinds);
        }
        let carried: Vec<String> = OrganelleType::all()
            .iter()
            .filter(|k| census[(**k as u8 as usize) % mm_core::organelle::SLOT_COUNT] * 100 >= n)
            .map(|k| {
                format!(
                    "{} {}%",
                    k.name(),
                    census[(*k as u8 as usize) % mm_core::organelle::SLOT_COUNT] * 100 / n
                )
            })
            .collect();

        eprintln!(
            "{:>7}% {:>7} {:>9.2} {:>9}   {}",
            percent,
            cells.len(),
            organs as f64 / n as f64,
            loadouts.len(),
            carried.join(", ")
        );
    }
    eprintln!(
        "\nthe ancestor is seeded with four: membrane, nucleus, mitochondrion, chloroplast.\n\
         Anything above four is machinery evolution paid for; anything below is machinery it \
         shed."
    );
}
