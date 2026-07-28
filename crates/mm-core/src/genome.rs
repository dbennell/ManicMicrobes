//! Genomes: flat byte strings, content-hashed, interned and shared (SPEC §4.1).
//!
//! In a 200k population of clonal lineages the number of *distinct* genomes is in the low
//! thousands, so genomes are deduplicated by content hash and handed out as `Arc<Genome>`.
//! Mutation produces a new byte string and interns it; nothing is ever mutated in place.
//!
//! Construction also precomputes two tables that the VM would otherwise rebuild on every
//! instruction:
//!
//! * the maximal template run starting at each byte offset, which turns the complementary
//!   jump search into one array lookup per candidate offset instead of a rescan;
//! * the list of `GENE` promoters, which turns `EXPRESS` into a scan of the genes rather
//!   than of the whole genome.
//!
//! Both are pure functions of the bytes, so an interned genome computes them once and every
//! cell carrying it reads them for free.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};

use crate::isa::{mask, Op, Template, MAX_TEMPLATE_LEN};
use crate::rng::mix64;
use crate::state_hash::{StateHash, StateHasher};

/// Upper bound on genome length.
///
/// Offsets — `IP`, `PA`, `PB` and call-stack entries — are `u16` to keep per-cell fixed
/// state inside the 512-byte budget of SPEC §6.1, so a genome may not exceed what a `u16`
/// can address. In the running simulation nucleus capacity bounds length far below this.
pub const MAX_GENOME_LEN: usize = 65_536;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenomeError {
    /// Longer than [`MAX_GENOME_LEN`].
    TooLong(usize),
}

impl std::fmt::Display for GenomeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenomeError::TooLong(n) => write!(
                f,
                "genome of {n} bytes exceeds the {MAX_GENOME_LEN}-byte addressing limit"
            ),
        }
    }
}

impl std::error::Error for GenomeError {}

/// A `GENE` header and the promoter it declares (SPEC §4.4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Promoter {
    /// Offset of the `GENE` instruction itself.
    pub offset: u16,
    /// Offset of the first instruction of the gene body: just past the `GENE` and its
    /// template. `EXPRESS` calls here.
    pub entry: u16,
    /// The promoter pattern.
    pub template: Template,
}

/// An immutable, content-addressed genome.
pub struct Genome {
    bytes: Box<[u8]>,
    /// Maximal template run beginning at each offset, capped at 8 letters. Parallel to
    /// `bytes`. Runs do not wrap past the end of the genome.
    templates: Box<[Template]>,
    /// Every `GENE` header, ascending by offset — so a scan that keeps the first minimum
    /// resolves ties to the lowest offset, as SPEC §4.4 requires.
    promoters: Box<[Promoter]>,
    hash: u64,
}

impl Genome {
    /// Build a genome from bytes, computing its hash and lookup tables.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Genome, GenomeError> {
        let bytes: Vec<u8> = bytes.into();
        if bytes.len() > MAX_GENOME_LEN {
            return Err(GenomeError::TooLong(bytes.len()));
        }
        let hash = content_hash(&bytes);
        let templates = build_template_table(&bytes);
        let promoters = build_promoter_table(&bytes, &templates);
        Ok(Genome {
            bytes: bytes.into_boxed_slice(),
            templates,
            promoters,
            hash,
        })
    }

    /// The empty genome. Executing it is legal and does nothing.
    #[must_use]
    pub fn empty() -> Genome {
        Genome {
            bytes: Box::new([]),
            templates: Box::new([]),
            promoters: Box::new([]),
            hash: content_hash(&[]),
        }
    }

    #[inline(always)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    #[inline(always)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    #[inline(always)]
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// 64-bit content hash. Equal genomes have equal hashes; unequal genomes almost always
    /// do not, and the intern pool compares bytes on collision.
    #[inline(always)]
    #[must_use]
    pub fn hash(&self) -> u64 {
        self.hash
    }

    #[inline(always)]
    #[must_use]
    pub fn promoters(&self) -> &[Promoter] {
        &self.promoters
    }

    /// Byte at `off`, which must already be reduced modulo the length. Out-of-range offsets
    /// read as 0 (`NOP0`) rather than panicking — totality does not depend on callers.
    #[inline(always)]
    #[must_use]
    pub fn byte(&self, off: usize) -> u8 {
        match self.bytes.get(off) {
            Some(b) => *b,
            None => 0,
        }
    }

    /// The maximal template run beginning at `off`. Out-of-range offsets yield the empty
    /// template.
    #[inline(always)]
    #[must_use]
    pub fn template_at(&self, off: usize) -> Template {
        match self.templates.get(off) {
            Some(t) => *t,
            None => Template::EMPTY,
        }
    }

    /// Reduce an arbitrary offset into range. The genome is circular for execution: `IP`
    /// wraps modulo length (SPEC §5), so every offset is legal.
    ///
    /// The common case — an offset already in range — costs a comparison rather than a
    /// division, which matters because this is on the per-instruction path.
    #[inline(always)]
    #[must_use]
    pub fn wrap(&self, off: usize) -> usize {
        let len = self.bytes.len();
        if len == 0 {
            0
        } else if off < len {
            off
        } else {
            off % len
        }
    }

    /// Same, for a signed step backwards from an in-range offset.
    #[inline(always)]
    #[must_use]
    pub fn wrap_back(&self, from: usize, delta: usize) -> usize {
        let len = self.bytes.len();
        if len == 0 {
            return 0;
        }
        let d = delta % len;
        if from >= d {
            from.wrapping_sub(d)
        } else {
            // from < d <= len, so this stays in range without underflow.
            from.wrapping_add(len).wrapping_sub(d)
        }
    }

    /// Mutable copy for the copy-on-write path. Mutate this and re-intern.
    #[must_use]
    pub fn to_vec(&self) -> Vec<u8> {
        self.bytes.to_vec()
    }
}

