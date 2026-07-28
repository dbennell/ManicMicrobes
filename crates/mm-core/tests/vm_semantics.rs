//! Behavioural tests for the VM: that it does the right thing, not merely that it never
//! panics doing it.
//!
//! The totality fuzz proves the VM cannot break. These prove it works.

mod common;

use mm_core::genome::Genome;
use mm_core::host::{Host, NullHost, RecordingHost, INJECT_SELF};
use mm_core::isa::{Op, Template};
use mm_core::rng::RandCtx;
use mm_core::vm::{Vm, CALL_STACK_LEN, DATA_STACK_LEN};
use mm_core::VmConfig;

fn genome(bytes: Vec<u8>) -> Genome {
    Genome::new(bytes).expect("genome")
}

/// Run `n` instructions in one call, from a fresh VM.
fn run(g: &Genome, n: u32) -> (Vm, RecordingHost) {
    let mut vm = Vm::new();
    let mut host = RecordingHost::default();
    vm.run(g, &VmConfig::DEFAULT, &RandCtx::new(0, 0, 0), &mut host, n);
    (vm, host)
}

fn ops(list: &[Op]) -> Vec<u8> {
    list.iter().map(|o| o.canonical_byte()).collect()
}

/// Template letters, least-significant bit first (SPEC §4.3).
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

/// Opcode plus its template letters.
fn with_template(op: Op, t: Template) -> Vec<u8> {
    let mut v = vec![op.canonical_byte()];
    v.extend(letters(t));
    v
}

// ---------------------------------------------------------------- circular stacks

#[test]
fn popping_an_empty_stack_yields_zero() {
    let (vm, _) = run(&genome(ops(&[Op::Add])), 1);
    assert_eq!(vm.peek(), 0);
    assert_eq!(vm.dlen, 1);
}

#[test]
fn pushing_a_full_stack_overwrites_the_oldest_entry() {
    let mut vm = Vm::new();
    for i in 0..DATA_STACK_LEN as i16 {
        vm.push(i);
    }
    assert_eq!(vm.dlen as usize, DATA_STACK_LEN);
    vm.push(999);
    assert_eq!(
        vm.dlen as usize, DATA_STACK_LEN,
        "depth is capped, not grown"
    );
    assert_eq!(vm.peek(), 999);
    // Popping back down reaches the survivors, and never the entry that was overwritten.
    let mut seen = Vec::new();
    for _ in 0..DATA_STACK_LEN {
        seen.push(vm.pop());
    }
    assert_eq!(vm.dlen, 0);
    assert!(
        !seen.contains(&0),
        "the oldest entry should have been lost: {seen:?}"
    );
    assert_eq!(vm.pop(), 0, "and past the bottom is zero");
}

#[test]
fn the_call_stack_is_circular_too() {
    // Nine nested calls on an eight-deep stack: the outermost return is lost, and nothing
    // faults.
    let mut bytes = Vec::new();
    for _ in 0..CALL_STACK_LEN + 1 {
        bytes.extend(with_template(Op::Call, Template::new(2, 0b01)));
        bytes.push(Op::Nop1.canonical_byte());
        bytes.push(Op::Nop0.canonical_byte());
        bytes.push(Op::Add.canonical_byte());
    }
    let g = genome(bytes);
    let (vm, _) = run(&g, 64);
    assert!(vm.clen as usize <= CALL_STACK_LEN);
}

#[test]
fn stack_ops_have_their_documented_effects() {
    let cases: &[(&[Op], &[i16])] = &[
        (&[Op::One, Op::Dup], &[1, 1]),
        (&[Op::One, Op::Zero, Op::Swap], &[0, 1]),
        (&[Op::One, Op::Zero, Op::Over], &[1, 0, 1]),
        (&[Op::One, Op::Zero, Op::One, Op::Rot], &[0, 1, 1]),
        (&[Op::One, Op::Drop], &[]),
    ];
    for (program, want) in cases {
        let g = genome(ops(program));
        let (mut vm, _) = run(&g, program.len() as u32);
        let mut got = Vec::new();
        while vm.dlen > 0 {
            got.push(vm.pop());
        }
        got.reverse();
        assert_eq!(&got, want, "{program:?}");
    }
}

// ---------------------------------------------------------------- templates

