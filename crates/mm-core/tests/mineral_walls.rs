//! Rock that holds its minerals, and the leak that closes.
//!
//! `ledger.rs` has carried this note since M1:
//!
//! > Matter deliberately removed from the world, per chemical. **Only barriers do this.**
//!
//! Raising a barrier over occupied water evicted what was there, and `Ledger::record_evicted`
//! existed so that the loss was said out loud rather than silently breaking I4. It was the one
//! genuine exit from a world that otherwise conserves matter exactly.
//!
//! `docs/CHEMISTRY.md` §10 turns that exit into a compartment. Matter buried under a wall is
//! pushed into the neighbouring water, and where there is nowhere to push it a *mineral* is kept
//! as solid — in planes the fluid never touches, counted by `World::total_matter` and carried
//! through a snapshot. For the minerals the exit is closed.
//!
//! **The two kinds of wall, and why nothing declares which is which.** A blocked square holding
//! solid is rock: it dissolves into thirsty water and opens when it is worn past the threshold. A
//! blocked square holding none is bedrock — there is nothing to dissolve, so it never enters the
//! loop and is permanent. That is the whole of the distinction, it needs no flag, and it is why a
//! wall must not help itself to the mineral it happens to be raised over: such a wall would be
//! rock made of a crust, and a crust does not survive a weathering step. A scenario that wants
//! rock says `Seeding::Rock`.

use mm_core::chem::{solid_slot, CHEM_COUNT, SOLID_CHEMICALS};
use mm_core::fixed::q10;
use mm_core::{LightRegime, Scenario, Seeding, Snapshot, World};

/// A lit, still slide holding a little of everything the test needs.
fn slide() -> Scenario {
    Scenario {
        name: "mineral walls".to_string(),
        seed: 7,
        width: 24,
        height: 24,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        seeding: vec![
            // A mineral, which a wall should keep — and **below its own saturation**, which the
            // first version of this fixture was not. At fifty a square against a solubility of
            // eight the whole slide was supersaturated, so it nucleated everywhere and every test
            // that tried to control one square was measuring its neighbours instead. A fixture
            // has to start in the state the tests assume, and "dissolved" is that state.
            Seeding::Uniform {
                chemical: SOLID_CHEMICALS[0],
                per_square: q10(2),
            },
            Seeding::Uniform {
                chemical: SOLID_CHEMICALS[1],
                per_square: q10(10),
            },
            // ...and a substrate, which it should not.
            Seeding::Uniform {
                chemical: 8,
                per_square: q10(40),
            },
        ],
        ..Scenario::default()
    }
}

/// **The claim: a wall raised over mineral pushes it aside, and holds nothing.**
///
/// This test asserted the opposite for a day — that the wall *kept* the mineral as solid, which
/// closes the eviction leak in the tidiest possible way and is why it was written that way first.
/// It cannot stand, because it erases the distinction the weathering depends on: a blocked square
/// holding solid is rock and dissolves, a blocked square holding none is bedrock and cannot. A
/// wall that helps itself to the mineral it was raised over is a wall that has quietly become
/// rock, and the next weathering step wears its thin crust away and opens it.
///
/// So the mineral goes where the sugar goes — into the water around — and the leak is closed by
/// [`a_wall_with_nowhere_to_push_keeps_the_mineral_rather_than_evicting_it`] instead, which is
/// the case where there is genuinely no alternative.
#[test]
fn a_wall_raised_over_mineral_pushes_it_aside() {
    let mut world = World::new(slide()).expect("world");
    let before = world.total_matter();
    let mineral = SOLID_CHEMICALS[0];
    assert!(before[mineral] > 0, "the slide holds no mineral to bury");

    world.set_barrier(10, 10, true);

    let after = world.total_matter();
    assert_eq!(
        after[mineral], before[mineral],
        "burying mineral under a wall destroyed {} of it",
        before[mineral] - after[mineral]
    );
    let k = solid_slot(mineral).expect("a solid-capable chemical");
    assert_eq!(
        world.substrate().solid_at(k, 10, 10),
        0,
        "the wall took the mineral up as solid; that makes it rock rather than bedrock, and rock \
         this thin dissolves away and opens"
    );
    let in_fluid: i64 = world
        .substrate()
        .chem_plane(mineral)
        .iter()
        .map(|v| i64::from(*v))
        .sum();
    assert_eq!(
        in_fluid, before[mineral],
        "the mineral left the water without becoming solid"
    );
    assert!(
        world.substrate().blocked()[world.substrate().index(10, 10)],
        "the square did not become a wall"
    );
}

