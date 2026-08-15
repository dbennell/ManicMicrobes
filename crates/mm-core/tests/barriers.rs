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
use mm_core::{Barrier, Organelle, OrganelleType, Scenario, World};

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
        badge: 0,
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

// ---------------------------------------------------------------------------
// The holdfast (SPEC §17.6). A barrier is something to hold on to.
// ---------------------------------------------------------------------------

/// Give a cell a holdfast of the given `param`, already finished and gripping at full effort.
fn anchor(world: &mut World, id: CellId, param: u8) {
    let Some(i) = world.cells_mut().index(id) else {
        return;
    };
    // Gripping, asked for. A holdfast's control word now starts at zero like every other
    // control that acts on the world — an organelle nobody wired up is a cost, not a free
    // action — so a test about *holding on* has to say so.
    let mut hold = Organelle::finished(OrganelleType::Holdfast, param);
    hold.control[0] = mm_core::Q10_ONE as i16;
    world.cells_mut().slots_mut(i)[4] = hold;
}

/// How far a cell has travelled from where it was put, along each axis.
fn drifted(world: &World, id: CellId, from_x: i32) -> i32 {
    world
        .cells()
        .index(id)
        .map_or(0, |i| world.cells().x[i] - from_x)
}

fn drifted_y(world: &World, id: CellId, from_y: i32) -> i32 {
    world
        .cells()
        .index(id)
        .map_or(0, |i| world.cells().y[i] - from_y)
}

#[test]
fn a_holdfast_holds_a_cell_against_the_current_and_bare_water_does_not() {
    // The current runs *along* the wall, not into it, and that is the whole point rather than
    // an incidental choice of axis. A current blowing into a wall is stopped by the wall: the
    // collision pass alone pins both cells and the anchor is invisible. Tangentially there is
    // no constraint at all — a cell against a wall slides down it freely — so this is the one
    // direction in which holding station is a thing only a holdfast can do.
    //
    // The first version of this test pushed into the wall and passed for both cells at exactly
    // 128 `POS`, which is the distance to the wall face. It was measuring the barrier.
    // One cell per world, same square, same current, so the only difference is the anchor.
    // Sixty ticks: at a quarter of a square each, a free cell covers fifteen squares, which is
    // well clear of the anchored one and well short of the slide's floor. An earlier version
    // ran for four hundred and both cells hit the bottom edge and clamped, which made the
    // *free* one look like the one that had stopped.
    fn slid(param: Option<u8>) -> i32 {
        let mut world = World::new(walled_slide(
            16,
            CurrentField::Uniform {
                vx: 0,
                vy: Q10_ONE / 4,
            },
        ))
        .expect("world");
        let (x, y) = (pos(15) + 128, pos(4));
        let id = put_cell(&mut world, x, y);
        if let Some(p) = param {
            anchor(&mut world, id, p);
        }
        for _ in 0..60 {
            world.step();
        }
        drifted_y(&world, id, y).abs()
    }

    let (held_moved, adrift_moved) = (slid(Some(200)), slid(None));
    eprintln!("anchored slid {held_moved} POS, free slid {adrift_moved} POS");
    assert!(
        held_moved * 4 < adrift_moved.max(1),
        "the anchored cell slid {held_moved} along the wall and the free one {adrift_moved}; \
         the holdfast is not holding"
    );
}

#[test]
fn a_holdfast_with_nothing_to_grip_holds_nothing() {
    // It anchors to a barrier, not to the water. Without this a holdfast would be a general
    // brake on the current — which would make it useful everywhere, and useful everywhere is
    // exactly what SPEC §17.6 says a sessile strategy must not be.
    let mut scenario = walled_slide(
        16,
        CurrentField::Uniform {
            vx: Q10_ONE / 4,
            vy: 0,
        },
    );
    scenario.barriers.clear();
    let mut world = World::new(scenario).expect("world");

    let start = pos(4);
    let gripping = put_cell(&mut world, start, pos(10));
    let bare = put_cell(&mut world, start, pos(20));
    anchor(&mut world, gripping, 255);

    for _ in 0..200 {
        world.step();
    }
    assert_eq!(
        drifted(&world, gripping, start),
        drifted(&world, bare, start),
        "a holdfast slowed a cell in open water, with no barrier anywhere on the slide"
    );
}

#[test]
fn a_bigger_cell_is_harder_to_hold() {
    // The load term. A larger body presents more of itself to the current, so the same grip
    // buys less holding — which is what stops a sponge growing without limit, and is the same
    // frontal-area reasoning particulate capture will want.
    fn slip_at(mass: i32) -> i32 {
        let mut world = World::new(walled_slide(
            16,
            CurrentField::Uniform {
                vx: Q10_ONE / 4,
                vy: 0,
            },
        ))
        .expect("world");
        let start = pos(15) + 128;
        let genome = world.genomes().intern(vec![0x2E]).expect("genome");
        let id = world.cells_mut().spawn(CellSeed {
            x: start,
            y: pos(10),
            mass: q10(mass),
            energy: q10(100_000),
            membrane: 16,
            key: 0,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        // Deliberately under-powered, so both are slipping and the comparison is of *how much*.
        anchor(&mut world, id, 24);
        for _ in 0..400 {
            world.step();
        }
        drifted(&world, id, start).abs()
    }

    let (small, large) = (slip_at(30), slip_at(360));
    assert!(
        large > small,
        "a cell of mass 360 slipped {large} and one of mass 30 slipped {small}; \
         size is not costing anything to hold"
    );
}

#[test]
fn holding_on_costs_energy_and_letting_go_is_free() {
    // Charged on the force resisted, so the same anchor in still water is free. Without that
    // asymmetry a holdfast is a flat tax on having built one, and the decision a genome makes
    // each tick — hold or let go — would not be a decision.
    fn energy_after(current: CurrentField, effort: i16) -> i32 {
        let mut world = World::new(walled_slide(16, current)).expect("world");
        let start = pos(15) + 128;
        let id = put_cell(&mut world, start, pos(10));
        anchor(&mut world, id, 200);
        if let Some(i) = world.cells_mut().index(id) {
            world.cells_mut().slots_mut(i)[4].control[0] = effort;
        }
        for _ in 0..200 {
            world.step();
        }
        world
            .cells()
            .index(id)
            .map_or(0, |i| world.cells().energy[i])
    }

    let flowing = || CurrentField::Uniform {
        vx: Q10_ONE / 4,
        vy: 0,
    };
    let holding = energy_after(flowing(), Q10_ONE as i16);
    let let_go = energy_after(flowing(), 0);
    let still = energy_after(CurrentField::Still, Q10_ONE as i16);

    assert!(
        holding < let_go,
        "holding on ({holding}) cost no more than letting go ({let_go})"
    );
    assert!(
        still >= let_go,
        "gripping in still water ({still}) cost something; the charge is not on the force \
         actually resisted"
    );
}
