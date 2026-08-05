//! What a frame costs at fifty thousand cells, and which tier costs it.
//!
//! Run with `cargo test -p mm-app --release --test frame_cost -- --ignored --nocapture`.
//!
//! `mm-core`'s population gate measures the *tick*. Nothing measured the frame, and the frame is
//! half of what "fifty thousand cells at thirty frames a second" means — and the more awkward
//! half, because [`Slide::frame`] runs on the simulation thread, under the same lock a tick
//! takes. A frame that costs 20 ms does not merely arrive late; it is 20 ms the world is not
//! being stepped in.
//!
//! The population comes from the same cache `mm-core`'s `population` bench grows, so both halves
//! are measured on one slide. Grow it with `cargo bench -p mm-core --bench population` if it is
//! not there.

use std::time::Instant;

use mm_app::slide::{Lod, Slide};
use mm_core::Scenario;

fn cached_world() -> Option<mm_core::World> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/bench-cache/ancestor_mm-1-50000.mmslide");
    let bytes = std::fs::read(&path).ok()?;
    match mm_core::Snapshot::read(&bytes) {
        Ok(world) => Some(world),
        Err(e) => {
            eprintln!("cache unusable ({e:?}); grow it with `cargo bench -p mm-core`");
            None
        }
    }
}

/// What one published frame costs, at each level of detail.
///
/// The zooms are the tier thresholds `Lod::for_zoom` uses, in pixels per substrate square, plus
/// one either side — because the interesting number is not any single tier but the step between
/// them, which is what a person crossing it with the scroll wheel feels.
#[test]
#[ignore = "diagnostic; run with --release --ignored --nocapture"]
fn what_a_frame_costs_at_fifty_thousand_cells() {
    let Some(world) = cached_world() else {
        eprintln!("no cached population; skipping");
        return;
    };
    let population = world.cells().len();
    let (w, h) = (
        world.substrate().width() as f32,
        world.substrate().height() as f32,
    );
    let mut slide = Slide::new(Scenario::stress(8, 8)).expect("slide");
    slide.set_world(world);

    // A tick, for scale: the frame is only interesting against what it is competing with.
    slide.advance(4);
    let n = 12;
    let t = Instant::now();
    for _ in 0..n {
        slide.advance(1);
    }
    let tick = t.elapsed() / n;

    eprintln!("\n{population} cells on a {w:.0}x{h:.0} slide, {} threads", rayon::current_num_threads());
    eprintln!("  one tick:  {:>8.2} ms   ({:.1} ticks/s)", tick.as_secs_f64() * 1e3, 1.0 / tick.as_secs_f64());
    eprintln!(
        "\n  {:>7} {:>12} {:>10} {:>9} {:>10}   what it is",
        "px/sq", "frame", "of a tick", "cells", "lod"
    );
    for zoom in [2.5f32, 6.0, 12.0, 28.0, 48.0, 120.0] {
        // The camera covers the window at that zoom: a 1280x720 window, which is what the
        // application opens at, so `visible` culls the way it really culls.
        let (half_w, half_h) = (640.0 / zoom, 360.0 / zoom);
        slide.set_camera(w / 2.0, h / 2.0, half_w, half_h);
        slide.set_zoom(zoom);
        // Once to warm whatever the first call allocates, then measured.
        let frame = slide.frame();
        let (lod, drawn) = (frame.lod, frame.cells.len());
        let n = 8;
        let t = Instant::now();
        for _ in 0..n {
            std::hint::black_box(slide.frame());
        }
        let per = t.elapsed() / n;
        eprintln!(
            "  {zoom:>7.1} {:>10.2} ms {:>8.0}% {:>9} {:>10}   {}",
            per.as_secs_f64() * 1e3,
            100.0 * per.as_secs_f64() / tick.as_secs_f64(),
            drawn,
            format!("{lod:?}"),
            match lod {
                Lod::Dots => "position and colour only",
                Lod::Packed => "+ seams, for the cells on camera",
                Lod::Organelles => "+ organelle rings",
                Lod::Full => "+ membranes and junctions",
            }
        );
    }
    eprintln!(
        "\n  Thirty frames a second is 33.3 ms for a frame *and* a tick together, because\n  \
         `Slide::frame` runs on the simulation thread under the lock a tick takes."
    );
}
