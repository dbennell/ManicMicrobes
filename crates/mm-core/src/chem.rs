//! The chemical table (SPEC §7.1).
//!
//! Sixteen species, indexed `c % 16`, each described by a data-driven entry that comes from
//! the scenario file. Nothing about a chemical is hard-coded: which ones are structural,
//! which yield energy, which are toxic and what colour they render in are all scenario
//! authoring decisions, because the interesting question is what evolves under a *given*
//! chemistry and that is only askable if the chemistry is a parameter.

use serde::{Deserialize, Serialize};

use crate::fixed::Q10_ONE;
use crate::state_hash::{StateHash, StateHasher};

/// Number of chemical species. Fixed at 16: the index is a 4-bit operand so that a mutation
/// to one is a small local perturbation rather than a 1-in-100 lottery, the same reasoning
/// as the 16 organelle slots of SPEC §6.2.
pub const CHEM_COUNT: usize = 16;

/// Reduce an arbitrary index to a chemical. Addressing wraps (SPEC §3).
#[inline(always)]
#[must_use]
pub const fn chem_index(c: i16) -> usize {
    (c as u16 as usize) % CHEM_COUNT
}

/// One chemical species.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ChemicalDef {
    pub name: String,
    /// Fraction of the difference between neighbours that crosses per fluid step, `Q10`.
    ///
    /// Capped at a quarter of `Q10_ONE` when the table is built. The fluid update is
    /// simultaneous in both axes, so a square gives up this fraction of its difference across
    /// all four of its edges at once; a quarter is the largest value for which the update
    /// stays a convex combination and therefore cannot drive a square negative. It is already
    /// faster than anything a scenario wants — at a quarter, a gradient halves in a handful
    /// of steps.
    pub diffusion: i32,
    /// Membrane damage per unit above threshold (M8).
    pub toxicity: i32,
    /// Energy released when oxidised by a mitochondrion (M2).
    pub energy_yield: i32,
    /// Usable as build material (M2).
    pub structural: bool,
    /// Colour for the false-colour overlay (M4).
    pub colour: [u8; 3],

    /// What this species turns into on its own, if anything.
    ///
    /// SPEC §12 lists decay as part of the fluid step without saying what governs it; this is
    /// that, and it is data rather than a special case so that any chemistry can have an
    /// unstable species. It is what closes the loop around a byproduct: peroxide is a real
    /// dead end unless something turns it back into something usable, and hydrogen peroxide
    /// decomposing on its own is what real hydrogen peroxide does.
    ///
    /// Decay is a *balanced reaction* — the units leaving this species arrive in `decay_to` —
    /// so it goes through the ledger like any other and I4 stays exact.
    #[serde(default)]
    pub decay_to: Option<usize>,
    /// Fraction of this species that decays per fluid step, `Q10`.
    #[serde(default)]
    pub decay_rate: i32,
}

impl ChemicalDef {
    /// An inert filler species. The table is always 16 long, so unnamed slots need a value.
    #[must_use]
    pub fn inert(name: &str) -> ChemicalDef {
        ChemicalDef {
            name: name.to_string(),
            diffusion: Q10_ONE / 8,
            toxicity: 0,
            energy_yield: 0,
            structural: false,
            colour: [128, 128, 128],
            decay_to: None,
            decay_rate: 0,
        }
    }
}

/// The largest diffusion rate a chemical may have. See [`ChemicalDef::diffusion`].
pub const MAX_DIFFUSION: i32 = Q10_ONE / 4;

/// All sixteen species.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(from = "Vec<ChemicalDef>", into = "Vec<ChemicalDef>")]
pub struct ChemTable {
    defs: [ChemicalDef; CHEM_COUNT],
}

impl ChemTable {
    /// Build a table, padding a short list with inert filler and clamping diffusion rates.
    ///
    /// A scenario that names fewer than sixteen chemicals is not an error — most will name
    /// far fewer — and the unnamed slots still have to be legal, because `c % 16` means a
    /// genome can address every one of them.
    #[must_use]
    pub fn new(mut defs: Vec<ChemicalDef>) -> ChemTable {
        defs.truncate(CHEM_COUNT);
        while defs.len() < CHEM_COUNT {
            let i = defs.len();
            defs.push(ChemicalDef::inert(&format!("inert_{i}")));
        }
        for d in &mut defs {
            d.diffusion = d.diffusion.clamp(0, MAX_DIFFUSION);
        }
        let mut array = std::array::from_fn(|_| ChemicalDef::inert(""));
        for (slot, def) in array.iter_mut().zip(defs) {
            *slot = def;
        }
        ChemTable { defs: array }
    }

