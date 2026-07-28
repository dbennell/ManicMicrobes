//! Mutation (SPEC §9).
//!
//! Mutation happens at two points, which differ in kind.
//!
//! **Per-byte copy error, during `COPYB`.** The probability is a function of the nucleus
//! organelle's copy-fidelity control input, and high fidelity costs more energy per byte
//! copied. That makes the mutation rate **genetically encoded and physically costly**, so
//! mutator alleles can evolve, and the observed fidelity of a lineage becomes a measurable,
//! plottable trait rather than a constant somebody chose.
//!
//! **Structural mutation, at `SPLIT`.** Point, insertion, deletion, duplication, inversion,
//! translocation.
//!
//! # Duplication is not optional
//!
//! Duplication-and-divergence is the principal engine of novelty in biology. Combined with
//! promoter binding (SPEC §4.4) it produces paralogs — the same gene expressed under
//! different conditions — which is the actual mechanism by which new capability arises rather
//! than existing capability being tuned. A mutation set without it can only refine what is
//! already there. `CLAUDE.md` says not to ship one, and [`MutationRates::duplication`] is
//! checked by a test that fails if it is ever set to zero in the default set.
//!
//! # Randomness
//!
//! Every draw is `hash(seed, tick, cell_id, purpose, index)` (SPEC §11) — no stream, no
//! state. Two cells dividing in the same tick cannot perturb one another's mutations
//! whatever order they are scheduled in, which is what makes I1 and I6 hold through the one
//! part of the simulation that is *supposed* to be random.

use crate::genome::MAX_GENOME_LEN;
use crate::rng::{Purpose, RandCtx};

/// Probability denominator for mutation rates.
///
/// A rate of `n` means `n` chances in `RATE_SCALE`. `1 << 20` gives useful resolution down to
/// about one in a million, which is the order a per-byte copy error wants — a thousand-byte
/// genome copied with a rate of 1000 averages one error per division.
pub const RATE_SCALE: u32 = 1 << 20;

/// How often each structural operator fires, per division.
///
/// Scenario data, not constants: the interesting output of this project is knowing which of
/// these matter, and that is only askable if they are a parameter (`CLAUDE.md`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MutationRates {
    /// Substitute one byte.
    pub point: u32,
    /// Insert a random byte.
    pub insertion: u32,
    /// Remove a byte.
    pub deletion: u32,
    /// Copy a segment. Never zero in a shipped set — see the module docs.
    pub duplication: u32,
    /// Reverse a segment.
    pub inversion: u32,
    /// Move a segment.
    pub translocation: u32,
    /// Longest segment a structural operator moves, copies or reverses.
    pub max_segment: u16,
    /// Per-byte copy error at the worst possible nucleus fidelity.
    ///
    /// The nucleus scales this down; a cell that invests in fidelity copies more accurately
    /// and pays more energy per byte to do it.
    pub copy_error_max: u32,
}

impl Default for MutationRates {
    fn default() -> Self {
        MutationRates {
            point: RATE_SCALE / 16,
            insertion: RATE_SCALE / 64,
            deletion: RATE_SCALE / 64,
            duplication: RATE_SCALE / 48,
            inversion: RATE_SCALE / 128,
            translocation: RATE_SCALE / 128,
            max_segment: 64,
            copy_error_max: RATE_SCALE / 512,
        }
    }
}

impl MutationRates {
    /// Everything off. Arena mode (SPEC §0) runs with mutation clamped or absent, and a
    /// determinism test needs a world where nothing drifts.
    #[must_use]
    pub const fn none() -> MutationRates {
        MutationRates {
            point: 0,
            insertion: 0,
            deletion: 0,
            duplication: 0,
            inversion: 0,
            translocation: 0,
            max_segment: 0,
            copy_error_max: 0,
        }
    }

    /// Whether any operator can fire.
    #[must_use]
    pub const fn is_off(&self) -> bool {
        self.point == 0
            && self.insertion == 0
            && self.deletion == 0
            && self.duplication == 0
            && self.inversion == 0
            && self.translocation == 0
            && self.copy_error_max == 0
    }
}

