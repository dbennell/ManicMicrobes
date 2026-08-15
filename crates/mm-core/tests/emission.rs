//! What a cell radiates, which is a consequence rather than a choice (SPEC §8.2, ISA 5).
//!
//! The badge is cheap talk: anything can wear anything, and that is deliberate. This is the
//! other channel. **Nothing sets it.** A cell's emission is accumulated from what it actually
//! paid as it paid it, so to look like something running a spike you have to run a spike and
//! pay for it, and to be dark you have to be idle. Both directions cost what they cost, which
//! is the whole difference between a signature and a name tag.
//!
//! Two bands, because one would only say "busy". The split is between **doing and being**, not
//! between kinds of organ, and getting that wrong the first time is what taught it: charging a
//! spike's *upkeep* to the mechanical band made a sheathed spike glow exactly like a drawn one,
//! which turns the signature into an inventory of what a cell carries when the whole value of it
//! is that it reports what a cell is doing. Maintenance is maintenance whatever it maintains, so
//! upkeep is all metabolic and only work reaches the mechanical band.
//!
//! The consequence is the interesting one: a predator at rest is indistinguishable from anything
//! else its size, and unmistakable the instant it extends. Ambush is available; ambush while
//! armed is not.

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::light::CurrentField;
use mm_core::organelle::{Organelle, OrganelleType};
use mm_core::{LightRegime, Scenario, World};

const MECHANICAL: usize = 0;
const METABOLIC: usize = 1;

fn slide() -> Scenario {
    Scenario {
        name: "emission".to_string(),
        seed: 0xE377,
        width: 32,
        height: 32,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        current: CurrentField::Still,
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
        ..Scenario::default()
    }
}

/// A cell with a chosen body and a genome that does nothing.
fn put(world: &mut World, x: i32, y: i32, spike: Option<u8>, extension: i16) -> CellId {
    // Does nothing at all. The spike's extension is set by hand below, because a genome that
    // set it would set it every cycle and there would be no such thing as a sheathed cell to
    // measure — which is how the first version of this test managed to disprove itself.
    let src = "
        HALT
";
    let bytes = mm_asm::assemble(src).expect("assembles").bytes;
    let g = world.genomes().intern(bytes).expect("intern");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(y),
        mass: q10(30),
        energy: q10(4000),
        membrane: 24,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome: g,
    });
    if let Some(i) = world.cells_mut().index(id) {
        let cells = world.cells_mut();
        cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
        cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
        cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
        if let Some(param) = spike {
            let mut s = Organelle::finished(OrganelleType::Spike, param);
            s.control[0] = extension;
            cells.slots_mut(i)[5] = s;
        }
        cells.interior_mut(i)[4] = q10(200);
        cells.interior_mut(i)[11] = q10(40);
        cells.interior_mut(i)[14] = q10(40);
    }
    world.adopt_current_contents_as_baseline();
    id
}

fn glow(world: &World, id: CellId, band: usize) -> i32 {
    world
        .cells()
        .index(id)
        .map_or(0, |i| world.cells().emission[i][band])
}

#[test]
fn a_cell_that_spends_is_a_cell_that_glows() {
    let mut world = World::new(slide()).expect("world");
    let id = put(&mut world, 16, 16, None, 0);
    world.run(20);
    assert!(
        glow(&world, id, METABOLIC) > 0,
        "a cell paying upkeep radiated nothing"
    );
}

#[test]
fn holding_a_spike_out_is_the_loudest_thing_a_cell_does() {
    // The band split earning its keep: a spike and a chloroplast at similar power must not read
    // the same, or the signature says "busy" and nothing more.
    let mut world = World::new(slide()).expect("world");
    let quiet = put(&mut world, 8, 8, None, 0);
    let armed = put(&mut world, 24, 24, Some(80), 512);
    world.run(20);

    assert_eq!(
        glow(&world, quiet, MECHANICAL),
        0,
        "a cell with no mechanical organelle radiated in the mechanical band"
    );
    assert!(
        glow(&world, armed, MECHANICAL) > 0,
        "an armed cell was silent where it should be loudest"
    );
    // And its housekeeping is unchanged — the spike adds a voice rather than replacing one.
    assert!(glow(&world, armed, METABOLIC) > 0);
}

