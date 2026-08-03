//! Matter across the boundary of the slide, and whether the books still balance (SPEC §17.7, "Matter has a boundary too").
//!
//! Every other scenario is closed to matter and open only to light, which is one habitat out of
//! several worth having: a deep-sea vent is a slide in the dark with inorganic matter welling up
//! through it, and marine snow is a slide in the dark with organic matter falling through.
//!
//! The invariants are the whole difficulty. I4 says matter is conserved *exactly*, and a source
//! is a mechanism that creates it — so the ledger has to be told, and told the amount that
//! actually landed rather than the amount intended. I5 says `energy_in == energy_out +
//! Δstored`, and matter that something can metabolise carries energy with it, so an inflow of
//! sulphide is an inflow of energy whether or not anyone remembered to say so.
//!
//! These are the tests that would have caught getting either wrong.

use mm_core::chem::CHEM_COUNT;
use mm_core::ecology::DETRITUS;
use mm_core::fixed::{q10, Q10_ONE};
use mm_core::light::CurrentField;
use mm_core::{Flux, LightRegime, Scenario, Seeding, World};

/// A bare slide with nothing alive on it, so the only thing moving matter is the flux.
fn empty(flux: Vec<Flux>) -> Scenario {
    Scenario {
        name: "flux".to_string(),
        seed: 0x11F0,
        width: 32,
        height: 32,
        light: LightRegime::Uniform { intensity: 0 },
        current: CurrentField::Still,
        jitter: 0,
        flux,
        ..Scenario::default()
    }
}

fn total(world: &World, c: usize) -> i64 {
    world.total_matter()[c]
}

#[test]
fn a_source_puts_matter_on_the_slide_and_the_ledger_knows_about_all_of_it() {
    // I4 with a mechanism that creates matter. The ledger's claim and an independent
    // recomputation of the world's contents must agree exactly, not nearly.
    let mut world = World::new(empty(vec![Flux::Source {
        chemical: DETRITUS,
        x: 0,
        y: 0,
        width: 4,
        height: 32,
        per_tick: q10(10),
    }]))
    .expect("world");
    assert_eq!(total(&world, DETRITUS), 0, "it starts with none");

    for tick in 0..400 {
        world.step();
        if tick % 50 == 0 {
            world
                .check_matter()
                .unwrap_or_else(|e| panic!("matter drifted at tick {tick}: {e}"));
        }
    }
    let held = total(&world, DETRITUS);
    assert!(held > 0, "the source put nothing on the slide");
    assert_eq!(
        world.ledger().injected()[DETRITUS],
        held,
        "the world holds a different amount than the ledger says arrived"
    );
    world.check_matter().expect("matter drifted over the run");
}

#[test]
fn a_drain_takes_it_away_again_and_says_so() {
    let mut world = World::new(Scenario {
        seeding: vec![Seeding::Uniform {
            chemical: DETRITUS,
            per_square: q10(100),
        }],
        ..empty(vec![Flux::Drain {
            chemical: DETRITUS,
            x: 0,
            y: 0,
            width: 32,
            height: 32,
            rate: Q10_ONE / 16,
        }])
    })
    .expect("world");
    let before = total(&world, DETRITUS);
    world.run(200);
    let after = total(&world, DETRITUS);

    assert!(
        after < before,
        "the drain took nothing: {before} -> {after}"
    );
    assert_eq!(
        world.ledger().drained()[DETRITUS],
        before - after,
        "what left the slide and what the ledger says left disagree"
    );
    world.check_matter().expect("matter drifted over the run");
}

#[test]
fn a_drain_cannot_take_what_is_not_there() {
    // A fraction rather than an amount, so this can never go negative — and a slide it has
    // already emptied stays empty rather than going into debt.
    let mut world = World::new(empty(vec![Flux::Drain {
        chemical: DETRITUS,
        x: 0,
        y: 0,
        width: 32,
        height: 32,
        rate: Q10_ONE,
    }]))
    .expect("world");
    world.run(50);
    assert_eq!(total(&world, DETRITUS), 0);
    assert_eq!(world.ledger().drained()[DETRITUS], 0, "it drained nothing");
    world.check_matter().expect("matter drifted");
}

