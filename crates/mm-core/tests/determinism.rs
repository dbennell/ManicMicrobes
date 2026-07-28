//! M0 acceptance test 2 — determinism (invariant I1).
//!
//! > Identical seed and program produce identical VM state after 1,000,000 instructions,
//! > across 3 runs and across debug and release builds.
//!
//! The first half is checked directly. The second cannot be checked from inside a single
//! build, so it is checked against a pinned digest: [`state_after_a_million_instructions`]
//! asserts a constant, and the test binary is run in both profiles. A value that differs
//! between `cargo test` and `cargo test --release` fails in exactly one of them.
//!
//! Pinning is also what catches the change nobody meant to make. Any edit to opcode
//! semantics, the template search, the promoter distance or the random draw moves this
//! number — and moving it means every archived genome now means something different, which
//! is an ISA version bump (hard rule 8), not a test update.
//!
//! The subject genome is built here rather than read from `genomes/`, so that a failure
//! means "the VM changed" and never "someone edited a fixture".

mod common;

use common::{random_config, random_genome_bytes, random_vm, run_instructions};
use mm_core::genome::Genome;
use mm_core::host::{NullHost, RecordingHost};
use mm_core::isa::Op;
use mm_core::rng::{Purpose, RandCtx};
use mm_core::state_hash::StateHash;
use mm_core::vm::Vm;
use mm_core::VmConfig;

const INSTRUCTIONS: u64 = 1_000_000;

// Jump-label patterns, 4 letters each, and the 8-letter promoter. Chosen so that no run in
// the genome — including every suffix of the promoter — accidentally matches a pattern some
// jump is searching for.
const TOP: u8 = 0b1011;
const DONE: u8 = 0b0110;
const WORK: u8 = 0b1101;
const PROMOTER: u8 = 0b1111_0000;

#[derive(Default)]
struct Build {
    bytes: Vec<u8>,
    /// Where each template was written, and how long it was meant to be.
    sites: Vec<(usize, u8)>,
}

impl Build {
    fn op(mut self, op: Op) -> Self {
        self.bytes.push(op.canonical_byte());
        self
    }
    /// Template letters, least-significant bit first.
    fn letters(mut self, value: u8, len: u8) -> Self {
        self.sites.push((self.bytes.len(), len));
        for i in 0..len {
            self.bytes.push(if (value >> i) & 1 == 1 {
                Op::Nop1.canonical_byte()
            } else {
                Op::Nop0.canonical_byte()
            });
        }
        self
    }
    /// A template opcode plus its letters.
    fn with(self, op: Op, value: u8, len: u8) -> Self {
        self.op(op).letters(value, len)
    }
    /// A jump emits the complement of the label it means to reach.
    fn jump(self, op: Op, label: u8) -> Self {
        self.with(op, !label & 0b1111, 4)
    }
    /// A template is the *maximal* run of `NOP` letters, so two written back to back fuse
    /// into one and every search over them looks for something nobody wrote. The assembler
    /// refuses that; this is the same check for a genome built by hand.
    fn finish(self) -> Genome {
        let genome = Genome::new(self.bytes).expect("subject genome");
        for (start, len) in self.sites {
            assert_eq!(
                genome.template_at(start).len,
                len,
                "the template written at {start} fused with the letters after it"
            );
        }
        genome
    }
}

/// A genome with enough structure to keep every interesting path warm over a million
/// instructions: a countdown loop, a forward conditional jump, a backward jump, a nested
/// call, an `EXPRESS` binding a promoter, and a `RAND` that keeps it from ever settling.
fn subject() -> Genome {
    Build::default()
        .with(Op::Imm, 0b10101, 5) // 21
        .op(Op::Zero)
        .op(Op::RStore) // reg0 = 21
        // top:
        .letters(TOP, 4)
        .op(Op::Zero)
        .op(Op::RLoad)
        .jump(Op::JmpZ, DONE)
        .jump(Op::Call, WORK)
        .op(Op::Zero)
        .op(Op::RLoad)
        .op(Op::One)
        .op(Op::Sub)
        .op(Op::Zero)
        .op(Op::RStore) // reg0 -= 1
        .jump(Op::JmpB, TOP)
        .op(Op::Halt) // unreachable; separates the jump's letters from the label's
        // done: reseed the counter from RAND, so the run never settles into a fixed point
        .letters(DONE, 4)
        .op(Op::Rand)
        .with(Op::Imm, 0b111, 3)
        .op(Op::And)
        .op(Op::Zero)
        .op(Op::RStore)
        .jump(Op::JmpB, TOP)
        .op(Op::Halt) // ditto
        // work:
        .letters(WORK, 4)
        .op(Op::One)
        .op(Op::One)
        .op(Op::Add)
        .op(Op::Drop)
        .with(Op::Express, PROMOTER, 8)
        .op(Op::Ret)
        .op(Op::Halt)
        // a gene, reached only by promoter binding
        .with(Op::Gene, PROMOTER, 8)
        .with(Op::Imm, 0b11, 2)
        .with(Op::Imm, 0b101, 3)
        .op(Op::Eat)
        .op(Op::Drop)
        .op(Op::Ret)
        .finish()
}

fn run_once(genome: &Genome, cfg: &VmConfig, seed: u64, instrs: u64) -> (Vm, u64) {
    let mut vm = Vm::new();
    let mut host = NullHost;
    let ran = run_instructions(&mut vm, genome, cfg, seed, 7, &mut host, instrs);
    assert_eq!(ran, instrs);
    let hash = vm.state_hash();
    (vm, hash)
}