/// The structural operators, in the order SPEC §9 lists them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Operator {
    Point,
    Insertion,
    Deletion,
    Duplication,
    Inversion,
    Translocation,
}

/// Per-byte copy error rate for a given nucleus fidelity.
///
/// `fidelity` is the nucleus control input, `Q10`, clamped to `0..=1`. At zero the cell pays
/// nothing and copies at `copy_error_max`; at one it copies almost perfectly and pays the most
/// per byte. The relationship is deliberately linear rather than something cleverer, because
/// what matters at M2 is that the trade-off *exists* and is visible in the phylogeny — the
/// shape of it is a balancing question for M8.
#[inline]
#[must_use]
pub fn copy_error_rate(rates: &MutationRates, fidelity: i32) -> u32 {
    let f = fidelity.clamp(0, crate::fixed::Q10_ONE) as u32;
    let remaining = (crate::fixed::Q10_ONE as u32).saturating_sub(f);
    ((rates.copy_error_max as u64 * remaining as u64) / crate::fixed::Q10_ONE as u64) as u32
}

/// Whether one copied byte comes out wrong, and what it becomes.
///
/// Returns `None` for a clean copy. The draw index is the byte offset, so the same byte of the
/// same division always mutates the same way however the copy loop is scheduled.
#[inline]
#[must_use]
pub fn copy_error(ctx: &RandCtx, rate: u32, offset: u16, byte: u8) -> Option<u8> {
    if rate == 0 {
        return None;
    }
    let roll = ctx.draw_below(Purpose::MutationSite, offset as u64, RATE_SCALE as u64) as u32;
    if roll >= rate {
        return None;
    }
    // A copy error is a substitution, not a random byte: flipping one bit is a much smaller
    // step than rerolling, and with degenerate encoding (SPEC §4.2) a quarter of bit flips
    // are synonymous anyway.
    let bit = ctx.draw_below(Purpose::MutationOperator, offset as u64, 8) as u32;
    Some(byte ^ (1u8 << bit))
}

/// Apply the structural operators to a daughter genome at `SPLIT`.
///
/// Order is fixed and every operator gets its own draw index, so adding one later does not
/// perturb the others — which matters because a scenario's recorded results are only
/// comparable if the random stream did not shift under them.
#[must_use]
pub fn mutate_structural(
    bytes: &mut Vec<u8>,
    rates: &MutationRates,
    ctx: &RandCtx,
) -> Vec<Operator> {
    let mut applied = Vec::new();
    if bytes.is_empty() || rates.is_off() {
        return applied;
    }

    let fires = |rate: u32, index: u64| -> bool {
        rate > 0
            && (ctx.draw_below(Purpose::MutationOperator, index, RATE_SCALE as u64) as u32) < rate
    };
    let pick = |bound: usize, index: u64| -> usize {
        if bound == 0 {
            0
        } else {
            ctx.draw_below(Purpose::MutationSite, index, bound as u64) as usize
        }
    };

    if fires(rates.point, 1) {
        let at = pick(bytes.len(), 101);
        let to = ctx.draw_below(Purpose::MutationSite, 102, 256) as u8;
        bytes[at] = to;
        applied.push(Operator::Point);
    }

    if fires(rates.insertion, 2) && bytes.len() < MAX_GENOME_LEN {
        let at = pick(bytes.len() + 1, 201);
        let b = ctx.draw_below(Purpose::MutationSite, 202, 256) as u8;
        bytes.insert(at, b);
        applied.push(Operator::Insertion);
    }

    if fires(rates.deletion, 3) && bytes.len() > 1 {
        let at = pick(bytes.len(), 301);
        bytes.remove(at);
        applied.push(Operator::Deletion);
    }

    // Duplication, biased toward gene-block boundaries: a copy that starts and ends at a
    // `GENE` header is a whole gene, and a whole gene is what diverges into a paralog. A copy
    // that starts mid-instruction is usually rubble.
    if fires(rates.duplication, 4) && bytes.len() < MAX_GENOME_LEN {
        let (from, len) = segment(bytes, rates, ctx, 401, true);
        if len > 0 && bytes.len().saturating_add(len) <= MAX_GENOME_LEN {
            let piece: Vec<u8> = bytes[from..from + len].to_vec();
            let at = pick(bytes.len() + 1, 403);
            let tail = bytes.split_off(at);
            bytes.extend_from_slice(&piece);
            bytes.extend_from_slice(&tail);
            applied.push(Operator::Duplication);
        }
    }

    if fires(rates.inversion, 5) {
        let (from, len) = segment(bytes, rates, ctx, 501, false);
        if len > 1 {
            bytes[from..from + len].reverse();
            applied.push(Operator::Inversion);
        }
    }

    if fires(rates.translocation, 6) {
        let (from, len) = segment(bytes, rates, ctx, 601, false);
        if len > 0 && len < bytes.len() {
            let piece: Vec<u8> = bytes.drain(from..from + len).collect();
            let at = pick(bytes.len() + 1, 603);
            let tail = bytes.split_off(at);
            bytes.extend_from_slice(&piece);
            bytes.extend_from_slice(&tail);
            applied.push(Operator::Translocation);
        }
    }

    bytes.truncate(MAX_GENOME_LEN);
    applied
}

