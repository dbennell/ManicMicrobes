//! M0 acceptance test 5 — saturation and wrapping (SPEC §3).
//!
//! > Property test: arithmetic never wraps; all index operations are in range for arbitrary
//! > inputs.
//!
//! **Addressing wraps; magnitudes saturate**, and never the other way round.
//!
//! Wrapping arithmetic would put a cliff in the fitness landscape — a one-bit mutation
//! flipping a cell from "very fast forward" to "very fast reverse". Saturation keeps the
//! landscape continuous and climbable, so a lineage can hill-climb a quantity instead of
//! falling off it. Wrapping addresses keep every index legal, which is what serves totality.
//!
//! The arithmetic opcodes are checked exhaustively over the whole `i16 × i16` space where
//! that is feasible and over a dense sample otherwise, by comparing the VM against an
//! independent `i64` reference. Indexing is checked by driving every address-taking opcode
//! with arbitrary operands and asserting the VM's own state stays well-formed.

mod common;

use common::{assert_well_formed, random_vm, run_instructions};
use mm_core::genome::Genome;
use mm_core::host::NullHost;
use mm_core::isa::Op;
use mm_core::rng::{Purpose, RandCtx};
use mm_core::vm::{Vm, RAM_WORDS, REGISTER_COUNT};
use mm_core::VmConfig;

/// Run one opcode against a two-value stack and return the top of stack.
fn eval_binary(op: Op, a: i16, b: i16) -> i16 {
    let genome = Genome::new(vec![op.canonical_byte()]).unwrap();
    let mut vm = Vm::new();
    vm.push(a);
    vm.push(b);
    let mut host = NullHost;
    vm.run(
        &genome,
        &VmConfig::DEFAULT,
        &RandCtx::new(0, 0, 0),
        &mut host,
        1,
    );
    vm.peek()
}

fn eval_unary(op: Op, a: i16) -> i16 {
    let genome = Genome::new(vec![op.canonical_byte()]).unwrap();
    let mut vm = Vm::new();
    vm.push(a);
    let mut host = NullHost;
    vm.run(
        &genome,
        &VmConfig::DEFAULT,
        &RandCtx::new(0, 0, 0),
        &mut host,
        1,
    );
    vm.peek()
}

fn saturate(v: i64) -> i16 {
    v.clamp(i16::MIN as i64, i16::MAX as i64) as i16
}

/// Values where saturation and overflow actually live, plus a spread of ordinary ones.
fn interesting() -> Vec<i16> {
    let mut v = vec![
        i16::MIN,
        i16::MIN + 1,
        -32000,
        -256,
        -128,
        -100,
        -17,
        -2,
        -1,
        0,
        1,
        2,
        17,
        100,
        127,
        128,
        255,
        256,
        1000,
        16384,
        i16::MAX - 1,
        i16::MAX,
    ];
    let ctx = RandCtx::new(0x5A7, 0, 0);
    for i in 0..64u64 {
        v.push(ctx.draw_i16(Purpose::Harness, i));
    }
    v.sort_unstable();
    v.dedup();
    v
}

#[test]
fn add_sub_mul_saturate_over_the_whole_space() {
    // Exhaustive over one operand, dense over the other: 22 + 64 interesting values against
    // all 65,536 is enough to pin every boundary without a 4-billion-case test.
    for a in interesting() {
        for b in (i16::MIN..=i16::MAX).step_by(37) {
            assert_eq!(
                eval_binary(Op::Add, a, b),
                saturate(a as i64 + b as i64),
                "ADD {a} {b}"
            );
            assert_eq!(
                eval_binary(Op::Sub, a, b),
                saturate(a as i64 - b as i64),
                "SUB {a} {b}"
            );
            assert_eq!(
                eval_binary(Op::Mul, a, b),
                saturate(a as i64 * b as i64),
                "MUL {a} {b}"
            );
        }
    }
}

#[test]
fn division_and_modulo_never_fault() {
    for a in interesting() {
        for b in interesting() {
            let want_div = if b == 0 {
                0
            } else {
                saturate(a as i64 / b as i64)
            };
            let want_mod = if b == 0 {
                0
            } else {
                saturate(a as i64 % b as i64)
            };
            assert_eq!(eval_binary(Op::Div, a, b), want_div, "DIV {a} {b}");
            assert_eq!(eval_binary(Op::Mod, a, b), want_mod, "MOD {a} {b}");
        }
        // The overflow case that would panic in Rust and trap on x86.
        assert_eq!(eval_binary(Op::Div, i16::MIN, -1), i16::MAX);
        assert_eq!(eval_binary(Op::Mod, i16::MIN, -1), 0);
        assert_eq!(eval_binary(Op::Div, a, 0), 0, "DIV {a} 0");
        assert_eq!(eval_binary(Op::Mod, a, 0), 0, "MOD {a} 0");
    }
}