impl std::fmt::Debug for Genome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Genome")
            .field("len", &self.bytes.len())
            .field("hash", &format_args!("{:#018x}", self.hash))
            .field("promoters", &self.promoters.len())
            .finish()
    }
}

impl PartialEq for Genome {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.bytes == other.bytes
    }
}

impl Eq for Genome {}

impl StateHash for Genome {
    fn hash_state(&self, h: &mut StateHasher) {
        // The content hash stands in for the bytes: it is a pure function of them, and
        // hashing 64 bits beats hashing a few kilobytes per cell per tick.
        h.u64(self.hash);
        h.u64(self.bytes.len() as u64);
    }
}

/// Content hash. FNV-1a for the sweep, splitmix64 for avalanche, length mixed in so that
/// trailing-zero differences cannot alias.
#[must_use]
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01B3);
    }
    mix64(h ^ (bytes.len() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// For each offset, the maximal run of template letters starting there, capped at 8.
///
/// Computed right to left so each entry extends the next in constant time. Bits are read
/// least-significant-first, so `value[o] = letter(o) | (value[o+1] << 1)`; a consequence is
/// that the first `k` letters of a run are exactly its low `k` bits, which is what lets the
/// jump search test a candidate offset with a single mask and compare.
fn build_template_table(bytes: &[u8]) -> Box<[Template]> {
    let n = bytes.len();
    let mut out = vec![Template::EMPTY; n];
    let mut next = Template::EMPTY;
    for i in (0..n).rev() {
        let b = match bytes.get(i) {
            Some(b) => *b,
            None => 0,
        };
        let op = Op::from_byte(b);
        let here = if op.is_nop() {
            let letter = u8::from(op == Op::Nop1);
            let len = next.len.saturating_add(1).min(MAX_TEMPLATE_LEN);
            let value = (letter | (next.value << 1)) & mask(len);
            Template { len, value }
        } else {
            Template::EMPTY
        };
        if let Some(slot) = out.get_mut(i) {
            *slot = here;
        }
        next = here;
    }
    out.into_boxed_slice()
}

/// Every `GENE` header in the genome, ascending by offset.
fn build_promoter_table(bytes: &[u8], templates: &[Template]) -> Box<[Promoter]> {
    let n = bytes.len();
    let mut out = Vec::new();
    for (i, b) in bytes.iter().enumerate() {
        if Op::from_byte(*b) != Op::Gene {
            continue;
        }
        let t = match templates.get(i.wrapping_add(1)) {
            Some(t) => *t,
            None => Template::EMPTY,
        };
        // `entry` is the first byte after the header and its template, wrapped: a `GENE` at
        // the very end of a genome has its body at the start, which is consistent with
        // execution wrapping modulo length.
        let entry = i
            .wrapping_add(1)
            .wrapping_add(t.len as usize)
            .checked_rem(n.max(1))
            .unwrap_or(0);
        out.push(Promoter {
            offset: i as u16,
            entry: entry as u16,
            template: t,
        });
    }
    out.into_boxed_slice()
}

/// The genome intern pool (SPEC §4.1).
///
/// Deduplicates by content, so a clonal population shares one allocation and one set of
/// lookup tables. Entries are weak: when the last cell carrying a genome dies, the genome
/// is dropped, and the empty slot is reclaimed on the next intern of the same hash.
///
/// The map is a `BTreeMap` rather than a `HashMap` on purpose. Nothing here feeds simulation
/// outcomes, but hard rule 6 is much easier to keep when the crate contains no hash-ordered
/// iteration at all.
#[derive(Default)]
pub struct GenomePool {
    // Vec per hash so that a content-hash collision stores both genomes rather than
    // silently returning the wrong one.
    map: Mutex<BTreeMap<u64, Vec<Weak<Genome>>>>,
}

impl std::fmt::Debug for GenomePool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenomePool")
            .field("live", &self.live_count())
            .finish()
    }
}

