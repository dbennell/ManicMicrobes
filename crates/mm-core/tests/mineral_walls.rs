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
//! `docs/CHEMISTRY.md` §10 turns that exit into a compartment. A wall now *holds* the mineral it
//! is raised over, in solid planes the fluid never touches, counted by `World::total_matter` and
//! carried through a snapshot. For the minerals it is no longer an exit at all.
//!
//! What a wall does *not* keep is everything else: only a mineral can be solid, which is the whole
//! reason the solid planes are two and not seventeen — see `chem::SOLID_CHEMICALS`. Sugar buried
//! under a rock is pushed into the neighbouring water instead, which is better than the eviction
//! this file was first written expecting, and is a claim worth a test of its own rather than a
//! footnote to the total.

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
            // A mineral, which a wall should keep...
            Seeding::Uniform {
                chemical: SOLID_CHEMICALS[0],
                per_square: q10(50),
            },
            Seeding::Uniform {
                chemical: SOLID_CHEMICALS[1],
                per_square: q10(30),
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

/// **The claim: a wall raised over mineral keeps it, and the world's books do not move.**
#[test]
fn a_wall_raised_over_mineral_holds_it_rather_than_destroying_it() {
    let mut world = World::new(slide()).expect("world");
    let before = world.total_matter();
    let mineral = SOLID_CHEMICALS[0];
    assert!(before[mineral] > 0, "the slide holds no mineral to bury");

    world.set_barrier(10, 10, true);

    let after = world.total_matter();
    assert_eq!(
        after[mineral], before[mineral],
        "burying mineral under a wall destroyed {} of it; the solid planes exist so that a wall \
         is a compartment rather than an exit",
        before[mineral] - after[mineral]
    );
    let k = solid_slot(mineral).expect("a solid-capable chemical");
    assert!(
        world.substrate().solid_at(k, 10, 10) > 0,
        "the wall holds no mineral, so wherever that matter went it was not into the rock"
    );
    assert!(
        world.substrate().blocked()[world.substrate().index(10, 10)],
        "the square did not become a wall"
    );
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
    let mut world = World::new(slide()).expect("world");
    world.set_barrier(10, 10, true);
    world.set_barrier(11, 10, true);
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
