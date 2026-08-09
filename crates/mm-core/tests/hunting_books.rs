//! One hunter in a crowd, and where its energy goes.
//!
//! `hunting_probe` asked whether a hunter can *find* prey and `predator_probe` asked why one
//! genome stopped reproducing. This asks the question the microscope raises the moment a hunter
//! can finally move: **it ploughs a visible track through a mat of ancestors and then dies with
//! a cytoplasm full of sugar.** Is it failing to eat what it kills, or eating fine and unable to
//! do anything with it?
//!
//! `ECONOMY.md` §4 says the second, from arithmetic. This is the same claim watched happening,
//! in the setup the observation came from — a hand-placed hunter dropped into a saturated slide,
//! rather than two lineages seeded together at tick zero.
//!
//! Run with
//! `cargo test --release -p mm-core --test hunting_books -- --ignored --nocapture --test-threads=1`.

use std::path::Path;

use mm_core::fixed::{q10, Q10_ONE};
use mm_core::organelle::{OrganelleType, SLOT_COUNT};
use mm_core::{MutationRates, Scenario, World};

fn assemble(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    mm_asm::assemble(&src)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .bytes
}

fn soup(w: u32, h: u32) -> Scenario {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/soup.ron"),
    )
    .expect("soup");
    let mut scenario = Scenario::from_ron(&src).expect("parse");
    scenario.width = w;
    scenario.height = h;
    scenario.biology.mutation = MutationRates::none();
    scenario
}

fn loadout(world: &World, i: usize) -> String {
    let mut parts = Vec::new();
    for slot in 0..SLOT_COUNT {
        let o = world.cells().slots(i)[slot];
        if o.is_present() {
            parts.push(format!(
                "{}:{}({}){}",
                slot,
                o.kind.name(),
                o.param,
                if o.is_active() { "" } else { "…" }
            ));
        }
    }
    parts.join(" ")
}

/// What one hunter does after it is dropped into a full slide, tick by tick.
///
/// The setup is the microscope's: grow a mat of `ancestor.mm` to saturation first, *then* place
/// one hunter into the middle of it, exactly as the seed tool does.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_a_hunter_dropped_into_a_full_slide_actually_does() {
    for name in ["stalker.mm", "sentinel.mm", "predator.mm", "hunter.mm"] {
        let mut world = World::new(soup(64, 64)).expect("world");
        world.place_founders(&assemble("ancestor.mm"), 16);
        world.run(8_000);
        let mat = world.cells().len();

        // One hunter, dropped in the middle, the way `Tool::PlaceCell` does it.
        world.place_founders_at(&assemble(name), 1, Some((32, 32)));
        let Some(id) = world
            .cells()
            .iter()
            .find(|&i| world.cells().genome[i].len() == assemble(name).len())
            .map(|i| world.cells().id_at(i))
        else {
            eprintln!("{name}: never landed");
            continue;
        };

        eprintln!("\n=== {name} into a mat of {mat} ===");
        eprintln!(
            "{:>7} {:>8} {:>7} {:>7} {:>7} {:>8} {:>7} {:>10} {:>10}",
            "tick", "energy", "sugar", "oxygen", "mass", "wounded", "deaths", "digested", "scavenged"
        );
        let (mut wounded, mut deaths, mut digested, mut scavenged) = (0u64, 0u64, 0i64, 0i64);
        let mut last = String::new();
        for t in 1..=2_000u64 {
            world.step();
            let r = world.report();
            wounded += r.ecology.wounded as u64;
            deaths += r.biology.deaths as u64;
            digested += r.ecology.digested;
            scavenged += r.ecology.scavenged;
            let Some(i) = world.cells().index(id) else {
                eprintln!("{t:>7}   died");
                break;
            };
            if t % 250 == 0 {
                let c = world.cells();
                eprintln!(
                    "{:>7} {:>8} {:>7} {:>7} {:>8} {:>7} {:>10} {:>10} {:>10}",
                    t,
                    c.energy[i] / Q10_ONE,
                    c.interior(i)[8] / Q10_ONE,
                    c.interior(i)[14] / Q10_ONE,
                    c.mass[i] / Q10_ONE,
                    wounded,
                    deaths,
                    digested / Q10_ONE as i64,
                    scavenged / Q10_ONE as i64,
                );
                last = format!(
                    "nucleus {} B · {}",
                    mm_core::biology::nucleus_capacity(world.cells(), i),
                    loadout(&world, i)
                );
            }
        }
        if !last.is_empty() {
            eprintln!("  loadout: {last}");
        }
    }
    eprintln!("\n`wounded`, `deaths`, `digested` and `scavenged` are the whole slide's — but the");
    eprintln!("mat is in equilibrium before the hunter lands and no ancestor carries a spike or a");
    eprintln!("lysosome, so all four are its work. `digested` is carrion a lysosome broke down and");
    eprintln!("`scavenged` is the substrate that came back out of it: that is eating what it kills,");
    eprintln!("counted. A zero there and a full cytoplasm are different diagnoses.");
}

