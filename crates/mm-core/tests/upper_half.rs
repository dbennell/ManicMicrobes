//! The catalogue's upper half: `n + 16` is the same job done a different way.
//!
//! Widening from sixteen types to thirty-two is ISA 7, and the layout is the reason it is safe.
//! A copy error is a single bit flip, so bit 4 of a type operand is one mutation from every
//! genome in the library. Laid out at random, flipping it would turn a working organelle into a
//! no-op — one flip in eight, on every type byte. Laid out in pairs it turns a cilium into a
//! flagellum, and evolution can hill-climb between stirring and swimming rather than having to
//! find it. `docs/FEEDING.md` §6 is the argument; this file is the guard on it.
//!
//! Five pairs are filled. The rest are `Reserved`, which up here means "this organ has no variant
//! yet" rather than "this number is spare".

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::{LightRegime, Organelle, OrganelleType, Scenario, Seeding, World, Q10_ONE};

fn slide(light: i32) -> Scenario {
    Scenario {
        name: "upper half".to_string(),
        seed: 3,
        width: 32,
        height: 32,
        light: LightRegime::Uniform { intensity: light },
        seeding: vec![
            Seeding::Uniform {
                chemical: 11,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 14,
                per_square: q10(400),
            },
        ],
        ..Scenario::default()
    }
}