    /// The default table of SPEC §7.1: four inert signalling species, four structural
    /// monomers, three energy substrates, two metabolic wastes, one toxin, two inert filler.
    ///
    /// The names matter to the wiki, not to the engine; a scenario is free to replace all of
    /// them. What the engine reads is `structural`, `energy_yield`, `toxicity` and
    /// `diffusion`.
    #[must_use]
    pub fn spec_default() -> ChemTable {
        let signal = |name: &str, colour: [u8; 3]| ChemicalDef {
            name: name.to_string(),
            diffusion: Q10_ONE / 4,
            toxicity: 0,
            energy_yield: 0,
            structural: false,
            colour,
            decay_to: None,
            decay_rate: 0,
        };
        let monomer = |name: &str, colour: [u8; 3]| ChemicalDef {
            name: name.to_string(),
            diffusion: Q10_ONE / 16,
            toxicity: 0,
            energy_yield: 0,
            structural: true,
            colour,
            decay_to: None,
            decay_rate: 0,
        };
        let substrate = |name: &str, yield_: i32, colour: [u8; 3]| ChemicalDef {
            name: name.to_string(),
            diffusion: Q10_ONE / 8,
            toxicity: 0,
            energy_yield: yield_,
            structural: false,
            colour,
            decay_to: None,
            decay_rate: 0,
        };
        let waste = |name: &str, colour: [u8; 3]| ChemicalDef {
            name: name.to_string(),
            diffusion: Q10_ONE / 6,
            toxicity: 0,
            energy_yield: 0,
            structural: false,
            colour,
            decay_to: None,
            decay_rate: 0,
        };

        ChemTable::new(vec![
            signal("signal_a", [90, 160, 255]),
            signal("signal_b", [120, 200, 255]),
            signal("signal_c", [70, 130, 220]),
            signal("signal_d", [150, 190, 240]),
            monomer("carbon", [70, 70, 80]),
            monomer("nitrogen", [110, 150, 110]),
            monomer("phosphorus", [220, 180, 90]),
            monomer("silicon", [180, 180, 200]),
            // Energy yields are `Q10` energy per `Q10` of matter oxidised, so 1024 is "one
            // unit of sugar is worth one unit of energy". They are set against the organelle
            // upkeep in `OrganelleCatalogue::balanced`: a cell carrying a membrane, a
            // nucleus, a mitochondrion and a chloroplast pays roughly 0.4 energy a tick and
            // can process a few units of matter in that time, so a yield much below 1024
            // means no loadout can pay for itself and every lineage starves regardless of
            // what it evolves. Balancing is M8's milestone; these are the numbers that make
            // the first thing alive.
            substrate("sugar", 1024, [240, 220, 140]),
            substrate("lipid", 1536, [230, 190, 120]),
            substrate("sulphide", 768, [200, 210, 120]),
            waste("carbon_dioxide", [140, 120, 130]),
            waste("ammonia", [160, 140, 190]),
            // Respiration's byproduct. Toxic, so a cell has to get rid of it or take
            // damage; unstable, so what it gets rid of finds its way back into the loop
            // instead of being a permanent matter sink. Both halves matter — without the
            // toxicity nothing ages, and without the decay the world slowly turns into
            // peroxide and dies of it.
            ChemicalDef {
                name: "peroxide".to_string(),
                diffusion: Q10_ONE / 5,
                toxicity: 24,
                energy_yield: 0,
                structural: false,
                colour: [255, 120, 120],
                decay_to: Some(11),
                decay_rate: Q10_ONE / 64,
            },
            ChemicalDef::inert("brine"),
            // What a dead cell leaves behind (SPEC §7.2, M8). A chemical rather than an
            // object, so it is conserved, diffuses and decays through machinery that already
            // exists — but barely diffuses, so a corpse stays where it fell and is worth
            // swimming to, and decays slowly into ordinary waste so nothing accumulates
            // forever. A lysosome turns it back into substrate, which is scavenging.
            ChemicalDef {
                name: "carrion".to_string(),
                diffusion: Q10_ONE / 64,
                toxicity: 0,
                energy_yield: 0,
                // Not build material. `structural` means "a cell can build its body out of
                // this", and a cell cannot build itself out of a corpse — it has to digest one
                // into substrate first, which is what a lysosome is for. Marking it structural
                // would let a cell skip the digestion and eat the dead directly.
                structural: false,
                colour: [150, 90, 90],
                decay_to: Some(11),
                decay_rate: Q10_ONE / 512,
            },
        ])
    }

