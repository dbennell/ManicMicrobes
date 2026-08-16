//! A wall is drawn as a wall, all the way from the world to the texels (`docs/UI.md` §7).
//!
//! `blocked` has been in the substrate since M1 and the renderer was never told about it, so a
//! barrier was visible only as an *absence* — `set_blocked` evicts the square's chemistry to its
//! neighbours and the light regime shadows behind it, which reads as a dark patch and is
//! indistinguishable from a square that merely has nothing in it. On an unlit slide it read as
//! nothing at all.
//!
//! This walks the whole path the renderer walks — `World` → [`Slide::frame`] → `art::paint_field`
//! → RGBA — and stops one step short of Bevy, which is exactly where `slide.rs` puts the wall so
//! that this is checkable on a machine with no display.

use mm_app::art;
use mm_app::slide::Slide;
use mm_core::{Barrier, LightRegime, Scenario};

fn walled_slide() -> Slide {
    let scenario = Scenario {
        name: "barrier view".to_string(),
        seed: 7,
        width: 16,
        height: 16,
        // Dark, deliberately. A lit slide would shadow behind the wall and could pass this by
        // accident — the old behaviour dressed up as the new one.
        light: LightRegime::Uniform { intensity: 0 },
        barriers: vec![Barrier::Rect {
            x: 4,
            y: 0,
            width: 1,
            height: 16,
        }],
        ..Scenario::default()
    };
    Slide::new(scenario).expect("slide")
}

#[test]
fn the_frame_carries_the_barrier_mask() {
    let mut slide = walled_slide();
    let frame = slide.frame();
    assert_eq!(
        frame.barriers.len(),
        (frame.width * frame.height) as usize,
        "the frame did not carry a mask the size of the grid"
    );
    let at = |x: u32, y: u32| frame.barriers[(y * frame.width + x) as usize];
    assert!(
        at(4, 0) && at(4, 8) && at(4, 15),
        "the wall is not in the mask"
    );
    assert!(!at(3, 8) && !at(5, 8), "open water was marked as wall");
}

#[test]
fn a_slide_with_no_barriers_carries_no_mask() {
    // Empty rather than all-false, so a slide without barriers pays neither the copy nor the
    // per-texel branch. At 512×512 that is a quarter of a megabyte a frame not being moved.
    let mut scenario = Scenario {
        width: 16,
        height: 16,
        ..Scenario::default()
    };
    scenario.barriers.clear();
    let mut slide = Slide::new(scenario).expect("slide");
    assert!(slide.frame().barriers.is_empty());
}

