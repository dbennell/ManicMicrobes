//! Barriers are solid to bodies as well as to the fluid (SPEC §17.1).
//!
//! `blocked` has been in the substrate since M1, and until now it stopped chemistry and light
//! and let cells straight through. Every scenario in `scenarios/` that draws a wall was drawing
//! it for the water only — `archipelago.ron` fragments the fluid and has never fragmented the
//! population it exists to isolate.
//!
//! These are not a milestone's acceptance tests, because §17 has no milestone yet. They are the
//! properties the mechanism has to have for anything built on top of it to mean what it says,
//! and the third one is the load-bearing one: it is the difference between a wall and a
//! suggestion.

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::light::CurrentField;
use mm_core::{Barrier, Scenario, World};

/// A small slide with a vertical wall down column `at`, and whatever current is asked for.
fn walled_slide(at: u32, current: CurrentField) -> Scenario {
    Scenario {
        name: "barrier probe".to_string(),
        seed: 0x8A_11,
        width: 32,
        height: 32,
        current,
        // Nothing is trying to live here; the question is purely mechanical.
        jitter: 0,
        barriers: vec![Barrier::Rect {
            x: at,
            y: 0,
            width: 1,
            height: 32,
        }],
        ..Scenario::default()
    }
}

fn put_cell(world: &mut World, x: i32, y: i32) -> CellId {
    let genome = world.genomes().intern(vec![0x2E]).expect("genome");
    world.cells_mut().spawn(CellSeed {
        x,
        y,
        mass: q10(40),
        energy: q10(10_000),
        membrane: 16,
        key: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    })
}

/// Every square any living cell's centre stands on, over the whole run.
fn any_cell_inside_a_wall(world: &World) -> Option<(i32, i32)> {
    let s = world.substrate();
    let cells = world.cells();
    (0..cells.capacity())
        .filter(|i| cells.occupied(*i))
        .map(|i| {
            (
                mm_core::fixed::pos_to_square(cells.x[i]),
                mm_core::fixed::pos_to_square(cells.y[i]),
            )
        })
        .find(|(x, y)| s.is_blocked(*x, *y))
}

#[test]
fn a_cell_that_starts_inside_a_wall_gets_out_of_it() {
    // Not a hypothetical: `tools::set_barrier` lets the user draw a barrier over a standing
    // cell, and a daughter can be budded into one. The solver has to answer for a centre that
    // is *inside* the square, where there is no closest point to push away from — so this is
    // the degenerate branch of `barrier_correction`, exercised through the whole tick.
    let mut world = World::new(walled_slide(16, CurrentField::Still)).expect("world");
    put_cell(&mut world, pos(16) + 128, pos(8) + 128);
    // Dead centre of the blocked square to begin with.
    assert!(
        any_cell_inside_a_wall(&world).is_some(),
        "the probe did not start inside the wall"
    );
    for _ in 0..200 {
        world.step();
    }
    assert_eq!(
        any_cell_inside_a_wall(&world),
        None,
        "a cell buried in a barrier never got out of it"
    );
}

#[test]
fn a_current_cannot_drive_a_cell_through_a_wall() {
    // The property the whole mechanism exists for. A uniform current at the fluid's own
    // ceiling, blowing straight into a wall, for long enough that a cell that leaked through
    // even a fraction of a square per tick would be well clear on the other side.
    //
    // `MAX_VELOCITY` is `Q10_ONE / 4`, a quarter of a square per tick, and the solver's clamp
    // allows three eighths of a radius per tick — so the margin is real but it is not large,
    // and it is exactly the thing that would silently stop holding if either number moved.
    let mut world = World::new(walled_slide(
        16,
        CurrentField::Uniform {
            vx: Q10_ONE / 4,
            vy: 0,
        },
    ))
    .expect("world");

    // A row of them, so the test does not depend on one lucky starting offset.
    let ids: Vec<CellId> = (4..12)
        .map(|y| put_cell(&mut world, pos(8), pos(y)))
        .collect();

    for tick in 0..2_000 {
        world.step();
        if let Some((x, y)) = any_cell_inside_a_wall(&world) {
            panic!("the current drove a cell into the wall at ({x}, {y}) on tick {tick}");
        }
    }

    // And they are all still alive and *at* the wall rather than having been deleted or
    // stopped somewhere upstream — otherwise this passes for the wrong reason.
    let cells = world.cells();
    let arrived = ids
        .iter()
        .filter_map(|id| cells.index(*id))
        .filter(|i| mm_core::fixed::pos_to_square(cells.x[*i]) >= 14)
        .count();
    assert!(
        arrived >= 6,
        "only {arrived} of 8 cells reached the wall; they were stopped by something else"
    );
}

#[test]
fn a_sealed_room_holds_what_is_put_in_it() {
    // Four walls rather than one, because a cell escaping a box is the failure that a single
    // wall cannot show: it only takes one corner where the push points the wrong way.
    let mut scenario = walled_slide(
        0,
        CurrentField::Rotational {
            strength: Q10_ONE / 4,
        },
    );
    scenario.barriers = vec![
        Barrier::Rect {
            x: 8,
            y: 8,
            width: 16,
            height: 1,
        },
        Barrier::Rect {
            x: 8,
            y: 23,
            width: 16,
            height: 1,
        },
        Barrier::Rect {
            x: 8,
            y: 8,
            width: 1,
            height: 16,
        },
        Barrier::Rect {
            x: 23,
            y: 8,
            width: 1,
            height: 16,
        },
    ];
    let mut world = World::new(scenario).expect("world");

    let ids: Vec<CellId> = (10..22)
        .step_by(3)
        .flat_map(|x| (10..22).step_by(3).map(move |y| (x, y)))
        .map(|(x, y)| put_cell(&mut world, pos(x), pos(y)))
        .collect();

    for tick in 0..2_000 {
        world.step();
        if let Some((x, y)) = any_cell_inside_a_wall(&world) {
            panic!("a cell entered a wall of the room at ({x}, {y}) on tick {tick}");
        }
    }

    let cells = world.cells();
    for id in &ids {
        let Some(i) = cells.index(*id) else { continue };
        let (x, y) = (
            mm_core::fixed::pos_to_square(cells.x[i]),
            mm_core::fixed::pos_to_square(cells.y[i]),
        );
        assert!(
            (9..=22).contains(&x) && (9..=22).contains(&y),
            "a cell escaped the sealed room to ({x}, {y})"
        );
    }
}