/// And when there is nowhere to push to, it is kept rather than evicted.
///
/// This is the leak `ledger.rs` has carried since M1 — "the one genuine exit is a barrier raised
/// over an occupied square" — and for a mineral it is now closed: walled in on every side, the
/// matter changes compartment instead of leaving the world.
#[test]
fn a_wall_with_nowhere_to_push_keeps_the_mineral_rather_than_evicting_it() {
    // A slide three squares wide with the middle column open: wall the middle square and the
    // rings outward find nothing but rock.
    let mut world = World::new(Scenario {
        width: 3,
        height: 3,
        seeding: vec![Seeding::Uniform {
            chemical: SOLID_CHEMICALS[0],
            per_square: q10(4),
        }],
        ..slide()
    })
    .expect("world");
    for (x, y) in [
        (0, 0),
        (1, 0),
        (2, 0),
        (0, 1),
        (2, 1),
        (0, 2),
        (1, 2),
        (2, 2),
    ] {
        world.set_barrier(x, y, true);
    }
    let mineral = SOLID_CHEMICALS[0];
    let k = solid_slot(mineral).expect("solid-capable");
    let before = world.total_matter()[mineral];

    world.set_barrier(1, 1, true);

    assert_eq!(
        world.total_matter()[mineral],
        before,
        "mineral was destroyed by the last wall going up"
    );
    assert!(
        world.substrate().solid_at(k, 1, 1) > 0,
        "there was nowhere to push it and it did not become solid either, so it left the world"
    );
    assert_eq!(
        world.ledger().evicted()[mineral],
        0,
        "the mineral was recorded as evicted; the solid planes are what that column exists to \
         stop being needed"
    );
}

/// **Bedrock does not erode.**
///
/// The regression this whole arrangement is arranged around. A scenario's barrier holds no solid,
/// so it is not in the dissolution loop at all and there is nothing for the threshold to judge —
/// which is what makes an authored wall permanent without a flag saying so. Run in water hungry
/// for both minerals, which is the state that dissolves rock fastest.
#[test]
fn a_scenario_barrier_is_bedrock_and_stays_shut() {
    let mut world = World::new(Scenario {
        barriers: vec![mm_core::Barrier::Square { x: 12, y: 12 }],
        // Empty water: the maximum deficit, so anything soluble in that square is on its way out.
        seeding: Vec::new(),
        ..slide()
    })
    .expect("world");
    let at = world.substrate().index(12, 12);
    assert!(world.substrate().blocked()[at], "it did not start as a wall");

    world.run(5_000);

    assert!(
        world.substrate().blocked()[at],
        "an authored wall dissolved away. Bedrock is a blocked square with nothing solid in it; \
         if this fails, something put mineral into one"
    );
}