#[test]
fn the_wall_is_painted_and_the_water_beside_it_is_not() {
    let mut slide = walled_slide();
    let frame = slide.frame();
    let (w, h) = (frame.width as usize, frame.height as usize);
    let mut pixels = vec![0u8; w * h * 4];
    // No vignette, so what is left is the wall and nothing else.
    art::paint_barriers(&mut pixels, w, h, &frame.barriers, &frame.mineral, &|_, _| 1.0);

    let texel = |x: usize, y: usize| {
        let at = (y * w + x) * 4;
        [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
    };
    let wall = texel(4, 8);
    let water = texel(6, 8);

    assert_eq!(
        water[3], 0,
        "open water is not transparent, so it would hide the field under it"
    );
    assert_eq!(
        wall[3],
        art::WALL_BEDROCK,
        "a wall with no mineral in it did not come out as bedrock: {wall:?}. The alpha channel \
         carries which kind of wall a square is, not how much of one there is — see \
         `art::WALL_BEDROCK`"
    );
    assert!(
        wall[..3].iter().all(|c| *c > 20),
        "the wall came out as a hole rather than a wall: {wall:?}"
    );
    assert!(
        wall[2] > wall[0],
        "the wall should be cooler than the warm light it sits among: {wall:?}"
    );
}

#[test]
fn the_wall_layer_is_binary_which_is_what_makes_it_crisp() {
    // The property behind the sampler choice. Every texel is a wall or not, with no *partial*
    // coverage anywhere — so nearest sampling has nothing to lose, and the edge on screen falls
    // exactly on the square boundary the simulation put it at.
    //
    // The alpha takes one of two non-zero values rather than one, because it also says which
    // kind of wall the square is (`art::WALL_BEDROCK`, `art::WALL_MINERAL`). Both are opaque:
    // what matters here is that neither is *between* opaque and clear, which is the value a
    // sampler could smear and the world does not have.
    //
    // Painted through a *vignette* rather than a flat one, because the vignette scales the
    // colour and must not be allowed to leak into the coverage: a wall that faded out towards
    // the edge of the field would be a wall the sampler could smear again.
    let mut slide = walled_slide();
    let frame = slide.frame();
    let (w, h) = (frame.width as usize, frame.height as usize);
    let mut pixels = vec![0u8; w * h * 4];
    art::paint_barriers(&mut pixels, w, h, &frame.barriers, &frame.mineral, &|x, _| x / w as f32);

    for (i, chunk) in pixels.chunks_exact(4).enumerate() {
        assert!(
            chunk[3] == 0 || chunk[3] == art::WALL_BEDROCK || chunk[3] == art::WALL_MINERAL,
            "texel {i} has partial coverage {}, which a nearest sampler cannot represent \
             and a linear one would smear",
            chunk[3]
        );
        assert_eq!(
            chunk[3] != 0,
            frame.barriers[i],
            "texel {i} disagrees with the mask about whether it is a wall"
        );
    }
}

#[test]
fn drawing_a_barrier_changes_what_the_next_frame_shows() {
    // End to end through the tool, because the mask is gathered in `frame()` and a mask
    // captured once at construction would show the scenario's walls and never the user's.
    let mut scenario = Scenario {
        width: 16,
        height: 16,
        light: LightRegime::Uniform { intensity: 0 },
        ..Scenario::default()
    };
    scenario.barriers.clear();
    let mut slide = Slide::new(scenario).expect("slide");
    assert!(slide.frame().barriers.is_empty(), "started with walls");

    mm_app::tools::set_barrier(slide.world_mut(), 9, 9, true);
    let frame = slide.frame();
    assert!(
        frame.barriers[(9 * frame.width + 9) as usize],
        "a barrier drawn with the tool never reached the frame"
    );

    // Erasing the last one takes the slide back to carrying no mask at all, because
    // `Substrate::refresh_masks` recomputes `has_barriers` from the squares. That is the
    // stronger statement and the one worth asserting: the empty-mask path is not a
    // construction-time special case, it is reachable again at any time.
    mm_app::tools::set_barrier(slide.world_mut(), 9, 9, false);
    assert!(
        slide.frame().barriers.is_empty(),
        "erasing the last wall left a mask behind"
    );
}

#[test]
fn a_brush_stroke_lays_a_wall_of_the_width_it_says() {
    // The tool path, end to end: a stroke's worth of squares gathered the way `handle_mouse`
    // gathers them, applied through the batched world call, and measured off the frame.
    let mut scenario = Scenario {
        width: 32,
        height: 32,
        light: LightRegime::Uniform { intensity: 0 },
        ..Scenario::default()
    };
    scenario.barriers.clear();
    let mut slide = Slide::new(scenario).expect("slide");

    // A horizontal stroke across the middle, five squares wide.
    let mut squares: Vec<(u32, u32)> = Vec::new();
    for centre in mm_app::ui::line_squares((8, 16), (24, 16)) {
        for (x, y) in mm_app::ui::brush_squares(centre, 5) {
            if x >= 0 && y >= 0 {
                squares.push((x as u32, y as u32));
            }
        }
    }
    slide.world_mut().set_barriers(&squares, true);

    let frame = slide.frame();
    let at = |x: u32, y: u32| frame.barriers[(y * frame.width + x) as usize];
    // Five squares deep through the middle of the run, and open either side of that.
    for dy in 0..5u32 {
        assert!(
            at(16, 14 + dy),
            "the wall is thinner than five at row {}",
            14 + dy
        );
    }
    assert!(!at(16, 13) && !at(16, 19), "the wall is thicker than five");
    // And solid along its length rather than a row of discs with gaps between them.
    for x in 9..=23u32 {
        assert!(at(x, 16), "the stroke has a hole at x={x}");
    }
}

#[test]
fn the_eraser_can_take_back_what_the_pen_drew() {
    // Same width both ways, which is the point of one setting covering both: an eraser
    // narrower than the pen cannot undo its own stroke without repainting the gaps.
    let mut scenario = Scenario {
        width: 32,
        height: 32,
        ..Scenario::default()
    };
    scenario.barriers.clear();
    let mut slide = Slide::new(scenario).expect("slide");

    let stroke: Vec<(u32, u32)> = mm_app::ui::line_squares((6, 16), (26, 16))
        .into_iter()
        .flat_map(|c| mm_app::ui::brush_squares(c, 7))
        .filter(|(x, y)| *x >= 0 && *y >= 0)
        .map(|(x, y)| (x as u32, y as u32))
        .collect();

    slide.world_mut().set_barriers(&stroke, true);
    assert!(!slide.frame().barriers.is_empty(), "the pen drew nothing");

    slide.world_mut().set_barriers(&stroke, false);
    assert!(
        slide.frame().barriers.is_empty(),
        "the eraser left some of the wall behind"
    );
}

/// **The rock tool draws a wall that is made of something, and the picture says so.**
///
/// The barrier tool can only ever draw bedrock — a blocked square holding nothing, permanent
/// because there is nothing in it to dissolve. This is the other kind, end to end through the
/// stroke the front end actually makes: the brush's squares, the batched world call, the frame,
/// and the texels. It is one test rather than three because the failure it is guarding against
/// is the seam — rock that reaches the substrate and not the frame, or the frame and not the
/// alpha, looks exactly like rock that was never laid.
#[test]
fn a_stroke_of_rock_reaches_the_texels_as_rock() {
    let mut scenario = mm_core::Scenario {
        width: 32,
        height: 32,
        light: LightRegime::Uniform { intensity: 0 },
        ..Scenario::default()
    };
    scenario.barriers.clear();
    let mut slide = Slide::new(scenario).expect("slide");

    let silica = mm_core::chem::SOLID_CHEMICALS[1];
    let squares: Vec<(u32, u32)> = mm_app::ui::line_squares((8, 16), (24, 16))
        .into_iter()
        .flat_map(|c| mm_app::ui::brush_squares(c, 3))
        .filter(|(x, y)| *x >= 0 && *y >= 0)
        .map(|(x, y)| (x as u32, y as u32))
        .collect();
    let dose = slide.world().rock_dose();
    let laid = slide.world_mut().set_rock(&squares, silica, dose);
    assert!(laid > 0, "the stroke laid no mineral at all");

    let frame = slide.frame();
    let (w, h) = (frame.width as usize, frame.height as usize);
    let at = |x: u32, y: u32| (y * frame.width + x) as usize;
    assert!(
        frame.barriers[at(16, 16)],
        "the rock never became a wall the frame knows about"
    );
    assert!(
        frame.mineral[at(16, 16)].iter().sum::<f32>() > 0.0,
        "the wall carries no mineral colour, so it would be painted as plain bedrock"
    );

    let mut pixels = vec![0u8; w * h * 4];
    art::paint_barriers(&mut pixels, w, h, &frame.barriers, &frame.mineral, &|_, _| 1.0);
    let texel = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    };
    assert_eq!(
        texel(16, 16)[3],
        art::WALL_MINERAL,
        "the texel says bedrock, so `rock.wgsl` leaves it flat and a reef looks like a hole in \
         the world rather than like stone"
    );
    // And beside the stroke is still water, which is the half that catches a tool painting the
    // whole slide.
    assert_eq!(texel(16, 22)[3], 0, "open water was painted as rock");
}

