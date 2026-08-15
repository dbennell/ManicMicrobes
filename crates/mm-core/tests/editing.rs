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
use mm_core::{Barrier, Flux, Inhabitant, LightRegime, Placement, Scenario, World};

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
    // Not equality any more, and the reason is a fix rather than a tolerance. Detritus decays to
    // structural carbon, which it did not when this was written: `decay_rate` read
    // `Q10_ONE / 2048`, and `Q10_ONE` is 1024, so the rate was zero and the plane was inert.
    // What this test is actually about is that a removed source stops *adding* — so the
    // assertion is that the total does not rise, and separately that the decay it is now subject
    // to is the only thing moving it.
    let after = world.total_matter()[DETRITUS];
    assert!(
        after <= held,
        "detritus rose from {held} to {after} after the source was removed: it went on running"
    );
    assert!(
        after < held,
        "detritus held at {held} exactly; it should be mineralising to carbon"
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
                place: mm_core::Placement::At { x: 7, y: 21 },
            },
            Inhabitant {
                genome: "ancestor.mm".to_string(),
                count: 4,
                place: mm_core::Placement::Spread,
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

#[test]
fn chemistry_painted_by_hand_survives_being_saved() {
    // The same bug the walls had. Chemistry in the substrate and nowhere else is chemistry that
    // vanishes when the scenario is written out, and a scenario is a recipe for a slide.
    let mut world = World::new(slide()).expect("world");
    world.inject(DETRITUS, 9, 9, q10(60));
    world.inject(DETRITUS, 9, 9, q10(40));

    let back =
        World::new(Scenario::from_ron(&world.scenario().to_ron().expect("render")).expect("parse"))
            .expect("world");
    assert_eq!(
        back.substrate().chem_at(DETRITUS, 9, 9),
        q10(100),
        "the paint did not come back"
    );
    assert_eq!(
        world.scenario().seeding.len(),
        1,
        "two strokes on one square made two entries rather than one that grew"
    );
}

#[test]
fn unpainting_back_to_nothing_leaves_no_trace_in_the_recipe() {
    let mut world = World::new(slide()).expect("world");
    world.inject(DETRITUS, 9, 9, q10(60));
    world.extract(DETRITUS, 9, 9, q10(60));
    assert!(
        world.scenario().seeding.is_empty(),
        "an erased stroke left {:?} behind",
        world.scenario().seeding
    );
    world.check_matter().expect("the books moved");
}

/// The editor's whole promise, end to end: build a slide by hand, write it out, open it again,
/// and get back what you built.
///
/// Every piece of this was a separate near-miss. Walls lived only in the substrate. Painted
/// chemistry lived only in the substrate. A hand-placed cell had nowhere in the format to be
/// said. Any one of them missing turns "save your scenario" into "save most of your scenario",
/// which is worse than not offering it.
#[test]
fn a_slide_built_by_hand_comes_back_the_way_it_was_left() {
    let genome = {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/sponge.mm");
        let src = std::fs::read_to_string(path).expect("genome");
        mm_asm::assemble(&src).expect("assembles").bytes
    };

    let mut built = World::new(slide()).expect("world");
    // A channel wall.
    built.set_barriers(&[(10, 6), (11, 6), (12, 6), (13, 6)], true);
    // A patch of something to build bodies out of.
    for x in 2..6 {
        built.inject(4, x, 20, q10(80));
    }
    // A supply at one edge and a way out at the other.
    built.add_flux(Flux::Source {
        chemical: DETRITUS,
        x: 0,
        y: 14,
        width: 2,
        height: 8,
        per_tick: q10(12),
    });
    built.add_flux(Flux::Drain {
        chemical: DETRITUS,
        x: 30,
        y: 0,
        width: 2,
        height: 32,
        rate: Q10_ONE / 8,
    });
    // And somebody to live there, against the wall where a holdfast has something to grip.
    built.seed_inhabitant("sponge.mm", &genome, 2, Placement::At { x: 11, y: 8 });

    let text = built.scenario().to_ron().expect("render");
    let reopened = Scenario::from_ron(&text).expect("parse");
    let mut back = World::new(reopened.clone()).expect("world");

    assert!(
        back.substrate().blocked()[(6 * 32 + 12) as usize],
        "the wall did not come back"
    );
    assert_eq!(
        back.substrate().chem_at(4, 3, 20),
        q10(80),
        "the painted chemistry did not come back"
    );
    assert_eq!(
        back.flux().len(),
        2,
        "the source and drain did not come back"
    );
    assert_eq!(
        reopened.inhabitants,
        vec![Inhabitant {
            genome: "sponge.mm".to_string(),
            count: 2,
            place: mm_core::Placement::At { x: 11, y: 8 },
        }],
        "who lives there did not come back"
    );

    // And it runs: the recipe is a slide somebody can press play on.
    back.place_founders_at(&genome, 2, Some((11, 8)));
    back.run(200);
    back.check_invariants()
        .expect("a hand-built slide broke an invariant when it was played");
}

/// Every placement puts the founders where it says, and never inside a wall.
///
/// The gap this closes was not a bug in the arrangement code — there was no arrangement code.
/// `Inhabitant::at` was declared, documented at length, round-trip tested, and read by nothing:
/// both front ends called `place_founders`, which spreads. So a scenario could describe a walled
/// habitat with a population in it and get a population sprayed across the whole slide, with no
/// error anywhere.
#[test]
fn founders_go_where_the_placement_says_and_never_into_a_wall() {
    use mm_core::Placement;

    let genome = mm_asm::assemble("        GENE #x\n        HALT\n")
        .expect("assembles")
        .bytes;

    // A slide split by a wall down the middle, with a gap in neither half.
    let scenario = mm_core::Scenario {
        name: "two rooms".to_string(),
        width: 40,
        height: 40,
        barriers: vec![mm_core::Barrier::Rect {
            x: 19,
            y: 0,
            width: 2,
            height: 40,
        }],
        ..mm_core::Scenario::default()
    };

    for place in [
        Placement::Grid {
            x: 2,
            y: 2,
            width: 15,
            height: 36,
        },
        Placement::Hex {
            x: 2,
            y: 2,
            width: 15,
            height: 36,
        },
        Placement::Scatter {
            x: 2,
            y: 2,
            width: 15,
            height: 36,
            spacing: 2,
        },
    ] {
        let mut world = mm_core::World::new(scenario.clone()).expect("world");
        let placed = world.place_inhabitants(&genome, 12, place);
        assert_eq!(placed, 12, "{place:?} lost founders on an open rectangle");

        for i in world.cells().iter() {
            let sx = mm_core::fixed::pos_to_square(world.cells().x[i]);
            let sy = mm_core::fixed::pos_to_square(world.cells().y[i]);
            assert!(
                !world.substrate().is_blocked(sx, sy),
                "{place:?} put a founder inside a wall at ({sx}, {sy})"
            );
            assert!(
                sx < 19,
                "{place:?} put a founder at ({sx}, {sy}), outside the room it was given"
            );
        }
    }
}

/// Two species, two rooms, and neither can reach the other.
///
/// The thing a scenario could not describe before: a walled-off habitat with a named population
/// in it. This is the shape `archipelago.ron` is drawn for and could not seed.
#[test]
fn two_rooms_can_hold_two_different_populations() {
    use mm_core::Placement;
    let a = mm_asm::assemble("        GENE #a\n        HALT\n").expect("a").bytes;
    let b = mm_asm::assemble("        GENE #b\n        NOP0\n        HALT\n")
        .expect("b")
        .bytes;
    let mut world = mm_core::World::new(mm_core::Scenario {
        width: 40,
        height: 40,
        barriers: vec![mm_core::Barrier::Rect {
            x: 19,
            y: 0,
            width: 2,
            height: 40,
        }],
        ..mm_core::Scenario::default()
    })
    .expect("world");

    world.place_inhabitants(
        &a,
        8,
        Placement::Grid {
            x: 2,
            y: 2,
            width: 15,
            height: 36,
        },
    );
    world.place_inhabitants(
        &b,
        8,
        Placement::Grid {
            x: 23,
            y: 2,
            width: 15,
            height: 36,
        },
    );

    let (mut left, mut right) = (0, 0);
    for i in world.cells().iter() {
        let sx = mm_core::fixed::pos_to_square(world.cells().x[i]);
        let is_a = world.cells().genome[i].bytes() == a.as_slice();
        if sx < 19 {
            left += 1;
            assert!(is_a, "the wrong species is in the left room");
        } else {
            right += 1;
            assert!(!is_a, "the wrong species is in the right room");
        }
    }
    assert_eq!((left, right), (8, 8));
}

/// A scatter is the same scatter on every machine (hard rule 5).
#[test]
fn a_scatter_is_deterministic() {
    use mm_core::Placement;
    let genome = mm_asm::assemble("        GENE #x\n        HALT\n")
        .expect("assembles")
        .bytes;
    let place = Placement::Scatter {
        x: 0,
        y: 0,
        width: 32,
        height: 32,
        spacing: 3,
    };
    let run = || {
        let mut world = mm_core::World::new(mm_core::Scenario {
            seed: 99,
            width: 32,
            height: 32,
            ..mm_core::Scenario::default()
        })
        .expect("world");
        world.place_inhabitants(&genome, 10, place);
        world
            .cells()
            .iter()
            .map(|i| (world.cells().x[i], world.cells().y[i]))
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());

    // And a different scenario seed scatters differently, or it is not a scatter.
    let mut other = mm_core::World::new(mm_core::Scenario {
        seed: 100,
        width: 32,
        height: 32,
        ..mm_core::Scenario::default()
    })
    .expect("world");
    other.place_inhabitants(&genome, 10, place);
    let moved: Vec<_> = other
        .cells()
        .iter()
        .map(|i| (other.cells().x[i], other.cells().y[i]))
        .collect();
    assert_ne!(run(), moved, "the seed does not reach the scatter");
}

/// `Spread` is the lattice it has always been.
///
/// Every acceptance number in the tree was taken on it, so a refactor that moved it by half a
/// square would move results that have nothing to do with placement.
#[test]
fn spread_is_unchanged_by_the_placement_rewrite() {
    let genome = mm_asm::assemble("        GENE #x\n        HALT\n")
        .expect("assembles")
        .bytes;
    let mut world = mm_core::World::new(mm_core::Scenario {
        width: 64,
        height: 64,
        ..mm_core::Scenario::default()
    })
    .expect("world");
    world.place_founders(&genome, 16);
    let positions: Vec<(i32, i32)> = world
        .cells()
        .iter()
        .map(|i| (world.cells().x[i], world.cells().y[i]))
        .collect();
    // Sixteen founders on a 64-square slide, four across: the middle of each quarter.
    let step = mm_core::fixed::pos(8);
    assert_eq!(positions.len(), 16);
    assert_eq!(positions[0], (step, step));
    assert_eq!(positions[5], (mm_core::fixed::pos(24), mm_core::fixed::pos(24)));
}

#[test]
fn seeding_records_the_arrangement_it_actually_used() {
    // The gap this closes: the editor placed with one call and recorded with another, and the
    // recording one only ever wrote `Placement::At`. So the four other arrangements were
    // reachable from a hand-written file and from nothing else, and a rectangle of founders
    // drawn in the front end saved as a single point.
    let genome = vec![0x2E; 24];
    for place in [
        Placement::Spread,
        Placement::At { x: 9, y: 9 },
        Placement::Grid {
            x: 4,
            y: 4,
            width: 16,
            height: 16,
        },
        Placement::Hex {
            x: 4,
            y: 4,
            width: 16,
            height: 16,
        },
        Placement::Scatter {
            x: 4,
            y: 4,
            width: 16,
            height: 16,
            spacing: 3,
        },
    ] {
        let mut world = World::new(slide()).expect("world");
        let placed = world.seed_inhabitant("ancestor.mm", &genome, 6, place);
        assert!(placed > 0, "{place:?} placed nobody");
        assert_eq!(
            world.inhabitants(),
            [Inhabitant {
                genome: "ancestor.mm".to_string(),
                count: placed,
                place,
            }],
            "{place:?} was not the arrangement written down"
        );

        // And the count recorded is what landed, so reopening gives the slide that was drawn.
        let text = world.scenario().to_ron().expect("render");
        let back = World::new(Scenario::from_ron(&text).expect("parse")).expect("world");
        assert_eq!(back.inhabitants(), world.inhabitants(), "{place:?} drifted");
    }
}

#[test]
fn seeding_the_same_spot_twice_adds_up_and_two_arrangements_do_not() {
    // Leaning on the button reads as "twelve here" rather than twelve entries. Two *different*
    // arrangements of the same genome are two different claims and stay apart.
    let genome = vec![0x2E; 24];
    let mut world = World::new(slide()).expect("world");
    let at = Placement::At { x: 9, y: 9 };
    world.seed_inhabitant("ancestor.mm", &genome, 2, at);
    world.seed_inhabitant("ancestor.mm", &genome, 3, at);
    assert_eq!(world.inhabitants().len(), 1, "one spot became two entries");
    assert_eq!(world.inhabitants()[0].count, 5);

    world.seed_inhabitant("ancestor.mm", &genome, 1, Placement::Spread);
    assert_eq!(world.inhabitants().len(), 2, "two arrangements were merged");
}

#[test]
fn an_inhabitant_can_be_taken_out_of_the_recipe_without_touching_the_slide() {
    // The other half of the gap: the table listed founders and offered no way to remove one, so
    // a dozen dropped in the wrong place stayed in the recipe for good — and deleting the
    // *cells* left the entry behind, so reopening put them straight back.
    //
    // The recipe only, deliberately. Undoing a placement means unspawning a cell through the
    // ledger, and `kill_cell` is not that: it returns the body to the water, which is a death.
    let genome = vec![0x2E; 24];
    let mut world = World::new(slide()).expect("world");
    world.seed_inhabitant("a.mm", &genome, 2, Placement::At { x: 4, y: 4 });
    world.seed_inhabitant("b.mm", &genome, 3, Placement::At { x: 20, y: 20 });
    let alive = world.cells().len();

    let gone = world.remove_inhabitant(0).expect("there were two");
    assert_eq!(gone.genome, "a.mm");
    assert_eq!(world.inhabitants().len(), 1);
    assert_eq!(world.inhabitants()[0].genome, "b.mm");
    assert_eq!(
        world.cells().len(),
        alive,
        "removing a recipe entry killed something on the slide"
    );
    world
        .check_matter()
        .expect("taking an entry out of the recipe moved the books");

    assert!(
        world.remove_inhabitant(9).is_none(),
        "removed one that is not there"
    );
}

/// Where every cell is, as squares, sorted — for comparing two placements.
fn squares_of(world: &World) -> Vec<(i32, i32)> {
    let cells = world.cells();
    let mut out: Vec<(i32, i32)> = (0..cells.len())
        .map(|i| {
            (
                cells.x[i] / mm_core::fixed::POS_ONE,
                cells.y[i] / mm_core::fixed::POS_ONE,
            )
        })
        .collect();
    out.sort_unstable();
    out
}

#[test]
fn two_genomes_in_one_region_do_not_land_on_top_of_each_other() {
    // The bug: a slot is a pure function of the placement, `k`, the count and the seed, with
    // nothing in it saying *which* inhabitant is being placed. So two entries with the same
    // placement and count were put on exactly the same squares — the lattices coincide, and
    // `Scatter` draws from the same stream. Two species seeded into one region came out as one
    // pile of pairs.
    //
    // Unreachable until the editor could name two, which is why it survived and why fixing it
    // moves nothing that was already measured.
    let a = vec![0x2E; 24];
    let b = vec![0x2F; 24];
    for place in [
        Placement::Spread,
        Placement::Grid {
            x: 2,
            y: 2,
            width: 28,
            height: 28,
        },
        Placement::Hex {
            x: 2,
            y: 2,
            width: 28,
            height: 28,
        },
        Placement::Scatter {
            x: 2,
            y: 2,
            width: 28,
            height: 28,
            spacing: 2,
        },
    ] {
        let mut world = World::new(slide()).expect("world");
        world.place_cohort(&[(&a, 4), (&b, 4)], place);
        let mut seen = squares_of(&world);
        let before = seen.len();
        assert_eq!(before, 8, "{place:?} did not place all eight");
        seen.dedup();
        assert_eq!(
            seen.len(),
            before,
            "{place:?} put two founders on one square"
        );
    }
}

#[test]
fn a_cohort_lays_its_members_out_as_one_arrangement() {
    // Not merely "not stacked" — *interleaved*. Four and four over one region has to be the
    // same eight positions as eight of one genome, or "mixed" would mean two arrangements
    // sharing a rectangle rather than one arrangement holding both.
    let a = vec![0x2E; 24];
    let b = vec![0x2F; 24];
    let place = Placement::Grid {
        x: 0,
        y: 0,
        width: 32,
        height: 32,
    };

    let mut mixed = World::new(slide()).expect("world");
    mixed.place_cohort(&[(&a, 4), (&b, 4)], place);

    let mut alone = World::new(slide()).expect("world");
    alone.place_inhabitants(&a, 8, place);

    assert_eq!(squares_of(&mixed), squares_of(&alone));
}

#[test]
fn a_mixed_seeding_reopens_as_the_slide_that_was_drawn() {
    // The round trip the editor depends on. Two entries share a placement, so opening the file
    // has to place them as one cohort — one at a time is what stacks them, which is the whole
    // point of `place_recipe`.
    let a = vec![0x2E; 24];
    let b = vec![0x2F; 24];
    let place = Placement::Scatter {
        x: 4,
        y: 4,
        width: 20,
        height: 20,
        spacing: 2,
    };
    let mut built = World::new(slide()).expect("world");
    built.seed_cohort(&[("a.mm", &a, 3), ("b.mm", &b, 3)], place);
    assert_eq!(built.inhabitants().len(), 2, "one entry each");

    let text = built.scenario().to_ron().expect("render");
    let reopened = Scenario::from_ron(&text).expect("parse");
    let mut back = World::new(reopened).expect("world");
    let members: Vec<(&[u8], u32, Placement)> = back
        .inhabitants()
        .iter()
        .map(|i| {
            let bytes: &[u8] = if i.genome == "a.mm" { &a } else { &b };
            (bytes, i.count, i.place)
        })
        .collect();
    back.place_recipe(&members);

    assert_eq!(
        squares_of(&back),
        squares_of(&built),
        "the mixed seeding came back as a different slide"
    );
}

#[test]
fn founders_in_their_own_rectangles_stay_in_them() {
    // The other half of the pair: "apart" is disjoint sub-regions, one per genome, so each type
    // starts as its own clump. Nothing shared, and nothing to interleave.
    let a = vec![0x2E; 24];
    let b = vec![0x2F; 24];
    let mut world = World::new(slide()).expect("world");
    world.seed_inhabitant(
        "a.mm",
        &a,
        4,
        Placement::Grid {
            x: 0,
            y: 0,
            width: 16,
            height: 32,
        },
    );
    world.seed_inhabitant(
        "b.mm",
        &b,
        4,
        Placement::Grid {
            x: 16,
            y: 0,
            width: 16,
            height: 32,
        },
    );
    let left = squares_of(&world).into_iter().filter(|(x, _)| *x < 16).count();
    assert_eq!(left, 4, "the two clumps ran into each other");
}

#[test]
fn every_shipped_scenarys_founders_still_land_where_they_did() {
    // `place_recipe` is the new way in and it groups by placement. Every shipped scenario names
    // at most one inhabitant, so grouping must be a no-op for all of them — this is the guard
    // that says the cohort change moved nothing that was already measured.
    let genome = vec![0x2E; 24];
    for place in [
        Placement::Spread,
        Placement::At { x: 7, y: 7 },
        Placement::Grid {
            x: 1,
            y: 1,
            width: 30,
            height: 30,
        },
        Placement::Hex {
            x: 1,
            y: 1,
            width: 30,
            height: 30,
        },
        Placement::Scatter {
            x: 1,
            y: 1,
            width: 30,
            height: 30,
            spacing: 2,
        },
    ] {
        let mut one_at_a_time = World::new(slide()).expect("world");
        one_at_a_time.place_inhabitants(&genome, 9, place);

        let mut through_recipe = World::new(slide()).expect("world");
        through_recipe.place_recipe(&[(&genome, 9, place)]);

        assert_eq!(
            squares_of(&through_recipe),
            squares_of(&one_at_a_time),
            "{place:?} moved when it went through place_recipe"
        );
    }
}
