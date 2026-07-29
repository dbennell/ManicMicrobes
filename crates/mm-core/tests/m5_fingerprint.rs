//! M5 acceptance 5 — fingerprint sanity.
//!
//! > SimHash distance correlates with true edit distance at Spearman ρ > 0.8 over a sampled
//! > corpus. If it does not, upgrade to MinHash per SPEC §10.2.
//!
//! Speciation asks "has this lineage drifted far enough to deserve a new name", which is a
//! question about degree, and the fingerprint is the only thing answering it. If the
//! correlation is weak then species boundaries are being drawn at random and every wiki page
//! downstream is fiction. So this runs before anything is built on top of it.
//!
//! Floats are used freely here: this is `tests/`, and hard rule 2 is about `mm-core/src`.

use mm_core::genome::{fingerprint_distance, simhash};
use mm_core::rng::mix64;

/// A deterministic stand-in for a random byte. No `rand` anywhere near this crate.
fn byte(seed: u64, n: u64) -> u8 {
    (mix64(seed ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15)) >> 24) as u8
}

fn random_genome(seed: u64, len: usize) -> Vec<u8> {
    (0..len as u64).map(|i| byte(seed, i)).collect()
}

/// True Levenshtein distance. Quadratic, which is why the corpus is small and the genomes
/// are short — this is the ground truth the cheap measure is being checked against, so it has
/// to be the real thing and not another approximation.
fn edit_distance(a: &[u8], b: &[u8]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Spearman's rank correlation: Pearson over the ranks, with ties averaged.
fn spearman(xs: &[f64], ys: &[f64]) -> f64 {
    fn ranks(v: &[f64]) -> Vec<f64> {
        let mut order: Vec<usize> = (0..v.len()).collect();
        order.sort_by(|a, b| {
            v[*a]
                .partial_cmp(&v[*b])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut out = vec![0.0; v.len()];
        let mut i = 0;
        while i < order.len() {
            // Ties share the average of the ranks they span, which is what makes this
            // Spearman rather than something that merely resembles it — and the fingerprint
            // is an integer 0..=64, so ties are the common case, not an edge case.
            let mut j = i;
            while j + 1 < order.len() && v[order[j + 1]] == v[order[i]] {
                j += 1;
            }
            let mean = ((i + j) as f64) / 2.0 + 1.0;
            for k in i..=j {
                out[order[k]] = mean;
            }
            i = j + 1;
        }
        out
    }
    let (rx, ry) = (ranks(xs), ranks(ys));
    let n = rx.len() as f64;
    let (mx, my) = (rx.iter().sum::<f64>() / n, ry.iter().sum::<f64>() / n);
    let mut cov = 0.0;
    let mut vx = 0.0;
    let mut vy = 0.0;
    for i in 0..rx.len() {
        let (dx, dy) = (rx[i] - mx, ry[i] - my);
        cov += dx * dy;
        vx += dx * dx;
        vy += dy * dy;
    }
    if vx == 0.0 || vy == 0.0 {
        return 0.0;
    }
    cov / (vx * vy).sqrt()
}

/// A corpus of mutated descendants, which is the population the measure actually serves.
///
/// Deliberately *not* pairs of unrelated random genomes. Speciation only ever compares a cell
/// against its own species founder, so the distances that matter are the small-to-moderate
/// ones a lineage accumulates by drifting — and a corpus of unrelated genomes would measure
/// the fingerprint on inputs it is never asked about, and would score well for the
/// uninteresting reason that everything is far from everything.
fn drifting_corpus(seed: u64) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    for lineage in 0..24u64 {
        let founder = random_genome(seed ^ lineage, 220);
        let mut current = founder.clone();
        for step in 0..24u64 {
            // One mutation per step, of the kinds the real operator set applies.
            let r = mix64(seed ^ (lineage << 32) ^ step);
            let at = (r % current.len().max(1) as u64) as usize;
            match r % 3 {
                0 => current[at] ^= 1 << ((r >> 8) % 8),
                1 => current.insert(at, byte(r, step)),
                _ => {
                    if current.len() > 8 {
                        current.remove(at);
                    }
                }
            }
            out.push((founder.clone(), current.clone()));
        }
    }
    out
}

#[test]
fn acceptance_fingerprint_distance_tracks_edit_distance() {
    let corpus = drifting_corpus(0xA11CE);
    let mut fingerprints = Vec::new();
    let mut edits = Vec::new();
    for (a, b) in &corpus {
        fingerprints.push(f64::from(fingerprint_distance(simhash(a), simhash(b))));
        edits.push(edit_distance(a, b) as f64);
    }
    let rho = spearman(&fingerprints, &edits);
    eprintln!(
        "{} pairs, edit distance {:.0}..{:.0}, fingerprint distance {:.0}..{:.0}, Spearman rho {rho:.3}",
        corpus.len(),
        edits.iter().cloned().fold(f64::INFINITY, f64::min),
        edits.iter().cloned().fold(0.0, f64::max),
        fingerprints.iter().cloned().fold(f64::INFINITY, f64::min),
        fingerprints.iter().cloned().fold(0.0, f64::max),
    );
    assert!(
        rho > 0.8,
        "SimHash distance correlates with edit distance at only rho = {rho:.3}; SPEC §10.2 \
         says to upgrade to a 32x u16 MinHash sketch when this happens, behind the same \
         interface"
    );
}

#[test]
fn an_identical_genome_is_at_distance_zero() {
    let g = random_genome(7, 200);
    assert_eq!(fingerprint_distance(simhash(&g), simhash(&g.clone())), 0);
}

#[test]
fn one_byte_changing_moves_a_few_bits_not_all_of_them() {
    // The whole property a content hash lacks. If this ever reads like an avalanche, the
    // fingerprint has stopped being locality-sensitive and speciation is drawing boundaries
    // at random.
    let g = random_genome(11, 200);
    let mut moved = Vec::new();
    for at in (0..g.len()).step_by(7) {
        let mut m = g.clone();
        m[at] ^= 0x20;
        moved.push(fingerprint_distance(simhash(&g), simhash(&m)));
    }
    let mean = moved.iter().sum::<u32>() as f64 / moved.len() as f64;
    eprintln!("one byte flipped moves {mean:.1} bits of 64 on average");
    assert!(
        mean < 12.0,
        "one byte moved {mean:.1} bits; that is an avalanche, not a fingerprint"
    );
    assert!(
        mean > 0.5,
        "one byte moved {mean:.1} bits; the fingerprint is barely responding to change at all"
    );
}

#[test]
fn short_and_empty_genomes_still_get_a_fingerprint() {
    // Anything shorter than one k-mer has no windows to vote. Without a fallback every short
    // genome would fingerprint to the same value and they would all be one species.
    let short: Vec<u64> = (0..4u64).map(|n| simhash(&random_genome(n, 3))).collect();
    let distinct: std::collections::BTreeSet<u64> = short.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        short.len(),
        "short genomes collided: {short:?}"
    );
    // And the empty genome has *a* fingerprint, whatever it is.
    let _ = simhash(&[]);
}

#[test]
fn unrelated_genomes_sit_far_apart() {
    // The other end of the scale: if drifted descendants and total strangers scored the same,
    // no threshold could tell them apart.
    let mut total = 0u32;
    let n = 64;
    for i in 0..n as u64 {
        let a = simhash(&random_genome(i, 220));
        let b = simhash(&random_genome(i ^ 0xFFFF, 220));
        total += fingerprint_distance(a, b);
    }
    let mean = f64::from(total) / f64::from(n);
    eprintln!("unrelated genomes sit {mean:.1} bits apart on average");
    assert!(
        mean > 20.0,
        "unrelated genomes are only {mean:.1} bits apart; there is no room for a threshold \
         between drift and difference"
    );
}
