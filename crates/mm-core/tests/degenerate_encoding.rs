//! M0 acceptance test 4 — degenerate encoding (SPEC §4.2).
//!
//! > For all `b` in `0..=255`, opcode dispatch equals `b % 64`.
//!
//! Four byte values map to each of the 64 opcodes, mirroring codon degeneracy. A large
//! fraction of point mutations are therefore **synonymous** and produce no phenotypic
//! change, which gives both a smoother search landscape and a molecular clock the phylogeny
//! layer can read at M5. If dispatch ever stopped being exactly `b % 64` — a lookup table
//! with a hole in it, a `match` arm that forgot the high bytes — synonymous sites would stop
//! being synonymous and both of those would quietly break.

mod common;

use mm_core::genome::Genome;
use mm_core::host::NullHost;
use mm_core::isa::{Op, OPCODE_COUNT};
use mm_core::rng::RandCtx;
use mm_core::state_hash::StateHash;
use mm_core::vm::Vm;
use mm_core::VmConfig;

#[test]
fn dispatch_is_byte_modulo_sixty_four() {
    for b in 0..=255u8 {
        let op = Op::from_byte(b);
        assert_eq!(
            op.canonical_byte(),
            b % OPCODE_COUNT,
            "byte {b:#04x} dispatched to {}",
            op.name()
        );
    }
}

#[test]
fn every_opcode_has_exactly_four_encodings() {
    let mut counts = std::collections::BTreeMap::new();
    for b in 0..=255u8 {
        *counts.entry(Op::from_byte(b)).or_insert(0u32) += 1;
    }
    assert_eq!(counts.len(), OPCODE_COUNT as usize);
    for (op, n) in counts {
        assert_eq!(n, 4, "{} has {n} encodings", op.name());
    }
}

#[test]
fn synonymous_substitutions_are_phenotypically_silent() {
    // The property that matters, stated end to end: replacing every byte of a genome with a
    // different encoding of the same opcode must not change what the genome does. Run the
    // whole VM over all four variant sets and compare state hashes.
    let base: Vec<u8> = (0..=255u8).chain(0..=255u8).collect();
    let cfg = VmConfig::DEFAULT;

    let mut reference: Option<u64> = None;
    for shift in 0..4u8 {
        let bytes: Vec<u8> = base
            .iter()
            .map(|b| (b % OPCODE_COUNT) + shift * OPCODE_COUNT)
            .collect();
        let genome = Genome::new(bytes).unwrap();
        let mut vm = Vm::new();
        let mut host = NullHost;
        let mut tick = 0u64;
        while tick < 400 {
            vm.halted = false;
            vm.run(&genome, &cfg, &RandCtx::new(11, tick, 3), &mut host, 16);
            tick += 1;
        }
        let hash = vm.state_hash();
        match reference {
            None => reference = Some(hash),
            Some(r) => assert_eq!(hash, r, "variant set {shift} behaved differently"),
        }
    }
}

#[test]
fn degenerate_nop_bytes_are_template_letters() {
    // Template scanning decodes bytes the same way execution does, so 0x41 is as good a
    // NOP1 as 0x01. A mutation that swaps one for the other must not silently truncate a
    // template and send a jump somewhere else.
    let canonical = Genome::new(vec![0x02, 0x01, 0x00, 0x01, 0x10]).unwrap();
    let degenerate = Genome::new(vec![0x42, 0x41, 0x80, 0xC1, 0xD0]).unwrap();
    assert_eq!(canonical.template_at(1), degenerate.template_at(1));
    assert_eq!(canonical.template_at(1).len, 3);
}

#[test]
fn promoter_tables_ignore_the_encoding() {
    let canonical = Genome::new(vec![0x26, 0x01, 0x01, 0x10]).unwrap();
    let degenerate = Genome::new(vec![0xE6, 0x41, 0x81, 0x10]).unwrap();
    assert_eq!(canonical.promoters().len(), 1);
    assert_eq!(canonical.promoters(), degenerate.promoters());
}