/// Choose a segment to copy, reverse or move.
///
/// With `gene_biased`, the start is snapped to the nearest `GENE` header behind it and the
/// length extended to the next one, when there is one within reach.
fn segment(
    bytes: &[u8],
    rates: &MutationRates,
    ctx: &RandCtx,
    index: u64,
    gene_biased: bool,
) -> (usize, usize) {
    let n = bytes.len();
    if n == 0 {
        return (0, 0);
    }
    let max = (rates.max_segment as usize).min(n).max(1);
    let mut from = ctx.draw_below(Purpose::MutationSite, index, n as u64) as usize;
    let mut len = 1 + ctx.draw_below(Purpose::MutationSite, index + 1, max as u64) as usize;

    if gene_biased {
        // Half the time, snap to gene boundaries. The other half stays arbitrary, because a
        // mutation set that only ever moved whole genes could not create a new one.
        let snap = ctx.draw_below(Purpose::MutationOperator, index + 2, 2) == 0;
        if snap {
            if let Some(start) = gene_at_or_before(bytes, from) {
                from = start;
                if let Some(end) = gene_after(bytes, from) {
                    len = end.saturating_sub(from).max(1);
                }
            }
        }
    }

    let len = len.min(n.saturating_sub(from));
    (from, len)
}

fn gene_at_or_before(bytes: &[u8], at: usize) -> Option<usize> {
    (0..=at.min(bytes.len().saturating_sub(1)))
        .rev()
        .find(|i| crate::isa::Op::from_byte(bytes[*i]) == crate::isa::Op::Gene)
}

