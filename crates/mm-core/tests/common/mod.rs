//! Shared harness for the M0 acceptance tests.
//!
//! Everything here derives from the deterministic hash of SPEC §11, so a failing case is
//! identified by its index alone and reproduces exactly.

#![allow(dead_code)]

use mm_core::genome::Genome;
use mm_core::host::Host;
use mm_core::rng::{Purpose, RandCtx};
use mm_core::vm::{Vm, CALL_STACK_LEN, DATA_STACK_LEN, RAM_WORDS, REGISTER_COUNT};
use mm_core::VmConfig;

/// Fixed seed for the harness itself, distinct from any scenario seed.
pub const HARNESS_SEED: u64 = 0x4D_61_6E_69_63_4D_4D_00;

fn ctx(salt: u64) -> RandCtx {
    RandCtx::new(HARNESS_SEED, salt, 0)
}

fn word(salt: u64, i: u64) -> u64 {
    ctx(salt).draw(Purpose::Harness, i)
}

/// A genome of 1..=`max_len` arbitrary bytes.
pub fn random_genome_bytes(salt: u64, max_len: usize) -> Vec<u8> {
    let len = 1 + ctx(salt).draw_below(Purpose::Harness, 0, max_len as u64) as usize;
    let mut out = Vec::with_capacity(len);
    // Eight bytes per draw: the fuzz spends most of its time here otherwise.
    let mut i = 0u64;
    while out.len() < len {
        let w = word(salt, i.wrapping_add(1));
        for shift in 0..8 {
            if out.len() == len {
                break;
            }
            out.push((w >> (shift * 8)) as u8);
        }
        i = i.wrapping_add(1);
    }
    out
}

/// An arbitrary VM state, including stack pointers and lengths that are out of range. A `Vm`
/// filled with nonsense is still a legal `Vm`, and proving that is half of what the fuzz is
/// for.
pub fn random_vm(salt: u64) -> Vm {
    let mut vm = Vm::new();
    let mut i = 100u64;
    let mut next = || {
        i = i.wrapping_add(1);
        word(salt, i)
    };
    for slot in vm.data.iter_mut() {
        *slot = next() as u16 as i16;
    }
    for slot in vm.call.iter_mut() {
        *slot = next() as u16;
    }
    for slot in vm.regs.iter_mut() {
        *slot = next() as u16 as i16;
    }
    for slot in vm.ram.iter_mut() {
        *slot = next() as u16 as i16;
    }
    vm.ip = next() as u16;
    vm.pa = next() as u16;
    vm.pb = next() as u16;
    vm.ln = next() as u16;
    vm.rand_ctr = next() as u32;
    vm.dsp = next() as u8;
    vm.dlen = next() as u8;
    vm.csp = next() as u8;
    vm.clen = next() as u8;
    vm.halted = next() & 1 == 1;
    vm
}

/// An arbitrary but legal configuration. Varying these widens what the fuzz covers: a search
/// range of 0 and a bind threshold of 8 are both legal scenario settings and both change
/// which paths the VM takes.
pub fn random_config(salt: u64) -> VmConfig {
    let c = ctx(salt);
    VmConfig {
        instr_per_tick: 1 + c.draw_below(Purpose::Harness, 900, 64) as u16,
        template_search_range: c.draw_below(Purpose::Harness, 901, 513) as u16,
        promoter_bind_threshold: c.draw_below(Purpose::Harness, 902, 9) as u16,
    }
}

/// Everything the VM's public contract promises about its own state.
///
/// Checked after every fuzz case: a panic is the loud failure, silent state corruption is
/// the quiet one, and only this catches the second.
pub fn assert_well_formed(vm: &Vm, genome: &Genome, what: &str) {
    assert!(
        (vm.ip as usize) < genome.len().max(1),
        "{what}: ip {} outside genome of {} bytes",
        vm.ip,
        genome.len()
    );
    assert!(
        (vm.dsp as usize) < DATA_STACK_LEN,
        "{what}: data stack pointer {} out of range",
        vm.dsp
    );
    assert!(
        (vm.dlen as usize) <= DATA_STACK_LEN,
        "{what}: data stack length {} out of range",
        vm.dlen
    );
    assert!(
        (vm.csp as usize) < CALL_STACK_LEN,
        "{what}: call stack pointer {} out of range",
        vm.csp
    );
    assert!(
        (vm.clen as usize) <= CALL_STACK_LEN,
        "{what}: call stack length {} out of range",
        vm.clen
    );
    assert_eq!(vm.regs.len(), REGISTER_COUNT);
    assert_eq!(vm.ram.len(), RAM_WORDS);
}

/// Run `total` instructions, tick by tick, exactly as the simulation will.
///
/// `HALT` yields the rest of a tick, so a fuzz case that ran a single `run` call would stop
/// at the first `HALT` — and with random bytes that is almost immediately, which would make
/// the whole test vacuous. Ticking keeps going, and returns the number actually executed.
pub fn run_instructions<H: Host>(
    vm: &mut Vm,
    genome: &Genome,
    cfg: &VmConfig,
    seed: u64,
    cell_id: u64,
    host: &mut H,
    total: u64,
) -> u64 {
    let per_tick = cfg.instr_per_tick.max(1) as u64;
    let mut done = 0u64;
    let mut tick = 0u64;
    while done < total {
        let budget = per_tick.min(total - done) as u32;
        let rand_ctx = RandCtx::new(seed, tick, cell_id);
        vm.halted = false;
        let ran = vm.run(genome, cfg, &rand_ctx, host, budget) as u64;
        // Guaranteed by construction, and the reason "no hangs" is structural: a budget of
        // at least one instruction always retires at least one.
        assert!(ran > 0, "no progress at tick {tick}");
        done = done.saturating_add(ran);
        tick = tick.wrapping_add(1);
    }
    done
}

/// Environment override for a test size, so the long acceptance runs are reachable from CI
/// without editing the source.
pub fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
