//! Editing a slide by hand, and whether the books survive it (`docs/UI.md` §4).
//!
//! A tool is a mechanism like any other and gets no exemption from I4 and I5. A brush that
//! paints chemistry creates matter, an eraser destroys it, and both have to say so — the
//! difference between a tool and a bug is entirely whether the ledger was told.
//!
//! The other half is that an edit must **survive being saved**. A wall drawn on a slide that
//! exists in the substrate and nowhere else is a wall that vanishes the moment the scenario is
//! written out and opened again, and the natural reading of that is a broken save rather than a
//! wall that was never part of what a scenario is.

use mm_core::fixed::{q10, Q10_ONE};
use mm_core::light::CurrentField;
use mm_core::{Barrier, Flux, Inhabitant, LightRegime, Scenario, World};

const SULPHIDE: usize = 10;
const DETRITUS: usize = 12;

fn slide() -> Scenario {
    Scenario {
        name: "editing".to_string(),
        seed: 0x0ED1,
        width: 32,
        height: 32,
        light: LightRegime::Uniform { intensity: 0 },
        current: CurrentField::Still,
        jitter: 0,
        ..Scenario::default()
    }
}

#[test]
fn painting_chemistry_creates_matter_and_the_ledger_is_told() {
    let mut world = World::new(slide()).expect("world");
    let landed = world.inject(DETRITUS, 8, 8, q10(50));
    assert_eq!(landed, q10(50));
    assert_eq!(world.ledger().injected()[DETRITUS], i64::from(landed));
    world
        .check_matter()
        .expect("a brush stroke unbalanced the books");
    world.run(20);
    world.check_matter().expect("and it stayed unbalanced");
}

#[test]
fn erasing_chemistry_is_matter_leaving_rather_than_matter_vanishing() {
    let mut world = World::new(slide()).expect("world");
    world.inject(DETRITUS, 8, 8, q10(50));
    let taken = world.extract(DETRITUS, 8, 8, q10(20));
    assert_eq!(taken, q10(20));
    assert_eq!(world.ledger().drained()[DETRITUS], i64::from(taken));
    world.check_matter().expect("erasing unbalanced the books");
}

#[test]
fn an_eraser_cannot_take_more_than_is_in_the_square() {
    let mut world = World::new(slide()).expect("world");
    world.inject(DETRITUS, 4, 4, q10(10));
    let taken = world.extract(DETRITUS, 4, 4, q10(999));
    assert_eq!(taken, q10(10), "it took more than was there");
    assert_eq!(world.substrate().chem_at(DETRITUS, 4, 4), 0);
    world.check_matter().expect("the books moved");
}

#[test]
fn painting_onto_a_wall_records_what_it_managed_which_is_nothing() {
    let mut world = World::new(Scenario {
        barriers: vec![Barrier::Square { x: 5, y: 5 }],
        ..slide()
    })
    .expect("world");
    assert_eq!(world.inject(DETRITUS, 5, 5, q10(50)), 0);
    assert_eq!(world.ledger().injected()[DETRITUS], 0);
    world
        .check_matter()
        .expect("a refused stroke still moved the books");
}

#[test]
fn painting_something_that_burns_paints_energy_too() {
    // I5. Sulphide is a pathway substrate, so a brush loaded with it is a brush loaded with
    // energy — and `substrate_mut().add_chem` at the call site would compile, work, and put the
    // world out by exactly that much. This is why the tool is a `World` method.
    let mut world = World::new(slide()).expect("world");
    let before = world.ledger().energy_in();
    world.inject(SULPHIDE, 10, 10, q10(80));
    assert!(
        world.ledger().energy_in() > before,
        "sulphide arrived and brought no energy with it"
    );
    world
        .ledger()
        .check_energy()
        .expect("I5 broke on a brush stroke");

    let imported = world.ledger().energy_imported();
    world.extract(SULPHIDE, 10, 10, q10(80));
    assert_eq!(
        world.ledger().energy_exported(),
        imported,
        "what the brush let in and the eraser let out disagree"
    );
    world.ledger().check_energy().expect("I5 broke on an erase");
}

#[test]
fn a_source_added_by_hand_runs_like_one_from_a_file() {
    let mut world = World::new(slide()).expect("world");
    assert!(world.flux().is_empty());
    world.add_flux(Flux::Source {
        chemical: DETRITUS,
        x: 0,
        y: 0,
        width: 4,
        height: 32,
        per_tick: q10(10),
    });
    world.run(100);
    assert!(world.total_matter()[DETRITUS] > 0, "the source did nothing");
    world
        .check_matter()
        .expect("a hand-placed source unbalanced the books");

    assert!(world.remove_flux(0).is_some());
    let held = world.total_matter()[DETRITUS];
    world.run(100);
    assert_eq!(
        world.total_matter()[DETRITUS],
        held,
        "it went on running after being removed"
    );
    assert!(
        world.remove_flux(7).is_none(),
        "removed one that is not there"
    );
}