impl GenomePool {
    #[must_use]
    pub fn new() -> GenomePool {
        GenomePool {
            map: Mutex::new(BTreeMap::new()),
        }
    }

    /// Intern a byte string, returning the shared genome. Identical bytes always return the
    /// same `Arc`.
    pub fn intern(&self, bytes: impl Into<Vec<u8>>) -> Result<Arc<Genome>, GenomeError> {
        let bytes: Vec<u8> = bytes.into();
        if bytes.len() > MAX_GENOME_LEN {
            return Err(GenomeError::TooLong(bytes.len()));
        }
        let hash = content_hash(&bytes);

        let mut map = match self.map.lock() {
            Ok(m) => m,
            // A poisoned pool means another thread panicked while holding the lock. The
            // contents are still structurally valid — every entry is an immutable genome —
            // so recovering is strictly better than propagating a panic into the simulation.
            Err(poisoned) => poisoned.into_inner(),
        };
        let slot = map.entry(hash).or_default();
        slot.retain(|w| w.strong_count() > 0);
        for w in slot.iter() {
            if let Some(g) = w.upgrade() {
                if g.bytes() == bytes.as_slice() {
                    return Ok(g);
                }
            }
        }
        let g = Arc::new(Genome::new(bytes)?);
        slot.push(Arc::downgrade(&g));
        Ok(g)
    }

    /// Copy-on-write: apply `edit` to a copy of `base`'s bytes and intern the result.
    ///
    /// If the edit is a no-op the original `Arc` comes back, so an unmutated division costs
    /// a hash rather than an allocation.
    pub fn derive(
        &self,
        base: &Genome,
        edit: impl FnOnce(&mut Vec<u8>),
    ) -> Result<Arc<Genome>, GenomeError> {
        let mut bytes = base.to_vec();
        edit(&mut bytes);
        self.intern(bytes)
    }

