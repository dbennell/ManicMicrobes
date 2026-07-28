//! The M2 and M3 performance gates.
//!
//! * **M2** — 50,000 cells at ≥ 60 ticks/second headless on 8 cores.
//! * **M3** — 50,000 cells *with sensors and cilia* at ≥ 45 ticks/second on 8 cores.
//!
//! Benchmarks are gates, not information (CLAUDE.md). The two gates measure the same slide
//! with two different populations on it, and the gap between them is the price of M3: sensing
//! reads the world around each cell, and cilia write momentum back into the fluid.
//!
//! # Reaching the population honestly
//!
//! The gate says fifty thousand cells, so the benchmark grows fifty thousand cells by letting
//! the ancestor reproduce, rather than by spawning them. A slide populated by hand would have
//! every cell at the same age, on the same genome, holding the same chemistry, in a grid — and
//! it would measure a workload the simulation never actually runs. A grown population has the
//! age spread, the genome spread and the clumping that make branch prediction and the
//! neighbour index behave the way they behave in a real run.
//!
//! That takes a while, which is why this is a benchmark and not a test. It is reported rather
//! than asserted, for the same reason `fluid.rs` reports: a gate that failed the build would
//! turn "met on eight threads, missed on two" into a red cross with no number attached.

use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::light::CurrentField;
use mm_core::neighbours::{self, NeighbourIndex};
use mm_core::{LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding, World};

const TARGET_CELLS: usize = 50_000;
const M2_GATE: f64 = 60.0;
const M3_GATE: f64 = 45.0;

fn assemble(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path).expect("genome file");
    mm_asm::assemble(&src).expect("it assembles").bytes
}

