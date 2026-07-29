//! Latinate binomials for species (SPEC §10.4).
//!
//! > Auto-generated Linnaean binomials from Latinate syllable tables, seeded by lineage hash,
//! > with the specific epithet biased by dominant traits.
//!
//! # Why a name matters
//!
//! "Species 4,182" and *Cilius rapidus* carry the same information and are not the same
//! thing. The wiki, the timeline and the tree are the product (CLAUDE.md), and a thing with a
//! name is a thing you can care about, notice the absence of, and tell a story about. This
//! module is small and it is not decoration.
//!
//! # The rules it plays by
//!
//! **Deterministic.** A name is a pure function of the lineage hash and the loadout, so the
//! same run names the same species the same way on any machine — and a replayed archive reads
//! identically to the run it came from.
//!
//! **The epithet is earned, the genus is not.** A genus comes out of the lineage hash and
//! means nothing except "related to". The epithet is chosen from what the cell is actually
//! built out of, so *lucens* really is full of chloroplasts and *rapidus* really does have
//! cilia. A name that described nothing would be worse than a number, because it would look
//! like it described something.

use crate::organelle::{OrganelleType, SLOT_COUNT};

/// Genus stems. Latin-ish, chosen to be pronounceable and distinguishable at a glance — a
/// list where half the entries look like each other makes the tree unreadable.
const STEMS: [&str; 32] = [
    "Cili", "Vacu", "Mito", "Chloro", "Membra", "Nucle", "Pyro", "Halo", "Litho", "Aqui", "Thermo",
    "Crypto", "Glauco", "Rubi", "Viri", "Stella", "Umbra", "Lumen", "Fluvi", "Terra", "Nebul",
    "Arca", "Basi", "Cera", "Dendri", "Echin", "Filo", "Gemmi", "Hyali", "Icthy", "Lepto", "Micro",
];

/// Genus endings.
const ENDINGS: [&str; 8] = ["us", "a", "ella", "ium", "ina", "opsis", "onema", "ospira"];

/// Epithets that are not about traits, for a species with nothing that stands out. Still
/// meaningful — they say "unremarkable", which is a true thing to say.
const PLAIN: [&str; 12] = [
    "vulgaris", "communis", "simplex", "modestus", "quietus", "medius", "levis", "tenuis",
    "placidus", "obscurus", "sobrius", "incertus",
];

/// A generated binomial.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Binomial {
    pub genus: String,
    pub epithet: String,
}

impl Binomial {
    #[must_use]
    pub fn full(&self) -> String {
        format!("{} {}", self.genus, self.epithet)
    }

    /// *C. rapidus* — how a species is written once its genus is established, as in the wiki
    /// prose of SPEC §10.5.
    #[must_use]
    pub fn abbreviated(&self) -> String {
        let initial = self.genus.chars().next().unwrap_or('?');
        format!("{initial}. {}", self.epithet)
    }
}

impl std::fmt::Display for Binomial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.genus, self.epithet)
    }
}

/// What a species is made of, as far as naming cares.
///
/// Counts of each organelle type across a representative member, plus the few runtime facts
/// that show up in a name. Deliberately small: a name should fall out of what a cell *is*,
/// and anything richer belongs in the wiki page's prose rather than in two words.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Traits {
    pub counts: [u8; SLOT_COUNT],
    /// Genome length, for the epithets about size.
    pub genome_len: u16,
}

impl Traits {
    /// Read a loadout off a cell's organelle slots.
    #[must_use]
    pub fn of(slots: &[crate::organelle::Organelle], genome_len: usize) -> Traits {
        let mut counts = [0u8; SLOT_COUNT];
        for o in slots {
            if o.is_present() {
                let k = o.kind as usize % SLOT_COUNT;
                counts[k] = counts[k].saturating_add(1);
            }
        }
        Traits {
            counts,
            genome_len: genome_len.min(u16::MAX as usize) as u16,
        }
    }

    fn count(&self, kind: OrganelleType) -> u8 {
        self.counts.get(kind as usize).copied().unwrap_or(0)
    }
}

/// Build a species name.
///
/// `lineage` is the species' own hash — its founder fingerprint mixed with its id — so sister
/// species get different names and a replay gets the same ones.
#[must_use]
pub fn name(lineage: u64, traits: &Traits) -> Binomial {
    let h = crate::rng::mix64(lineage);
    let stem = STEMS[(h % STEMS.len() as u64) as usize];
    let ending = ENDINGS[((h >> 8) % ENDINGS.len() as u64) as usize];
    Binomial {
        genus: format!("{stem}{ending}"),
        epithet: epithet(h >> 16, traits),
    }
}