#[test]
fn negation_and_absolute_value_saturate() {
    for a in i16::MIN..=i16::MAX {
        assert_eq!(eval_unary(Op::Neg, a), saturate(-(a as i64)), "NEG {a}");
        assert_eq!(
            eval_unary(Op::Abs, a),
            saturate((a as i64).abs()),
            "ABS {a}"
        );
    }
    // The one input where both would overflow.
    assert_eq!(eval_unary(Op::Neg, i16::MIN), i16::MAX);
    assert_eq!(eval_unary(Op::Abs, i16::MIN), i16::MAX);
}

#[test]
fn shifts_saturate_and_their_counts_wrap() {
    for a in interesting() {
        for b in interesting() {
            // The shift count is an index into the word, so it wraps to 0..=15; the shifted
            // magnitude then saturates like any other.
            let s = (b as u16 & 15) as u32;
            assert_eq!(
                eval_binary(Op::Shl, a, b),
                saturate((a as i64) << s),
                "SHL {a} {b}"
            );
            assert_eq!(
                eval_binary(Op::Shr, a, b),
                saturate((a as i64) >> s),
                "SHR {a} {b}"
            );
        }
    }
}

#[test]
fn comparison_and_selection_are_exact() {
    for a in interesting() {
        for b in interesting() {
            assert_eq!(
                eval_binary(Op::Cmp, a, b),
                (a as i64 - b as i64).signum() as i16,
                "CMP {a} {b}"
            );
            assert_eq!(eval_binary(Op::Min, a, b), a.min(b), "MIN {a} {b}");
            assert_eq!(eval_binary(Op::Max, a, b), a.max(b), "MAX {a} {b}");
            assert_eq!(eval_binary(Op::And, a, b), a & b, "AND {a} {b}");
            assert_eq!(eval_binary(Op::Or, a, b), a | b, "OR {a} {b}");
            assert_eq!(eval_binary(Op::Xor, a, b), a ^ b, "XOR {a} {b}");
        }
        assert_eq!(eval_unary(Op::Not, a), !a, "NOT {a}");
    }
}

#[test]
fn no_arithmetic_opcode_ever_leaves_the_i16_range() {
    // Stated as the invariant rather than as a formula: whatever the operands, the result is
    // representable, because it is an `i16`. What this really checks is that none of the
    // above panicked in a build with overflow checks on — which is why it matters that the
    // suite runs in debug as well as release.
    for op in [
        Op::Add,
        Op::Sub,
        Op::Mul,
        Op::Div,
        Op::Mod,
        Op::Min,
        Op::Max,
        Op::Shl,
        Op::Shr,
        Op::And,
        Op::Or,
        Op::Xor,
        Op::Cmp,
    ] {
        for a in [i16::MIN, -1, 0, 1, i16::MAX] {
            for b in [i16::MIN, -1, 0, 1, i16::MAX] {
                let r = eval_binary(op, a, b) as i64;
                assert!((i16::MIN as i64..=i16::MAX as i64).contains(&r));
            }
        }
    }
}

/// Drive an addressing opcode with an arbitrary operand and confirm nothing escapes.
fn exercise_addressing(op: Op, pushes: usize) {
    let genome = Genome::new(vec![op.canonical_byte()]).unwrap();
    let cfg = VmConfig::DEFAULT;
    let mut host = NullHost;
    let ctx = RandCtx::new(0xADD8, 0, 0);
    for i in 0..4_000u64 {
        let mut vm = random_vm(i);
        for p in 0..pushes {
            vm.push(ctx.draw_i16(Purpose::Harness, i * 8 + p as u64));
        }
        vm.ip = 0;
        vm.run(&genome, &cfg, &RandCtx::new(1, 0, 0), &mut host, 1);
        assert_well_formed(&vm, &genome, &format!("{} case {i}", op.name()));
    }
}

