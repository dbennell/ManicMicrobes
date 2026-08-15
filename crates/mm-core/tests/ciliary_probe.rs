//! Whether a ciliate can hold on and pump — `docs/FEEDING.md` §7's probe, and the guard on it.
//!
//! # The question
//!
//! §7 makes the flagellum's case conditional and says so outright: *"it is possible that a
//! ciliated, anchored cell already generates measurable slip and already filter-feeds on its own
//! current. If it does, the flagellum's job shrinks to being the propulsive counterpart and the
//! case for the slot is weaker. That is one probe, and it comes before any of this."*
//!
//! The answer, when it was first run, was **no** — and the reason was not the cilium and not the
//! filter. `step_physics` advances a body by `velocity + drift`, and the holdfast was offered
//! only the `drift`. A gripping cell with two cilia at full power travelled **twenty-four
//! squares in four hundred ticks** while an identical cell holding station against a
//! quarter-speed current moved half a square. A cell could beat its way off its own anchor for
//! nothing, so the one arrangement §7 is about could not be assembled.
//!
//! Grip now resists the *net* of the two. This file is what says it still does.
//!
//! # Why the movement column is an assertion and not a print
//!
//! `ecology::captured` reads `|v_water - v_cell|` and **cannot tell a sessile pump from a
//! swimmer ram-feeding** — that symmetry is deliberate and documented on the function. So a
//! filtering number on its own proves nothing about sessility, and the first run of this probe
//! reported a beating cell earning 0.86 of a current while it swam clean across the slide.
//! Whatever else changes here, the cell must not go anywhere.

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::light::CurrentField;
use mm_core::{Barrier, LightRegime, Organelle, OrganelleType, Scenario, Seeding, World, Q10_ONE};

const SIDE: u32 = 48;
const TICKS: u64 = 400;

/// What one arm of the probe reported.
struct Arm {
    /// Detritus taken out of the water over the window, `Q10`.
    filtered: i64,
    /// How far the cell travelled, in squares. The control that decides what was measured.
    moved: f64,
}

/// One cell, gripping a floor, with two cilia at `cilium_power`, in `current`.
fn arm(current: CurrentField, cilium_power: i16) -> Arm {
    let mut world = World::new(Scenario {
        name: "ciliary probe".to_string(),
        seed: 99,
        width: SIDE,
        height: SIDE,
        light: LightRegime::Uniform {
            intensity: Q10_ONE,
        },
        current,
        // Detritus everywhere, so the answer is about the flux past the cell and never about
        // the supply running out under it.
        seeding: vec![Seeding::Uniform {
            chemical: mm_core::ecology::DETRITUS,
            per_square: q10(4_000),
        }],
        // A floor to hold on to. The grip is gated on `touches_barrier` — a holdfast is an
        // attachment and there has to be something to attach to — so without this the arms
        // called "anchored" are nothing of the kind.
        barriers: vec![Barrier::Rect {
            x: 0,
            y: SIDE - 2,
            width: SIDE,
            height: 2,
        }],
        ..Scenario::default()
    })
    .expect("world");

    // A genome that does nothing, so the control words stay where this test puts them and the
    // behaviour measured is the probe's rather than some genome's.
    let genome = world
        .genomes()
        .intern(mm_asm::assemble("HALT\n").expect("assembles").bytes)
        .expect("interned");
    let id = world.spawn_cell(CellSeed {
        x: pos(i32::try_from(SIDE / 2).unwrap_or(24)),
        y: pos(i32::try_from(SIDE - 3).unwrap_or(45)),
        mass: q10(40),
        // Enough that upkeep cannot end the window early: this is about what a filter catches,
        // not about whether the cell can afford to be one.
        energy: q10(1_000_000),
        membrane: 48,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    let i = world.cells_mut().index(id).expect("the cell exists");
    {
        let cells = world.cells_mut();
        // Grip and filter effort are one control word on purpose — see `ecology::filter_strength`.
        let mut hold = Organelle::finished(OrganelleType::Holdfast, 200);
        hold.control[0] = Q10_ONE as i16;
        cells.slots_mut(i)[1] = hold;
        // Two cilia on perpendicular mounts, as every swimming genome in the library builds them.
        for (slot, mount) in [(2usize, 0i16), (3usize, 12i16)] {
            let mut cil = Organelle::finished(OrganelleType::Cilium, 200);
            cil.control[0] = cilium_power;
            cil.control[1] = mount;
            cells.slots_mut(i)[slot] = cil;
        }
    }

    let start = (world.cells().x[i], world.cells().y[i]);
    let mut filtered = 0i64;
    for _ in 0..TICKS {
        world.step();
        filtered += world.report().ecology.filtered;
    }
    let cells = world.cells();
    let (dx, dy) = (cells.x[i] - start.0, cells.y[i] - start.1);
    Arm {
        filtered,
        moved: f64::from(dx.abs() + dy.abs()) / f64::from(mm_core::POS_ONE),
    }
}

/// A gripping cell does not travel, however hard it beats.
///
/// This is the regression the change was made for. Before it, this arm moved twenty-four squares.
#[test]
fn a_holdfast_holds_against_the_cell_it_is_attached_to() {
    let pump = arm(CurrentField::Still, Q10_ONE as i16);
    assert!(
        pump.moved < 1.0,
        "a gripping cell with cilia at full power travelled {:.2} squares; a holdfast that \
         resists the water but not its own body is not an anchor",
        pump.moved
    );
}

/// And the point of holding on: the thrust goes into the water instead, and comes back as food.
///
/// The comparison is against a cell of the same build holding station in a current, which is the
/// living the holdfast was written for. A pump within a factor of two of that is the sessile
/// ciliary suspension feeder of SPEC §17.6, assembled from parts each built for something else.
#[test]
fn beating_while_anchored_is_a_living() {
    let pump = arm(CurrentField::Still, Q10_ONE as i16);
    let current = arm(
        CurrentField::Uniform {
            vx: Q10_ONE / 4,
            vy: 0,
        },
        0,
    );
    assert!(
        pump.filtered > 0,
        "an anchored cell beating its cilia filtered nothing: it is not pumping"
    );
    assert!(
        pump.filtered * 2 > current.filtered && current.filtered * 2 > pump.filtered,
        "pumping its own water earned {} against {} for holding station in a current; \
         `ecology::captured` claims those are the same expression read from two ends",
        pump.filtered,
        current.filtered
    );
}

/// Nothing moving past means nothing caught — the floor the other two are measured against.
///
/// Sharper than it looks. Before the grip resisted the cell's own motion this arm still caught a
/// steady trickle, because Brownian jitter gave a stationary cell a relative speed it had not
/// earned. A held cell is now genuinely held, and a filter in still water is a filter in still
/// water.
#[test]
fn a_held_cell_in_still_water_catches_nothing() {
    let idle = arm(CurrentField::Still, 0);
    assert_eq!(
        idle.filtered, 0,
        "a motionless cell in motionless water caught {}; capture is a flux and there is no flux",
        idle.filtered
    );
    assert!(idle.moved < 1.0, "it moved {:.2} squares", idle.moved);
}
