//! The seam between the VM and the world.
//!
//! Opcodes `0x30`–`0x3F` act on a body and a substrate that do not exist until M2 and M7.
//! Their *stack effects* are part of the ISA and are executed by the VM regardless, because
//! a genome's stack discipline must not depend on whether the world is present — otherwise
//! an M0 fuzz case and an M2 cell would diverge on the same bytes.
//!
//! So the VM pops and pushes exactly what SPEC §5.1 says, and routes the effect through this
//! trait. Every method has a default that does nothing and reports nothing, which is what
//! [`NullHost`] uses. M2 implements these against the cell arena; nothing about the VM
//! changes when it does.
//!
//! Values crossing this boundary are cell-visible `i16` (SPEC §3). Implementors must treat
//! every argument as arbitrary: indices reduce modulo their range, magnitudes saturate, and
//! no input may panic.

/// The world, as a running genome can affect it.
#[allow(unused_variables)]
pub trait Host {
    /// `BUILD ( param type slot -- )` — begin constructing an organelle.
    fn build(&mut self, param: i16, ty: i16, slot: i16) {}

    /// `TEAR ( slot -- )` — dismantle, recovering part of its matter.
    fn tear(&mut self, slot: i16) {}

    /// `OSET ( v idx slot -- )` — write an organelle control input.
    fn oset(&mut self, v: i16, idx: i16, slot: i16) {}

    /// `OGET ( idx slot -- v )` — read an organelle output. Slot 0 is the membrane, which
    /// is the cell's self-sensor (SPEC §5.1).
    fn oget(&mut self, idx: i16, slot: i16) -> i16 {
        0
    }

    /// `OTYPE ( slot -- type )` — the organelle catalogue index in a slot.
    fn otype(&mut self, slot: i16) -> i16 {
        0
    }

    /// `EAT ( amount chem -- got )` — ingest from the local fluid.
    fn eat(&mut self, amount: i16, chem: i16) -> i16 {
        0
    }

    /// `EMIT ( amount chem -- sent )` — excrete to the local fluid.
    fn emit(&mut self, amount: i16, chem: i16) -> i16 {
        0
    }

    /// `BUD ( size -- ok )` — allocate the daughter genome buffer. The VM sets `PB = 0`
    /// whether or not this succeeds.
    fn bud(&mut self, size: i16) -> i16 {
        0
    }

    /// `COPYB` — write one byte into the daughter buffer. The VM has already read `src`
    /// from the parent genome at `PA` and advances `PA`, `PB` and `LN` itself.
    fn copy_byte(&mut self, dst: u16, src: u8) {}

    /// `SPLIT` — finalise division.
    fn split(&mut self) {}

    /// `JOIN ( key kind handle -- ok )` — attempt a junction (SPEC §8.2). A failed attempt
    /// must return failure and nothing else: leaking the Hamming distance to the true
    /// receptor key makes it hill-climbable in about seven probes.
    fn join(&mut self, key: i16, kind: i16, handle: i16) -> i16 {
        0
    }

    /// `LEAVE ( jidx -- )` — dissolve a junction.
    fn leave(&mut self, jidx: i16) {}

    /// `JXFER ( amount what jidx -- moved )` — transfer over a soft junction.
    fn jxfer(&mut self, amount: i16, what: i16, jidx: i16) -> i16 {
        0
    }

    /// `JLEN ( v jidx -- )` — offset a hard junction's rest length. This is muscle.
    fn jlen(&mut self, v: i16, jidx: i16) {}

    /// `SETKEY ( v -- )` — set this cell's 7-bit receptor key. Already masked to `0..=127`.
    fn set_key(&mut self, key: u8) {}

    /// `INJECT ( jidx -- ok )` — write one byte into a target nucleus. `jidx` selects a soft
    /// junction, or [`INJECT_SELF`] for this cell's own genome; reading and writing genome
    /// bytes is the same interface either way (SPEC §8.3), which is why viruses are
    /// emergent rather than implemented.
    ///
    /// As with `COPYB`, the VM has read `src` at `PA` and advances `PA`, `PB` and `LN`.
    fn inject(&mut self, jidx: i16, dst: u16, src: u8) -> i16 {
        0
    }
}

/// The reserved junction index meaning "my own nucleus" (SPEC §8.3).
pub const INJECT_SELF: i16 = -1;

/// A world that does not exist. Every effect is discarded and every read returns 0.
///
/// This is what M0 runs against, and it is also the right host for assembling, disassembling
/// or fuzzing a genome outside a simulation.
#[derive(Clone, Copy, Default, Debug)]
pub struct NullHost;

impl Host for NullHost {}

/// Records what a genome tried to do, without doing any of it.
///
/// Useful for testing genomes at M0 — the replication loop in SPEC §5.2 can be checked for
/// producing the right daughter bytes long before there is a cell to divide.
#[derive(Clone, Default, Debug)]
pub struct RecordingHost {
    /// Bytes written by `COPYB`, indexed by the `PB` they were written to.
    pub daughter: std::collections::BTreeMap<u16, u8>,
    pub bud_calls: Vec<i16>,
    pub splits: u32,
    pub emits: Vec<(i16, i16)>,
    pub eats: Vec<(i16, i16)>,
    pub key: Option<u8>,
    /// What `OGET` should return, by `(slot, idx)`. Anything unlisted reads as 0.
    pub oget_values: std::collections::BTreeMap<(i16, i16), i16>,
}

impl Host for RecordingHost {
    fn oget(&mut self, idx: i16, slot: i16) -> i16 {
        self.oget_values.get(&(slot, idx)).copied().unwrap_or(0)
    }

    fn eat(&mut self, amount: i16, chem: i16) -> i16 {
        self.eats.push((amount, chem));
        amount
    }

    fn emit(&mut self, amount: i16, chem: i16) -> i16 {
        self.emits.push((amount, chem));
        amount
    }

    fn bud(&mut self, size: i16) -> i16 {
        self.bud_calls.push(size);
        1
    }

    fn copy_byte(&mut self, dst: u16, src: u8) {
        self.daughter.insert(dst, src);
    }

    fn split(&mut self) {
        self.splits = self.splits.saturating_add(1);
    }

    fn set_key(&mut self, key: u8) {
        self.key = Some(key);
    }
}

impl RecordingHost {
    /// The daughter buffer as a contiguous byte string, from offset 0 up to the highest
    /// offset written. Gaps read as 0.
    #[must_use]
    pub fn daughter_bytes(&self) -> Vec<u8> {
        let Some(max) = self.daughter.keys().copied().max() else {
            return Vec::new();
        };
        let mut out = vec![0u8; (max as usize).saturating_add(1)];
        for (k, v) in &self.daughter {
            if let Some(slot) = out.get_mut(*k as usize) {
                *slot = *v;
            }
        }
        out
    }
}