#[test]
fn imm_pushes_the_template_value_and_zero_when_empty() {
    let (vm, _) = run(&genome(with_template(Op::Imm, Template::new(5, 21))), 1);
    assert_eq!(vm.peek(), 21);
    // Zero-length: still pushes, with value 0. The one exception SPEC §4.3 names.
    let (vm, _) = run(&genome(ops(&[Op::Imm, Op::Add])), 1);
    assert_eq!(vm.peek(), 0);
    assert_eq!(vm.dlen, 1);
    assert_eq!(vm.ip, 1, "an empty template consumes no letters");
}

#[test]
fn a_zero_length_template_still_applies_the_stack_effect() {
    // SPEC §4.3 says a zero-length template makes its host a no-op, and IMM the exception.
    // Read strictly, that would make JMPZ sometimes consume its operand and sometimes not,
    // which puts a stack-balance cliff in the mutation landscape for no gain. So: the
    // documented stack effect always happens; what a zero-length template suppresses is the
    // search. IMM pushing 0 is the same rule, not an exception to it.
    let g = genome(ops(&[Op::One, Op::JmpZ, Op::Add]));
    let (vm, _) = run(&g, 2);
    assert_eq!(vm.dlen, 0, "JMPZ consumed its operand");
    assert_eq!(vm.ip, 2, "and did not jump");
}

#[test]
fn jumps_find_the_complementary_template() {
    // JMPF %110 searches forward for %001.
    let mut bytes = with_template(Op::JmpF, Template::new(3, 0b011));
    bytes.push(Op::Zero.canonical_byte()); // skipped
    bytes.extend([
        Op::Nop0.canonical_byte(),
        Op::Nop0.canonical_byte(),
        Op::Nop1.canonical_byte(),
    ]); // %001 = complement of %011
    bytes.push(Op::One.canonical_byte()); // landed here
    let g = genome(bytes);
    let (vm, _) = run(&g, 2);
    assert_eq!(vm.peek(), 1, "did not land past the matched template");
    assert_eq!(vm.dlen, 1, "the skipped ZERO did not run");
}

#[test]
fn a_jump_that_finds_nothing_is_a_no_op() {
    let mut bytes = with_template(Op::JmpF, Template::new(4, 0b1111));
    bytes.push(Op::One.canonical_byte());
    let g = genome(bytes);
    let (vm, _) = run(&g, 2);
    assert_eq!(vm.peek(), 1, "execution should fall through the jump");
}

#[test]
fn the_search_range_bounds_how_far_a_jump_looks() {
    // The same genome, two configurations: the target is 40 bytes away, so a range of 8
    // cannot reach it and a range of 512 can.
    let mut bytes = with_template(Op::JmpF, Template::new(3, 0b011));
    bytes.resize(40, Op::Drop.canonical_byte());
    bytes.extend([
        Op::Nop0.canonical_byte(),
        Op::Nop0.canonical_byte(),
        Op::Nop1.canonical_byte(),
    ]);
    bytes.push(Op::One.canonical_byte());
    let g = genome(bytes);

    for (range, should_find) in [(8u16, false), (512, true)] {
        let cfg = mm_core::VmConfig {
            template_search_range: range,
            ..VmConfig::DEFAULT
        };
        let mut vm = Vm::new();
        let mut host = NullHost;
        vm.run(&g, &cfg, &RandCtx::new(0, 0, 0), &mut host, 1);
        // The complement sits at 40..42, so a hit resumes at 43, on the ONE.
        assert_eq!(
            vm.ip == 43,
            should_find,
            "range {range} landed at {}",
            vm.ip
        );
    }
}

#[test]
fn the_search_wraps_but_never_scans_a_short_genome_twice() {
    // probes = min(range, genome length), so a 6-byte genome is scanned once round even
    // with the default 512-byte range — and a backward jump can therefore reach a target
    // that is physically ahead of it.
    let mut bytes = vec![
        Op::Nop0.canonical_byte(),
        Op::Nop1.canonical_byte(),
        Op::One.canonical_byte(),
        Op::Halt.canonical_byte(),
    ];
    let jump_at = bytes.len();
    bytes.extend(with_template(Op::JmpB, Template::new(2, 0b01)));
    let g = genome(bytes);

    let mut vm = Vm::new();
    vm.ip = jump_at as u16;
    let mut host = NullHost;
    vm.run(&g, &VmConfig::DEFAULT, &RandCtx::new(0, 0, 0), &mut host, 1);
    assert_eq!(
        vm.ip, 2,
        "the backward search should have wrapped to offset 0"
    );
}

