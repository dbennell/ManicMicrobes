//! **Nothing suspended in the water is drawn on the far side of a wall from the water holding it.**
//!
//! The specks of detritus and the flakes of carrion are not particles. There is no object in the
//! simulation behind any of them — both are chemical fields, and what the stipple says is
//! *density* (`art::speck`). But a picture of a density is still a claim about **where**, and the
//! claim it was making was false: the motes were scattered over a lattice four squares coarse and
//! then carried along one velocity sample for a whole life, with nothing consulting the barrier
//! layout at either step. So a block straddling a wall scattered specks into the half of it that
//! is stone, and a flake beside a wall was carried clean over it.
//!
//! Seen on the slide as particulate flowing through a sealed reef while the overlay showed the
//! concentration piling up behind it — the fluid solver and the picture disagreeing about whether
//! there was a wall there, with the solver right.
//!
//! The fixture is the one that makes it unarguable: **a sealed room**, four walls two squares
//! thick with no gap in them, particulate seeded inside and the water outside stirred. Anything
//! drawn outside the room came from the drawing and not from the world, because there is no path
//! through which the world could have put it there. `docs/UI.md` §1.

use mm_app::slide::{Frame, Particulate, Slide};
use mm_core::{Barrier, CurrentField, LightRegime, Scenario, Seeding};

/// The room, in squares: `x0..x1`, `y0..y1` inclusive of the walls themselves.
const ROOM: (u32, u32, u32, u32) = (24, 24, 71, 71);

fn sealed_room() -> Slide {
    let (x0, y0, x1, y1) = ROOM;
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let scenario = Scenario {
        name: "the box".to_string(),
        seed: 20260816,
        width: 96,
        height: 96,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        // Stirred, so there is a velocity to carry a mote anywhere at all. Still water is the
        // case that passes by accident: nothing drifts, so nothing drifts through a wall.
        current: CurrentField::Rotational { strength: 96 },
        fluid_interval: 1,
        seeding: vec![
            // Both suspended fields, inside the room and nowhere else.
            Seeding::Patch {
                chemical: mm_core::ecology::DETRITUS,
                x: x0 + 2,
                y: y0 + 2,
                width: w - 4,
                height: h - 4,
                per_square: mm_core::fixed::q10(400),
            },
            Seeding::Patch {
                chemical: mm_core::ecology::CARRION,
                x: x0 + 2,
                y: y0 + 2,
                width: w - 4,
                height: h - 4,
                per_square: mm_core::fixed::q10(400),
            },
        ],
        barriers: vec![
            Barrier::Rect { x: x0, y: y0, width: w, height: 2 },
            Barrier::Rect { x: x0, y: y1 - 1, width: w, height: 2 },
            Barrier::Rect { x: x0, y: y0, width: 2, height: h },
            Barrier::Rect { x: x1 - 1, y: y0, width: 2, height: h },
        ],
        // No inhabitants. Cells in the room would eat some of what is seeded and shed more of it
        // when they die, which is the slide the bug was seen on and is noise here: what is under
        // test is where a mote may be *drawn*, and that is decided by the fields and the walls.
        ..Scenario::default()
    };
    Slide::new(scenario).expect("slide")
}

/// Inside the room, walls excluded — where the world's particulate actually is.
fn inside(x: f32, y: f32) -> bool {
    let (x0, y0, x1, y1) = ROOM;
    x > (x0 + 2) as f32 && x < (x1 - 1) as f32 && y > (y0 + 2) as f32 && y < (y1 - 1) as f32
}

/// Every mote of both fields, over the range of screen densities the camera can ask for.
///
/// Both halves of the density pair, because they are different code paths: `skip` thins the
/// lattice when blocks crowd and `per_side` fills a block when they spread, and a mote that
/// escapes at one magnification need not escape at another.
fn every_mote(frame: &Frame) -> Vec<(Particulate, f32, f32)> {
    let mut out = Vec::new();
    for kind in [Particulate::Detritus, Particulate::Carrion] {
        for (skip, per_side) in [(1, 1), (1, 8), (2, 1), (4, 1), (1, 4)] {
            for m in frame.drifting(kind, skip, per_side) {
                out.push((kind, m.x, m.y));
            }
        }
    }
    out
}