/// A slide big enough to hold fifty thousand cells without the population being limited by
/// the walls rather than by the chemistry.
fn slide(seed: u64) -> Scenario {
    Scenario {
        name: "gate".to_string(),
        seed,
        width: 256,
        height: 256,
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

/// Grow a population to `TARGET_CELLS`, or give up and report what it reached.
fn grown(genome_file: &str, seed: u64) -> Option<World> {
    let bytes = assemble(genome_file);
    let mut world = World::new(slide(seed)).expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::default(),
        ..BiologyConfig::default()
    });
    for k in 0..64u32 {
        let genome = world.genomes().intern(bytes.clone()).expect("interned");
        let id = world.spawn_cell(CellSeed {
            x: pos((16 + (k % 8) * 28) as i32),
            y: pos((16 + (k / 8) * 28) as i32),
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
            cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
            cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
            cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
            cells.interior_mut(i)[11] = q10(40);
            cells.interior_mut(i)[14] = q10(40);
        }
    }
    world.adopt_current_contents_as_baseline();

    // Grow, checking often enough to stop near the target rather than far past it.
    for _ in 0..4_000 {
        world.run(25);
        let n = world.cells().len();
        if n >= TARGET_CELLS {
            return Some(world);
        }
        if n == 0 {
            eprintln!("  {genome_file}: went extinct while growing");
            return None;
        }
    }
    eprintln!(
        "  {genome_file}: only reached {} cells, not {TARGET_CELLS}",
        world.cells().len()
    );
    Some(world)
}

fn gate(_c: &mut Criterion) {
    if cfg!(debug_assertions) {
        return;
    }
    let threads = rayon::current_num_threads();
    eprintln!("\nPopulation gates ({threads} threads):");

    // The same slide with nothing alive on it. The gate is about cells, so when it is missed
    // the number that matters is how much of the tick the cells were even responsible for: a
    // fluid that already eats the whole budget is a different problem from slow biology and
    // needs a different fix.
    let mut empty = World::new(slide(1)).expect("world");
    empty.run(20);
    let n = 120;
    let t = Instant::now();
    empty.run(n);
    let bare_per_tick = t.elapsed().as_secs_f64() / n as f64;
    eprintln!(
        "  fluid alone, no cells:          {:6.1} ticks/s",
        1.0 / bare_per_tick
    );

    for (milestone, file, want) in [
        ("M2", "ancestor.mm", M2_GATE),
        ("M3", "drifter.mm", M3_GATE),
    ] {
        let Some(mut world) = grown(file, 1) else {
            continue;
        };
        let population = world.cells().len();
        // A few ticks to settle any allocation the growth phase left behind, so the timed
        // window measures steady state.
        world.run(20);

        let n = 120;
        let t = Instant::now();
        world.run(n);
        let per_tick = t.elapsed().as_secs_f64() / n as f64;
        let rate = 1.0 / per_tick;
        let cells_share = (per_tick - bare_per_tick).max(0.0) / per_tick * 100.0;
        eprintln!(
            "  {milestone}: {population:>6} cells  {rate:6.1} ticks/s  (need {want:.0})  {}  \
             — {cells_share:.0}% of the tick is the cells",
            if rate >= want { "MET" } else { "MISSED" }
        );
    }
}

/// Which phase the tick is actually spent in.
///
/// Added when both gates were missed by a wide margin and the fluid turned out to account for
/// none of it. A number that says "too slow" is not actionable; a number that says which of
/// the six phases is too slow is. Every phase here is a public function taking public types,
/// so this measures them exactly as `World::step` calls them, without instrumenting `World`
/// itself — which could not carry an `Instant` anyway (hard rule 5).
fn phase_breakdown(_c: &mut Criterion) {
    if cfg!(debug_assertions) {
        return;
    }
    let Some(mut world) = grown("ancestor.mm", 1) else {
        return;
    };
    let population = world.cells().len();
    let (w, h) = (world.substrate().width(), world.substrate().height());
    let n = 60u32;

    eprintln!("\nPhase breakdown at {population} cells ({w}x{h}):");

    let mut index = NeighbourIndex::default();
    let t = Instant::now();
    for _ in 0..n {
        index.rebuild(world.cells(), w, h);
    }
    let rebuild = t.elapsed() / n;

    let mut radii = Vec::new();
    let t = Instant::now();
    for _ in 0..n {
        std::hint::black_box(neighbours::resolve_collisions(
            world.cells_mut(),
            &index,
            &mut radii,
        ));
    }
    let collisions = t.elapsed() / n;

    // `execute` and `resolve` need pieces `World` keeps private, so they are measured as the
    // remainder: whole tick minus everything above and minus the fluid.
    let t = Instant::now();
    world.run(n as u64);
    let whole = t.elapsed() / n;

    let mut empty = World::new(slide(1)).expect("world");
    empty.run(10);
    let t = Instant::now();
    empty.run(n as u64);
    let fluid = t.elapsed() / n;

    let accounted = rebuild * 2 + collisions + fluid;
    let rest = whole.saturating_sub(accounted);
    let pct = |d: std::time::Duration| d.as_secs_f64() / whole.as_secs_f64() * 100.0;
    eprintln!("  whole tick            {whole:>10.2?}");
    eprintln!(
        "  neighbour rebuild x2  {:>10.2?}  {:5.1}%",
        rebuild * 2,
        pct(rebuild * 2)
    );
    eprintln!(
        "  collision separation  {collisions:>10.2?}  {:5.1}%",
        pct(collisions)
    );
    eprintln!(
        "  fluid + bookkeeping   {fluid:>10.2?}  {:5.1}%",
        pct(fluid)
    );
    eprintln!(
        "  execute + resolve +   {rest:>10.2?}  {:5.1}%  (the remainder)",
        pct(rest)
    );
    eprintln!("    metabolism + physics");
}

/// Per-phase throughput, for finding out *where* a regression went rather than only that one
/// happened.
fn phases(c: &mut Criterion) {
    if cfg!(debug_assertions) {
        return;
    }
    let mut group = c.benchmark_group("population");
    group.sample_size(10);
    for (name, file) in [("ancestor", "ancestor.mm"), ("drifter", "drifter.mm")] {
        if let Some(mut world) = grown(file, 1) {
            group.bench_function(name, |b| b.iter(|| world.step()));
        }
    }
    group.finish();
}

criterion_group!(benches, gate, phase_breakdown, phases);
criterion_main!(benches);