#[test]
fn skipz_steps_over_an_instruction_and_its_template() {
    let mut bytes = ops(&[Op::Zero, Op::SkipZ]);
    bytes.extend(with_template(Op::Imm, Template::new(6, 0b111111))); // skipped entirely
    bytes.push(Op::One.canonical_byte());
    let g = genome(bytes);
    let (vm, _) = run(&g, 3);
    assert_eq!(vm.peek(), 1);
    assert_eq!(vm.dlen, 1, "the skipped IMM must not have pushed");
}

// ---------------------------------------------------------------- promoters

fn promoter_genome(promoters: &[u8], query: u8) -> Genome {
    // EXPRESS <query>, HALT, then one gene per promoter, each returning its own index.
    let mut bytes = with_template(Op::Express, Template::new(8, query));
    bytes.push(Op::Halt.canonical_byte());
    for (i, p) in promoters.iter().enumerate() {
        bytes.extend(with_template(Op::Gene, Template::new(8, *p)));
        bytes.extend(with_template(Op::Imm, Template::new(8, i as u8 + 1)));
        bytes.push(Op::Ret.canonical_byte());
    }
    genome(bytes)
}

#[test]
fn express_binds_the_nearest_promoter() {
    // Query 0b0000_0000 against promoters at distance 1, 0 and 2: the exact match wins.
    let g = promoter_genome(&[0b0000_0001, 0b0000_0000, 0b0000_0011], 0b0000_0000);
    let (vm, _) = run(&g, 4);
    assert_eq!(vm.peek(), 2, "should have called the second gene");
}

#[test]
fn express_binds_an_inexact_promoter_within_the_threshold() {
    // Nothing matches exactly; the closest is distance 1.
    let g = promoter_genome(&[0b0001_0000, 0b0000_0001, 0b0011_0011], 0b0000_0000);
    let (vm, _) = run(&g, 4);
    assert!(matches!(vm.peek(), 1 | 2), "bound gene {}", vm.peek());
}

#[test]
fn express_ties_resolve_to_the_lowest_offset() {
    // Two promoters at the same distance. SPEC §4.4: the earlier one wins.
    let g = promoter_genome(&[0b0000_0001, 0b0000_0010], 0b0000_0000);
    let (vm, _) = run(&g, 4);
    assert_eq!(vm.peek(), 1);
}

#[test]
fn express_beyond_the_threshold_is_a_no_op() {
    // Distance 4 against a threshold of 2.
    let g = promoter_genome(&[0b0000_1111], 0b0000_0000);
    let (vm, _) = run(&g, 1);
    assert_eq!(vm.ip, 9, "should have stepped past its own template");
    assert_eq!(vm.dlen, 0);
    assert_eq!(vm.clen, 0, "and pushed no return address");
}

#[test]
fn deleting_a_gene_rebinds_its_callers_rather_than_orphaning_them() {
    // The property that makes promoter binding worth the complexity: EXPRESS survives the
    // loss of the gene it used to call.
    let intact = promoter_genome(&[0b0000_0000, 0b0000_0001], 0b0000_0000);
    let (vm, _) = run(&intact, 4);
    assert_eq!(vm.peek(), 1);

    let deleted = promoter_genome(&[0b0000_0001], 0b0000_0000);
    let (vm, _) = run(&deleted, 4);
    assert_eq!(
        vm.peek(),
        1,
        "the caller should have bound the next-best match"
    );
}

#[test]
fn express_behaves_as_a_call() {
    let g = promoter_genome(&[0b0000_0000], 0b0000_0000);
    let mut vm = Vm::new();
    let mut host = NullHost;
    vm.run(&g, &VmConfig::DEFAULT, &RandCtx::new(0, 0, 0), &mut host, 1);
    assert_eq!(vm.clen, 1, "EXPRESS should push a return address");
    assert_eq!(
        vm.call[vm.csp as usize], 9,
        "returning past its own template"
    );
}

// ---------------------------------------------------------------- replication

