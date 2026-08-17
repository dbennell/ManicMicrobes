//! A cell can digest what it is carrying, not only what it is standing on.
//!
//! # The dead end this closes
//!
//! `EAT ( amount chem -- got )` takes *any* chemical into the cytoplasm, and until now the two
//! organelles that transform an inedible one both read from the substrate square:
//!
//! - the lysosome turned carrion into substrate, from the square;
//! - the holdfast's filter turned detritus into structural matter, from the square.
//!
//! So a cell that ate carrion or detritus put matter somewhere it could never come out of again.
//! Nothing was destroyed — I4 held, the matter sat in the cytoplasm forever — but it was a sink
//! wearing the costume of a meal, and no shipped genome had found it because none of them eats
//! either chemical.
//!
//! It is also the reason a cell could not eat another cell. Engulfment puts a body *inside* the
//! eater, and a stomach that can only digest the floor is not a stomach: `genomes/engulfer.mm`
//! builds its whole body, swallows, and starves holding its dinner.
//!
//! Both transformers now draw from the interior first and the square for the remainder.

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::ecology::{CARRION, DETRITUS};
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::{MutationRates, Organelle, OrganelleType, Scenario, World};

fn slide() -> Scenario {
    Scenario {
        seed: 42,
        width: 16,
        height: 16,
        ..Scenario::default()
    }
}

fn world() -> World {
    let mut world = World::new(slide()).expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    world
}

/// One cell at (8, 8) with the given organelle in slot 4, at full throttle.
fn cell_with(world: &mut World, kind: OrganelleType, param: u8) -> usize {
    let genome = world.genomes().intern(vec![0x2E]).expect("genome");
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
    let i = world.cells().index(id).expect("alive");
    let mut o = Organelle::finished(kind, param);
    // `default_control` starts every organelle that reaches *outside* the membrane retracted —
    // cilium, flagellum, spike, holdfast, shell, exoenzyme — so that a cell does not begin life
    // gripping and straining and stabbing by accident. A holdfast has to be put out on purpose,
    // which is what a genome's `OSET` does and what this stands in for.
    o.control[0] = Q10_ONE as i16;
    world.cells_mut().slots_mut(i)[4] = o;
    world.adopt_current_contents_as_baseline();
    i
}

fn substrate_chem(world: &World) -> usize {
    world.biology().metabolism.catalogue.metabolism.primary().substrate
}

fn structural_chem(world: &World) -> usize {
    world.biology().structural_chemical
}

#[test]
fn a_lysosome_digests_the_carrion_a_cell_is_carrying() {
    let mut world = world();
    let i = cell_with(&mut world, OrganelleType::Lysosome, 200);
    let sub = substrate_chem(&world);
    // Nothing on the square: the only carrion in the world is inside the cell.
    world.cells_mut().interior_mut(i)[CARRION] = q10(40);
    world.adopt_current_contents_as_baseline();

    let before = world.cells().interior(i)[sub];
    world.run(20);

    let i = world.cells().iter().next().expect("still alive");
    let after = world.cells().interior(i)[sub];
    let left = world.cells().interior(i)[CARRION];
    eprintln!(
        "carried carrion {} -> {}, substrate {} -> {}",
        q10(40) / Q10_ONE,
        left / Q10_ONE,
        before / Q10_ONE,
        after / Q10_ONE
    );
    assert!(
        left < q10(40),
        "the lysosome did not touch the carrion the cell was carrying"
    );
    assert!(
        after > before,
        "carrion was consumed but no substrate appeared: {before} then {after}"
    );
    world.check_matter().expect("books balance");
    world.check_energy().expect("energy accounted");
}

