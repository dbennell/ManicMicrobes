//! The rolling world-state hash.
//!
//! Invariant I1 is verified by comparing this value between runs: same scenario, same seed,
//! same input events must give the same hash at every tick, on any platform, at any thread
//! count. At M0 there is no world, so only the VM and the genome contribute — but the
//! interface is the point, and every piece of state added from M1 onward must feed itself
//! into a `StateHasher` here (hard rule 7).
//!
//! The hash is *order-dependent by design*: feeding the same values in a different order
//! gives a different result. That is what makes it detect an ordering bug rather than hide
//! one. Callers over unordered collections must therefore iterate in a stable order — sorted
//! by cell id, never `HashMap` order (hard rule 6).

use crate::rng::mix64;

/// Accumulates state into a 64-bit digest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StateHasher {
    h: u64,
}

impl Default for StateHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl StateHasher {
    #[inline]
    #[must_use]
    pub const fn new() -> StateHasher {
        StateHasher {
            h: 0xCBF2_9CE4_8422_2325,
        }
    }

    #[inline]
    pub fn u64(&mut self, v: u64) {
        self.h = mix64(self.h ^ v).wrapping_add(0x9E37_79B9_7F4A_7C15);
    }

    #[inline]
    pub fn u32(&mut self, v: u32) {
        self.u64(v as u64);
    }

    #[inline]
    pub fn u16(&mut self, v: u16) {
        self.u64(v as u64);
    }

    #[inline]
    pub fn u8(&mut self, v: u8) {
        self.u64(v as u64);
    }

    #[inline]
    pub fn i16(&mut self, v: i16) {
        self.u64(v as u16 as u64);
    }

    #[inline]
    pub fn i32(&mut self, v: i32) {
        self.u64(v as u32 as u64);
    }

    #[inline]
    pub fn bool(&mut self, v: bool) {
        self.u64(v as u64);
    }

    /// Feeds the length first, so that concatenated byte strings cannot alias.
    pub fn bytes(&mut self, v: &[u8]) {
        self.u64(v.len() as u64);
        let mut acc = 0u64;
        for (i, b) in v.iter().enumerate() {
            acc ^= (*b as u64) << ((i % 8) * 8);
            if i % 8 == 7 {
                self.u64(acc);
                acc = 0;
            }
        }
        self.u64(acc);
    }

    pub fn i16_slice(&mut self, v: &[i16]) {
        self.u64(v.len() as u64);
        for x in v {
            self.i16(*x);
        }
    }

    #[inline]
    #[must_use]
    pub const fn finish(&self) -> u64 {
        mix64(self.h)
    }
}

/// Anything that contributes to the world-state hash.
///
/// Implement this for every new piece of world state, and feed it in a deterministic order.
pub trait StateHash {
    fn hash_state(&self, h: &mut StateHasher);

    /// Convenience: this value's digest on its own.
    fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        self.hash_state(&mut h);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_hash_is_stable() {
        // Pinned so an accidental change to the mixing constants is caught here rather than
        // by every downstream determinism test at once.
        assert_eq!(StateHasher::new().finish(), 0xC381_7C01_6BA4_FF30);
    }

    #[test]
    fn order_matters() {
        let mut a = StateHasher::new();
        a.u16(1);
        a.u16(2);
        let mut b = StateHasher::new();
        b.u16(2);
        b.u16(1);
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn byte_strings_do_not_alias() {
        let mut a = StateHasher::new();
        a.bytes(b"ab");
        a.bytes(b"c");
        let mut b = StateHasher::new();
        b.bytes(b"a");
        b.bytes(b"bc");
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn every_input_changes_the_digest() {
        let base = {
            let mut h = StateHasher::new();
            h.u64(0);
            h.finish()
        };
        for bit in 0..64 {
            let mut h = StateHasher::new();
            h.u64(1u64 << bit);
            assert_ne!(h.finish(), base);
        }
    }
}
