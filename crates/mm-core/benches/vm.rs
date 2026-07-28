//! The M0 performance gate: **≥ 50M instructions/second, single core, release build**.
//!
//! Benchmarks are gates, not information (CLAUDE.md). A change that regresses this is not
//! done, however correct it is — the whole reason `mm-core` is a separate crate with no
//! Bevy in it is so that parameter sweeps can run headless at a thousand times realtime,
//! and that only works if a cell's instruction budget is nearly free.
//!
//! ```text
//! cargo bench --workspace
//! ```
//!
//! The mixes below are separated on purpose. `arithmetic` is the floor: straight-line work
//! with no searching, which is what most instructions in a real genome are. `search` is the
//! ceiling on cost: every iteration pays for a template scan, which is the worst a genome
//! can inflict on the scheduler. If the two ever converge it means the search has stopped
//! being skipped in the common case, and the gate would be met on paper while being missed
//! in the world.

use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use mm_core::genome::Genome;
use mm_core::host::NullHost;
use mm_core::isa::{Op, Template};
use mm_core::rng::{Purpose, RandCtx};
use mm_core::vm::Vm;
use mm_core::VmConfig;

/// The gate, in instructions per second.
const GATE: f64 = 50_000_000.0;

fn letters(t: Template) -> Vec<u8> {
    (0..t.len)
        .map(|i| {
            if t.letter(i) == 1 {
                Op::Nop1.canonical_byte()
            } else {
                Op::Nop0.canonical_byte()
            }
        })
        .collect()
}

/// Straight-line arithmetic and stack traffic: no jumps, no searching.
fn arithmetic_genome() -> Genome {
    let pattern = [
        Op::One,
        Op::Dup,
        Op::Add,
        Op::Dup,
        Op::Mul,
        Op::Zero,
        Op::RStore,
        Op::Zero,
        Op::RLoad,
        Op::One,
        Op::Sub,
        Op::Abs,
        Op::Dup,
        Op::Xor,
        Op::Drop,
        Op::One,
        Op::One,
        Op::Cmp,
        Op::Drop,
        Op::Zero,
        Op::Store,
        Op::Zero,
        Op::Load,
        Op::Drop,
    ];
    let bytes: Vec<u8> = pattern
        .iter()
        .cycle()
        .take(1024)
        .map(|o| o.canonical_byte())
        .collect();
    Genome::new(bytes).expect("arithmetic genome")
}

/// A loop closed by a backward jump: every iteration pays for a template search.
fn search_genome() -> Genome {
    const LOOP: Template = Template {
        len: 4,
        value: 0b1011,
    };
    let mut bytes = letters(LOOP);
    for _ in 0..64 {
        bytes.push(Op::One.canonical_byte());
        bytes.push(Op::Drop.canonical_byte());
    }
    bytes.push(Op::JmpB.canonical_byte());
    bytes.extend(letters(LOOP.complement()));
    bytes.push(Op::Halt.canonical_byte());
    Genome::new(bytes).expect("search genome")
}

/// Nothing but `EXPRESS`, against a table of 32 genes: the promoter-scan cost.
fn express_genome() -> Genome {
    let mut bytes = Vec::new();
    for _ in 0..64 {
        bytes.push(Op::Express.canonical_byte());
        bytes.extend(letters(Template::new(8, 0b1010_1010)));
    }
    bytes.push(Op::Halt.canonical_byte());
    for i in 0..32u8 {
        bytes.push(Op::Gene.canonical_byte());
        bytes.extend(letters(Template::new(8, i.wrapping_mul(7))));
        bytes.push(Op::Ret.canonical_byte());
    }
    Genome::new(bytes).expect("express genome")
}

/// Random bytes: the mix a genome drifting under mutation actually presents.
fn random_genome() -> Genome {
    let ctx = RandCtx::new(0xB3_9C_11, 0, 0);
    let bytes: Vec<u8> = (0..4096u64)
        .map(|i| (ctx.draw(Purpose::Harness, i) >> 32) as u8)
        .collect();
    Genome::new(bytes).expect("random genome")
}

/// Execute `budget` instructions in tick-sized slices, as the simulation will.
fn execute(genome: &Genome, cfg: &VmConfig, budget: u32) -> u64 {
    let mut vm = Vm::new();
    let mut host = NullHost;
    let mut done = 0u32;
    let mut tick = 0u64;
    while done < budget {
        let slice = u32::from(cfg.instr_per_tick.max(1)).min(budget - done);
        vm.halted = false;
        done += vm.run(genome, cfg, &RandCtx::new(1, tick, 1), &mut host, slice);
        tick += 1;
    }
    vm.ip as u64
}

fn throughput(c: &mut Criterion) {
    let cfg = VmConfig::DEFAULT;
    const BUDGET: u32 = 1_000_000;

    let mut group = c.benchmark_group("vm");
    group.throughput(Throughput::Elements(BUDGET as u64));
    for (name, genome) in [
        ("arithmetic", arithmetic_genome()),
        ("search", search_genome()),
        ("express", express_genome()),
        ("random", random_genome()),
    ] {
        group.bench_function(name, |b| {
            b.iter(|| criterion::black_box(execute(&genome, &cfg, BUDGET)))
        });
    }
    group.finish();
}

/// Genome construction is O(length) and happens once per distinct genome, not per cell —
/// but from M2 every non-clonal division makes a new one, so it is worth watching now.
fn interning(c: &mut Criterion) {
    let bytes = random_genome().to_vec();
    c.bench_function("genome/construct_4096", |b| {
        b.iter(|| criterion::black_box(Genome::new(bytes.clone()).is_ok()))
    });
}

/// The gate itself, asserted rather than reported.
///
/// Criterion measures and compares; this fails the run outright if the floor is not met, so
/// `cargo bench` is pass/fail and not just a wall of numbers.
fn gate(_c: &mut Criterion) {
    if cfg!(debug_assertions) {
        eprintln!("M0 performance gate skipped: not a release build");
        return;
    }
    let cfg = VmConfig::DEFAULT;
    let genome = arithmetic_genome();
    execute(&genome, &cfg, 1_000_000); // warm up

    let budget: u32 = 200_000_000;
    let start = Instant::now();
    let sink = execute(&genome, &cfg, budget);
    let secs = start.elapsed().as_secs_f64();
    let rate = budget as f64 / secs;

    eprintln!(
        "M0 performance gate: {:.1}M instructions/second (need {:.0}M) [ip {sink}]",
        rate / 1e6,
        GATE / 1e6
    );
    assert!(
        rate >= GATE,
        "M0 performance gate missed: {:.1}M instructions/second, need {:.0}M",
        rate / 1e6,
        GATE / 1e6
    );
}

criterion_group!(benches, throughput, interning, gate);
criterion_main!(benches);
