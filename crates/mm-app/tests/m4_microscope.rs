//! M4 acceptance tests — the microscope.
//!
//! Three of M4's four acceptance tests can be checked without a graphics stack, and are:
//!
//! 1. **Rendering cannot affect simulation.** Here, in its literal form: a world driven the
//!    way the front-end drives it, compared against a bare [`World`] stepped the way `mm-cli`
//!    steps it.
//! 3. **Decoupling.** Dropping the render rate does not change tick output or ordering.
//! 4. **Zero Bevy in core.** Checked in `mm-core/tests/hard_rules.rs` against the resolved
//!    dependency graph, which is a stronger check than anything that could be written here.
//!
//! Acceptance 2 — a hundred thousand cells at sixty frames a second on a mid-range discrete
//! GPU — **cannot be verified in this repository's test suite**. It needs a GPU and a display,
//! and this was developed on a machine with neither. What can be said is that the frame-build
//! cost is bounded and measured: see `a_frame_is_cheap_to_take` below, which is the part of
//! that budget the simulation side owns.

use mm_app::slide::{Lod, Slide};
use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::light::CurrentField;
use mm_core::{LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding, World};

/// The same slide M2's tests use: light, food, no flow.
///
/// Not `Scenario::stress`, which is a physics workload rather than a habitat — the ancestor
/// starves in it, and a test that compared two extinct worlds would pass whatever the renderer
/// did.
fn scenario(seed: u64) -> Scenario {
    Scenario {
        name: "petri".to_string(),
        seed,
        width: 64,
        height: 64,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        current: CurrentField::Still,
        seeding: vec![
            Seeding::Uniform {
                chemical: 11,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 14,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 4,
                per_square: q10(400),
            },
        ],
        ..Scenario::default()
    }
}