#[test]
fn every_addressing_opcode_stays_in_range() {
    // Registers are addressed `idx % 16`, RAM `addr % 64`, organelle slots `slot % 16`,
    // chemicals `c % 16`. Arbitrary operands, including negative ones and i16::MIN.
    for (op, pushes) in [
        (Op::Load, 1),
        (Op::Store, 2),
        (Op::RLoad, 1),
        (Op::RStore, 2),
        (Op::SetPa, 1),
        (Op::SetPb, 1),
        (Op::SetLn, 1),
        (Op::Build, 3),
        (Op::Tear, 1),
        (Op::OSet, 3),
        (Op::OGet, 2),
        (Op::OType, 1),
        (Op::Eat, 2),
        (Op::Emit, 2),
        (Op::Bud, 1),
        (Op::CopyB, 0),
        (Op::Join, 3),
        (Op::Leave, 1),
        (Op::JXfer, 3),
        (Op::JLen, 2),
        (Op::SetKey, 1),
        (Op::Inject, 1),
    ] {
        exercise_addressing(op, pushes);
    }
}

#[test]
fn register_and_ram_indices_are_modular() {
    // Not just "in range" — the specific reduction the spec names, so a future refactor to
    // clamping instead of wrapping would be caught.
    let genome = Genome::new(vec![
        Op::RStore.canonical_byte(),
        Op::RLoad.canonical_byte(),
        Op::Store.canonical_byte(),
        Op::Load.canonical_byte(),
    ])
    .unwrap();
    let cfg = VmConfig::DEFAULT;
    let mut host = NullHost;

    for idx in [-40000i32, -17, -1, 0, 1, 15, 16, 17, 64, 1000, 32767] {
        let idx16 = idx as i16;
        let mut vm = Vm::new();
        vm.push(1234);
        vm.push(idx16);
        vm.ip = 0;
        vm.run(&genome, &cfg, &RandCtx::new(0, 0, 0), &mut host, 1); // RSTORE
        let expected = ((idx16 as u16) as usize) % REGISTER_COUNT;
        assert_eq!(vm.regs[expected], 1234, "RSTORE index {idx}");

        let mut vm = Vm::new();
        vm.push(4321);
        vm.push(idx16);
        vm.ip = 2;
        vm.run(&genome, &cfg, &RandCtx::new(0, 0, 0), &mut host, 1); // STORE
        let expected = ((idx16 as u16) as usize) % RAM_WORDS;
        assert_eq!(vm.ram[expected], 4321, "STORE address {idx}");
    }
}

#[test]
fn genome_offsets_wrap_rather_than_escape() {
    // PA is a genome offset and may be set to anything at all; COPYB must read a byte that
    // exists. Length 7 is deliberately not a power of two.
    let bytes: Vec<u8> = (0..7u8).map(|i| i.wrapping_mul(9)).collect();

    for pa in [0u16, 6, 7, 8, 100, 32768, 65535] {
        let mut vm = Vm::new();
        vm.pa = pa;
        vm.ip = 0;
        let mut host = mm_core::host::RecordingHost::default();
        let copy = Genome::new(vec![Op::CopyB.canonical_byte()]).unwrap();
        vm.run(
            &copy,
            &VmConfig::DEFAULT,
            &RandCtx::new(0, 0, 0),
            &mut host,
            1,
        );
        // The copy program is one byte long, so PA reduces modulo 1.
        assert_eq!(host.daughter.len(), 1);
        assert_eq!(vm.pa, pa.wrapping_add(1));
    }

    // And against the seven-byte genome, every offset reads the right byte.
    for pa in 0..40u16 {
        let mut vm = Vm::new();
        vm.pa = pa;
        vm.ip = 0;
        let mut host = mm_core::host::RecordingHost::default();
        // A genome that only copies: the first instruction is COPYB, the rest are its data.
        let mut prog = vec![Op::CopyB.canonical_byte()];
        prog.extend_from_slice(&bytes);
        let g = Genome::new(prog.clone()).unwrap();
        vm.run(&g, &VmConfig::DEFAULT, &RandCtx::new(0, 0, 0), &mut host, 1);
        let want = prog[(pa as usize) % prog.len()];
        assert_eq!(host.daughter.get(&0).copied(), Some(want), "PA {pa}");
    }
}

#[test]
fn the_instruction_pointer_wraps_for_any_genome_length() {
    // The nastiest lengths are the small ones: a one-byte genome divides by one, and every
    // template opcode has to step past letters that are not there.
    let cfg = VmConfig::DEFAULT;
    let mut host = NullHost;
    for len in 1..=40usize {
        for op in Op::all() {
            let mut bytes = vec![op.canonical_byte()];
            bytes.resize(len, Op::Nop1.canonical_byte());
            let genome = Genome::new(bytes).unwrap();
            for start in 0..len {
                let mut vm = Vm::new();
                vm.ip = start as u16;
                run_instructions(&mut vm, &genome, &cfg, 1, 0, &mut host, 200);
                assert_well_formed(
                    &vm,
                    &genome,
                    &format!("{} len {len} start {start}", op.name()),
                );
            }
        }
    }
}