#[test]
fn a_filter_does_not_digest_what_it_is_carrying_and_that_is_deliberate() {
    // The filter was given an interior draw alongside the lysosome, on the symmetry that both
    // transform something a cell might be carrying. `tests/sponge.rs` caught it in one run:
    //
    //   a_cell_carried_by_the_water_catches_nothing
    //   anchored took 38,805,955 -- adrift took 41,290,393
    //
    // A holdfast earns by *interception*: `captured` scales with the slip the holdfast refused,
    // so a cell carried along by the current catches nothing. An interior draw is flow-free by
    // construction, and passive transport fills every cytoplasm with whatever the cell is
    // standing in — so the drifting cell converted its own leakage and beat the anchored one.
    // Holding station stopped buying anything, which is the one thing the organelle is for.
    //
    // This asserts the absence so that the symmetry is not "restored" later by someone who
    // notices the lysosome has an interior route and the filter does not.
    let mut world = world();
    let i = cell_with(&mut world, OrganelleType::Holdfast, 200);
    let structural = structural_chem(&world);
    world.cells_mut().interior_mut(i)[DETRITUS] = q10(40);
    world.adopt_current_contents_as_baseline();
    let before = world.cells().interior(i)[structural];

    world.run(20);

    let i = world.cells().iter().next().expect("still alive");
    assert_eq!(
        world.cells().interior(i)[DETRITUS],
        q10(40),
        "the filter consumed detritus from the cytoplasm. That gives a holdfast an income that \
         does not depend on flow, and `sponge.rs` will tell you what it costs."
    );
    assert_eq!(
        world.cells().interior(i)[structural],
        before,
        "structural matter appeared without the filter straining any water"
    );
    world.check_matter().expect("books balance");
}

#[test]
fn digesting_from_inside_moves_no_matter_into_or_out_of_the_world() {
    // The property that matters most: this is a transfer between two pools that `total_matter`
    // already counts, plus one balanced conversion. It must not create or destroy anything.
    let mut world = world();
    let i = cell_with(&mut world, OrganelleType::Lysosome, 200);
    world.cells_mut().interior_mut(i)[CARRION] = q10(60);
    world.adopt_current_contents_as_baseline();
    let before: i64 = world.total_matter().iter().sum();

    world.run(200);

    let after: i64 = world.total_matter().iter().sum();
    assert_eq!(before, after, "total matter moved while digesting from inside");
    world.check_matter().expect("books balance");
    world.check_energy().expect("energy accounted");
}

#[test]
fn the_square_is_still_digested_when_the_cell_carries_nothing() {
    // The regression guard. No shipped genome eats carrion or detritus, so every existing result
    // in the tree was taken with these interior pools empty — and with them empty the new path
    // must reduce exactly to the old one.
    let mut world = world();
    let i = cell_with(&mut world, OrganelleType::Lysosome, 200);
    let sub = substrate_chem(&world);
    world.substrate_mut().add_chem(CARRION, 8, 8, q10(50));
    world.adopt_current_contents_as_baseline();
    assert_eq!(
        world.cells().interior(i)[CARRION],
        0,
        "this test is only meaningful with an empty interior pool"
    );
    let before = world.cells().interior(i)[sub];

    world.run(20);

    let i = world.cells().iter().next().expect("still alive");
    assert!(
        world.cells().interior(i)[sub] > before,
        "scavenging from the square stopped working"
    );
    assert!(
        world.substrate().chem_at(CARRION, 8, 8) < q10(50),
        "the carrion on the square was not touched"
    );
    world.check_matter().expect("books balance");
}

#[test]
fn what_is_carried_is_digested_before_what_is_underfoot() {
    // Order matters, and it is the whole reason for swallowing: a meal inside you is yours, and
    // one on the square is contested. A cell with both should spend its throughput on its own.
    let mut world = world();
    let i = cell_with(&mut world, OrganelleType::Lysosome, 40);
    world.cells_mut().interior_mut(i)[CARRION] = q10(200);
    world.substrate_mut().add_chem(CARRION, 8, 8, q10(200));
    world.adopt_current_contents_as_baseline();
    let square_before = world.substrate().chem_at(CARRION, 8, 8);
    let inside_before = world.cells().interior(i)[CARRION];

    world.run(5);

    let i = world.cells().iter().next().expect("still alive");
    let ate_inside = inside_before - world.cells().interior(i)[CARRION];
    let ate_square = square_before - world.substrate().chem_at(CARRION, 8, 8);
    eprintln!("from inside {ate_inside}, from the square {ate_square}");
    assert!(
        ate_inside > 0,
        "nothing was taken from the cell's own store at all"
    );
    assert!(
        ate_inside >= ate_square,
        "the square was preferred over the cell's own store: inside {ate_inside}, square \
         {ate_square}. A swallowed meal that can be taken by whatever is standing on the same \
         square is not swallowed."
    );
    world.check_matter().expect("books balance");
}