/// Put something alive on the slide, so the comparison is between two *living* worlds.
///
/// A test that compared two empty worlds would pass no matter what the renderer did.
fn seed_life(world: &mut World) {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../genomes/ancestor.mm"
    ))
    .expect("the ancestor genome is in the repository");
    let bytes = mm_asm::assemble(&src).expect("it assembles").bytes;
    world.set_biology(BiologyConfig {
        mutation: MutationRates::default(),
        ..BiologyConfig::default()
    });
    for k in 0..12u32 {
        let genome = world.genomes().intern(bytes.clone()).expect("interned");
        let id = world.spawn_cell(CellSeed {
            x: pos((6 + (k % 6) * 9) as i32),
            y: pos((6 + (k / 6) * 9) as i32),
            mass: q10(30),
            energy: q10(400),
            membrane: 24,
            key: 11,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        if let Some(i) = world.cells_mut().index(id) {
            let cells = world.cells_mut();
            cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
            cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
            cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
            cells.interior_mut(i)[11] = q10(40);
            cells.interior_mut(i)[14] = q10(40);
        }
    }
    world.adopt_current_contents_as_baseline();
}

/// Run the world the way the front-end runs it: a frame taken every tick, panels open, the
/// user fiddling with everything they can reach.
fn watched(ticks: u64, seed: u64) -> (u64, usize) {
    let mut slide = Slide::new(scenario(seed)).expect("scenario");
    seed_life(slide.world_mut());
    for tick in 0..ticks {
        slide.advance(1);
        // Everything the renderer does, and everything the user can do to it.
        slide.set_zoom(((tick % 200) as f32) * 0.6);
        if tick % 97 == 0 {
            slide.toggle_overlay((tick % 16) as usize);
        }
        slide.optics.focus = ((tick % 11) as f32 - 5.0) * 0.1;
        let frame = slide.frame();
        if let Some(dot) = frame.cells.first() {
            let _ = slide.inspect(dot.id);
        }
        let _ = slide.history().series(|s| s.dissipation);
    }
    (slide.world().state_hash(), slide.world().cells().len())
}

/// Run it the way `mm-cli` does: nothing but `step`.
fn headless(ticks: u64, seed: u64) -> (u64, usize) {
    let mut world = World::new(scenario(seed)).expect("scenario");
    seed_life(&mut world);
    for _ in 0..ticks {
        world.step();
    }
    (world.state_hash(), world.cells().len())
}

fn compare(ticks: u64, seed: u64) {
    let (watched_hash, watched_n) = watched(ticks, seed);
    let (headless_hash, headless_n) = headless(ticks, seed);
    assert!(
        watched_n > 0,
        "the population died, so this compared two empty worlds"
    );
    assert_eq!(
        watched_n, headless_n,
        "watched world has {watched_n} cells, headless has {headless_n}"
    );
    assert_eq!(
        watched_hash, headless_hash,
        "at {ticks} ticks the watched world hashed {watched_hash:#018x} and the headless one \
         {headless_hash:#018x} — rendering reached the simulation"
    );
}

#[test]
fn a_watched_world_matches_a_headless_one() {
    // M4 acceptance 1, at guard length. The ignored test below is the same thing at the
    // 100,000 ticks the milestone asks for.
    compare(if cfg!(debug_assertions) { 400 } else { 5_000 }, 1);
}

#[test]
#[ignore = "100,000 ticks; run with --release --ignored"]
fn acceptance_rendering_cannot_affect_simulation() {
    let ticks: u64 = std::env::var("MM_M4_TICKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);
    for seed in [1u64, 2, 3] {
        compare(ticks, seed);
    }
}

#[test]
fn the_render_rate_does_not_change_tick_output() {
    // M4 acceptance 3. Sixty frames a second against five: the same ticks, grouped
    // differently.
    let ticks = if cfg!(debug_assertions) { 480 } else { 4_800 };

    let mut fast = Slide::new(scenario(2)).unwrap();
    seed_life(fast.world_mut());
    fast.set_speed(1);
    for _ in 0..ticks {
        fast.advance_one_frame();
        let _ = fast.frame();
    }

    let mut slow = Slide::new(scenario(2)).unwrap();
    seed_life(slow.world_mut());
    // 5fps against 60fps is twelve ticks a frame instead of one.
    slow.set_speed(12);
    for _ in 0..ticks / 12 {
        slow.advance_one_frame();
        let _ = slow.frame();
    }

    assert_eq!(fast.world().tick_count(), ticks);
    assert_eq!(slow.world().tick_count(), ticks);
    assert_eq!(
        fast.world().state_hash(),
        slow.world().state_hash(),
        "the frame rate changed the world"
    );
}

#[test]
fn a_frame_is_cheap_to_take() {
    // The simulation side of M4's frame budget. Not the GPU test — that needs a GPU — but the
    // part this repository owns: building a frame must not cost a meaningful fraction of the
    // budget before a single triangle is drawn.
    //
    // Asserted as a ratio against the cost of a tick rather than in milliseconds, so it means
    // the same thing on a slow machine as on a fast one and does not fail in CI for being CI.
    let mut slide = Slide::new(scenario(3)).unwrap();
    seed_life(slide.world_mut());
    // Grown to a population rather than for a fixed number of ticks. How long it takes to
    // reach a thousand cells is a property of the build — a debug build takes many more
    // wall-clock seconds but the same number of ticks, and a fixed tick budget tuned for
    // release left the debug run measuring seventy cells.
    let mut population = 0;
    for _ in 0..400 {
        slide.advance(25);
        population = slide.world().cells().len();
        if population >= 1_000 {
            break;
        }
    }
    assert!(
        population >= 1_000,
        "only grew to {population} cells; not a real test"
    );

    slide.set_zoom(1.0);
    assert_eq!(
        slide.frame().lod,
        Lod::Dots,
        "whole-slide zoom is the dot tier"
    );

    let started = std::time::Instant::now();
    let frames = 60;
    for _ in 0..frames {
        let f = slide.frame();
        std::hint::black_box(f.cells.len());
    }
    let per_frame = started.elapsed() / frames;

    let started = std::time::Instant::now();
    let ticks = 60;
    slide.advance(ticks);
    let per_tick = started.elapsed() / ticks as u32;

    eprintln!(
        "{population} cells: frame {per_frame:?}, tick {per_tick:?}, ratio {:.2}",
        per_frame.as_secs_f64() / per_tick.as_secs_f64().max(f64::EPSILON)
    );
    assert!(
        per_frame < per_tick * 4,
        "building a frame of {population} cells took {per_frame:?} against {per_tick:?} for a \
         tick; the renderer is doing the simulation's work"
    );
}

#[test]
fn detail_costs_nothing_until_it_is_asked_for() {
    // The LOD tiers exist to make acceptance 2 reachable: a hundred thousand cells at
    // whole-slide zoom must not each build an organelle list that draws as one pixel.
    let mut slide = Slide::new(scenario(4)).unwrap();
    seed_life(slide.world_mut());
    slide.advance(if cfg!(debug_assertions) { 200 } else { 2_000 });

    slide.set_zoom(1.0);
    let far = slide.frame();
    slide.set_zoom(64.0);
    let near = slide.frame();

    assert_eq!(far.lod, Lod::Dots);
    assert_eq!(near.lod, Lod::Full);
    assert_eq!(
        far.cells.iter().map(|c| c.organelles.len()).sum::<usize>(),
        0,
        "the far tier built organelle lists nobody can see"
    );
    assert!(
        near.cells.iter().any(|c| !c.organelles.is_empty()),
        "the near tier resolved no organelles at all"
    );
    // The same cells either way — the tier changes detail, never contents.
    assert_eq!(far.cells.len(), near.cells.len());
}