/// `genomes/replicator.mm`, built here so that `mm-core` stays independent of the toolchain.
/// The assembled form is exercised in `mm-asm`'s round-trip test.
fn replicator() -> Genome {
    let mut bytes = with_template(Op::Gene, Template::new(8, 0b1011_0010));
    bytes.extend(ops(&[
        Op::GLen,
        Op::SetLn,
        Op::GLen,
        Op::Bud,
        Op::Drop,
        Op::Zero,
        Op::SetPa,
        Op::Zero,
        Op::SetPb,
    ]));
    // The loop label, then COPYB — so the label's run stops at four letters.
    const LOOP: Template = Template {
        len: 4,
        value: 0b1011,
    };
    let loop_at = bytes.len();
    bytes.extend(letters(LOOP));
    bytes.push(Op::CopyB.canonical_byte());
    bytes.extend(with_template(Op::LoopLn, LOOP.complement()));
    bytes.push(Op::Split.canonical_byte());
    let g = genome(bytes);
    assert_eq!(g.template_at(loop_at).len, 4, "the loop label's run fused");
    g
}

/// Step until the genome divides, so the assertions describe one replication rather than
/// however many happened to fit in a fixed instruction count.
fn run_one_replication(g: &Genome) -> (Vm, RecordingHost, u32) {
    let mut vm = Vm::new();
    let mut host = RecordingHost::default();
    let mut n = 0u32;
    while host.splits == 0 {
        vm.halted = false;
        vm.run(g, &VmConfig::DEFAULT, &RandCtx::new(1, 0, 0), &mut host, 1);
        n += 1;
        assert!(n < 10_000, "the replicator never reached SPLIT");
    }
    (vm, host, n)
}

#[test]
fn the_replicator_copies_itself_exactly() {
    let g = replicator();
    let (_, host, instructions) = run_one_replication(&g);

    assert_eq!(
        host.bud_calls,
        vec![g.len() as i16],
        "BUD asked for one genome"
    );
    assert_eq!(
        host.daughter_bytes(),
        g.bytes(),
        "the daughter is not a copy of the parent"
    );
    assert_eq!(host.splits, 1);
    // Ten instructions of preamble, the label's four letters on the way in, then two per
    // byte copied, then SPLIT. Cheap enough that random bytes can reach it (SPEC §5.2).
    assert_eq!(instructions as usize, 15 + 2 * g.len());
}

#[test]
fn loopln_stops_when_the_counter_reaches_zero() {
    let g = replicator();
    let (vm, _, _) = run_one_replication(&g);
    assert_eq!(vm.ln, 0);
    assert_eq!(vm.pa as usize, g.len(), "PA advanced exactly once per byte");
}

#[test]
fn copyb_saturates_its_counter_rather_than_wrapping() {
    // LN at zero must stay at zero, or a copy loop would run 65,536 more times.
    let g = genome(ops(&[Op::CopyB]));
    let mut vm = Vm::new();
    vm.ln = 0;
    let mut host = RecordingHost::default();
    vm.run(&g, &VmConfig::DEFAULT, &RandCtx::new(0, 0, 0), &mut host, 1);
    assert_eq!(vm.ln, 0);
}

#[test]
fn bud_resets_the_destination_pointer() {
    let g = genome(ops(&[Op::Bud]));
    let mut vm = Vm::new();
    vm.pb = 4321;
    let mut host = RecordingHost::default();
    vm.run(&g, &VmConfig::DEFAULT, &RandCtx::new(0, 0, 0), &mut host, 1);
    assert_eq!(vm.pb, 0);
}

#[test]
fn inject_moves_the_same_pointers_as_copyb() {
    // SPEC §8.3: reading and writing genome bytes is one interface whether the target is
    // self or a neighbour. That is why viruses are emergent rather than implemented, and it
    // only holds if INJECT advances PA, PB and LN exactly as COPYB does.
    #[derive(Default)]
    struct Injected(Vec<(i16, u16, u8)>);
    impl Host for Injected {
        fn inject(&mut self, jidx: i16, dst: u16, src: u8) -> i16 {
            self.0.push((jidx, dst, src));
            1
        }
    }

    let mut bytes = ops(&[Op::Inject]);
    bytes.extend([7u8, 8, 9]);
    let g = genome(bytes);

    let mut vm = Vm::new();
    vm.pa = 1;
    vm.pb = 100;
    vm.ln = 3;
    vm.push(INJECT_SELF);
    let mut host = Injected::default();
    vm.run(&g, &VmConfig::DEFAULT, &RandCtx::new(0, 0, 0), &mut host, 1);

    assert_eq!(host.0, vec![(INJECT_SELF, 100, 7)]);
    assert_eq!((vm.pa, vm.pb, vm.ln), (2, 101, 2));
    assert_eq!(vm.peek(), 1, "and pushes the host's result");
}