#[test]
fn the_subject_genome_actually_exercises_the_vm() {
    // A determinism test over a genome that no-ops would pass for the wrong reason.
    let genome = subject();
    assert_eq!(genome.promoters().len(), 1);

    let mut vm = Vm::new();
    let mut host = RecordingHost::default();
    run_instructions(
        &mut vm,
        &genome,
        &VmConfig::DEFAULT,
        1,
        0,
        &mut host,
        50_000,
    );
    assert!(
        host.eats.len() > 100,
        "EXPRESS never bound its promoter: {} calls",
        host.eats.len()
    );
    assert!(vm.rand_ctr > 100, "RAND never ran: {}", vm.rand_ctr);
    assert_eq!(host.eats[0], (3, 5));
}

#[test]
fn three_runs_agree_after_a_million_instructions() {
    let genome = subject();
    let cfg = VmConfig::DEFAULT;
    let (first, h1) = run_once(&genome, &cfg, 0xA11CE, INSTRUCTIONS);
    let (second, h2) = run_once(&genome, &cfg, 0xA11CE, INSTRUCTIONS);
    let (third, h3) = run_once(&genome, &cfg, 0xA11CE, INSTRUCTIONS);
    assert_eq!(first, second);
    assert_eq!(second, third);
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
}

#[test]
fn state_after_a_million_instructions() {
    // Run under both profiles: a digest that differs between them is a determinism failure,
    // and this is how it surfaces.
    let (_, hash) = run_once(&subject(), &VmConfig::DEFAULT, 0xA11CE, INSTRUCTIONS);
    assert_eq!(
        hash, 0xB07E_21BD_D20D_C84F,
        "state hash moved; if that was intended it is an ISA version bump"
    );
}

#[test]
fn a_different_seed_gives_a_different_state() {
    let genome = subject();
    let (_, a) = run_once(&genome, &VmConfig::DEFAULT, 1, 10_000);
    let (_, b) = run_once(&genome, &VmConfig::DEFAULT, 2, 10_000);
    assert_ne!(a, b);
}

#[test]
fn randomised_cases_are_reproducible() {
    for case in 0..200u64 {
        let genome = Genome::new(random_genome_bytes(case, 1024)).unwrap();
        let cfg = random_config(case);

        let mut a = random_vm(case);
        let mut ha = RecordingHost::default();
        run_instructions(&mut a, &genome, &cfg, case, case, &mut ha, 20_000);

        let mut b = random_vm(case);
        let mut hb = RecordingHost::default();
        run_instructions(&mut b, &genome, &cfg, case, case, &mut hb, 20_000);

        assert_eq!(a, b, "case {case}: VM state diverged");
        assert_eq!(
            ha.daughter, hb.daughter,
            "case {case}: host effects diverged"
        );
        assert_eq!(ha.emits, hb.emits, "case {case}: emissions diverged");
    }
}

#[test]
fn resuming_from_a_checkpoint_matches_an_uninterrupted_run() {
    // Invariant I7 in miniature. There is no serialisation format yet, so this checks the
    // property the format will have to preserve: a `Vm` value is the whole of the VM's
    // state, and stopping between ticks loses nothing.
    let genome = subject();
    let cfg = VmConfig::DEFAULT;
    let (whole, _) = run_once(&genome, &cfg, 0xBEEF, 100_000);

    let per_tick = cfg.instr_per_tick as u64;
    let ticks = 100_000 / per_tick;
    let mut host = NullHost;
    let mut vm = Vm::new();
    for tick in 0..ticks / 2 {
        vm.halted = false;
        vm.run(
            &genome,
            &cfg,
            &RandCtx::new(0xBEEF, tick, 7),
            &mut host,
            per_tick as u32,
        );
    }
    let mut resumed = vm.clone();
    for tick in ticks / 2..ticks {
        resumed.halted = false;
        resumed.run(
            &genome,
            &cfg,
            &RandCtx::new(0xBEEF, tick, 7),
            &mut host,
            per_tick as u32,
        );
    }
    assert_eq!(resumed, whole);
}

#[test]
fn randomness_does_not_depend_on_execution_order() {
    // SPEC §11: a draw is a function of (seed, tick, cell_id, purpose, index) and nothing
    // else, so two cells in one tick cannot perturb each other whatever order rayon
    // schedules them in.
    let a = RandCtx::new(5, 100, 1);
    let b = RandCtx::new(5, 100, 2);
    let interleaved: Vec<i16> = (0..64)
        .flat_map(|i| [a.draw_i16(Purpose::Rand, i), b.draw_i16(Purpose::Rand, i)])
        .collect();
    for i in 0..64u64 {
        assert_eq!(interleaved[i as usize * 2], a.draw_i16(Purpose::Rand, i));
        assert_eq!(
            interleaved[i as usize * 2 + 1],
            b.draw_i16(Purpose::Rand, i)
        );
    }
}

#[test]
fn two_rands_in_one_tick_differ() {
    let genome = Genome::new(vec![Op::Rand.canonical_byte(); 8]).unwrap();
    let mut vm = Vm::new();
    let mut host = NullHost;
    vm.run(
        &genome,
        &VmConfig::DEFAULT,
        &RandCtx::new(1, 1, 1),
        &mut host,
        8,
    );
    assert_eq!(vm.rand_ctr, 8);
    let distinct: std::collections::BTreeSet<i16> = vm.data.iter().copied().collect();
    assert!(
        distinct.len() >= 7,
        "eight draws in one tick produced {} distinct values: {:?}",
        distinct.len(),
        vm.data
    );
}
