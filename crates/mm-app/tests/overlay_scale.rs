//! What sets the chemical overlay's scale, and how much it moves between frames.
//!
//! The overlay normalises each square against the plane's own peak, so the scale is a statistic
//! of the frame and the whole picture's brightness is that statistic's reciprocal. When the
//! statistic jumps, every square on the slide changes shade at once — which is seen as the field
//! flickering, and it is not the field: it is the ruler.
//!
//! The measurement is here rather than argued about because the two candidate causes look
//! identical on screen. A scale that moves because the *world* changed is honest and wants
//! smoothing; a scale that moves because one square out of a quarter of a million spiked is a
//! bad statistic and wants a better one. Only the numbers say which.
//!
//! Run the measurement with:
//!
//! ```text
//! cargo test -p mm-app --test overlay_scale --release -- --ignored --nocapture
//! ```

use mm_app::slide::Slide;
use mm_core::biology::BiologyConfig;

use mm_core::fixed::{q10, Q10_ONE};
use mm_core::{LightRegime, MutationRates, Scenario, Seeding, World};

/// A slide with a population growing on it — the condition the flicker was reported in.
///
/// It has to be *alive*. A diffusing field with nobody in it relaxes towards uniform and its
/// peak decays smoothly; the spikes come from cells, which take matter out of one square and put
/// it back into another when they divide, die or excrete.
fn living(size: u32) -> World {
    let sc = Scenario {
        name: "overlay scale".into(),
        seed: 1,
        width: size,
        height: size,
        light: LightRegime::Uniform { intensity: Q10_ONE },
        seeding: vec![
            Seeding::Uniform { chemical: 4, per_square: q10(400) },
            Seeding::Uniform { chemical: 11, per_square: q10(400) },
            Seeding::Uniform { chemical: 14, per_square: q10(400) },
        ],
        ..Scenario::default()
    };
    let mut world = World::new(sc).expect("a world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::default(),
        ..BiologyConfig::default()
    });
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm"),
    )
    .expect("ancestor.mm");
    let bytes = mm_asm::assemble(&src).expect("assembles").bytes;
    world.place_founders(&bytes, 4);
    world
}

/// The value `share` of the way up the sorted plane. `1.0` is the maximum.
fn quantile(sorted: &[i32], share: f64) -> i32 {
    if sorted.is_empty() {
        return 0;
    }
    let at = ((sorted.len() - 1) as f64 * share).round() as usize;
    sorted[at.min(sorted.len() - 1)]
}

/// How much a series jumps from one reading to the next, as a fraction — the quantity the eye
/// is actually complaining about.
///
/// Reported as the worst single step and the mean step, because they answer different
/// questions: the worst is whether there is a visible flash at all, and the mean is whether the
/// picture is restless the whole time.
fn steps(series: &[i32]) -> (f64, f64) {
    let mut worst = 0.0f64;
    let mut total = 0.0f64;
    let mut n = 0usize;
    for pair in series.windows(2) {
        let (a, b) = (f64::from(pair[0]).max(1.0), f64::from(pair[1]).max(1.0));
        // Symmetric, so a halving and a doubling count the same: both are one stop of exposure.
        let step = (a.max(b) / a.min(b)) - 1.0;
        worst = worst.max(step);
        total += step;
        n += 1;
    }
    (worst, if n > 0 { total / n as f64 } else { 0.0 })
}

/// The regression test, against the shipped frame path rather than a reimplementation of it.
///
/// The maximum moved 43.8% in a single tick on this slide and averaged 5.36%; through the
/// square-root curve that is a 17% change in the brightness of every texel at once, which is
/// what was reported as the field flickering. The thresholds here are far above what the fix
/// measures and far below what the bug did — the point is to fail if the scale ever goes back to
/// being an extreme-value statistic, not to pin a number that will drift with the biology.
#[test]
fn the_overlay_scale_does_not_lurch_between_frames() {
    let world = living(96);
    let mut slide = Slide::new(world.scenario().clone()).expect("a slide");
    slide.set_world(world);
    slide.set_overlay(4);
    slide.world_mut().run(3_000);

    // The first frame after a world is set takes its reading outright, so it is not a step and
    // is not measured — it is what the easing is measured *from*.
    let mut scales = vec![slide.frame().overlays[0].scale];
    for _ in 0..200 {
        slide.world_mut().run(1);
        let f = slide.frame();
        let layer = &f.overlays[0];
        assert_eq!(layer.chemical, 4);
        assert!(layer.scale > 0, "the ramp must never divide by zero");
        assert!(
            layer.field.iter().all(|v| (0.0..=1.0).contains(v)),
            "a normalised field escaped 0..=1, which the painter's curve assumes"
        );
        scales.push(layer.scale);
    }

    let (worst, mean) = steps(&scales);
    assert!(
        worst < 0.05,
        "the overlay scale jumped {:.1}% in one frame; it is supposed to be a high quantile \
         eased over frames, and a jump like that is what an unsmoothed maximum does",
        worst * 100.0
    );
    assert!(
        mean < 0.01,
        "the overlay scale moved {:.2}% per frame on average; the picture will be restless",
        mean * 100.0
    );
}

