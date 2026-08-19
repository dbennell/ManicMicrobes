//! A swallowed cell is four compartments, and all four now go somewhere.
//!
//! Cytoplasm crosses as itself, the body arrives as carrion, the organelles' minerals cross as
//! themselves, and — since `engulf_charge_recovery` — a share of the charge crosses too instead
//! of being dissipated whole. Digestion then splits the flesh between something to burn and
//! something to build with, which is what `digestion_structural_share` is for.
//!
//! What these are guarding, in one line each: that the shares are *shares* and cannot mint matter
//! between them; that zero on either dial is exactly the old behaviour; and that matter with
//! nowhere to go is held rather than destroyed.

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::chem::CHEM_COUNT;
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::{MutationRates, Organelle, OrganelleType, Scenario, World};

fn world(tune: impl FnOnce(&mut BiologyConfig)) -> World {
    let mut w = World::new(Scenario {
        seed: 5,
        width: 16,
        height: 16,
        ..Scenario::default()
    })
    .expect("world");
    let mut biology = BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    };
    tune(&mut biology);
    w.set_biology(biology);
    w
}

fn spawn(w: &mut World, mass: i32, energy: i32, membrane: u8) -> CellId {
    let genome = w.genomes().intern(vec![0x2E]).expect("genome");
    w.spawn_cell(CellSeed {
        x: pos(8),
        y: pos(8),
        mass: q10(mass),
        energy: q10(energy),
        membrane,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    })
}

/// A predator with a mouth and a gut, and a prey it outweighs two to one.
fn pair(w: &mut World, eater_mass: i32, membrane: u8) -> (CellId, CellId) {
    let eater = spawn(w, eater_mass, 400, membrane);
    let i = w.cells().index(eater).expect("alive");
    w.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 56);
    w.cells_mut().slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
    let mut vac = Organelle::finished(OrganelleType::Vacuole, 120);
    vac.control[1] = Q10_ONE as i16; // appetite is shut on a fresh vacuole, and must be asked for
    w.cells_mut().slots_mut(i)[4] = vac;
    w.cells_mut().slots_mut(i)[6] = Organelle::finished(OrganelleType::Lysosome, 100);

    let prey = spawn(w, 60, 400, 24);
    let j = w.cells().index(prey).expect("alive");
    w.cells_mut().slots_mut(j)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
    w.adopt_current_contents_as_baseline();
    (eater, prey)
}

#[test]
fn a_swallowed_cell_hands_over_some_of_its_charge() {
    let mut w = world(|_| {});
    let (eater, prey) = pair(&mut w, 200, 24);
    let before = w.cells().energy[w.cells().index(eater).expect("alive")];
    let charge = w.cells().energy[w.cells().index(prey).expect("alive")];
    w.run(1);

    assert!(w.cells().index(prey).is_none(), "the prey was not swallowed");
    let after = w.cells().energy[w.cells().index(eater).expect("alive")];
    assert!(
        after > before,
        "the eater is no better off for a meal: {before} then {after}"
    );
    // A share, not the lot — the physical reading is that some of the charge survives the meal.
    let kept = after - before;
    assert!(
        kept < charge / 2,
        "kept {kept} of {charge}, which is not a share"
    );
}

#[test]
fn at_zero_the_charge_dies_with_its_owner_exactly_as_it_used_to() {
    // The polarity that lets an old scenario keep its old physics.
    let mut w = world(|b| b.ecology.engulf_charge_recovery = 0);
    let (eater, prey) = pair(&mut w, 200, 24);
    let before = w.cells().energy[w.cells().index(eater).expect("alive")];
    w.run(1);
    assert!(w.cells().index(prey).is_none(), "the prey was not swallowed");
    let after = w.cells().energy[w.cells().index(eater).expect("alive")];
    assert!(
        after <= before,
        "charge crossed with the dial at zero: {before} then {after}"
    );
}

#[test]
fn digestion_yields_something_to_burn_and_something_to_build_with() {
    let mut w = world(|_| {});
    let structural = w.biology().structural_chemical % CHEM_COUNT;
    let substrate = w
        .biology()
        .metabolism
        .catalogue
        .metabolism
        .primary()
        .substrate
        % CHEM_COUNT;
    // A world where the two are the same chemical would make this test say nothing.
    assert_ne!(structural, substrate, "the default chemistry changed");

    let (eater, _) = pair(&mut w, 200, 24);
    w.run(2);
    let i = w.cells().index(eater).expect("alive");
    assert!(
        w.cells().interior(i)[structural] > 0,
        "a meal delivered no structural matter, so a predator still cannot grow on one"
    );
    assert!(
        w.cells().interior(i)[substrate] > 0,
        "a meal delivered no fuel, which is how the engulfer starved with its mouth full"
    );
}

#[test]
fn with_the_brick_share_at_zero_it_is_all_fuel_again() {
    let mut w = world(|b| b.ecology.digestion_structural_share = 0);
    let structural = w.biology().structural_chemical % CHEM_COUNT;
    let (eater, _) = pair(&mut w, 200, 24);
    w.run(2);
    let i = w.cells().index(eater).expect("alive");
    assert_eq!(
        w.cells().interior(i)[structural],
        0,
        "structural matter appeared with the dial at zero"
    );
}

#[test]
fn a_meal_becomes_a_body_only_if_there_is_room_for_it() {
    // The half the engine cannot supply. Growth is capped at `q10(membrane.param)`, so a predator
    // whose organelles already outweigh its wall grows by nothing however much it eats — and the
    // matter is *held*, not lost, which is what keeps I4 intact.
    let mut grew = Vec::new();
    for membrane in [24u8, 200] {
        let mut w = world(|_| {});
        let (eater, _) = pair(&mut w, 130, membrane);
        let before = w.cells().mass[w.cells().index(eater).expect("alive")];
        let matter_before: i64 = w.total_matter().iter().sum();
        w.run(600);
        let i = w.cells().index(eater).expect("alive");
        grew.push((w.cells().mass[i] - before) / Q10_ONE);
        assert_eq!(
            matter_before,
            w.total_matter().iter().sum::<i64>(),
            "a meal minted or destroyed matter at membrane {membrane} (I4)"
        );
    }
    assert_eq!(grew[0], 0, "a cell with no wall to grow into grew anyway");
    assert!(
        grew[1] > 0,
        "a cell with room did not turn its meal into body: {:?}",
        grew
    );
}

#[test]
fn swallowing_still_creates_no_matter() {
    // I4, on the path the two new dials touch. The charge is energy and outside this sum; the
    // brick share only moves matter between two chemicals inside it.
    let mut w = world(|_| {});
    let _ = pair(&mut w, 200, 200);
    // After the cells exist, or this measures spawning rather than eating.
    let before: i64 = w.total_matter().iter().sum();
    w.run(400);
    assert_eq!(before, w.total_matter().iter().sum::<i64>(), "I4");
    w.check_matter().expect("matter conservation");
}
