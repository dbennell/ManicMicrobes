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

/// **Water that runs far enough above saturation grows its own nucleus.**
///
/// This test used to assert the opposite — `..._keeps_its_excess_for_now` — because surface-only
/// growth left a hole: water with no rock near it had nowhere to deposit and climbed without
/// limit, which is a supersaturated solution and not a state to be modelling. It was written as a
/// test rather than a comment precisely so that it would fail when the nucleation scan landed,
/// which is what it did.
///
/// The scan is an amortised slice — a contiguous run of one plane per weathering step, cycling by
/// tick — so nucleation is not instant and is not meant to be. It is thousands of ticks, which is
/// the right timescale for rock appearing where there was none.
#[test]
fn water_far_above_saturation_nucleates_without_a_surface() {
    let mut world = World::new(slide()).expect("world");
    let c = SOLID_CHEMICALS[0];
    let k = solid_slot(c).expect("solid-capable");
    let saturation = mm_core::chem::ChemTable::spec_default().get(c).saturation;
    // Well past the nucleation line, which is a multiple of saturation rather than saturation
    // itself: a crystal starts harder than it grows.
    world.substrate_mut().set_chem(c, 4, 4, saturation * 12);
    world.adopt_current_contents_as_baseline();

    world.run(4_000);
    world.check_matter().expect("nucleation must conserve");

    assert!(
        world.substrate().solid_at(k, 4, 4) > 0,
        "water at twelve times saturation never came out of solution; supersaturation is \
         metastable and the scan exists so it cannot persist"
    );
}

/// And water merely *over* saturation does not, which is the gap that makes the scan affordable.
///
/// Nucleation is harder than growth in the real thing — a dissolved salt deposits happily onto a
/// crystal that already exists at a concentration which would never start one — so the scan hunts
/// a multiple of saturation while surface growth uses saturation itself. Without the gap the
/// expensive path would fire everywhere, and with too much of it the water would never come back.
#[test]
fn water_just_over_saturation_waits_for_a_surface() {
    let mut world = World::new(slide()).expect("world");
    let c = SOLID_CHEMICALS[0];
    let k = solid_slot(c).expect("solid-capable");
    let saturation = mm_core::chem::ChemTable::spec_default().get(c).saturation;
    let nucleation = world.biology().minerals.nucleation;
    // Over the line that grows a crystal, under the line that starts one.
    let held = saturation + (mm_core::fixed::q10_scale(saturation, nucleation) - saturation) / 2;
    world.substrate_mut().set_chem(c, 4, 4, held);
    world.adopt_current_contents_as_baseline();

    world.run(4_000);

    assert_eq!(
        world.substrate().solid_at(k, 4, 4),
        0,
        "water below the nucleation line started a crystal anyway; the gap between starting and \
         growing is what keeps the scan a rare event rather than a second deposition rule"
    );
    world.check_matter().expect("waiting must also conserve");
}

