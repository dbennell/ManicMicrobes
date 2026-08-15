//! Two things the engine computed and no cell could perceive.
//!
//! Both are the same shape, and it is the shape the whole catalogue audit keeps turning up: a
//! cost that is applied, driving real consequences, with nothing able to read it. A pressure a
//! cell suffers and cannot sense is weather; one it can sense is a reason to do something.

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::{LightRegime, Scenario, Seeding, World, Q10_ONE};

/// The membrane index that reads crowding: after the five scalars, the sixteen chemicals and the
/// badge. Appended, so nothing before it renumbers.
const CROWDING: i16 = 22;

fn slide() -> Scenario {
    Scenario {
        name: "senses".to_string(),
        seed: 9,
        width: 24,
        height: 24,
        light: LightRegime::Uniform {
            intensity: Q10_ONE,
        },
        seeding: vec![Seeding::Uniform {
            chemical: 11,
            per_square: q10(200),
        }],
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
        mass: q10(60),
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

fn read(world: &World, i: usize, idx: i16) -> i16 {
    mm_core::biology::read_organelle(
        world.cells(),
        world.substrate(),
        world.neighbours(),
        i,
        0,
        idx,
        world.biology().ecology.spike_damage,
        world.biology().ecology.em_range,
        world.biology().metabolism.catalogue.metabolism,
        world.crowding().get(i).copied().unwrap_or(0),
    )
}

/// A cell in a press reads a crowd; a cell on its own reads none.
///
/// SPEC §17.8 says being buried is the best place to be. Until this reading existed, no cell on
/// the slide could tell whether it was buried — `resolve_collisions` computed the number every
/// tick, `split_pressure` refused divisions on it and `crowding_damage` charged for it, and it
/// was invisible to every sensor in the catalogue.
#[test]
fn a_cell_can_feel_a_crowd() {
    let mut world = World::new(slide()).expect("world");
    // A press: eight neighbours stacked on one square, and a hermit well clear of them.
    let pressed = spawn(&mut world, 8, 8);
    for _ in 0..8 {
        spawn(&mut world, 8, 8);
    }
    let hermit = spawn(&mut world, 20, 20);
    world.run(4);

    let crowded = read(&world, pressed, CROWDING);
    let alone = read(&world, hermit, CROWDING);
    assert!(
        crowded > 0,
        "a cell in a press of nine read no crowding at all"
    );
    assert_eq!(alone, 0, "a cell on its own read a crowd of {alone}");
}

/// And appending the reading renumbered nothing.
#[test]
fn the_older_membrane_readings_still_mean_what_they_did() {
    use mm_core::MembraneReading;
    assert_eq!(MembraneReading::decode(0), MembraneReading::Mass);
    assert_eq!(MembraneReading::decode(1), MembraneReading::Energy);
    assert_eq!(MembraneReading::decode(2), MembraneReading::Age);
    assert_eq!(MembraneReading::decode(3), MembraneReading::Radius);
    assert_eq!(MembraneReading::decode(4), MembraneReading::Damage);
    assert_eq!(MembraneReading::decode(5), MembraneReading::Chemical);
    assert_eq!(MembraneReading::decode(20), MembraneReading::Chemical);
    assert_eq!(MembraneReading::decode(21), MembraneReading::Badge);
    assert_eq!(MembraneReading::decode(22), MembraneReading::Crowding);
    // And it still wraps, so no operand is illegal (hard rule 3).
    assert_eq!(MembraneReading::decode(23), MembraneReading::Mass);
}

/// A wounded cell leaves a trail, and a healthy one does not.
///
/// `docs/FEEDING.md` §3: "Damage is private… A wounded cell looks exactly like a healthy one to
/// every sensor in the catalogue. So there is no blood in the water, and histophagy has nothing
/// to arrive towards." Now there is, and it is found with the chemosensor that already exists.
#[test]
fn a_wounded_cell_leaves_a_trail() {
    let mut world = World::new(slide()).expect("world");
    // Switched on for this test. It ships at zero — see `EcologyConfig::bleed_rate` for the
    // selection result that decided that — so a test of the mechanism has to ask for it.
    let mut biology = world.biology().clone();
    biology.ecology.bleed_rate = Q10_ONE / 256;
    biology.ecology.bleed_threshold = 0;
    world.set_biology(biology);
    let victim = spawn(&mut world, 8, 8);
    {
        let cells = world.cells_mut();
        // Something worth leaking, so the trail is visible against an empty square.
        cells.interior_mut(victim)[4] = q10(300);
        // The wound set directly rather than dealt by a spike. A spike at full extension kills
        // in under ten ticks, and a death converts mass to carrion — which is a real conversion
        // and not the one this test is about. What is being checked is that *carrying* a wound
        // leaks, whatever made it.
        cells.damage[victim] = q10(20);
    }
    world.adopt_current_contents_as_baseline();
    let before = world.total_matter();

    world.run(8);

    assert!(
        world.cells().occupied(victim),
        "the victim died; this test is about bleeding, not about dying"
    );
    assert!(
        world.cells().interior(victim)[4] < q10(300),
        "a wounded cell held on to everything; nothing bled"
    );
    assert!(
        world.substrate().chem_at(4, 8, 8) > 0,
        "the leak went nowhere — the whole point is that it is in the *water*, where a \
         chemosensor can find it"
    );
    assert_eq!(
        world.total_matter(),
        before,
        "matter changed over a bleeding"
    );
    world.check_matter().expect("I4 broke over blood in the water");
}

/// An unwounded cell keeps what it holds.
#[test]
fn an_unwounded_cell_does_not_bleed() {
    let mut world = World::new(slide()).expect("world");
    let i = spawn(&mut world, 8, 8);
    world.cells_mut().interior_mut(i)[4] = q10(300);
    let before = world.cells().interior(i)[4];
    world.run(20);
    assert_eq!(
        world.cells().interior(i)[4],
        before,
        "a healthy cell leaked; bleeding is supposed to be a consequence of damage"
    );
}

/// A leaky cell equalises with its square, in both directions, and loses nothing on the way.
///
/// Passive transport is the membrane's own `control[0]` — SPEC §8 specifies it, M2 lists it as a
/// deliverable, and until now nothing read it: a membrane was a perfect barrier, which is the one
/// thing a membrane is not.
///
/// It ships at rate zero because every archetype was written against that perfect barrier and
/// none of them closes its membrane. Whether the hand-written cells survive a leaky world is a
/// question for the balance panel, not a default to change in passing.
#[test]
fn a_leaky_membrane_runs_both_ways() {
    const SUGAR: usize = 8;
    let mut world = World::new(slide()).expect("world");
    let mut biology = world.biology().clone();
    biology.ecology.permeability_rate = Q10_ONE / 4;
    world.set_biology(biology);

    let rich = spawn(&mut world, 6, 6);
    let poor = spawn(&mut world, 18, 18);
    world.cells_mut().interior_mut(rich)[SUGAR] = q10(300);
    // Something for the poor cell to take up, in its square rather than in it.
    world.substrate_mut().set_chem(SUGAR, 18, 18, q10(300));
    world.adopt_current_contents_as_baseline();
    let before = world.total_matter();

    world.run(6);

    assert!(
        world.cells().interior(rich)[SUGAR] < q10(300),
        "a rich cell in empty water kept everything; nothing crossed outwards"
    );
    assert!(
        world.substrate().chem_at(SUGAR, 6, 6) > 0,
        "what left the cell did not arrive in the square"
    );
    assert!(
        world.cells().interior(poor)[SUGAR] > 0,
        "a poor cell standing in sugar took none of it up; the crossing only runs one way"
    );
    assert_eq!(
        world.total_matter(),
        before,
        "matter changed crossing a membrane"
    );
    world.check_matter().expect("I4 broke over passive transport");
}

/// And a membrane that is shut keeps what it has.
#[test]
fn a_closed_membrane_is_still_a_barrier() {
    const SUGAR: usize = 8;
    let mut world = World::new(slide()).expect("world");
    let mut biology = world.biology().clone();
    biology.ecology.permeability_rate = Q10_ONE / 4;
    world.set_biology(biology);

    let i = spawn(&mut world, 6, 6);
    world.cells_mut().interior_mut(i)[SUGAR] = q10(300);
    // Sealed. This is the thing a lineage evolves towards, and the reason `default_control`
    // starts a membrane *open*: a shut membrane is a derived state, not a starting one.
    world.cells_mut().slots_mut(i)[0].control[0] = 0;
    world.run(6);

    assert_eq!(
        world.cells().interior(i)[SUGAR],
        q10(300),
        "a sealed cell leaked anyway"
    );
}