fn gene_after(bytes: &[u8], from: usize) -> Option<usize> {
    (from + 1..bytes.len()).find(|i| crate::isa::Op::from_byte(bytes[*i]) == crate::isa::Op::Gene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isa::Op;

    fn ctx(seed: u64) -> RandCtx {
        RandCtx::new(seed, 0, 0)
    }

    #[test]
    fn duplication_is_never_absent_from_the_default_set() {
        // CLAUDE.md: do not ship a mutation set without duplication. It is the principal
        // engine of novelty; without it evolution can only refine what already exists.
        let d = MutationRates::default();
        assert!(d.duplication > 0, "duplication must be present");
        assert!(d.max_segment > 0, "and must be able to copy something");
        assert!(!d.is_off());
    }

    #[test]
    fn every_operator_fires_over_enough_divisions() {
        // A rate that never fires is a mutation set with a hole in it.
        let rates = MutationRates {
            point: RATE_SCALE / 2,
            insertion: RATE_SCALE / 2,
            deletion: RATE_SCALE / 2,
            duplication: RATE_SCALE / 2,
            inversion: RATE_SCALE / 2,
            translocation: RATE_SCALE / 2,
            max_segment: 16,
            copy_error_max: 0,
        };
        let base: Vec<u8> = (0..200u16).map(|i| (i % 64) as u8).collect();
        let mut seen: Vec<Operator> = Vec::new();
        for seed in 0..400u64 {
            let mut bytes = base.clone();
            for op in mutate_structural(&mut bytes, &rates, &ctx(seed)) {
                if !seen.contains(&op) {
                    seen.push(op);
                }
            }
        }
        for op in [
            Operator::Point,
            Operator::Insertion,
            Operator::Deletion,
            Operator::Duplication,
            Operator::Inversion,
            Operator::Translocation,
        ] {
            assert!(seen.contains(&op), "{op:?} never fired in 400 divisions");
        }
    }

    #[test]
    fn mutation_is_a_pure_function_of_the_draw_context() {
        // I1: two cells dividing in the same tick must not perturb one another, whatever
        // order they are scheduled in.
        let rates = MutationRates::default();
        let base: Vec<u8> = (0..300u16).map(|i| (i % 251) as u8).collect();
        for seed in 0..50u64 {
            let mut a = base.clone();
            let mut b = base.clone();
            let ops_a = mutate_structural(&mut a, &rates, &ctx(seed));
            let ops_b = mutate_structural(&mut b, &rates, &ctx(seed));
            assert_eq!(a, b);
            assert_eq!(ops_a, ops_b);
        }
    }

    #[test]
    fn mutation_never_produces_an_illegal_genome() {
        // Totality (I3) says any byte sequence is a legal program, so the only thing a
        // mutation can break is the length bound.
        let rates = MutationRates {
            max_segment: 4096,
            ..MutationRates::default()
        };
        for seed in 0..2_000u64 {
            let n = 1 + (seed as usize % 500);
            let mut bytes: Vec<u8> = (0..n).map(|i| (i * 7 % 256) as u8).collect();
            let _ = mutate_structural(&mut bytes, &rates, &ctx(seed));
            assert!(!bytes.is_empty(), "seed {seed} deleted the whole genome");
            assert!(bytes.len() <= MAX_GENOME_LEN);
        }
    }

    #[test]
    fn a_one_byte_genome_survives_every_operator() {
        // The edge case every segment operator has to handle: nothing to take a segment from.
        let rates = MutationRates {
            point: RATE_SCALE,
            insertion: RATE_SCALE,
            deletion: RATE_SCALE,
            duplication: RATE_SCALE,
            inversion: RATE_SCALE,
            translocation: RATE_SCALE,
            max_segment: 64,
            copy_error_max: 0,
        };
        for seed in 0..500u64 {
            let mut bytes = vec![0x2Eu8];
            let _ = mutate_structural(&mut bytes, &rates, &ctx(seed));
            assert!(!bytes.is_empty());
        }
    }

    #[test]
    fn mutation_off_means_off() {
        let base: Vec<u8> = (0..100u8).collect();
        for seed in 0..1000u64 {
            let mut bytes = base.clone();
            let ops = mutate_structural(&mut bytes, &MutationRates::none(), &ctx(seed));
            assert_eq!(bytes, base, "arena mode must be able to turn mutation off");
            assert!(ops.is_empty());
        }
    }

    #[test]
    fn duplication_lengthens_and_deletion_shortens() {
        let mut only_dup = MutationRates::none();
        only_dup.duplication = RATE_SCALE;
        only_dup.max_segment = 8;
        let base: Vec<u8> = (0..64u8).collect();
        let mut grew = 0;
        for seed in 0..100u64 {
            let mut bytes = base.clone();
            let _ = mutate_structural(&mut bytes, &only_dup, &ctx(seed));
            if bytes.len() > base.len() {
                grew += 1;
            }
        }
        assert!(
            grew > 90,
            "duplication rarely lengthened the genome: {grew}/100"
        );

        let mut only_del = MutationRates::none();
        only_del.deletion = RATE_SCALE;
        let mut bytes = base.clone();
        let _ = mutate_structural(&mut bytes, &only_del, &ctx(1));
        assert_eq!(bytes.len(), base.len() - 1);
    }

    #[test]
    fn duplication_favours_whole_genes() {
        // A copy that starts and ends at a GENE header is a gene; one that starts
        // mid-instruction is usually rubble. Both must happen — a set that only ever moved
        // whole genes could not create a new one — but the gene-aligned case should be common.
        let mut rates = MutationRates::none();
        rates.duplication = RATE_SCALE;
        rates.max_segment = 64;

        // Three gene blocks, each eight bytes.
        let mut base = Vec::new();
        for _ in 0..3 {
            base.push(Op::Gene.canonical_byte());
            base.extend([Op::Nop1.canonical_byte(), Op::Nop0.canonical_byte()]);
            base.extend([Op::One.canonical_byte(); 5]);
        }

        let mut gene_aligned = 0;
        let mut total = 0;
        for seed in 0..300u64 {
            let mut bytes = base.clone();
            let ops = mutate_structural(&mut bytes, &rates, &ctx(seed));
            if !ops.contains(&Operator::Duplication) {
                continue;
            }
            total += 1;
            // A gene-aligned copy leaves the genome a multiple of eight bytes longer.
            if (bytes.len() - base.len()) % 8 == 0 {
                gene_aligned += 1;
            }
        }
        assert!(total > 200, "duplication did not fire enough to measure");
        assert!(
            gene_aligned * 4 > total,
            "gene-aligned copies were rare: {gene_aligned}/{total}"
        );
    }

    #[test]
    fn copy_fidelity_buys_accuracy() {
        // The trade-off that makes mutation rate an evolvable trait: a cell that invests in
        // its nucleus copies more accurately.
        let rates = MutationRates::default();
        let sloppy = copy_error_rate(&rates, 0);
        let careful = copy_error_rate(&rates, crate::fixed::Q10_ONE);
        let middling = copy_error_rate(&rates, crate::fixed::Q10_ONE / 2);
        assert_eq!(sloppy, rates.copy_error_max);
        assert_eq!(careful, 0, "perfect fidelity copies perfectly");
        assert!(middling > careful && middling < sloppy);
        // and a nonsense control input is still legal
        assert_eq!(copy_error_rate(&rates, -9999), rates.copy_error_max);
        assert_eq!(copy_error_rate(&rates, i32::MAX), 0);
    }

    #[test]
    fn copy_errors_flip_one_bit_and_happen_at_about_the_stated_rate() {
        let rates = MutationRates {
            copy_error_max: RATE_SCALE / 4,
            ..MutationRates::default()
        };
        let rate = copy_error_rate(&rates, 0);
        let mut errors = 0;
        let n = 20_000u16;
        for i in 0..n {
            let c = RandCtx::new(7, i as u64 / 64, 3);
            if let Some(mutated) = copy_error(&c, rate, i, 0b1010_1010) {
                assert_eq!(
                    (mutated ^ 0b1010_1010).count_ones(),
                    1,
                    "a copy error should be one bit, not a reroll"
                );
                errors += 1;
            }
        }
        let expected = n as u32 / 4;
        assert!(
            errors > expected * 3 / 4 && errors < expected * 5 / 4,
            "copy errors happened {errors} times, expected around {expected}"
        );
    }

    #[test]
    fn a_zero_rate_never_errs() {
        for i in 0..10_000u16 {
            assert_eq!(copy_error(&ctx(1), 0, i, 42), None);
        }
    }

    #[test]
    fn rates_round_trip_through_ron() {
        let r = MutationRates::default();
        let back: MutationRates = ron::from_str(&ron::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