/// The books, at the moment it is hunting.
///
/// Income against every line of the bill, for the same hunter in the same mat. `ECONOMY.md` §1's
/// table was taken on a founder alone in a lit dish with its weapon stowed; this is the same
/// arithmetic with the weapon out and the cilia beating, which is the state it dies in.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_it_earns_while_it_is_hunting() {
    let mut world = World::new(soup(64, 64)).expect("world");
    world.place_founders(&assemble("ancestor.mm"), 16);
    world.run(8_000);

    eprintln!("\n`Q10` a tick, measured 400 ticks after the hunter lands in a full slide.\n");
    eprintln!(
        "{:>14} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9}",
        "genome", "gross", "upkeep", "thrust", "spike", "net", "held sugar"
    );
    for name in ["stalker.mm", "sentinel.mm", "predator.mm", "hunter.mm", "ancestor.mm"] {
        let mut w = world.clone();
        let before: Vec<_> = w.cells().iter().collect();
        w.place_founders_at(&assemble(name), 1, Some((32, 32)));
        let Some(i) = w.cells().iter().find(|i| !before.contains(i)) else {
            continue;
        };
        let id = w.cells().id_at(i);
        w.run(400);
        let Some(i) = w.cells().index(id) else {
            eprintln!("{name:>14}   died inside 400 ticks");
            continue;
        };

        let bio = w.biology();
        let rates = &bio.metabolism.rates;
        let cat = &bio.metabolism.catalogue;
        let chem = w.scenario().chemicals.clone();

        let mut gross = 0i64;
        for o in w.cells().slots(i) {
            if o.is_active() && o.kind == OrganelleType::Mitochondrion {
                let size = rates.throughput_per_param.saturating_mul(o.param as i32);
                let capacity = mm_core::fixed::q10_scale(size, o.throttle());
                let p = cat.metabolism.pathway(o.control[1]);
                let latent = chem
                    .get(p.substrate)
                    .energy_yield
                    .max(rates.latent_per_substrate);
                let released = (capacity as i64 * latent as i64) / Q10_ONE as i64;
                gross += (released * rates.respiration_efficiency as i64) / Q10_ONE as i64;
            }
        }
        let upkeep = cat.upkeep(w.cells().slots(i)) + rates.metabolic_floor;
        let thrust: i32 = w
            .cells()
            .slots(i)
            .iter()
            .map(|o| {
                mm_core::fixed::q10_scale(
                    mm_core::sensing::cilium_thrust(o).abs(),
                    mm_core::sensing::THRUST_ENERGY,
                )
            })
            .sum();
        let extension = mm_core::ecology::spike_extension(w.cells(), i);
        let spike = mm_core::fixed::q10_scale(bio.ecology.spike_upkeep, extension);

        eprintln!(
            "{:>14} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9}",
            name,
            gross,
            upkeep,
            thrust,
            spike,
            gross - (upkeep + thrust + spike) as i64,
            w.cells().interior(i)[8] / Q10_ONE,
        );
    }
    eprintln!("\n`gross` is what the mitochondria could recover if substrate were free. It does");
    eprintln!("not move when a cell eats, which is `ECONOMY.md` §1 and the whole question here:");
    eprintln!("a full cytoplasm and a negative net at the same time is a cell that is fed and");
    eprintln!("cannot burn it. One division costs {} plus copying.", q10(20));
}