/// Choose the specific epithet from whatever stands out most.
///
/// Ordered by how much a trait tells you about the organism, not alphabetically: motility and
/// how it earns its energy come before how big its genome is, because that is the order
/// someone reading the tree cares about. Ties fall through to the next test, and a species
/// with nothing notable gets a plain epithet keyed off its own hash rather than a shared
/// default — a slide where forty species are all called *vulgaris* has lost the plot.
fn epithet(salt: u64, t: &Traits) -> String {
    let cilia = t.count(OrganelleType::Cilium);
    let chloroplasts = t.count(OrganelleType::Chloroplast);
    let mitochondria = t.count(OrganelleType::Mitochondrion);
    let vacuoles = t.count(OrganelleType::Vacuole);
    let pumps = t.count(OrganelleType::Pump);
    let sensors = t
        .count(OrganelleType::Chemosensor)
        .saturating_add(t.count(OrganelleType::Photosensor))
        .saturating_add(t.count(OrganelleType::TouchSensor));

    // SPEC names three of these explicitly: `rapidus` for cilium investment, `lucens` for
    // chloroplast dominance, `vorax` for predation. Predation is M8 — there is no predation
    // rate to read yet — so `vorax` is not awarded rather than being awarded on a guess.
    let word = if cilia >= 3 {
        "rapidus"
    } else if cilia >= 1 && sensors >= 1 {
        // Sensors *and* motility is the interesting combination: something that goes
        // somewhere for a reason, which is M3's whole goal.
        "explorator"
    } else if cilia >= 1 {
        "natans"
    } else if sensors >= 2 {
        "sensilis"
    } else if chloroplasts >= 3 || (chloroplasts >= 2 && mitochondria == 0) {
        "lucens"
    } else if chloroplasts >= 1 && mitochondria >= 1 {
        "mixtus"
    } else if mitochondria >= 3 {
        "fervens"
    } else if vacuoles >= 2 {
        "capax"
    } else if pumps >= 2 {
        "avidus"
    } else if t.genome_len >= 600 {
        "prolixus"
    } else if t.genome_len > 0 && t.genome_len <= 96 {
        "minimus"
    } else {
        PLAIN[(salt % PLAIN.len() as u64) as usize]
    };
    word.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organelle::Organelle;

    fn with(kinds: &[(OrganelleType, u8)]) -> Traits {
        let mut slots = Vec::new();
        for (kind, n) in kinds {
            for _ in 0..*n {
                slots.push(Organelle::finished(*kind, 40));
            }
        }
        Traits::of(&slots, 240)
    }

    #[test]
    fn a_name_is_the_same_every_time() {
        let t = with(&[(OrganelleType::Cilium, 3)]);
        assert_eq!(name(12345, &t), name(12345, &t));
    }

    #[test]
    fn different_lineages_get_different_genera() {
        let t = with(&[]);
        let names: std::collections::BTreeSet<String> =
            (0..200u64).map(|i| name(i, &t).genus).collect();
        // Not all distinct — the tables are finite and collisions are fine — but a table that
        // collapsed to a handful of names would make the tree unreadable.
        assert!(
            names.len() > 60,
            "200 lineages produced only {} distinct genera",
            names.len()
        );
    }

    #[test]
    fn the_epithet_describes_the_organism() {
        // The property that makes a name worth more than a number.
        assert_eq!(
            name(1, &with(&[(OrganelleType::Cilium, 4)])).epithet,
            "rapidus"
        );
        assert_eq!(
            name(1, &with(&[(OrganelleType::Chloroplast, 4)])).epithet,
            "lucens"
        );
        assert_eq!(
            name(1, &with(&[(OrganelleType::Mitochondrion, 3)])).epithet,
            "fervens"
        );
        assert_eq!(
            name(1, &with(&[(OrganelleType::Chemosensor, 2)])).epithet,
            "sensilis"
        );
        // Sensors and cilia together: something that goes somewhere for a reason.
        assert_eq!(
            name(
                1,
                &with(&[(OrganelleType::Cilium, 1), (OrganelleType::Photosensor, 1)])
            )
            .epithet,
            "explorator"
        );
    }

    #[test]
    fn vorax_is_not_awarded_before_predation_exists() {
        // SPEC §10.4 names `vorax` for high predation rate. Predation is M8. Awarding it now
        // would mean the wiki described a behaviour the simulation cannot perform.
        for i in 0..500u64 {
            let t = with(&[(OrganelleType::Pump, (i % 4) as u8)]);
            assert_ne!(name(i, &t).epithet, "vorax");
        }
    }

    #[test]
    fn an_unremarkable_species_does_not_share_one_name_with_every_other() {
        let t = with(&[]);
        let epithets: std::collections::BTreeSet<String> =
            (0..200u64).map(|i| name(i, &t).epithet).collect();
        assert!(
            epithets.len() >= PLAIN.len() / 2,
            "plain species collapsed to {} distinct epithets",
            epithets.len()
        );
    }

    #[test]
    fn abbreviation_reads_the_way_the_wiki_writes_it() {
        let b = Binomial {
            genus: "Cilius".into(),
            epithet: "rapidus".into(),
        };
        assert_eq!(b.full(), "Cilius rapidus");
        assert_eq!(b.abbreviated(), "C. rapidus");
    }
}