fn spawn(world: &mut World, x: i32, y: i32, mass: i32) -> usize {
    let genome = world
        .genomes()
        .intern(mm_asm::assemble("HALT\n").expect("assembles").bytes)
        .expect("interned");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(y),
        mass,
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

fn put(world: &mut World, i: usize, slot: usize, kind: OrganelleType, param: u8, c0: i16) {
    let mut o = Organelle::finished(kind, param);
    o.control[0] = c0;
    world.cells_mut().slots_mut(i)[slot] = o;
}

/// Every pair is exactly sixteen apart, and reachable in one bit flip.
#[test]
fn the_pairs_are_one_bit_apart() {
    for (base, variant) in [
        (OrganelleType::Mitochondrion, OrganelleType::Diazosome),
        (OrganelleType::Chloroplast, OrganelleType::Chemosynth),
        (OrganelleType::Vacuole, OrganelleType::LipidDroplet),
        (OrganelleType::Cilium, OrganelleType::Flagellum),
        (OrganelleType::Spike, OrganelleType::Exoenzyme),
    ] {
        let (a, b) = (base.number(), variant.number());
        assert_eq!(b - a, 16, "{base:?} and {variant:?} are not a pair");
        assert_eq!(
            (a ^ b).count_ones(),
            1,
            "{base:?} and {variant:?} are more than one mutation apart, which is the entire \
             reason the upper half is laid out this way"
        );
    }
}

/// A type operand wraps into thirty-two, not sixteen. This is what the ISA bump is *about*.
#[test]
fn an_operand_reaches_the_upper_half() {
    assert_eq!(
        OrganelleType::from_operand(19),
        OrganelleType::Chemosynth,
        "19 still reduces to the chloroplast, so the catalogue did not widen"
    );
    assert_eq!(OrganelleType::from_operand(22), OrganelleType::Flagellum);
    // And it still wraps, so no byte is illegal (hard rule 3).
    assert_eq!(OrganelleType::from_operand(32), OrganelleType::Membrane);
    // -1 wraps to 31, which is the calcite test since ISA 13 — it was `Reserved31` and the
    // assertion is about the *wrap*, not about what happens to live at the top of the catalogue.
    assert_eq!(OrganelleType::from_operand(-1), OrganelleType::CalciteShell);
}

/// A flagellum drives the body harder than a cilium and the water less.
///
/// That split *is* the difference between them — `FEEDING.md` §7's "a cilium stirs and a
/// flagellum propels" — and it is a number, not a mechanism.
#[test]
fn a_flagellum_propels_where_a_cilium_stirs() {
    let travelled = |kind: OrganelleType| {
        let mut world = World::new(slide(Q10_ONE)).expect("world");
        let i = spawn(&mut world, 16, 16, q10(40));
        put(&mut world, i, 2, kind, 200, Q10_ONE as i16);
        let start = world.cells().x[i];
        world.run(60);
        let cells = world.cells();
        (cells.x[i] - start).abs()
    };
    let by_cilium = travelled(OrganelleType::Cilium);
    let by_flagellum = travelled(OrganelleType::Flagellum);
    assert!(
        by_flagellum > by_cilium,
        "a flagellum moved {by_flagellum} against a cilium's {by_cilium}; the whole point of the \
         pair is that one of them goes somewhere"
    );
}

/// A lipid droplet holds more per unit of size than the vacuole it pairs with.
#[test]
fn a_lipid_droplet_is_the_denser_store() {
    let capacity = |kind: OrganelleType| {
        let mut world = World::new(slide(Q10_ONE)).expect("world");
        let i = spawn(&mut world, 16, 16, q10(40));
        put(&mut world, i, 2, kind, 200, 0);
        mm_core::biology::interior_capacity(world.cells(), i)
    };
    assert!(
        capacity(OrganelleType::LipidDroplet) > capacity(OrganelleType::Vacuole),
        "the droplet held no more than the vacuole, so the pair is a duplicate"
    );
}

/// A chemosynthetic granule produces in the dark, where a chloroplast cannot.
#[test]
fn a_granule_makes_a_living_with_the_lights_off() {
    let banked = |kind: OrganelleType| {
        let mut world = World::new(slide(0)).expect("world");
        let i = spawn(&mut world, 16, 16, q10(40));
        // Throttle open: `control[0]` on a producer is its rate, and a granule turned off
        // banks nothing for reasons that have nothing to do with the light.
        put(&mut world, i, 2, kind, 200, Q10_ONE as i16);
        {
            let cells = world.cells_mut();
            cells.interior_mut(i)[11] = q10(300); // waste to fix
            cells.interior_mut(i)[10] = q10(200); // the reducer it runs on
        }
        world.run(20);
        i64::from(world.cells().interior(i)[8])
    };
    let dark_chloroplast = banked(OrganelleType::Chloroplast);
    let dark_granule = banked(OrganelleType::Chemosynth);
    assert_eq!(
        dark_chloroplast, 0,
        "a chloroplast banked substrate with no light at all"
    );
    assert!(
        dark_granule > 0,
        "the granule banked nothing in the dark, which is the one thing it is for"
    );
}

/// An exoenzyme takes mass off a neighbour and leaves it in the water, not in itself.
#[test]
fn an_exoenzyme_dissolves_into_the_square() {
    let mut world = World::new(slide(Q10_ONE)).expect("world");
    let digester = spawn(&mut world, 16, 16, q10(60));
    let victim = spawn(&mut world, 16, 16, q10(300));
    put(&mut world, digester, 2, OrganelleType::Exoenzyme, 200, Q10_ONE as i16);
    world.adopt_current_contents_as_baseline();
    let before_victim = world.cells().mass[victim];
    let before_digester = world.cells().interior(digester)[4];
    let before_total = world.total_matter();

    world.run(30);

    let cells = world.cells();
    assert!(
        cells.mass[victim] < before_victim,
        "the victim lost no mass; nothing was dissolved"
    );
    assert_eq!(
        cells.interior(digester)[4],
        before_digester,
        "the digester took the matter into itself — the leak into the water is the whole design, \
         and without it this is just a slower engulfment"
    );
    assert_eq!(
        world.total_matter(),
        before_total,
        "matter changed over a dissolving"
    );
    world.check_matter().expect("I4 broke over an exoenzyme");
}

/// A diazosome turns nitrogen into body, and oxidant stops it.
#[test]
fn a_diazosome_fixes_nitrogen_unless_there_is_oxidant() {
    // What it does now, and it used to do the opposite: this reads *dinitrogen* falling and
    // *nitrogen* rising, where before ISA 11 it read nitrogen falling and carbon rising. That was
    // a monomer transmutation — nitrogen as a second carbon source — and it meant nitrogen never
    // entered a body as nitrogen at all. Fixation costs energy to make nitrogen available; it
    // does not turn nitrogen into something else.
    // One tick, and the cell has to survive it.
    //
    // Ten ticks was the first version and it measured the wrong thing twice: a cell that cannot
    // afford the bond spends its last energy on it and **starves**, and a corpse's interior is
    // empty for reasons that have nothing to do with fixation. Both the zero-energy case and the
    // poor case therefore read the entire pool as "fixed". Over one tick nothing has time to die,
    // and the aliveness check is there so that it cannot come back.
    let run = |oxidant: i32, energy: i32| {
        let mut world = World::new(slide(Q10_ONE)).expect("world");
        let i = spawn(&mut world, 16, 16, q10(40));
        put(&mut world, i, 2, OrganelleType::Diazosome, 200, Q10_ONE as i16);
        {
            let cells = world.cells_mut();
            cells.interior_mut(i)[mm_core::chem::DINITROGEN] = q10(200);
            cells.interior_mut(i)[5] = 0;
            cells.interior_mut(i)[14] = oxidant;
            cells.energy[i] = energy;
        }
        let id = world.cells().id_at(i);
        let before = world.cells().interior(i)[mm_core::chem::DINITROGEN];
        world.run(1);
        let Some(i) = world.cells().index(id) else {
            panic!("the cell died inside one tick; this measures starvation, not fixation");
        };
        let after = world.cells().interior(i)[mm_core::chem::DINITROGEN];
        (before - after, world.cells().interior(i)[5])
    };
    let (cracked, made) = run(0, q10(100_000));
    assert!(cracked > 0, "no dinitrogen was cracked in clean water");
    assert_eq!(
        made, cracked,
        "the inert pool fell by {cracked} and the usable one rose by {made}: fixation is a \
         conversion between two chemicals on the slide, and it has to balance to the unit"
    );

    let (poisoned, _) = run(q10(40), q10(100_000));
    assert!(
        poisoned < cracked,
        "oxidant made no difference: {poisoned} against {cracked}, and the antagonism with the          mitochondrion is the whole reason this pairs with it"
    );

    // And it is paid for, which is the difference between a port and a tap.
    //
    // Asserted as a *bill* rather than as "a poor cell fixes less", because a cell poor enough to
    // be bounded by its energy is a cell that starves inside the tick, and then the interior
    // empties for reasons that have nothing to do with fixation. Two identical rich cells, one
    // with the organelle and one without: the difference in what they spend is the bond.
    let spent = |diazosome: bool| {
        let mut world = World::new(slide(Q10_ONE)).expect("world");
        let i = spawn(&mut world, 16, 16, q10(40));
        if diazosome {
            put(&mut world, i, 2, OrganelleType::Diazosome, 200, Q10_ONE as i16);
        }
        {
            let cells = world.cells_mut();
            cells.interior_mut(i)[mm_core::chem::DINITROGEN] = q10(200);
            cells.interior_mut(i)[5] = 0;
            cells.energy[i] = q10(100_000);
        }
        let before = world.cells().energy[i];
        world.run(1);
        before - world.cells().energy[i]
    };
    let with = spent(true);
    let without = spent(false);
    assert!(
        with > without,
        "a diazotroph spent {with} against {without} for the same body without the organelle: \
         cracking a triple bond is the dearest thing in the catalogue and this one is free, which \
         makes it a tap rather than a port"
    );
}

/// **A genome can actually build one.**
///
/// Every other test in this file installs its organelles with `slots_mut`, which asks what the
/// upper half *does* and never asks whether a cell can get there. That is the wrong way round for
/// this particular catalogue, because the entire argument for the `n + 16` layout is about the
/// path a *mutation* takes: bit 4 of a type operand is one copy error away, so `BUILD 6` and
/// `BUILD 22` are supposed to be one flip apart and to give a cilium and a flagellum.
///
/// So this is the claim `docs/FEEDING.md` §6 actually makes, tested through `BUILD`: the operand
/// reaches the type. Both halves are checked from one genome shape, because the fault this catches
/// is a wrap — and a wrap that squashes 22 to 6 makes the upper half unreachable while leaving
/// every lower-half test green.
#[test]
fn a_genome_can_build_the_upper_half() {
    for (operand, want) in [
        (6i32, OrganelleType::Cilium),
        (22, OrganelleType::Flagellum),
        (19, OrganelleType::Chemosynth),
        (28, OrganelleType::Exoenzyme),
        // Past the catalogue, so it wraps at thirty-two rather than at sixteen: 32 is the
        // membrane again and 54 is the flagellum, because 54 − 32 = 22. Out-of-range operands
        // are what mutation produces constantly, so where they land is not a curiosity — and
        // *which* modulus they wrap by is the whole of the bug this test was written for.
        (32, OrganelleType::Membrane),
        (54, OrganelleType::Flagellum),
    ] {
        let src = format!("IMM 100\nIMM {operand}\nIMM 5\nBUILD\nHALT\n");
        let mut world = World::new(slide(Q10_ONE)).expect("world");
        let genome = world
            .genomes()
            .intern(mm_asm::assemble(&src).expect("assembles").bytes)
            .expect("interned");
        let id = world.spawn_cell(CellSeed {
            x: pos(16),
            y: pos(16),
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
        let i = world.cells_mut().index(id).expect("spawned");
        // Plenty to build out of, so nothing here is about affording it — including the
        // recipe's ingredients, which since ISA 10 is most of the catalogue: an organelle costed
        // in nitrogen cannot be built by a cell with none, and this test is about the *operand*
        // reaching the type rather than about the shopping.
        world.cells_mut().interior_mut(i)[4] = q10(400);
        world.cells_mut().interior_mut(i)[5] = q10(400);
        world.cells_mut().interior_mut(i)[6] = q10(400);
        world.run(4);

        let got = world.cells().slots(i)[5].kind;
        assert_eq!(
            got, want,
            "`BUILD {operand}` made a {} where the catalogue says {}: the type operand is not \
             reaching the catalogue intact, and if it wraps at sixteen then no genome can ever \
             build anything in the upper half — which is the whole of ISA 7",
            got.name(),
            want.name()
        );
    }
}
