//! Deterministic randomness (SPEC §11).
//!
//! There is no RNG stream anywhere in the simulation. Every random value is derived by
//! hashing `(seed, tick, cell_id, purpose, index)`. This is what makes I1 (determinism) and
//! I6 (schedule independence) hold under rayon: no cell's randomness depends on when it was
//! scheduled relative to any other cell, and consuming a random number in one system cannot
//! perturb another.
//!
//! Adding a new consumer of randomness means adding a new [`Purpose`] tag, never reusing an
//! existing one.

/// splitmix64 finaliser. The mixing primitive for everything in this module.
#[inline(always)]
#[must_use]
pub const fn mix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Separates uses of randomness so that they cannot perturb one another.
///
/// Values are part of the reproducibility contract of a scenario: changing one changes every
/// run that used it. Add new tags at the end; never renumber.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Purpose {
    /// The `RAND` opcode.
    Rand = 0,
    /// Which byte a copy error strikes (M2).
    MutationSite = 1,
    /// Which structural operator applies at `SPLIT` (M2).
    MutationOperator = 2,
    /// Brownian jitter (M3).
    Jitter = 3,
    /// Test and fuzz harnesses. Never used by the simulation itself.
    Harness = 4,
    /// Where a scattered founder lands (`Placement::Scatter`). Setup, not simulation: it is
    /// drawn once when a slide is populated and never again.
    ///
    /// Appended rather than inserted, like a catalogue entry: the discriminants are mixed into
    /// every draw, so renumbering one would change every random number in every existing run.
    Placement = 5,
}

/// The part of a random draw that identifies *who* is drawing and *when*.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RandCtx {
    pub seed: u64,
    pub tick: u64,
    pub cell_id: u64,
}

impl RandCtx {
    #[inline]
    #[must_use]
    pub const fn new(seed: u64, tick: u64, cell_id: u64) -> RandCtx {
        RandCtx {
            seed,
            tick,
            cell_id,
        }
    }

    /// Draw a 64-bit value.
    ///
    /// `index` distinguishes repeated draws for the same purpose within one tick — the `n`th
    /// `RAND` executed by a cell, the `n`th mutation site considered. Without it every draw
    /// in a tick would return the same number.
    #[inline]
    #[must_use]
    pub const fn draw(&self, purpose: Purpose, index: u64) -> u64 {
        let mut h = mix64(self.seed);
        h = mix64(h ^ self.tick.wrapping_mul(0xD6E8_FEB8_6659_FD93));
        h = mix64(h ^ self.cell_id.wrapping_mul(0xA076_1D64_78BD_642F));
        h = mix64(h ^ ((purpose as u64) << 56) ^ index);
        h
    }

    /// Draw a cell-visible value. Uniform over the whole `i16` range.
    #[inline]
    #[must_use]
    pub const fn draw_i16(&self, purpose: Purpose, index: u64) -> i16 {
        // Take the high bits: they are the best-mixed part of the splitmix64 output.
        (self.draw(purpose, index) >> 48) as u16 as i16
    }

    /// Draw uniformly from `0..bound`, or 0 if `bound` is 0.
    ///
    /// Uses the high 32 bits and a widening multiply, which is unbiased enough for
    /// simulation use and has no rejection loop to make timing data-dependent.
    #[inline]
    #[must_use]
    pub const fn draw_below(&self, purpose: Purpose, index: u64, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        let r = self.draw(purpose, index) as u128;
        ((r * bound as u128) >> 64) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix64_is_a_permutation_on_a_sample() {
        // Not a proof, but catches a broken constant: no collisions over a dense range.
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..100_000u64 {
            assert!(seen.insert(mix64(i)), "collision at {i}");
        }
    }

    #[test]
    fn draws_separate_by_every_field() {
        let base = RandCtx::new(1, 2, 3);
        let v = base.draw(Purpose::Rand, 0);
        assert_ne!(v, RandCtx::new(2, 2, 3).draw(Purpose::Rand, 0));
        assert_ne!(v, RandCtx::new(1, 3, 3).draw(Purpose::Rand, 0));
        assert_ne!(v, RandCtx::new(1, 2, 4).draw(Purpose::Rand, 0));
        assert_ne!(v, base.draw(Purpose::MutationSite, 0));
        assert_ne!(v, base.draw(Purpose::Rand, 1));
    }

    #[test]
    fn draws_are_stateless_and_repeatable() {
        let ctx = RandCtx::new(0xDEAD_BEEF, 900, 17);
        for i in 0..1000 {
            assert_eq!(ctx.draw(Purpose::Rand, i), ctx.draw(Purpose::Rand, i));
        }
    }

    #[test]
    fn draw_below_stays_in_range() {
        let ctx = RandCtx::new(7, 7, 7);
        for bound in [1u64, 2, 3, 16, 127, 1_000_000] {
            for i in 0..5_000 {
                assert!(ctx.draw_below(Purpose::Harness, i, bound) < bound);
            }
        }
        assert_eq!(ctx.draw_below(Purpose::Harness, 0, 0), 0);
    }

    #[test]
    fn draw_below_is_roughly_uniform() {
        let ctx = RandCtx::new(99, 0, 0);
        let mut buckets = [0u32; 8];
        for i in 0..80_000u64 {
            let b = ctx.draw_below(Purpose::Harness, i, 8) as usize;
            buckets[b] += 1;
        }
        for b in buckets {
            assert!((9_000..11_000).contains(&b), "skewed buckets: {buckets:?}");
        }
    }
}