/// **A wall that grew is a wall the whole engine believes in.**
///
/// Rock closing a square is the one place in the crate where the barrier layout changes while the
/// world is running, and it changes it through the *deferred* setter — `place_barrier`, the same
/// one the drawing tool uses, which leaves `Substrate::rebuild_edge_masks` to the caller because
/// the rebuild walks the whole slide and a weathering step can close hundreds of squares at once.
///
/// `weather` did not make that call, and the result was a wall that only half the engine could
/// see. `blocked` was set, so the light regime shadowed the square and `add_chem` refused it — and
/// the `open_x`/`open_y` masks still said the edge was open, so the fluid fluxed straight through
/// the rock, and `has_barriers` still said the slide had no walls, so the renderer drew nothing
/// where the reef was. What you saw was a black hole in the picture with the chemistry piling up
/// behind it: the exact signature of a drawing bug, from a solver contract.
///
/// Three claims, because the three consumers of the barrier layout are independent and it was
/// possible to be right for one and wrong for the others.
#[test]
fn a_wall_that_grew_closes_the_masks_and_shows_on_the_slide() {
    let mut world = World::new(slide()).expect("world");
    let c = SOLID_CHEMICALS[0];
    let k = solid_slot(c).expect("solid-capable");
    let threshold = world.biology().minerals.wall_threshold;
    let deposit = world.biology().minerals.deposit;
    assert!(
        !world.substrate().has_barriers(),
        "the fixture starts walled, so this would pass without growing anything"
    );

    // A seed of rock, and beside it water holding enough that one step's deposit — a fraction
    // `deposit` of the excess over saturation — carries the square past the wall threshold on
    // its own. Growth is otherwise a slow ratchet and this test is about what happens at the
    // moment a square closes, not about how long it takes to get there.
    world.substrate_mut().add_solid(k, 12, 12, q10(20));
    let need = (i64::from(threshold) * i64::from(mm_core::fixed::Q10_ONE)) / i64::from(deposit);
    world
        .substrate_mut()
        .set_chem(c, 13, 12, (need as i32).saturating_mul(2));
    world.adopt_current_contents_as_baseline();

    let mut closed = None;
    for _ in 0..200 {
        world.step();
        if world.substrate().blocked()[world.substrate().index(13, 12)] {
            closed = Some(world.tick_count());
            break;
        }
    }
    let at = closed.expect("the water never deposited enough to close a square");
    let s = world.substrate();
    let i = s.index(13, 12);

    assert!(
        s.has_barriers(),
        "rock closed a square at tick {at} and the substrate still reports no barriers, so \
         `Slide::frame` carries an empty mask and the renderer draws no wall — the reef is a \
         black hole in the picture"
    );
    // The masks are edge-wise: `open_x[i]` is the edge from `i` to its right-hand neighbour, so
    // the wall closes the edge on its own index and the one to its left.
    assert!(
        !s.open_x()[i] && !s.open_y()[i],
        "the edges out of the new wall are still open, so the fluid fluxes through rock"
    );
    assert!(
        !s.open_x()[i - 1] && !s.open_y()[i - s.width() as usize],
        "the edges into the new wall are still open, so the fluid fluxes through rock"
    );

    // And having closed, it stays sealed: nothing the solver does afterwards puts matter back
    // inside it. This is the invariant the stale masks were quietly breaking.
    world.run(400);
    assert!(
        !world.substrate().any_matter_inside_a_barrier(),
        "the fluid filled the inside of a wall, which is matter somewhere it can never leave"
    );
    world.check_matter().expect("growing a wall must conserve");
}

/// **The hand can lay rock, and rock is not bedrock.**
///
/// `World::set_barriers` — the drawing tool's call — can only ever make the permanent kind: a
/// blocked square holding nothing, with nothing in it to dissolve. A reef that gives its mineral
/// up to thirsty water was something a scenario file could author with `Seeding::Rock` and a hand
/// could not draw. `World::set_rock` is the hand's version of that recipe.
///
/// The claim is the pair, not either half: the same stroke drawn both ways gives two walls that
/// look identical and behave oppositely. Testing only that rock blocks would pass on a tool that
/// quietly drew bedrock.
#[test]
fn rock_laid_by_hand_wears_away_and_bedrock_does_not() {
    let squares: Vec<(u32, u32)> = (8..16).map(|y| (12, y)).collect();
    // **Silicon, and the choice is not arbitrary.** Phosphate is deliberately immobile — zero
    // diffusion and zero advection, so that an outcrop is a *location* rather than a level and a
    // stripped patch heals only when something dies there. The consequence for a reef is that
    // what it dissolves has nowhere to go: the water against its face fills to saturation, the
    // deficit the dissolution rate is a fraction *of* goes to nothing, and a phosphate wall
    // stalls behind its own skin at 8177 of 8192 for as long as you care to run it. Measured,
    // not assumed. Silica is middling mobile and is the mineral a reef is actually made of.
    let c = SOLID_CHEMICALS[1];
    let k = solid_slot(c).expect("solid-capable");

    let mut rock = World::new(slide()).expect("world");
    let dose = rock.rock_dose();
    let placed = rock.set_rock(&squares, c, dose);
    assert_eq!(
        placed,
        dose * squares.len() as i32,
        "the stroke did not lay what it was asked for"
    );
    for &(x, y) in &squares {
        let i = rock.substrate().index(x as i32, y as i32);
        assert!(rock.substrate().blocked()[i], "rock at ({x},{y}) is not a wall");
        assert!(
            rock.substrate().solid_at(k, x as i32, y as i32) > 0,
            "the wall at ({x},{y}) holds no mineral, which makes it bedrock"
        );
    }
    assert!(
        rock.substrate().has_barriers(),
        "the slide does not report the walls the tool just drew, so nothing is drawn"
    );
    rock.check_matter().expect("laying rock must conserve");

    let mut bedrock = World::new(slide()).expect("world");
    bedrock.set_barriers(&squares, true);
    for &(x, y) in &squares {
        assert_eq!(
            bedrock.substrate().solid_at(k, x as i32, y as i32),
            0,
            "the barrier tool took up mineral, which would make its walls dissolve"
        );
    }

    // Thirsty water on both sides of both walls, so the weathering law has a deficit to work
    // against. Without it a wall in saturated water dissolves not at all, which is the whole
    // shape of the law and would pass this test for the wrong reason.
    for world in [&mut rock, &mut bedrock] {
        for y in 0..24i32 {
            for x in 0..24i32 {
                world.substrate_mut().set_chem(c, x, y, 0);
            }
        }
        world.adopt_current_contents_as_baseline();
        world.run(20_000);
    }

    let still_rock = squares
        .iter()
        .filter(|(x, y)| rock.substrate().blocked()[rock.substrate().index(*x as i32, *y as i32)])
        .count();
    assert!(
        still_rock < squares.len(),
        "twenty thousand ticks in water stripped bare and not one square of rock opened; a reef \
         that never wears is bedrock with extra steps"
    );
    let still_bedrock = squares
        .iter()
        .filter(|(x, y)| {
            bedrock.substrate().blocked()[bedrock.substrate().index(*x as i32, *y as i32)]
        })
        .count();
    assert_eq!(
        still_bedrock,
        squares.len(),
        "bedrock wore away, and it has nothing in it to wear"
    );
    rock.check_matter().expect("dissolving rock must conserve");
}