#[test]
fn matter_that_can_be_metabolised_brings_its_energy_with_it() {
    // I5, and the reason `Ledger::import` exists. Sulphide is a pathway substrate, so a slide
    // that gains sulphide gains stored energy — and `energy_in` has to gain the same, or the
    // identity fails on the very next tick.
    const SULPHIDE: usize = 10;
    let mut world = World::new(empty(vec![Flux::Source {
        chemical: SULPHIDE,
        x: 4,
        y: 4,
        width: 8,
        height: 8,
        per_tick: q10(20),
    }]))
    .expect("world");
    let before = world.ledger().energy_in();
    for tick in 0..300 {
        world.step();
        world
            .ledger()
            .check_energy()
            .unwrap_or_else(|e| panic!("energy broke at tick {tick}: {e}"));
    }
    assert!(
        world.ledger().energy_in() > before,
        "sulphide arrived but no energy did"
    );
    assert_eq!(
        world.ledger().energy_imported(),
        world.ledger().energy_in() - before,
        "the energy that came in is not the energy attributed to the influx"
    );
}

#[test]
fn an_outflow_of_food_is_not_the_world_getting_warmer() {
    // Energy leaving with matter is not dissipation, and the two are counted apart. A budget
    // panel that showed food washing off the slide as heat would describe a different world.
    const SULPHIDE: usize = 10;
    let mut world = World::new(Scenario {
        seeding: vec![Seeding::Uniform {
            chemical: SULPHIDE,
            per_square: q10(200),
        }],
        ..empty(vec![Flux::Drain {
            chemical: SULPHIDE,
            x: 0,
            y: 0,
            width: 32,
            height: 32,
            rate: Q10_ONE / 8,
        }])
    })
    .expect("world");
    world.run(100);

    let exported = world.ledger().energy_exported();
    assert!(exported > 0, "sulphide left but its energy did not");
    assert_eq!(
        world.ledger().energy_out(),
        exported,
        "nothing here dissipates, so every unit out should be exported rather than heat"
    );
    world.ledger().check_energy().expect("I5 broke");
}

#[test]
fn a_chemical_nothing_eats_carries_no_energy() {
    // Detritus is not a pathway substrate — it has to be taken apart before it is worth
    // anything — so an inflow of it is matter and nothing else. The counterpart of the sulphide
    // case, and it is the one that catches weighing *every* chemical instead of the right ones.
    let mut world = World::new(empty(vec![Flux::Source {
        chemical: DETRITUS,
        x: 0,
        y: 0,
        width: 32,
        height: 32,
        per_tick: q10(50),
    }]))
    .expect("world");
    let before = world.ledger().energy_in();
    world.run(200);
    assert!(total(&world, DETRITUS) > 0, "no detritus arrived");
    assert_eq!(
        world.ledger().energy_in(),
        before,
        "detritus brought energy with it, and nothing metabolises detritus"
    );
    assert_eq!(world.ledger().energy_imported(), 0);
}

#[test]
fn a_source_and_a_drain_together_settle_instead_of_filling_up() {
    // The reason a source needs a drain. Matter that arrives and never leaves counts up to the
    // quantity cap, and a population under a ceiling set by arithmetic is not a population
    // under a carrying capacity — so what makes a flow-through slide interesting is that it
    // has a level, and the level is where inflow and outflow meet.
    let mut world = World::new(empty(vec![
        Flux::Source {
            chemical: DETRITUS,
            x: 0,
            y: 0,
            width: 2,
            height: 32,
            per_tick: q10(40),
        },
        Flux::Drain {
            chemical: DETRITUS,
            x: 0,
            y: 0,
            width: 32,
            height: 32,
            rate: Q10_ONE / 64,
        },
    ]))
    .expect("world");

    world.run(400);
    let early = total(&world, DETRITUS);
    world.run(400);
    let mid = total(&world, DETRITUS);
    world.run(400);
    let late = total(&world, DETRITUS);

    eprintln!("standing stock: {early} -> {mid} -> {late}");
    assert!(mid > early / 2, "it emptied rather than settling");
    // Levelling off: the second interval's change must be a small fraction of the first's.
    let first = (mid - early).abs();
    let second = (late - mid).abs();
    assert!(
        second * 4 < first.max(1),
        "still climbing rather than settling: {early} -> {mid} -> {late}"
    );
    world.check_matter().expect("matter drifted");
}

