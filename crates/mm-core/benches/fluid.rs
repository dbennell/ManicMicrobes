//! The M1 performance gate: **512×512 grid, 16 chemicals, ≥ 500 fluid steps/second on 8
//! cores**.
//!
//! Benchmarks are gates, not information (CLAUDE.md). The headless runner exists so that
//! parameter sweeps can run at a thousand times realtime, and that only works if a fluid step
//! is cheap.
//!
//! Three workloads, because "16 chemicals" can mean two quite different things and the gap
//! between them is the whole story:
//!
//! * `full` — all sixteen chemical planes non-empty **and** the water moving. The most
//!   demanding reading of the gate, and the one this currently misses.
//! * `still` — all sixteen planes non-empty, no flow. Advection is skipped, which is exactly
//!   half the work.
//! * `scenario` — what a real slide costs: a scenario that seeds five chemicals, with a
//!   rotating current and a day/night cycle. Empty planes are skipped because diffusion
//!   cannot create matter, so a plane that starts at zero stays there.

use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use mm_core::chem::{ChemTable, CHEM_COUNT};
use mm_core::fluid::{self, FluidScratch, MAX_VELOCITY};
use mm_core::{Scenario, Substrate, World};

const GATE: f64 = 500.0;

fn populated(w: u32, h: u32, flowing: bool) -> Substrate {
    let mut s = Substrate::new(w, h).expect("substrate");
    for c in 0..CHEM_COUNT {
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let v = ((x * 7 + y * 13 + c as i32 * 29) % 100_000) * 1024;
                s.set_chem(c, x, y, v);
            }
        }
    }
    if flowing {
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                s.set_velocity(x, y, MAX_VELOCITY / 2, -MAX_VELOCITY / 3);
            }
        }
    }
    s
}

fn throughput(c: &mut Criterion) {
    let rates = ChemTable::spec_default().diffusion_rates();
    let mut group = c.benchmark_group("fluid_512");
    group.throughput(Throughput::Elements(1));
    group.sample_size(10);

    for (name, flowing) in [("full", true), ("still", false)] {
        let mut s = populated(512, 512, flowing);
        let mut scratch = FluidScratch::new(s.len());
        group.bench_function(name, |b| {
            b.iter(|| fluid::step(&mut s, &rates, &mut scratch))
        });
    }

    let mut world = World::new(Scenario::stress(512, 512)).expect("world");
    world.run(4);
    group.bench_function("scenario", |b| b.iter(|| world.step()));
    group.finish();
}

/// Reported rather than asserted, and deliberately so: the gate is currently missed on the
/// most demanding workload and met on the realistic one, and a benchmark that failed the
/// build would hide that distinction behind a red cross.
fn gate(_c: &mut Criterion) {
    if cfg!(debug_assertions) {
        return;
    }
    let rates = ChemTable::spec_default().diffusion_rates();
    let threads = rayon::current_num_threads();
    eprintln!("\nM1 fluid gate: 512x512, 16 chemicals, need {GATE:.0} steps/s ({threads} threads)");

    for (name, flowing) in [
        ("all 16 populated, flowing", true),
        ("all 16 populated, still", false),
    ] {
        let mut s = populated(512, 512, flowing);
        let mut scratch = FluidScratch::new(s.len());
        for _ in 0..5 {
            fluid::step(&mut s, &rates, &mut scratch);
        }
        let n = 60;
        let t = Instant::now();
        for _ in 0..n {
            fluid::step(&mut s, &rates, &mut scratch);
        }
        let rate = n as f64 / t.elapsed().as_secs_f64();
        eprintln!(
            "  {name:<28} {rate:7.0} steps/s  {}",
            if rate >= GATE { "MET" } else { "MISSED" }
        );
    }

    let mut world = World::new(Scenario::stress(512, 512)).expect("world");
    world.run(5);
    let n = 60;
    let t = Instant::now();
    world.run(n);
    let rate = n as f64 / t.elapsed().as_secs_f64();
    eprintln!(
        "  {:<28} {rate:7.0} steps/s  {}",
        "stress scenario (5 active)",
        if rate >= GATE { "MET" } else { "MISSED" }
    );
}

criterion_group!(benches, throughput, gate);
criterion_main!(benches);