    /// Number of distinct genomes currently alive in the pool. For instrumentation only.
    #[must_use]
    pub fn live_count(&self) -> usize {
        let map = match self.map.lock() {
            Ok(m) => m,
            Err(poisoned) => poisoned.into_inner(),
        };
        map.values()
            .map(|v| v.iter().filter(|w| w.strong_count() > 0).count())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asm(ops: &[Op]) -> Vec<u8> {
        ops.iter().map(|o| o.canonical_byte()).collect()
    }

    #[test]
    fn template_runs_are_maximal_and_lsb_first() {
        // NOP1 NOP0 NOP1 -> letters 1,0,1 -> value 0b101 = 5, len 3
        let g = Genome::new(asm(&[Op::Nop1, Op::Nop0, Op::Nop1, Op::Add])).unwrap();
        assert_eq!(g.template_at(0), Template::new(3, 0b101));
        assert_eq!(g.template_at(1), Template::new(2, 0b10));
        assert_eq!(g.template_at(2), Template::new(1, 0b1));
        assert_eq!(g.template_at(3), Template::EMPTY);
    }

    #[test]
    fn template_runs_cap_at_eight() {
        let g = Genome::new(vec![Op::Nop1.canonical_byte(); 12]).unwrap();
        for i in 0..12 {
            assert_eq!(g.template_at(i).len, 8.min(12 - i) as u8);
        }
        assert_eq!(g.template_at(0), Template::new(8, 0xFF));
    }

    #[test]
    fn first_k_letters_are_the_low_k_bits() {
        let g = Genome::new(asm(&[Op::Nop1, Op::Nop1, Op::Nop0, Op::Nop1])).unwrap();
        let run = g.template_at(0);
        assert_eq!(run, Template::new(4, 0b1011));
        // a 2-letter query matches this run's prefix iff it equals the low 2 bits
        assert_eq!(run.value & mask(2), Template::new(2, 0b11).value);
    }

    #[test]
    fn template_runs_use_degenerate_encoding() {
        // 0x40 and 0x41 also decode to NOP0 and NOP1.
        let g = Genome::new(vec![0x41, 0x80, 0xC1]).unwrap();
        assert_eq!(g.template_at(0), Template::new(3, 0b101));
    }

    #[test]
    fn promoters_are_found_in_offset_order() {
        let bytes = asm(&[
            Op::Gene,
            Op::Nop1,
            Op::Nop0,
            Op::Add,
            Op::Gene,
            Op::Nop1,
            Op::Nop1,
            Op::Ret,
        ]);
        let g = Genome::new(bytes).unwrap();
        let p = g.promoters();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].offset, 0);
        assert_eq!(p[0].entry, 3);
        assert_eq!(p[0].template, Template::new(2, 0b01));
        assert_eq!(p[1].offset, 4);
        assert_eq!(p[1].entry, 7);
        assert_eq!(p[1].template, Template::new(2, 0b11));
    }

    #[test]
    fn gene_at_the_end_wraps_its_entry() {
        let g = Genome::new(asm(&[Op::Add, Op::Gene])).unwrap();
        assert_eq!(g.promoters()[0].entry, 0);
    }

    #[test]
    fn empty_genome_is_usable() {
        let g = Genome::empty();
        assert_eq!(g.len(), 0);
        assert_eq!(g.byte(0), 0);
        assert_eq!(g.template_at(0), Template::EMPTY);
        assert_eq!(g.wrap(12345), 0);
        assert_eq!(g.wrap_back(0, 7), 0);
        assert!(g.promoters().is_empty());
    }

    #[test]
    fn wrapping_is_modular_in_both_directions() {
        let g = Genome::new(vec![0u8; 10]).unwrap();
        for off in 0..100usize {
            assert_eq!(g.wrap(off), off % 10);
        }
        for from in 0..10usize {
            for delta in 0..100usize {
                let want = ((from as i64 - delta as i64).rem_euclid(10)) as usize;
                assert_eq!(g.wrap_back(from, delta), want, "from {from} delta {delta}");
            }
        }
    }

    #[test]
    fn too_long_is_rejected_not_truncated() {
        let e = Genome::new(vec![0u8; MAX_GENOME_LEN + 1]).unwrap_err();
        assert_eq!(e, GenomeError::TooLong(MAX_GENOME_LEN + 1));
        assert!(Genome::new(vec![0u8; MAX_GENOME_LEN]).is_ok());
    }

    #[test]
    fn interning_deduplicates_by_content() {
        let pool = GenomePool::new();
        let a = pool.intern(vec![1, 2, 3]).unwrap();
        let b = pool.intern(vec![1, 2, 3]).unwrap();
        let c = pool.intern(vec![1, 2, 4]).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(pool.live_count(), 2);
    }

    #[test]
    fn dead_genomes_leave_the_pool() {
        let pool = GenomePool::new();
        {
            let _g = pool.intern(vec![9, 9, 9]).unwrap();
            assert_eq!(pool.live_count(), 1);
        }
        assert_eq!(pool.live_count(), 0);
        // and the slot is reused rather than accumulating
        let _g = pool.intern(vec![9, 9, 9]).unwrap();
        assert_eq!(pool.live_count(), 1);
    }

    #[test]
    fn copy_on_write_shares_when_unchanged() {
        let pool = GenomePool::new();
        let base = pool.intern(vec![1, 2, 3]).unwrap();
        let same = pool.derive(&base, |_| {}).unwrap();
        assert!(Arc::ptr_eq(&base, &same));
        let changed = pool.derive(&base, |b| b.push(4)).unwrap();
        assert!(!Arc::ptr_eq(&base, &changed));
        assert_eq!(base.bytes(), &[1, 2, 3]);
    }

    #[test]
    fn content_hash_separates_similar_inputs() {
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..2000u16 {
            assert!(seen.insert(content_hash(&i.to_le_bytes())));
        }
        // trailing zeros are not free
        assert_ne!(content_hash(&[1, 0]), content_hash(&[1]));
        assert_ne!(content_hash(&[]), content_hash(&[0]));
    }
}