#[test]
fn a_source_aimed_off_the_slide_records_what_it_managed() {
    // `Substrate::index` reduces modulo the grid, so a rectangle running off the right-hand
    // edge would reappear on the left and put an inlet at the outflow. Clamped instead — and
    // whatever is clamped away must not be recorded as having arrived.
    let mut world = World::new(empty(vec![Flux::Source {
        chemical: DETRITUS,
        x: 28,
        y: 28,
        width: 64,
        height: 64,
        per_tick: q10(10),
    }]))
    .expect("world");
    world.run(10);

    let held = total(&world, DETRITUS);
    assert_eq!(world.ledger().injected()[DETRITUS], held);
    // Four squares by four, ten ticks, not sixty-four by sixty-four.
    assert_eq!(held, i64::from(q10(10)) * 16 * 10);
    world.check_matter().expect("matter drifted");
}

#[test]
fn a_slide_with_flux_on_it_survives_being_put_away_and_taken_out_again() {
    // Hard rule 7. The counters are cumulative, so a resumed run that forgot them would report
    // a world that had never been fed and would carry that error for the rest of its life.
    const SULPHIDE: usize = 10;
    let mut world = World::new(empty(vec![
        Flux::Source {
            chemical: SULPHIDE,
            x: 0,
            y: 0,
            width: 4,
            height: 32,
            per_tick: q10(15),
        },
        Flux::Drain {
            chemical: SULPHIDE,
            x: 28,
            y: 0,
            width: 4,
            height: 32,
            rate: Q10_ONE / 4,
        },
    ]))
    .expect("world");
    world.run(300);

    let bytes = mm_core::snapshot::Snapshot::write(&world).expect("snapshot");
    let mut back = mm_core::snapshot::Snapshot::read(&bytes).expect("restore");

    assert_eq!(back.ledger().injected(), world.ledger().injected());
    assert_eq!(back.ledger().drained(), world.ledger().drained());
    assert_eq!(
        back.ledger().energy_imported(),
        world.ledger().energy_imported()
    );
    assert_eq!(
        back.ledger().energy_exported(),
        world.ledger().energy_exported()
    );

    // And it keeps going the same way, which is the part a field-by-field comparison misses.
    world.run(200);
    back.run(200);
    assert_eq!(back.state_hash(), world.state_hash(), "resumption diverged");
}

#[test]
fn every_chemical_can_flow_in_and_out_without_losing_a_unit() {
    // Not only the ones with an interesting story. Sixteen sources and sixteen drains at once,
    // over a slide that also has a current on it to move what arrives.
    let flux: Vec<Flux> = (0..CHEM_COUNT)
        .flat_map(|c| {
            [
                Flux::Source {
                    chemical: c,
                    x: 0,
                    y: 0,
                    width: 3,
                    height: 32,
                    per_tick: q10(5 + c as i32),
                },
                Flux::Drain {
                    chemical: c,
                    x: 26,
                    y: 0,
                    width: 6,
                    height: 32,
                    rate: Q10_ONE / (8 + c as i32),
                },
            ]
        })
        .collect();
    let mut world = World::new(Scenario {
        current: CurrentField::Uniform {
            vx: Q10_ONE / 8,
            vy: 0,
        },
        ..empty(flux)
    })
    .expect("world");

    for tick in 0..500 {
        world.step();
        if tick % 100 == 0 {
            world
                .check_matter()
                .unwrap_or_else(|e| panic!("matter drifted at tick {tick}: {e}"));
            world
                .ledger()
                .check_energy()
                .unwrap_or_else(|e| panic!("energy drifted at tick {tick}: {e}"));
        }
    }
    world.check_matter().expect("matter drifted");
    world.ledger().check_energy().expect("energy drifted");
}
