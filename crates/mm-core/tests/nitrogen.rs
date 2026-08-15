//! The nitrogen cycle: two pools on the slide, and the three arrows between them.
//!
//! `docs/CHEMISTRY.md` §8 argues the shape and this file holds the engine to it. The short of it:
//!
//! * **Fixation** is the small, slow, brutally expensive input — inert dinitrogen into the
//!   bioavailable monomer, at an energy price, stopped by oxidant inside the cell.
//! * **Ammonification** is the thick arrow, and it costs nothing new: an organelle's `build_trace`
//!   is returned in full when the cell dies, so nitrogen in bodies cycles back by itself.
//! * **Denitrification** is the small leak the other way, gated on the water being anoxic.
//!
//! In a real ecosystem the internal loop runs one to two orders of magnitude larger than either of
//! the two external arrows, and it is that hierarchy — not any single rate — that makes succession
//! legible: early colonisers depend on fixers, a mature mat lives off its own recycled nitrogen.
//!
//! # Why both pools are on the slide
//!
//! Only energy crosses the wall of this world; matter is neither created nor destroyed, only
//! transferred and transformed. A reservoir that is not on the slide therefore cannot exist, and
//! the alternative — an organelle calling `Ledger::record_injected` against an off-plane
//! atmosphere every tick — is a tap. A closed system with a tap is a flow reactor. So the
//! atmosphere costs a chemical, and `chem::DINITROGEN` is it.

use mm_core::cell::{CellId, CellSeed};
use mm_core::chem::DINITROGEN;
use mm_core::fixed::{pos, q10};
use mm_core::organelle::{NITROGEN, PHOSPHORUS};
use mm_core::{LightRegime, Organelle, OrganelleType, Scenario, Seeding, World, Q10_ONE};