/// Easing is only meaningful between two frames of the same world.
///
/// Carried across a load it would fade the new slide in from the old one's brightness, and a
/// layer switched on would open on a flash of black while its ramp climbed from nothing.
#[test]
fn a_new_world_and_a_new_layer_both_take_their_reading_outright() {
    let world = living(64);
    let mut slide = Slide::new(world.scenario().clone()).expect("a slide");
    slide.set_world(world);
    slide.set_overlay(4);
    slide.world_mut().run(500);

    let settled = slide.frame().overlays[0].scale;

    // A world with a tenth of the carbon in it. If the exposure were carried over, the first
    // frame would be scaled for the old slide and come out nearly black.
    let mut lean = living(64);
    lean.set_uniform_seeding(4, q10(40));
    slide.set_world(lean);
    let first = slide.frame().overlays[0].scale;
    assert!(
        first * 4 < settled,
        "a slide seeded with a tenth of the carbon opened at a scale of {first} against the old \
         slide's {settled}; the exposure was carried across the load"
    );

    // And a layer switched on mid-run, which has never had an exposure of its own.
    slide.toggle_overlay(11);
    let f = slide.frame();
    let co2 = f.overlays.iter().find(|l| l.chemical == 11).expect("on");
    let plane_max = slide
        .world()
        .substrate()
        .chem_plane(11)
        .iter()
        .copied()
        .max()
        .unwrap_or(0);
    assert!(
        co2.scale * 2 > plane_max,
        "carbon dioxide opened at {} against a plane whose highest square is {plane_max}; it \
         faded up from nothing instead of taking its reading",
        co2.scale
    );
}

#[test]
#[ignore = "measurement; run with --release --ignored --nocapture"]
fn what_the_overlay_scale_does_between_frames() {
    let world = living(128);
    let mut slide = Slide::new(world.scenario().clone()).expect("a slide");
    slide.set_world(world);
    slide.set_overlay(4);

    // Settle first: the interesting condition is a population that has grown into the slide and
    // is dividing and dying in it, not four founders on an untouched field.
    slide.world_mut().run(4_000);

    let mut max = Vec::new();
    let mut q999 = Vec::new();
    let mut q99 = Vec::new();
    let mut q95 = Vec::new();
    let mut shipped = Vec::new();
    let mut population = Vec::new();

    // One reading per tick, which is the fastest the picture can change.
    for _ in 0..400 {
        slide.world_mut().run(1);
        let mut plane: Vec<i32> = slide.world().substrate().chem_plane(4).to_vec();
        plane.sort_unstable();
        max.push(quantile(&plane, 1.0));
        q999.push(quantile(&plane, 0.999));
        q99.push(quantile(&plane, 0.99));
        q95.push(quantile(&plane, 0.95));
        // What the overlay is actually normalised against, quantile and easing together, so the
        // table's bottom line is the thing being shipped rather than an argument about it.
        shipped.push(slide.frame().overlays[0].scale);
        population.push(slide.world().cells().len());
    }

    println!(
        "\n{} cells, carbon plane, 400 consecutive ticks\n",
        population.last().copied().unwrap_or(0)
    );
    println!(
        "{:<12} {:>12} {:>12} {:>10} {:>10}",
        "statistic", "first", "last", "worst step", "mean step"
    );
    for (name, series) in [
        ("max", &max),
        ("99.9th", &q999),
        ("99th", &q99),
        ("95th", &q95),
        ("as shipped", &shipped),
    ] {
        let (worst, mean) = steps(series);
        println!(
            "{name:<12} {:>12} {:>12} {:>9.1}% {:>9.2}%",
            series.first().copied().unwrap_or(0),
            series.last().copied().unwrap_or(0),
            worst * 100.0,
            mean * 100.0
        );
    }
    println!();
}