/// The same drop, on the microscope's terms rather than a probe's.
///
/// Mutation **on** and a slide big enough to be the one a screenshot comes from. Reported
/// because a hunter placed by hand in the app died with `nucleus 0` where the same hunter in the
/// table above builds a nucleus of 80 and lives — and the two runs differ in exactly these two
/// things, so one of them is the difference.
#[test]
#[ignore = "a probe; run it on purpose"]
fn the_same_drop_with_mutation_on() {
    for (label, mutate, size) in [
        ("64x64, mutation off", false, 64u32),
        ("64x64, mutation on", true, 64),
        ("128x128, mutation on", true, 128),
    ] {
        let mut scenario = soup(size, size);
        if mutate {
            scenario.biology.mutation = MutationRates::default();
        }
        let mut world = World::new(scenario).expect("world");
        world.place_founders(&assemble("ancestor.mm"), 16);
        world.run(8_000);
        let mat = world.cells().len();
        let before: Vec<_> = world.cells().iter().collect();
        world.place_founders_at(&assemble("stalker.mm"), 1, Some((size as u32 / 2, size as u32 / 2)));
        let Some(i) = world.cells().iter().find(|i| !before.contains(i)) else {
            eprintln!("{label}: never landed");
            continue;
        };
        let id = world.cells().id_at(i);

        eprintln!("\n=== stalker into {label}, a mat of {mat} ===");
        eprintln!("{:>7} {:>8} {:>7} {:>9} {:>7}  {}", "tick", "energy", "mass", "nucleus", "slots", "loadout");
        for t in 1..=2_000u64 {
            world.step();
            let Some(i) = world.cells().index(id) else {
                eprintln!("{t:>7}   died");
                break;
            };
            if t % 400 == 0 {
                let slots = world.cells().slots(i).iter().filter(|o| o.is_present()).count();
                eprintln!(
                    "{:>7} {:>8} {:>7} {:>9} {:>7}  {}",
                    t,
                    world.cells().energy[i] / Q10_ONE,
                    world.cells().mass[i] / Q10_ONE,
                    mm_core::biology::nucleus_capacity(world.cells(), i),
                    slots,
                    loadout(&world, i),
                );
            }
        }
    }
    eprintln!("\na trailing … marks an organelle still under construction. `nucleus_capacity`");
    eprintln!("counts only finished ones, so a nucleus mid-build reads 0 and the inspector says");
    eprintln!("`no nucleus — cannot divide` about a cell that is in the middle of growing one.");
}

/// Every shipped genome can divide the moment it is seeded.
///
/// Not `#[ignore]`d, because this is an acceptance test rather than a probe: it guards a fault
/// that presents as a *strategy* failing rather than as a bug. `place_inhabitants` used to hand
/// every founder a nucleus of `param` 40 — 320 bytes — whatever it was seeding, and six of the
/// twenty-one genomes in `genomes/` are longer than that. `CellHost::bud` returns zero when the
/// genome will not fit, so those six could not reproduce at all until their own `#build` finished
/// upgrading the nucleus, and a body that is also funding a spike and two cilia may never afford
/// to. What it looks like from the microscope is an organism that hunts well and dies childless.
#[test]
fn a_seeded_founder_can_hold_its_own_genome() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("genomes/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "mm"))
        .collect();
    names.sort();
    assert!(names.len() >= 8, "genomes/ looks empty: {names:?}");

    for path in names {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
        let src = std::fs::read_to_string(&path).expect("readable");
        let bytes = mm_asm::assemble(&src)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"))
            .bytes;
        let mut world = World::new(soup(32, 32)).expect("world");
        assert_eq!(world.place_founders_at(&bytes, 1, Some((16, 16))), 1, "{name} did not land");
        let i = world.cells().iter().next().expect("a founder");
        let capacity = mm_core::biology::nucleus_capacity(world.cells(), i);
        assert!(
            capacity >= bytes.len(),
            "{name} is {} bytes and was seeded with a nucleus holding {capacity}: \
             `bud` refuses that, so this founder can never divide",
            bytes.len()
        );
    }
}

/// And a genome that already fitted gets exactly the nucleus it always got.
///
/// The `max(40, …)` half of the sizing, which is what keeps every acceptance number taken on an
/// ancestor-seeded world where it was.
#[test]
fn a_short_genome_still_gets_the_nucleus_it_always_had() {
    let bytes = assemble("ancestor.mm");
    assert!(bytes.len() <= 320, "ancestor grew past the old fixed kit");
    let mut world = World::new(soup(32, 32)).expect("world");
    world.place_founders_at(&bytes, 1, Some((16, 16)));
    let i = world.cells().iter().next().expect("a founder");
    assert_eq!(
        world.cells().slots(i)[1].param,
        40,
        "the starting nucleus moved for a genome that already fitted"
    );
}