/// A scenario declares rock, and it is rock from the first tick.
#[test]
fn a_scenario_authors_a_reef_with_rock() {
    let mineral = SOLID_CHEMICALS[1];
    let k = solid_slot(mineral).expect("solid-capable");
    let mut base = slide();
    base.seeding.push(Seeding::Rock {
        chemical: mineral,
        x: 6,
        y: 6,
        width: 3,
        height: 2,
        per_square: q10(400),
    });
    let world = World::new(base).expect("world");

    assert!(
        world.substrate().solid_at(k, 7, 6) > 0,
        "the reef holds no mineral"
    );
    for (x, y) in [(6, 6), (8, 7)] {
        assert!(
            world.substrate().blocked()[world.substrate().index(x, y)],
            "({x}, {y}) holds four hundred a square and is not a wall; the threshold is two \
             hundred, and rock thick enough to be a wall should not need a weathering step to \
             notice"
        );
    }
    assert!(
        !world.substrate().blocked()[world.substrate().index(9, 6)],
        "the square beyond the rectangle was blocked too"
    );
    world.check_invariants().expect("a world with a reef in it balances");
}

/// And what a wall cannot hold is pushed aside rather than kept — or lost.
///
/// The point of the compartment is not that a wall keeps everything: sugar does not become rock,
/// and a wall that held it would have stopped meaning anything. What happens instead is better
/// than the eviction this test was first written to expect — `place_barrier` walks outward in
/// rings and *displaces* the dissolved contents into the neighbouring water, and only records an
/// eviction when there is nowhere left to put them.
///
/// So there are two claims here, and the second is the one that would have been missed by
/// checking a total: the mineral is in the rock, and the sugar is still in the water.
#[test]
fn a_wall_pushes_aside_what_it_cannot_hold() {
    let mut world = World::new(slide()).expect("world");
    let sugar = 8usize;
    assert!(
        solid_slot(sugar).is_none(),
        "sugar is solid-capable; this test needs a chemical that is not"
    );
    let before = world.total_matter()[sugar];

    world.set_barrier(10, 10, true);

    assert_eq!(
        world.total_matter()[sugar],
        before,
        "sugar was destroyed by a wall going up over it; it should have been displaced"
    );
    // And it is genuinely in the water rather than in the rock: nothing that is not a mineral
    // can be held as solid, so the sugar has to be somewhere a cell could still eat it.
    let in_fluid: i64 = world
        .substrate()
        .chem_plane(sugar)
        .iter()
        .map(|v| i64::from(*v))
        .sum();
    assert_eq!(
        in_fluid, before,
        "the sugar left the fluid without becoming solid, which is neither of the two things a \
         wall is allowed to do to it"
    );
}

/// The compartment survives a snapshot, or it is not state (hard rule 7).
#[test]
fn a_wall_keeps_its_minerals_across_a_snapshot() {
    let mut base = slide();
    base.seeding.push(Seeding::Rock {
        chemical: SOLID_CHEMICALS[0],
        x: 10,
        y: 10,
        width: 2,
        height: 1,
        per_square: q10(400),
    });
    let mut world = World::new(base).expect("world");
    world.run(5);

    let held = world.total_matter();
    let bytes = Snapshot::write(&world).expect("write");
    let back = Snapshot::read(&bytes).expect("read");

    assert_eq!(
        back.total_matter(),
        held,
        "a restored world holds a different amount of matter than the one it was written from"
    );
    for (k, c) in SOLID_CHEMICALS.iter().enumerate() {
        let (a, b) = (
            world.substrate().solid_at(k, 10, 10),
            back.substrate().solid_at(k, 10, 10),
        );
        assert_eq!(a, b, "solid chemical {c} did not survive the round trip");
    }
    assert_eq!(
        back.state_hash(),
        world.state_hash(),
        "the solid planes are missing from the state hash, so two worlds differing only in their \
         rock are indistinguishable to every determinism check in the tree"
    );
}