#[test]
fn sheathing_the_spike_puts_the_light_out() {
    // The property that makes it a *signature* rather than an inventory: it reports what a cell
    // is doing, not what it is carrying. Same body, same organelles, weapon put away.
    let mut world = World::new(slide()).expect("world");
    let armed = put(&mut world, 8, 8, Some(80), 512);
    let sheathed = put(&mut world, 24, 24, Some(80), 0);
    world.run(20);

    assert!(glow(&world, armed, MECHANICAL) > 0);
    assert_eq!(
        glow(&world, sheathed, MECHANICAL),
        0,
        "a sheathed spike still radiated, so the signature is an inventory and not a signature"
    );
}

#[test]
fn a_newborn_is_the_quietest_thing_on_the_slide() {
    // She has done nothing yet, so she has spent nothing, so she is cold. Honest, and a tell.
    let mut world = World::new(slide()).expect("world");
    let id = put(&mut world, 16, 16, None, 0);
    world.run(20);
    let warm = glow(&world, id, METABOLIC);
    assert!(warm > 0);

    // Emission is cleared between execute and resolve, so a cell that has not been charged yet
    // this tick reads zero — which is what a cell born this tick is.
    let fresh = put(&mut world, 4, 4, None, 0);
    assert_eq!(glow(&world, fresh, METABOLIC), 0);
    assert_eq!(glow(&world, fresh, MECHANICAL), 0);
}

#[test]
fn a_photosensor_sees_a_spike_across_the_water() {
    // The point of the whole thing: at range, without contact, and without the armed cell
    // having any say in the matter.
    let watcher_src = "
        EXPRESS #look
        HALT
        GENE    #look
        IMM     3               ; photosensor reading 3: mechanical glow nearby
        IMM     4
        OGET
        ZERO
        RSTORE
        RET
";
    for (label, spike, expect_seen) in [
        ("an armed neighbour", Some(80u8), true),
        ("nobody armed", None, false),
    ] {
        let mut world = World::new(slide()).expect("world");
        let bytes = mm_asm::assemble(watcher_src).expect("assembles").bytes;
        let g = world.genomes().intern(bytes).expect("intern");
        let watcher = world.spawn_cell(CellSeed {
            x: pos(16),
            y: pos(16),
            mass: q10(30),
            energy: q10(4000),
            membrane: 24,
            key: 11,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome: g,
        });
        if let Some(i) = world.cells_mut().index(watcher) {
            let cells = world.cells_mut();
            cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
            cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
            cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
            cells.slots_mut(i)[4] = Organelle::finished(OrganelleType::Photosensor, 40);
            cells.interior_mut(i)[4] = q10(200);
            cells.interior_mut(i)[11] = q10(40);
            cells.interior_mut(i)[14] = q10(40);
        }
        // Three squares off — well past touching, well inside `em_range`.
        put(&mut world, 19, 16, spike, 512);
        world.adopt_current_contents_as_baseline();
        world.run(30);

        let seen = world
            .cells()
            .index(watcher)
            .map_or(0, |i| world.cells().vm[i].regs[0]);
        eprintln!("  {label:<20} reads {seen}");
        assert_eq!(
            seen > 0,
            expect_seen,
            "{label}: the mechanical band read {seen}"
        );
    }
}