// ---------------------------------------------------------------- world opcodes

#[test]
fn world_opcodes_have_the_stack_effects_the_isa_documents() {
    // Under the null host nothing happens, but the arity must still be right: a genome's
    // stack discipline cannot depend on whether a world is present, or an M0 fuzz case and
    // an M2 cell would diverge on the same bytes.
    let cases: &[(Op, usize, usize)] = &[
        (Op::Build, 3, 0),
        (Op::Tear, 1, 0),
        (Op::OSet, 3, 0),
        (Op::OGet, 2, 1),
        (Op::OType, 1, 1),
        (Op::Eat, 2, 1),
        (Op::Emit, 2, 1),
        (Op::Bud, 1, 1),
        (Op::CopyB, 0, 0),
        (Op::Split, 0, 0),
        (Op::Join, 3, 1),
        (Op::Leave, 1, 0),
        (Op::JXfer, 3, 1),
        (Op::JLen, 2, 0),
        (Op::SetKey, 1, 0),
        (Op::Inject, 1, 1),
    ];
    for (op, pops, pushes) in cases {
        let g = genome(ops(&[*op]));
        let mut vm = Vm::new();
        for i in 0..8i16 {
            vm.push(i);
        }
        let before = vm.dlen as i32;
        let mut host = NullHost;
        vm.run(&g, &VmConfig::DEFAULT, &RandCtx::new(0, 0, 0), &mut host, 1);
        let want = before - *pops as i32 + *pushes as i32;
        assert_eq!(vm.dlen as i32, want, "{} stack effect", op.name());
    }
}

#[test]
fn setkey_masks_to_seven_bits() {
    for v in [-1i16, 0, 127, 128, 255, i16::MIN, i16::MAX] {
        let g = genome(ops(&[Op::SetKey]));
        let mut vm = Vm::new();
        vm.push(v);
        let mut host = RecordingHost::default();
        vm.run(&g, &VmConfig::DEFAULT, &RandCtx::new(0, 0, 0), &mut host, 1);
        let key = host.key.expect("SETKEY should have reached the host");
        assert_eq!(key, (v as u16 & 0x7F) as u8, "SETKEY {v}");
        assert!(key <= 127);
    }
}

#[test]
fn reserved_opcodes_are_no_ops() {
    for op in [Op::Reserved0, Op::Reserved1] {
        let g = genome(ops(&[op, Op::One]));
        let (vm, _) = run(&g, 2);
        assert_eq!(vm.peek(), 1, "{} should have done nothing", op.name());
        assert_eq!(vm.dlen, 1);
    }
}

// ---------------------------------------------------------------- halting

#[test]
fn halt_yields_the_rest_of_the_tick() {
    let g = genome(ops(&[Op::One, Op::Halt, Op::One, Op::One]));
    let mut vm = Vm::new();
    let mut host = NullHost;
    let ran = vm.tick(&g, &VmConfig::DEFAULT, &RandCtx::new(0, 0, 0), &mut host);
    assert_eq!(ran, 2, "should have stopped at HALT");
    assert!(vm.halted);
    assert_eq!(vm.ip, 2, "and resumes after it");

    // The next tick picks up where it left off: ONE, ONE, then round the end of the genome
    // to the ONE and HALT again.
    let ran = vm.tick(&g, &VmConfig::DEFAULT, &RandCtx::new(0, 0, 0), &mut host);
    assert_eq!(ran, 4);
    assert_eq!(vm.ip, 2);
    assert!(vm.halted, "and halted again");
}

#[test]
fn an_empty_genome_executes_nothing_and_terminates() {
    let g = Genome::empty();
    let mut vm = Vm::new();
    vm.ip = 12345;
    let mut host = NullHost;
    let ran = vm.run(
        &g,
        &VmConfig::DEFAULT,
        &RandCtx::new(0, 0, 0),
        &mut host,
        16,
    );
    assert_eq!(
        ran, 16,
        "the budget is reported spent so callers make progress"
    );
    assert_eq!(vm.ip, 0, "and the VM is left well-formed");
}