/// Solid is matter, and matter is conserved while the world runs on top of it.
#[test]
fn a_world_with_rock_in_it_still_balances_every_tick() {
    let mut world = World::new(slide()).expect("world");
    for x in 8..14 {
        world.set_barrier(x, 12, true);
    }
    world.adopt_current_contents_as_baseline();
    let before = world.total_matter();

    for tick in 0..120 {
        world.step();
        world
            .check_matter()
            .unwrap_or_else(|e| panic!("the books stopped balancing at tick {tick}: {e}"));
    }
    // Nothing in this world converts a mineral, so the totals are constant rather than merely
    // accounted: no cell here builds, and the solid planes are never stirred.
    for c in 0..CHEM_COUNT {
        if solid_slot(c).is_some() {
            assert_eq!(
                world.total_matter()[c],
                before[c],
                "mineral {c} moved in a world where nothing should have touched it"
            );
        }
    }
}

/// **Rock dissolves into water that is below saturation, and stops when it is not.**
///
/// The self-limiting half of the law, and the reason dissolution follows the *deficit* rather
/// than the stock: a wall standing in saturated water gives up nothing, and the same wall beside
/// water that something is stripping gives up fast. That is biological weathering, and it costs
/// no mechanism of its own.
#[test]
fn rock_dissolves_into_thirsty_water_and_not_into_full_water() {
    let dissolved = |fill: i32| {
        let mut world = World::new(slide()).expect("world");
        let c = SOLID_CHEMICALS[0];
        let k = solid_slot(c).expect("solid-capable");
        // A wall with a stock, and the water around it set to `fill`.
        world.set_barrier(12, 12, true);
        world.substrate_mut().add_solid(k, 12, 12, q10(400));
        for (x, y) in [(11, 12), (13, 12), (12, 11), (12, 13)] {
            world.substrate_mut().set_chem(c, x, y, fill);
        }
        world.adopt_current_contents_as_baseline();
        let before = world.substrate().solid_at(k, 12, 12);
        world.run(200);
        world.check_matter().expect("weathering must conserve");
        before - world.substrate().solid_at(k, 12, 12)
    };

    let saturation = mm_core::chem::ChemTable::spec_default()
        .get(SOLID_CHEMICALS[0])
        .saturation;
    let thirsty = dissolved(0);
    let full = dissolved(saturation);
    assert!(thirsty > 0, "a wall in empty water gave up nothing");
    assert_eq!(
        full, 0,
        "a wall in saturated water dissolved {full} anyway; the rate follows the deficit, and \
         there is no deficit"
    );
}

/// And a wall worn below the threshold stops being a wall.
///
/// The wall is *derived*, not declared: nothing clears a flag, the stock simply falls past the
/// line. If this ever stops holding, rock becomes permanent again and the mechanism is only half
/// of itself.
#[test]
fn a_wall_worn_away_opens() {
    let mut world = World::new(slide()).expect("world");
    let c = SOLID_CHEMICALS[0];
    let k = solid_slot(c).expect("solid-capable");
    {
        let mut biology = world.biology().clone();
        // Barely a wall, and dissolving fast: the point is the crossing, not the wait.
        biology.minerals.wall_threshold = q10(10);
        biology.minerals.dissolve = mm_core::Q10_ONE / 2;
        world.set_biology(biology);
    }
    world.set_barrier(12, 12, true);
    world.substrate_mut().add_solid(k, 12, 12, q10(12));
    world.adopt_current_contents_as_baseline();
    let at = world.substrate().index(12, 12);
    assert!(world.substrate().blocked()[at], "it did not start as a wall");

    world.run(400);

    assert!(
        !world.substrate().blocked()[at],
        "the rock wore down to {} and stayed a wall; the threshold is what makes it one",
        world.substrate().solid_at(k, 12, 12)
    );
    world.check_matter().expect("opening a worn wall must conserve");
}

