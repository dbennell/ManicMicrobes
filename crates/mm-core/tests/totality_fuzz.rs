//! M0 acceptance test 1 — totality.
//!
//! > 10,000,000 random byte arrays of length 1–4096, each executed for 100,000 instructions
//! > from randomised initial VM state. Zero panics, zero hangs, zero aborts.
//!
//! This test is the foundation of everything and must never be weakened. Invariant I3 is
//! what lets every other part of the design be simple: nothing downstream has to handle a
//! faulted cell, because there is no fault state.
//!
//! # Running it
//!
//! ```text
//! cargo test -p mm-core --test totality_fuzz                        # the guard, seconds
//! cargo test --release --test totality_fuzz -- --ignored --nocapture # the acceptance run
//! ```
//!
//! [`acceptance_10m_cases`] is the milestone criterion, at its full specified size: 10¹²
//! instructions, about three and a half minutes on twenty cores and an hour on one. It is
//! `#[ignore]`d so that it is opted into rather than paid for on every `cargo test`, not
//! because it is optional. [`fuzz_guard`] runs the identical case function over a prefix of
//! the identical case sequence, so it catches anything the long run would catch that shows
//! up early, and it runs unconditionally.
//!
//! Sizes are overridable: `MM_FUZZ_CASES`, `MM_FUZZ_INSTRS`, `MM_FUZZ_THREADS`.
//!
//! # Why there can be no hang
//!
//! Termination is structural rather than empirical. Each case runs a fixed instruction
//! count; every instruction is O(1) except the complementary jump search, bounded by
//! `min(template_search_range, genome length)` probes, and `EXPRESS`, bounded by the
//! promoter table built once at genome construction. There is no loop in the VM whose trip
//! count a genome controls.

mod common;

use common::{
    assert_well_formed, env_usize, random_config, random_genome_bytes, random_vm, run_instructions,
};
use mm_core::genome::Genome;
use mm_core::host::NullHost;
use mm_core::state_hash::StateHash;

/// Longest genome the fuzz generates, per the acceptance criterion.
const MAX_GENOME: usize = 4096;

/// Execute one case. Total by contract: if this returns, the case passed.
///
/// Returns the state hash so the caller can keep the work from being optimised away, and so
/// a divergence between two runs of the same case shows up as a mismatch rather than as
/// nothing at all.
fn run_case(case: u64, instrs: u64) -> u64 {
    let bytes = random_genome_bytes(case, MAX_GENOME);
    assert!(!bytes.is_empty() && bytes.len() <= MAX_GENOME);

    let genome = match Genome::new(bytes) {
        Ok(g) => g,
        Err(e) => panic!("case {case}: genome construction failed: {e}"),
    };
    let cfg = random_config(case);
    let mut vm = random_vm(case);
    let mut host = NullHost;

    let ran = run_instructions(
        &mut vm,
        &genome,
        &cfg,
        case.wrapping_mul(0x9E37_79B9_7F4A_7C15),
        case,
        &mut host,
        instrs,
    );
    assert_eq!(ran, instrs, "case {case}: wrong instruction count");
    assert_well_formed(&vm, &genome, &format!("case {case}"));
    vm.state_hash()
}

fn run_range(start: u64, end: u64, instrs: u64) -> u64 {
    let mut acc = 0u64;
    for case in start..end {
        acc ^= run_case(case, instrs);
    }
    acc
}

/// Spread cases over the machine. Case `i` is a pure function of `i`, so the split changes
/// nothing about what is tested — only how long it takes.
fn fuzz(cases: u64, instrs: u64, threads: usize) {
    let threads = threads.max(1).min(cases.max(1) as usize);
    let mut acc = 0u64;
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for t in 0..threads as u64 {
            let start = cases.saturating_mul(t) / threads as u64;
            let end = cases.saturating_mul(t.saturating_add(1)) / threads as u64;
            handles.push(scope.spawn(move || run_range(start, end, instrs)));
        }
        for h in handles {
            match h.join() {
                Ok(v) => acc ^= v,
                Err(e) => std::panic::resume_unwind(e),
            }
        }
    });
    // Consume the accumulator so nothing above is dead code.
    assert_ne!(acc, u64::MAX.wrapping_add(1), "unreachable");
}

fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// Runs on every `cargo test`. Same cases, same case function, fewer of them.
#[test]
fn fuzz_guard() {
    let cases = env_usize(
        "MM_FUZZ_CASES",
        if cfg!(debug_assertions) { 256 } else { 4_000 },
    );
    let instrs = env_usize(
        "MM_FUZZ_INSTRS",
        if cfg!(debug_assertions) {
            20_000
        } else {
            100_000
        },
    );
    let threads = env_usize("MM_FUZZ_THREADS", default_threads());
    fuzz(cases as u64, instrs as u64, threads);
}

/// The M0 acceptance criterion, at full size.
#[test]
#[ignore = "10^12 instructions; run explicitly with --release --ignored"]
fn acceptance_10m_cases() {
    let cases = env_usize("MM_FUZZ_CASES", 10_000_000);
    let instrs = env_usize("MM_FUZZ_INSTRS", 100_000);
    let threads = env_usize("MM_FUZZ_THREADS", default_threads());
    eprintln!("totality fuzz: {cases} cases x {instrs} instructions on {threads} threads");
    fuzz(cases as u64, instrs as u64, threads);
}

/// The corners random sampling reaches slowly or not at all.
#[test]
fn pathological_genomes_are_total() {
    let mut cases: Vec<Vec<u8>> = vec![
        vec![],           // empty: every offset computation divides by length
        vec![0x00],       // one NOP0
        vec![0x2E],       // nothing but HALT
        vec![0x25],       // RET with an empty call stack
        vec![0x24, 0x01], // CALL whose complement is never found
        vec![0x2D, 0x01], // LOOPLN with LN saturating at zero
        vec![0x38],       // COPYB forever
        vec![0x3F],       // INJECT forever
        vec![0x27, 0x01], // EXPRESS with no genes at all
        vec![0x26],       // a GENE with a zero-length promoter
        vec![0x28, 0x02], // SKIPZ over a template instruction at the wrap point
        vec![0x13],       // DIV, forever, on an empty stack
    ];
    // Every opcode alone, and every opcode followed by a maximal template.
    for b in 0..=255u8 {
        cases.push(vec![b]);
        let mut with_template = vec![b];
        with_template.extend(std::iter::repeat_n(0x01u8, 8));
        cases.push(with_template);
    }
    // Uniform genomes of a single byte value: the worst case for both searches.
    for b in [0x00u8, 0x01, 0x20, 0x21, 0x26, 0x27, 0x2D] {
        cases.push(vec![b; 4096]);
    }
    // Alternating GENE and promoter letters: the largest promoter table a genome can have.
    cases.push(
        std::iter::repeat_n([0x26u8, 0x01], 2048)
            .flatten()
            .collect(),
    );

    let mut host = mm_core::host::NullHost;
    for (i, bytes) in cases.into_iter().enumerate() {
        let genome = Genome::new(bytes).unwrap_or_else(|e| panic!("case {i}: {e}"));
        for salt in 0..4u64 {
            let cfg = random_config(salt);
            let mut vm = random_vm(salt.wrapping_add(i as u64));
            run_instructions(&mut vm, &genome, &cfg, 1, i as u64, &mut host, 20_000);
            assert_well_formed(&vm, &genome, &format!("pathological case {i} salt {salt}"));
        }
    }
}

/// Extreme configurations, which a scenario file is free to contain.
#[test]
fn extreme_configurations_are_total() {
    use mm_core::VmConfig;
    let configs = [
        VmConfig {
            instr_per_tick: 1,
            template_search_range: 0,
            promoter_bind_threshold: 0,
        },
        VmConfig {
            instr_per_tick: u16::MAX,
            template_search_range: u16::MAX,
            promoter_bind_threshold: u16::MAX,
        },
        VmConfig {
            instr_per_tick: 0, // degenerate: no cell may execute anything
            template_search_range: 1,
            promoter_bind_threshold: 8,
        },
    ];
    let mut host = mm_core::host::NullHost;
    for (ci, cfg) in configs.iter().enumerate() {
        for case in 0..24u64 {
            let genome = Genome::new(random_genome_bytes(case, 512)).unwrap();
            let mut vm = random_vm(case);
            run_instructions(&mut vm, &genome, cfg, 3, case, &mut host, 5_000);
            assert_well_formed(&vm, &genome, &format!("config {ci} case {case}"));
        }
    }
}