#[test]
fn a_wall_drawn_by_hand_is_in_the_scenario_and_survives_being_saved() {
    // The bug this exists for: `place_barrier` wrote to the substrate and nothing else, so a
    // slide drawn on in the front end saved a scenario with no walls in it.
    let mut world = World::new(slide()).expect("world");
    world.set_barriers(&[(3, 3), (3, 4), (3, 5)], true);
    assert_eq!(world.scenario().barriers.len(), 3);

    let text = world.scenario().to_ron().expect("render");
    let back = World::new(Scenario::from_ron(&text).expect("parse")).expect("world");
    for square in [(3, 3), (3, 4), (3, 5)] {
        assert!(
            back.substrate().blocked()[(square.1 * 32 + square.0) as usize],
            "{square:?} did not come back"
        );
    }
}

#[test]
fn erasing_a_wall_takes_it_out_of_the_scenario_too() {
    let mut world = World::new(slide()).expect("world");
    world.set_barriers(&[(3, 3), (3, 4)], true);
    world.set_barriers(&[(3, 3)], false);
    assert_eq!(world.scenario().barriers.len(), 1);
    assert!(world
        .scenario()
        .barriers
        .contains(&Barrier::Square { x: 3, y: 4 }));
}

#[test]
fn rubbing_a_hole_in_an_authored_rectangle_leaves_the_rest_of_it() {
    // A square in the middle of a `Rect` cannot be removed by deleting a list entry, so the
    // list is flattened to squares and the erased one dropped. What must not happen is the
    // whole rectangle disappearing, which is what a `retain` over shapes would have done.
    let mut world = World::new(Scenario {
        barriers: vec![Barrier::Rect {
            x: 4,
            y: 4,
            width: 6,
            height: 2,
        }],
        ..slide()
    })
    .expect("world");
    world.set_barriers(&[(6, 4)], false);

    let text = world.scenario().to_ron().expect("render");
    let back = World::new(Scenario::from_ron(&text).expect("parse")).expect("world");
    let blocked = |x: u32, y: u32| back.substrate().blocked()[(y * 32 + x) as usize];
    assert!(!blocked(6, 4), "the hole did not survive the save");
    assert!(
        blocked(5, 4) && blocked(7, 4),
        "the rest of the wall went with it"
    );
    assert!(blocked(4, 5) && blocked(9, 5), "the second row went too");
}

#[test]
fn a_scenario_that_ships_a_shape_keeps_it_until_somebody_rubs_it_out() {
    // Flattening on any erase anywhere would turn every authored rectangle into a heap of
    // squares the first time the eraser was used in an empty corner.
    let mut world = World::new(Scenario {
        barriers: vec![Barrier::Rect {
            x: 4,
            y: 4,
            width: 6,
            height: 2,
        }],
        ..slide()
    })
    .expect("world");
    world.set_barriers(&[(20, 20)], false);
    assert_eq!(
        world.scenario().barriers,
        vec![Barrier::Rect {
            x: 4,
            y: 4,
            width: 6,
            height: 2
        }],
        "an erase somewhere else flattened the shape"
    );
}

#[test]
fn founders_can_be_placed_where_they_are_wanted_rather_than_on_a_grid() {
    let genome = {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/sponge.mm");
        let src = std::fs::read_to_string(path).expect("genome");
        mm_asm::assemble(&src).expect("assembles").bytes
    };
    let mut world = World::new(slide()).expect("world");
    assert_eq!(world.place_founders_at(&genome, 1, Some((7, 21))), 1);

    let i = world.cells().iter().next().expect("a cell");
    let (x, y) = (
        world.cells().x[i] / mm_core::fixed::POS_ONE,
        world.cells().y[i] / mm_core::fixed::POS_ONE,
    );
    assert_eq!((x, y), (7, 21), "it did not land where it was put");
}

#[test]
fn a_crowd_dropped_on_one_square_spreads_out_rather_than_stacking() {
    let genome = {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/sponge.mm");
        let src = std::fs::read_to_string(path).expect("genome");
        mm_asm::assemble(&src).expect("assembles").bytes
    };
    let mut world = World::new(slide()).expect("world");
    assert_eq!(world.place_founders_at(&genome, 9, Some((16, 16))), 9);

    let mut seen = std::collections::BTreeSet::new();
    for i in world.cells().iter() {
        seen.insert((
            world.cells().x[i] / mm_core::fixed::POS_ONE,
            world.cells().y[i] / mm_core::fixed::POS_ONE,
        ));
    }
    assert_eq!(
        seen.len(),
        9,
        "nine founders landed on {} squares",
        seen.len()
    );
}

#[test]
fn where_the_inhabitants_go_survives_the_round_trip() {
    let s = Scenario {
        inhabitants: vec![
            Inhabitant {
                genome: "sponge.mm".to_string(),
                count: 3,
                at: Some((7, 21)),
            },
            Inhabitant {
                genome: "ancestor.mm".to_string(),
                count: 4,
                at: None,
            },
        ],
        flux: vec![Flux::Drain {
            chemical: DETRITUS,
            x: 30,
            y: 0,
            width: 2,
            height: 32,
            rate: Q10_ONE / 8,
        }],
        ..slide()
    };
    let back = Scenario::from_ron(&s.to_ron().expect("render")).expect("parse");
    assert_eq!(back.inhabitants, s.inhabitants);
    assert_eq!(back.flux, s.flux);
}