/// The acidity overlay: derived, absolute, and reachable the same way every chemical's is.
///
/// pH has no plane to switch on, so it rides at the end of the overlay space — which means the
/// legend, the number keys and the bitmask all reach it with no special case, and adding it
/// renumbered nothing.
#[test]
fn the_acidity_overlay_reads_the_water_and_not_a_plane() {
    use mm_app::slide::ACIDITY;
    let scenario = Scenario {
        width: 16,
        height: 16,
        seeding: vec![
            mm_core::Seeding::Uniform {
                chemical: mm_core::chem::CARBON_DIOXIDE,
                per_square: mm_core::fixed::q10(400),
            },
            mm_core::Seeding::Uniform {
                chemical: mm_core::chem::CARBONATE,
                per_square: mm_core::fixed::q10(400),
            },
        ],
        ..Scenario::default()
    };
    let mut slide = Slide::new(scenario).expect("slide");
    // The fresh slide opens with carbon dioxide on; what matters here is that acidity is not
    // among them until it is asked for.
    assert!(
        !slide
            .frame()
            .overlays
            .iter()
            .any(|l| l.chemical == ACIDITY),
        "acidity was on before anything switched it on"
    );
    // Named and coloured alongside the chemicals, so nothing that walks that list needs to know.
    assert_eq!(slide.chemical_names().len(), mm_core::chem::CHEM_COUNT + 1);
    assert_eq!(slide.chemical_names()[ACIDITY], "acidity");
    assert_eq!(slide.chemical_colours().len(), mm_core::chem::CHEM_COUNT + 1);

    slide.toggle_overlay(ACIDITY);
    let frame = slide.frame();
    let layer = frame
        .overlays
        .iter()
        .find(|l| l.chemical == ACIDITY)
        .expect("the acidity layer");
    assert_eq!(layer.field.len(), 16 * 16);
    // Matched pools read neutral, and neutral is the bottom of an acidity ramp.
    assert!(
        layer.field.iter().all(|v| *v == 0.0),
        "neutral water was drawn as acid"
    );

    // Sour it, and the layer says so.
    for y in 0..16i32 {
        for x in 0..16i32 {
            slide
                .world_mut()
                .substrate_mut()
                .set_chem(mm_core::chem::CARBONATE, x, y, 0);
        }
    }
    let frame = slide.frame();
    let layer = frame
        .overlays
        .iter()
        .find(|l| l.chemical == ACIDITY)
        .expect("the acidity layer");
    assert!(
        layer.field.iter().all(|v| *v > 0.9),
        "water with no buffer at all was not drawn as sour"
    );
    // Absolute, not eased against the frame: pH is a nought-to-fourteen scale, and a ramp
    // normalised per frame would make "sour" mean whatever the sourest square happened to be.
    assert_eq!(layer.scale, mm_core::chem::PH_NEUTRAL);
}
