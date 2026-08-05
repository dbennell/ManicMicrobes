//! Why a tick costs what it costs, at fifty thousand cells.
//!
//! ```text
//! cargo test -p mm-core --release --test tick_cost -- --ignored --nocapture --test-threads=1
//! for n in 1 2 4 8 16 20; do RAYON_NUM_THREADS=$n cargo test -p mm-core --release \
//!   --test tick_cost -- --ignored --nocapture how_the_tick_scales; done
//! ```
//!
//! The `population` bench says how fast a tick is and breaks it into the phases that can be
//! called directly. Two things it cannot say, and they are the two that decide where the next
//! work goes:
//!
//! * **What is in the remainder.** The breakdown reports about a third of the tick under
//!   "births/deaths etc", which is a subtraction and not a measurement — and the phases it *can*
//!   call are timed by calling them repeatedly on one world, which is a fair way to price a phase
//!   and a poor way to price a tick. Here the pieces are taken away instead: the same world runs
//!   with births made unaffordable, with the causes of death switched off, and with both, and the
//!   difference is what those pieces actually cost in place.
//! * **How much of it can use another core.** Only `biology::execute`, the fluid and part of the
//!   neighbour index run on more than one thread. `metabolism::step`, `ecology::step`,
//!   `sensing::step_physics` and `biology::resolve` are sequential loops over the arena, and
//!   Amdahl puts a ceiling on the whole tick at whatever those add up to.
//!
//! Both run on the population the `population` bench grows and caches, so every number here is
//! comparable with every number there. Grow it with `cargo bench -p mm-core --bench population`.

use std::time::Instant;

use mm_core::biology::BiologyConfig;
use mm_core::World;

/// The pool the binaries build, so these numbers describe the machine they run on rather than
/// rayon's default. Honours `RAYON_NUM_THREADS`, so the scaling sweep still overrides it.
fn pool() {
    mm_core::threads::use_performance_cores();
}

fn cached_world() -> Option<World> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/bench-cache/ancestor_mm-1-50000.mmslide");
    let bytes = std::fs::read(&path).ok()?;
    match mm_core::Snapshot::read(&bytes) {
        Ok(w) => Some(w),
        Err(e) => {
            eprintln!("cache unusable ({e:?}); grow it with `cargo bench -p mm-core`");
            None
        }
    }
}

/// Milliseconds per tick, from a world loaded fresh so that no variant inherits another's
/// trajectory.
fn ms_per_tick(mut world: World, config: BiologyConfig, ticks: u64) -> (f64, usize) {
    world.set_biology(config);
    // Settle whatever the snapshot load left behind before the timed window.
    world.run(5);
    let before = world.cells().len();
    let t = Instant::now();
    world.run(ticks);
    let per = t.elapsed().as_secs_f64() / ticks as f64;
    (per * 1e3, before.max(world.cells().len()))
}

#[test]
#[ignore = "diagnostic; run with --release --ignored --nocapture"]
fn how_the_tick_scales_with_threads() {
    pool();
    let Some(mut world) = cached_world() else {
        return;
    };
    let population = world.cells().len();
    world.run(10);
    let n = 60;
    let t = Instant::now();
    world.run(n);
    let per_tick = t.elapsed().as_secs_f64() / n as f64;
    println!(
        "SCALING threads {:>2}  cells {population:>6}  {:>7.2} ms/tick  {:>6.1} ticks/s",
        rayon::current_num_threads(),
        per_tick * 1e3,
        1.0 / per_tick,
    );
}

/// What the unnamed third of the tick is, by removing pieces rather than subtracting phases.
///
/// Each row is the whole tick with one thing switched off, against the same world with nothing
/// switched off. The difference is what that piece costs *where it runs*, with the cache state,
/// the branch history and the population it really has — none of which a phase called sixty times
/// in a row on a static world reproduces.
///
/// Turning a piece off changes the trajectory, so each variant is loaded fresh and the populations
/// are reported: two rows whose populations have drifted apart are not measuring the same slide,
/// and the difference between them is worth that much less.
#[test]
#[ignore = "diagnostic; run with --release --ignored --nocapture"]
fn what_the_remainder_is_made_of() {
    pool();
    let Some(reference) = cached_world() else {
        return;
    };
    let base_config = reference.biology().clone();
    let ticks = 40;

    let (base, pop) = ms_per_tick(reference, base_config.clone(), ticks);
    println!(
        "\n{pop} cells, {} threads, {ticks} ticks a row",
        rayon::current_num_threads()
    );
    println!(
        "{:<34} {:>9} {:>11} {:>9}",
        "the tick with...", "ms/tick", "difference", "cells"
    );
    println!("{:<34} {base:>9.2} {:>11} {pop:>9}", "everything on", "—");

    // Division made unaffordable: no daughter is ever placed, so `apply_births`, the genome
    // interning and the arena growth it drives all stop with it.
    let mut no_births = base_config.clone();
    no_births.division_energy = i32::MAX / 2;

    // Nothing wears a cell down, so nothing starves: `apply_deaths` and the carrion it leaves
    // stop with it.
    let mut no_deaths = base_config.clone();
    no_deaths.metabolism.rates.background_damage = 0;
    no_deaths.metabolism.rates.metabolic_floor = 0;
    no_deaths.ecology.crowding_damage = 0;
    no_deaths.ecology.spike_damage = 0;

    let mut neither = no_births.clone();
    neither.metabolism.rates.background_damage = 0;
    neither.metabolism.rates.metabolic_floor = 0;
    neither.ecology.crowding_damage = 0;
    neither.ecology.spike_damage = 0;

    // Growth off as well, which stops mass changing and with it the radius, the distances
    // between neighbours, and everything downstream that has to be recomputed when a cell
    // changes size.
    let mut nothing_changes = neither.clone();
    nothing_changes.metabolism.rates.growth_rate = 0;

    for (label, config) in [
        ("no births", no_births),
        ("no deaths", no_deaths),
        ("neither births nor deaths", neither),
        ("nor growth either", nothing_changes),
    ] {
        let Some(world) = cached_world() else {
            return;
        };
        let (ms, n) = ms_per_tick(world, config, ticks);
        println!(
            "{label:<34} {ms:>9.2} {:>10.1}% {n:>9}",
            100.0 * (ms - base) / base
        );
    }
    println!(
        "\n  A large negative difference is a piece worth attacking; a small one is a piece the\n  \
         phase breakdown has already accounted for somewhere else."
    );
}