fn slide() -> Scenario {
    Scenario {
        name: "nitrogen".to_string(),
        seed: 5,
        width: 24,
        height: 24,
        light: LightRegime::Uniform {
            intensity: Q10_ONE,
        },
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

fn spawn(world: &mut World, x: i32, y: i32) -> usize {
    let genome = world
        .genomes()
        .intern(mm_asm::assemble("HALT\n").expect("assembles").bytes)
        .expect("interned");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(y),
        mass: q10(40),
        energy: q10(100_000),
        membrane: 48,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    world.cells_mut().index(id).expect("spawned")
}

/// **Total nitrogen never moves, whatever the cycle does with it.**
///
/// The load-bearing test of the whole design, and the reason the atmosphere is a chemical rather
/// than a ledger call. Fixation, a body being built out of it, and the body dying are three
/// transfers between four compartments — water, interior, organelle slot, and the inert pool —
/// and the sum across them is a constant. Not nearly a constant.
#[test]
fn the_two_pools_and_the_bodies_between_them_conserve_nitrogen_exactly() {
    let mut world = World::new(slide()).expect("world");
    let i = spawn(&mut world, 12, 12);
    {
        let cells = world.cells_mut();
        // A diazotroph with something to crack and something to build.
        cells.slots_mut(i)[2] = {
            let mut o = Organelle::finished(OrganelleType::Diazosome, 200);
            o.control[0] = Q10_ONE as i16;
            o
        };
        cells.interior_mut(i)[DINITROGEN] = q10(300);
        cells.interior_mut(i)[NITROGEN] = q10(50);
        cells.interior_mut(i)[PHOSPHORUS] = q10(50);
    }
    world.adopt_current_contents_as_baseline();

    let total = |w: &World| w.total_matter()[NITROGEN] + w.total_matter()[DINITROGEN];
    let before = total(&world);
    assert!(before > 0, "the slide holds no nitrogen at all");

    for tick in 0..200 {
        world.step();
        assert_eq!(
            total(&world),
            before,
            "nitrogen changed at tick {tick}: the two pools plus the bodies are a closed set and \
             the sum across them is the whole claim"
        );
        world
            .check_matter()
            .unwrap_or_else(|e| panic!("the books stopped balancing at tick {tick}: {e}"));
    }
}

/// Fixation moves nitrogen from the locked pool to the usable one, and not the other way.
///
/// Under ISA 10 and earlier this ran backwards — it *spent* nitrogen to make carbon, so nitrogen
/// never entered a body as nitrogen and the requirement it is supposed to impose could be
/// manufactured out of something else. A requirement that can be manufactured is a price.
#[test]
fn fixation_unlocks_rather_than_transmutes() {
    let mut world = World::new(slide()).expect("world");
    let i = spawn(&mut world, 12, 12);
    {
        let cells = world.cells_mut();
        let mut o = Organelle::finished(OrganelleType::Diazosome, 200);
        o.control[0] = Q10_ONE as i16;
        cells.slots_mut(i)[2] = o;
        cells.interior_mut(i)[DINITROGEN] = q10(300);
        cells.interior_mut(i)[NITROGEN] = 0;
    }
    let structural = world.biology().structural_chemical;
    let carbon_before = world.total_matter()[structural];
    world.run(5);

    let cells = world.cells();
    assert!(
        cells.interior(i)[NITROGEN] > 0,
        "nothing was unlocked; the usable pool is still empty"
    );
    assert!(
        cells.interior(i)[DINITROGEN] < q10(300),
        "the inert pool did not fall, so whatever appeared did not come from it"
    );
    assert_eq!(
        world.total_matter()[structural],
        carbon_before,
        "carbon moved during a fixation: this is the old transmutation shape, where nitrogen was \
         a second source of building material rather than a requirement of its own"
    );
}

/// The thick arrow: what a body is made of comes back when it dies.
///
/// This is ammonification and it costs no new mechanism — `build_trace` is returned in full at a
/// death, which is exactly the internal loop the flux hierarchy needs to be large. If this ever
/// stops holding, nitrogen becomes a one-way sink and every world runs down.
#[test]
fn a_death_returns_the_nitrogen_a_body_was_built_from() {
    let mut world = World::new(slide()).expect("world");
    let i = spawn(&mut world, 12, 12);
    {
        let cells = world.cells_mut();
        // A mitochondrion is costed in nitrogen; installing one directly puts that nitrogen in
        // the slot without going through a build.
        cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 200);
    }
    world.adopt_current_contents_as_baseline();
    let id = world.cells().id_at(i);
    let held = world.total_matter()[NITROGEN];
    let loose = world.substrate().chem_plane(NITROGEN).iter().map(|v| i64::from(*v)).sum::<i64>();

    world.kill_cell(id);

    let after_loose = world
        .substrate()
        .chem_plane(NITROGEN)
        .iter()
        .map(|v| i64::from(*v))
        .sum::<i64>();
    assert!(
        after_loose > loose,
        "a cell built from nitrogen died and returned none of it: {loose} then {after_loose}"
    );
    assert_eq!(
        world.total_matter()[NITROGEN],
        held,
        "nitrogen was created or destroyed by a death"
    );
    world.check_matter().expect("the books balance across a death");
}

/// The small leak: bioavailable nitrogen reverts where the water has no oxidant in it.
///
/// Off by default — the leak is the term to tune last, once the internal loop's speed is known —
/// so this switches it on to hold the mechanism to working. What it must show is the *gating*:
/// anoxic water leaks and oxygenated water does not, which is what makes this the counterpart of
/// fixation's inhibition rather than a second knob doing the same job.
#[test]
fn denitrification_needs_anoxic_water() {
    let reverted = |oxidant: i32| {
        let mut world = World::new(slide()).expect("world");
        {
            let mut biology = world.biology().clone();
            biology.metabolism.rates.denitrification_rate = Q10_ONE / 8;
            world.set_biology(biology);
        }
        for x in 0..24 {
            for y in 0..24 {
                world.substrate_mut().set_chem(NITROGEN, x, y, q10(100));
                world.substrate_mut().set_chem(14, x, y, oxidant);
            }
        }
        world.adopt_current_contents_as_baseline();
        let before: i64 = world
            .substrate()
            .chem_plane(DINITROGEN)
            .iter()
            .map(|v| i64::from(*v))
            .sum();
        world.run(20);
        let after: i64 = world
            .substrate()
            .chem_plane(DINITROGEN)
            .iter()
            .map(|v| i64::from(*v))
            .sum();
        world.check_matter().expect("the books balance across a reversion");
        after - before
    };

    let anoxic = reverted(0);
    let aerated = reverted(q10(40));
    assert!(anoxic > 0, "nothing reverted in water with no oxidant at all");
    assert_eq!(
        aerated, 0,
        "nitrogen reverted in oxygenated water: denitrification is anaerobic, and the whole \
         point of gating it that way is that an anoxic pocket is both the best place to fix and \
         the place the pool leaks from — a balance point rather than a ratchet"
    );
}

/// It ships off, and that is a decision rather than an oversight.
#[test]
fn the_leak_is_off_until_somebody_has_measured_the_loop() {
    let rates = mm_core::MetabolicRates::default();
    assert_eq!(
        rates.denitrification_rate, 0,
        "denitrification is on by default; the leak is the term to tune last, once the internal \
         loop's speed is known, and that measurement has not been taken"
    );
    assert!(
        rates.fixation_energy > 0,
        "fixation is free, which makes the diazosome a tap rather than a port"
    );
}
