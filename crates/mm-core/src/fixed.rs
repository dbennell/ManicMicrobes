//! Fixed-point arithmetic (SPEC §3).
//!
//! There are no floats anywhere in this crate (I2), because floating point is not
//! reproducible across compilers, architectures or optimisation levels, and I1 says two runs
//! must produce bit-identical state *on any platform*. Integers are.
//!
//! Two scales are in use:
//!
//! * **`Q10`** — fluid quantities, cell mass and energy: `i32` with an implied scale of 1024.
//! * **`POS`** — positions: `i32` with an implied scale of 256, in substrate-cell units.
//!
//! Everything here saturates rather than wrapping, per SPEC §3: magnitudes saturate, only
//! addressing wraps.

/// Implied scale for fluid quantities, mass and energy. One unit is `Q10_ONE`.
pub const Q10_ONE: i32 = 1024;
/// `log2(Q10_ONE)`, for shifts.
pub const Q10_BITS: u32 = 10;

/// Implied scale for positions, in substrate-cell units.
pub const POS_ONE: i32 = 256;
/// `log2(POS_ONE)`.
pub const POS_BITS: u32 = 8;

/// Multiply two `Q10` values. Computed in `i64` and saturated back, so no product can
/// overflow regardless of operands.
///
/// Division rather than a shift, deliberately. `>>` floors, so it rounds a negative product
/// *away* from zero — and a diffusion flux is `q10_mul(a - b, rate)`, where flooring would
/// make the flux from a poorer square to a richer one one unit larger than the flux the
/// other way for the same difference. That asymmetry is a systematic drift direction baked
/// into the solver. Truncation toward zero makes the magnitude depend only on `|a - b|`,
/// which is what a symmetric physical process requires. The extra couple of instructions
/// over a shift are not visible against the memory traffic this kernel is bound by.
#[inline(always)]
#[must_use]
pub const fn q10_mul(a: i32, b: i32) -> i32 {
    let p = (a as i64 * b as i64) / (1i64 << Q10_BITS);
    sat_i32(p)
}

/// Multiply a `Q10` quantity by a `Q10` fraction, rounding toward zero.
///
/// Used wherever a rate is applied to a quantity — a diffusion flux, a decay step. Rounding
/// toward zero (rather than to nearest) is what keeps a flux from ever exceeding the
/// quantity that sources it, which is what keeps the fluid non-negative.
#[inline(always)]
#[must_use]
pub const fn q10_scale(quantity: i32, fraction: i32) -> i32 {
    q10_mul(quantity, fraction)
}

/// Saturating `i64` to `i32`.
#[inline(always)]
#[must_use]
pub const fn sat_i32(v: i64) -> i32 {
    if v > i32::MAX as i64 {
        i32::MAX
    } else if v < i32::MIN as i64 {
        i32::MIN
    } else {
        v as i32
    }
}

/// Saturating `i32` to the cell-visible `i16` range (SPEC §3).
#[inline(always)]
#[must_use]
pub const fn sat_i16(v: i32) -> i16 {
    if v > i16::MAX as i32 {
        i16::MAX
    } else if v < i16::MIN as i32 {
        i16::MIN
    } else {
        v as i16
    }
}

/// Convert a `Q10` quantity to the `i16` a genome sees, saturating (SPEC §3).
#[inline(always)]
#[must_use]
pub const fn q10_to_cell(v: i32) -> i16 {
    sat_i16(v >> Q10_BITS)
}

/// Convert a cell-visible `i16` to a `Q10` quantity. Always exact.
#[inline(always)]
#[must_use]
pub const fn cell_to_q10(v: i16) -> i32 {
    (v as i32) << Q10_BITS
}

/// A whole number of units as `Q10`, saturating.
#[inline(always)]
#[must_use]
pub const fn q10(units: i32) -> i32 {
    sat_i32((units as i64) << Q10_BITS)
}

/// Position in substrate-cell units, as `POS`.
#[inline(always)]
#[must_use]
pub const fn pos(cells: i32) -> i32 {
    sat_i32((cells as i64) << POS_BITS)
}

/// The substrate square a position falls in, flooring toward negative infinity so that the
/// square boundary at 0 behaves like every other boundary.
#[inline(always)]
#[must_use]
pub const fn pos_to_square(p: i32) -> i32 {
    p >> POS_BITS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_round_trip_whole_units() {
        for units in [-1000i32, -1, 0, 1, 7, 1000] {
            assert_eq!(q10_to_cell(q10(units)) as i32, units);
        }
        for cells in [-1000i32, -1, 0, 1, 7, 1000] {
            assert_eq!(pos_to_square(pos(cells)), cells);
        }
    }

    #[test]
    fn multiplication_never_overflows() {
        for a in [i32::MIN, -1, 0, 1, i32::MAX, 1 << 20] {
            for b in [i32::MIN, -1, 0, 1, i32::MAX, Q10_ONE] {
                let r = q10_mul(a, b) as i64;
                assert!(
                    (i32::MIN as i64..=i32::MAX as i64).contains(&r),
                    "{a} * {b}"
                );
            }
        }
        assert_eq!(q10_mul(q10(3), q10(4)), q10(12));
        assert_eq!(q10_mul(i32::MAX, i32::MAX), i32::MAX);
    }

    #[test]
    fn scaling_a_quantity_never_exceeds_it() {
        // The property the fluid solver's non-negativity rests on: a fraction of a
        // non-negative quantity is never larger than the quantity.
        for q in [0i32, 1, 1023, 1024, 1_000_000, i32::MAX] {
            for frac in [0i32, 1, 512, 1023, Q10_ONE] {
                let f = q10_scale(q, frac);
                assert!(f >= 0 && f <= q, "scale({q}, {frac}) = {f}");
            }
        }
    }

    #[test]
    fn rounding_is_toward_zero_on_both_signs() {
        assert_eq!(q10_scale(1023, 1), 0);
        assert_eq!(q10_scale(-1023, 1), 0);
        assert_eq!(q10_scale(2048, 512), 1024);
        assert_eq!(q10_scale(-2048, 512), -1024);
    }

    #[test]
    fn scaling_is_symmetric_under_negation() {
        // The property the fluid solver needs: a flux depends on the magnitude of a
        // difference and not on which way round it is. Flooring would break this by one
        // unit on every negative product, which over a million ticks is a drift direction.
        for q in 0..4000i32 {
            for frac in [1i32, 7, 100, 511, 512, 1023] {
                assert_eq!(
                    q10_scale(q, frac),
                    -q10_scale(-q, frac),
                    "asymmetric at ({q}, {frac})"
                );
            }
        }
    }

    #[test]
    fn positions_floor_rather_than_truncate() {
        // Truncation toward zero would make square -1 twice as wide as every other.
        assert_eq!(pos_to_square(-1), -1);
        assert_eq!(pos_to_square(-POS_ONE), -1);
        assert_eq!(pos_to_square(-POS_ONE - 1), -2);
        assert_eq!(pos_to_square(POS_ONE - 1), 0);
    }
}