/// Rock made of something that cannot be solid is refused, not diverted into the water.
#[test]
fn a_reef_of_sugar_is_refused() {
    let mut world = World::new(slide()).expect("world");
    let sugar = 8;
    assert!(solid_slot(sugar).is_none(), "the fixture picked a mineral");
    let before = world.total_matter();
    let placed = world.set_rock(&[(10, 10)], sugar, world.rock_dose());
    assert_eq!(placed, 0, "sugar was laid as rock");
    assert!(
        !world.substrate().blocked()[world.substrate().index(10, 10)],
        "a refused ask still raised a wall"
    );
    assert_eq!(
        world.total_matter(),
        before,
        "a refused ask changed the world's totals"
    );
}

/// And what the hand drew is in the scenario, so saving and reopening gives it back.
#[test]
fn rock_laid_by_hand_is_written_into_the_scenario() {
    let mut world = World::new(slide()).expect("world");
    let c = SOLID_CHEMICALS[1];
    let dose = world.rock_dose();
    world.set_rock(&[(6, 6), (6, 7)], c, dose);
    // Twice over the same square, which is what leaning on a brush does.
    world.set_rock(&[(6, 6)], c, dose);

    let laid: Vec<_> = world
        .scenario()
        .seeding
        .iter()
        .filter(|s| matches!(s, Seeding::Rock { chemical, .. } if *chemical == c))
        .collect();
    assert_eq!(
        laid.len(),
        2,
        "a second stroke over the same square appended an entry instead of growing one: {laid:?}"
    );
    let at_66 = laid
        .iter()
        .find(|s| matches!(s, Seeding::Rock { x: 6, y: 6, .. }))
        .expect("the square that was drawn twice is not in the scenario");
    assert!(
        matches!(at_66, Seeding::Rock { per_square, .. } if *per_square == dose * 2),
        "the entry did not grow by the second stroke: {at_66:?}"
    );

    // And the recipe rebuilds the same slide.
    let reopened = World::new(world.scenario().clone()).expect("world");
    for (x, y) in [(6u32, 6u32), (6, 7)] {
        assert!(
            reopened.substrate().blocked()[reopened.substrate().index(x as i32, y as i32)],
            "reopening the scenario lost the rock at ({x},{y})"
        );
    }
}