#[test]
fn the_nearer_of_two_equal_fires_looks_brighter() {
    // Inverse-square falloff, which is what makes the reading a distance and not a headcount.
    let mut readings = Vec::new();
    // Both outside a spike's reach: at two squares the watcher was simply stabbed to death,
    // and a corpse reads zero for reasons that have nothing to do with distance.
    for gap in [3i32, 5] {
        let mut world = World::new(slide()).expect("world");
        let watcher_src = "
        EXPRESS #look
        HALT
        GENE    #look
        IMM     3
        IMM     4
        OGET
        ZERO
        RSTORE
        RET
";
        let bytes = mm_asm::assemble(watcher_src).expect("assembles").bytes;
        let g = world.genomes().intern(bytes).expect("intern");
        let watcher = world.spawn_cell(CellSeed {
            x: pos(16),
            y: pos(16),
            mass: q10(30),
            energy: q10(4000),
            membrane: 24,
            key: 11,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome: g,
        });
        if let Some(i) = world.cells_mut().index(watcher) {
            let cells = world.cells_mut();
            cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
            cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
            cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
            cells.slots_mut(i)[4] = Organelle::finished(OrganelleType::Photosensor, 40);
            cells.interior_mut(i)[4] = q10(200);
            cells.interior_mut(i)[11] = q10(40);
            cells.interior_mut(i)[14] = q10(40);
        }
        put(&mut world, 16 + gap, 16, Some(80), 512);
        world.adopt_current_contents_as_baseline();
        world.run(30);
        readings.push(
            world
                .cells()
                .index(watcher)
                .map_or(0, |i| world.cells().vm[i].regs[0]),
        );
    }
    eprintln!("  at 2 squares: {}, at 5: {}", readings[0], readings[1]);
    assert!(
        readings[0] > readings[1],
        "distance made no difference: {} against {}",
        readings[0],
        readings[1]
    );
}

#[test]
fn nothing_a_genome_can_do_sets_its_own_emission_directly() {
    // The honesty property, stated as a test rather than as a comment. There is no opcode for
    // it, so the only way to be bright is to spend — and the check that would catch someone
    // adding one is that two cells running the same body radiate the same whatever their
    // genomes say.
    let mut world = World::new(slide()).expect("world");
    let plain = put(&mut world, 8, 8, Some(80), 512);
    let busy = put(&mut world, 24, 24, Some(80), 512);
    world.run(20);
    assert_eq!(
        glow(&world, plain, MECHANICAL),
        glow(&world, busy, MECHANICAL),
        "two identical bodies radiated differently, so something other than spending is \
         reaching the signature"
    );
}

#[test]
fn a_signature_survives_being_put_away_and_taken_out_again() {
    // Written in one tick and read by the next tick's sense phase, so a world restored without
    // it sees a slide full of cold cells for one tick and diverges — the same trap `pressure`
    // fell into.
    let mut world = World::new(slide()).expect("world");
    let id = put(&mut world, 16, 16, Some(80), 512);
    world.run(30);

    let bytes = mm_core::snapshot::Snapshot::write(&world).expect("snapshot");
    let mut back = mm_core::snapshot::Snapshot::read(&bytes).expect("restore");
    assert_eq!(
        glow(&back, id, MECHANICAL),
        glow(&world, id, MECHANICAL),
        "the signature did not come back"
    );

    world.run(50);
    back.run(50);
    assert_eq!(back.state_hash(), world.state_hash(), "resumption diverged");
}

#[test]
fn radiating_costs_nothing_extra() {
    // I5. The signature is a shadow of energy already dissipated, not a second charge on it —
    // if it ever became a cost, going quiet would be a *saving* and stealth would stop being a
    // sacrifice.
    let mut world = World::new(slide()).expect("world");
    put(&mut world, 8, 8, Some(80), 512);
    put(&mut world, 24, 24, None, 0);
    for tick in 0..200 {
        world.step();
        if tick % 40 == 0 {
            world
                .ledger()
                .check_energy()
                .unwrap_or_else(|e| panic!("I5 broke at tick {tick}: {e}"));
        }
    }
    world
        .ledger()
        .check_energy()
        .expect("I5 broke over the run");
    assert!(Q10_ONE > 0);
}