    /// The definition for a chemical index, which wraps. Total for any input.
    #[inline(always)]
    #[must_use]
    pub fn get(&self, c: usize) -> &ChemicalDef {
        // `% CHEM_COUNT` proves the index against a `[_; CHEM_COUNT]`, so this carries no
        // bounds check to elide and no branch that could fail.
        &self.defs[c % CHEM_COUNT]
    }

    #[must_use]
    pub fn all(&self) -> &[ChemicalDef; CHEM_COUNT] {
        &self.defs
    }

    /// Diffusion rates in index order, for the fluid solver's inner loop.
    #[must_use]
    pub fn diffusion_rates(&self) -> [i32; CHEM_COUNT] {
        std::array::from_fn(|i| self.defs[i].diffusion)
    }

    /// Index of a named chemical, for scenario authoring and the wiki.
    #[must_use]
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.defs.iter().position(|d| d.name == name)
    }
}

impl Default for ChemTable {
    fn default() -> Self {
        Self::spec_default()
    }
}

impl From<Vec<ChemicalDef>> for ChemTable {
    fn from(v: Vec<ChemicalDef>) -> Self {
        ChemTable::new(v)
    }
}

impl From<ChemTable> for Vec<ChemicalDef> {
    fn from(t: ChemTable) -> Self {
        t.defs.to_vec()
    }
}

impl StateHash for ChemTable {
    fn hash_state(&self, h: &mut StateHasher) {
        for d in &self.defs {
            h.bytes(d.name.as_bytes());
            h.i32(d.diffusion);
            h.i32(d.toxicity);
            h.i32(d.energy_yield);
            h.bool(d.structural);
            for c in d.colour {
                h.u8(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_wrap() {
        assert_eq!(chem_index(0), 0);
        assert_eq!(chem_index(15), 15);
        assert_eq!(chem_index(16), 0);
        assert_eq!(chem_index(-1), 15);
        assert_eq!(chem_index(i16::MIN), 0);
        for c in i16::MIN..=i16::MAX {
            assert!(chem_index(c) < CHEM_COUNT);
        }
    }

    #[test]
    fn a_short_table_is_padded_not_rejected() {
        let t = ChemTable::new(vec![ChemicalDef::inert("only")]);
        assert_eq!(t.all().len(), CHEM_COUNT);
        assert_eq!(t.get(0).name, "only");
        assert_eq!(t.get(15).name, "inert_15");
        // and every slot is addressable, because a genome can name any of them
        for c in 0..64 {
            assert!(!t.get(c).name.is_empty());
        }
    }

    #[test]
    fn an_over_long_table_is_truncated() {
        let t = ChemTable::new(vec![ChemicalDef::inert("x"); 40]);
        assert_eq!(t.all().len(), CHEM_COUNT);
    }

    #[test]
    fn diffusion_is_clamped_to_the_safe_maximum() {
        let mut d = ChemicalDef::inert("fast");
        d.diffusion = Q10_ONE * 4;
        let t = ChemTable::new(vec![d]);
        assert_eq!(t.get(0).diffusion, MAX_DIFFUSION);

        let mut d = ChemicalDef::inert("backwards");
        d.diffusion = -500;
        let t = ChemTable::new(vec![d]);
        assert_eq!(t.get(0).diffusion, 0);
    }

    #[test]
    fn the_default_table_matches_the_spec_composition() {
        let t = ChemTable::spec_default();
        assert_eq!(t.all().iter().filter(|d| d.structural).count(), 4);
        assert_eq!(t.all().iter().filter(|d| d.energy_yield > 0).count(), 3);
        assert_eq!(t.all().iter().filter(|d| d.toxicity > 0).count(), 1);
        assert_eq!(t.index_of("sugar"), Some(8));
        assert!(t.all().iter().all(|d| d.diffusion <= MAX_DIFFUSION));
    }

    #[test]
    fn tables_round_trip_through_ron() {
        let t = ChemTable::spec_default();
        let text = ron::to_string(&t).unwrap();
        let back: ChemTable = ron::from_str(&text).unwrap();
        assert_eq!(back, t);
    }
}