#[test]
fn nothing_drifts_out_of_a_sealed_room() {
    let mut slide = sealed_room();
    // Long enough for the current to have carried a mote as far as it is ever going to: a flake
    // is carried for `art::FLECK_LIFE` ticks before it restarts, and the tick the frame is taken
    // at decides how far through that life every mote on the slide is.
    for run in 0..12 {
        slide.world_mut().run(97);
        let frame = slide.frame();
        assert!(
            !frame.barriers.is_empty(),
            "the slide is not carrying a barrier mask, so this test cannot fail"
        );
        let escaped: Vec<_> = every_mote(&frame)
            .into_iter()
            .filter(|(_, x, y)| !inside(*x, *y))
            .collect();
        assert!(
            escaped.is_empty(),
            "run {run}, tick {}: {} motes drawn outside a sealed room. The particulate is a \
             picture of a field the world keeps entirely inside those walls, so every one of \
             these is a speck the simulation never put there. First few: {:?}",
            slide.world().tick_count(),
            escaped.len(),
            &escaped[..escaped.len().min(6)],
        );
    }
}

/// And the picture is not simply empty, which is the way this test could pass for nothing.
#[test]
fn the_room_is_full_of_them() {
    let mut slide = sealed_room();
    slide.world_mut().run(400);
    let frame = slide.frame();
    for kind in [Particulate::Detritus, Particulate::Carrion] {
        let motes = frame.drifting(kind, 1, 4);
        assert!(
            motes.len() > 50,
            "{kind:?}: only {} motes drawn inside a room seeded thick with it — a wall test that \
             draws nothing passes without meaning anything",
            motes.len()
        );
    }
}

/// A slide with no walls draws the mote where the drift puts it, and does not lose it.
///
/// The empty mask is a real state — `Frame::barriers` is empty rather than all-false on a slide
/// with nothing on it, so a world without walls pays neither the copy nor the lookup — and a
/// path test that read an empty mask as "everything is stone" would silently delete the
/// particulate from every ordinary slide.
#[test]
fn a_slide_with_no_walls_keeps_its_particulate() {
    let scenario = Scenario {
        width: 32,
        height: 32,
        current: CurrentField::Rotational { strength: 96 },
        fluid_interval: 1,
        seeding: vec![Seeding::Uniform {
            chemical: mm_core::ecology::DETRITUS,
            per_square: mm_core::fixed::q10(400),
        }],
        barriers: Vec::new(),
        ..Scenario::default()
    };
    let mut slide = Slide::new(scenario).expect("slide");
    slide.world_mut().run(200);
    let frame = slide.frame();
    assert!(frame.barriers.is_empty(), "the fixture grew walls");
    assert!(
        !frame.drifting(Particulate::Detritus, 1, 4).is_empty(),
        "an empty barrier mask was read as a slide made of rock"
    );
}

/// The path test itself, on a wall one square thick.
///
/// One square is the hard case and the reason the walk is a supercover rather than a sampled
/// line: a flake covers several squares between one frame and the next, so a test that stepped
/// the segment at any fixed rate would step straight over a wall this thin. Diagonally too,
/// where a naive line walk cuts the corner between two squares that are both stone.
#[test]
fn a_wall_one_square_thick_is_not_stepped_over() {
    let scenario = Scenario {
        width: 16,
        height: 16,
        barriers: vec![Barrier::Rect {
            x: 8,
            y: 0,
            width: 1,
            height: 16,
        }],
        ..Scenario::default()
    };
    let mut slide = Slide::new(scenario).expect("slide");
    let frame = slide.frame();

    assert!(
        frame.carried((2.5, 8.5), (7.5, 8.5)),
        "open water on one side of the wall was called impassable"
    );
    assert!(
        !frame.carried((2.5, 8.5), (12.5, 8.5)),
        "a mote crossed a wall one square thick in a straight line"
    );
    assert!(
        !frame.carried((6.5, 2.5), (11.5, 12.5)),
        "a mote crossed the wall diagonally"
    );
    assert!(
        !frame.carried((8.5, 4.5), (8.5, 9.5)),
        "a mote born inside the wall was carried along it"
    );
    assert!(
        !frame.carried((2.5, 2.5), (-1.5, 2.5)),
        "a mote was carried off the edge of the slide, where the world does not reach"
    );
}
