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
    let slide = walled_slide();
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
    let slide = Slide::new(scenario).expect("slide");
    assert!(slide.frame().barriers.is_empty());
}

#[test]
fn the_wall_is_painted_and_the_water_beside_it_is_not() {
    let slide = walled_slide();
    let frame = slide.frame();
    let (w, h) = (frame.width as usize, frame.height as usize);
    let mut pixels = vec![0u8; w * h * 4];
    // No vignette, so what is left is the wall and nothing else.
    art::paint_barriers(&mut pixels, w, h, &frame.barriers, &|_, _| 1.0);

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
    assert_eq!(wall[3], 255, "the wall is not opaque: {wall:?}");
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
    // The property behind the sampler choice. Every texel is fully a wall or fully not, with
    // no partial alpha anywhere — so nearest sampling has nothing to lose, and the edge on
    // screen falls exactly on the square boundary the simulation put it at.
    //
    // Painted through a *vignette* rather than a flat one, because the vignette scales the
    // colour and must not be allowed to leak into the coverage: a wall that faded out towards
    // the edge of the field would be a wall the sampler could smear again.
    let slide = walled_slide();
    let frame = slide.frame();
    let (w, h) = (frame.width as usize, frame.height as usize);
    let mut pixels = vec![0u8; w * h * 4];
    art::paint_barriers(&mut pixels, w, h, &frame.barriers, &|x, _| x / w as f32);

    for (i, chunk) in pixels.chunks_exact(4).enumerate() {
        assert!(
            chunk[3] == 0 || chunk[3] == 255,
            "texel {i} has partial coverage {}, which a nearest sampler cannot represent \
             and a linear one would smear",
            chunk[3]
        );
        assert_eq!(
            chunk[3] == 255,
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