/// **A wall of two minerals is judged on the two of them.**
///
/// The threshold asks whether a *square* is rock, and the first version asked it once per
/// chemical: a wall holding sixty of phosphate and three hundred of silica was found to be under
/// the line during the phosphate pass and opened, while plainly still made of silica. On a reef of
/// mixed composition that emptied the whole thing — a hundred and seventy-one blocked squares to
/// none — and it did so silently, because each individual comparison was doing exactly what it
/// said.
#[test]
fn a_wall_thin_in_one_mineral_and_thick_in_another_stays_a_wall() {
    let mut world = World::new(slide()).expect("world");
    let (a, b) = (SOLID_CHEMICALS[0], SOLID_CHEMICALS[1]);
    let (ka, kb) = (
        solid_slot(a).expect("solid-capable"),
        solid_slot(b).expect("solid-capable"),
    );
    let threshold = world.biology().minerals.wall_threshold;
    world.set_barrier(12, 12, true);
    // Under the line on its own; over it together, twice over.
    world.substrate_mut().add_solid(ka, 12, 12, threshold / 4);
    world.substrate_mut().add_solid(kb, 12, 12, threshold * 2);
    world.adopt_current_contents_as_baseline();
    let at = world.substrate().index(12, 12);

    // A hundred ticks is a handful of weathering steps — far too few to wear away four hundred
    // units of silica, and the bug opened the square on the *first* one. Given long enough this
    // wall does dissolve and should, which is a different test.
    world.run(100);

    assert!(
        world.substrate().blocked()[at],
        "a square holding {} of one mineral and {} of another opened; the threshold is a property \
         of the square, and comparing one plane at a time empties a mixed reef",
        world.substrate().solid_at(ka, 12, 12),
        world.substrate().solid_at(kb, 12, 12)
    );
}

/// **Supersaturated water deposits onto a surface, and closes into rock.**
///
/// The other direction of the same law, and the reason §10 restricts it to surfaces: scanning
/// every open square for excess is a full-grid pass per mineral, on the phase already furthest
/// from its gate. Growth happens where solid already is, which is how crystals grow anyway — and
/// the cost is that a slide with no rock on it has nowhere to start, which the nucleation scan is
/// for and which is not built yet.
#[test]
fn supersaturated_water_grows_the_rock_it_touches() {
    let mut world = World::new(slide()).expect("world");
    let c = SOLID_CHEMICALS[0];
    let k = solid_slot(c).expect("solid-capable");
    let saturation = mm_core::chem::ChemTable::spec_default().get(c).saturation;

    // A seed of solid, and one square beside it holding far more than it can keep in solution.
    world.substrate_mut().add_solid(k, 12, 12, q10(20));
    world.substrate_mut().set_chem(c, 13, 12, saturation * 8);
    world.adopt_current_contents_as_baseline();
    let before = world.substrate().solid_at(k, 13, 12);

    world.run(200);
    world.check_matter().expect("deposition must conserve");

    assert!(
        world.substrate().solid_at(k, 13, 12) > before,
        "water at eight times saturation beside a seed of rock deposited nothing"
    );
    assert!(
        world.substrate().chem_at(c, 13, 12) < saturation * 8,
        "the water is as full as it started; nothing came out of solution"
    );
}

/// Growth is on surfaces only, so open water far from any rock keeps its excess.
///
/// Stated as a test rather than left implicit, because it is the *cost* of the surface
/// restriction and the thing the nucleation scan will change. Until that exists, a supersaturated
/// square with no solid anywhere near it stays supersaturated — which is a real state of matter
/// and a temporary state of this engine.
#[test]
fn open_water_with_no_surface_near_it_keeps_its_excess_for_now() {
    let mut world = World::new(slide()).expect("world");
    let c = SOLID_CHEMICALS[0];
    let k = solid_slot(c).expect("solid-capable");
    let saturation = mm_core::chem::ChemTable::spec_default().get(c).saturation;
    world.substrate_mut().set_chem(c, 4, 4, saturation * 8);
    world.adopt_current_contents_as_baseline();

    world.run(200);

    assert_eq!(
        world.substrate().solid_at(k, 4, 4),
        0,
        "solid appeared with no surface to grow on; that is nucleation, and it is supposed to \
         come from the amortised scan rather than from anywhere convenient"
    );
    world.check_matter().expect("doing nothing must also conserve");
}
