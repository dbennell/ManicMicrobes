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
pub const CHEM_COUNT: usize = 17;

/// Dinitrogen: the inert reservoir a diazosome cracks, and the reason the table is seventeen.
///
/// # Why it has to be on the slide
///
/// Only energy enters and leaves this world — light in, heat out. Matter is neither created nor
/// destroyed, only transferred and transformed, so a reservoir that is not on the slide cannot
/// exist. Real fixation draws on an atmospheric pool, and the honest way to have that here is to
/// *put the atmosphere in the table*, at the cost of a chemical. `Ledger::record_injected` would
/// have been cheaper and is scenario-setup machinery: an organelle calling it every tick is a
/// tap, and a closed system with a tap is a flow reactor.
///
/// What the slot buys is worth more than the slot. Under a tap, nitrogen availability is a number
/// somebody chose; with two pools on the slide, total nitrogen is fixed at seeding and the
/// **split between locked and available is a state variable that evolves**. A young world is
/// nearly all inert and gated on diazotrophs; a mature one has pulled its nitrogen into
/// circulation, and a 9,216-carbon diazosome becomes dead weight the lineage should drop.
/// Scarcity becomes a historical property of that world rather than a parameter.
pub const DINITROGEN: usize = 16;

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

    /// How strongly the flow carries this species, `Q10`. One is "goes where the water goes".
    ///
    /// SPEC §17.4 asks particulate for "a settling rate and a breakdown rate, since *solid* is a
    /// behaviour and not a category". The breakdown rate is `decay_to` and `decay_rate` and has
    /// been here since M10. This is the other half, and it is not called *settling* because in a
    /// slide seen from above there is no down to settle towards: gravity pulls to the middle of
    /// the plate, not out of the plane. What being heavy actually means here is coupling to the
    /// flow less than the water does — a grain lags the current, drops out of a plume, and ends
    /// up somewhere the dissolved fraction does not.
    ///
    /// Scaling the edge velocity is exactly conservative and cannot be otherwise: the flux is
    /// still one number subtracted from one square and added to its neighbour, and scaling it
    /// down only moves less. It cannot threaten the CFL bound either, for the same reason.
    ///
    /// Defaults to `Q10_ONE`, so a table that says nothing behaves as every table did before
    /// this field existed.
    #[serde(default = "full_advection")]
    pub advection: i32,
}

/// A chemical that goes exactly where the water goes, which is what all of them did before
/// [`ChemicalDef::advection`] existed.
fn full_advection() -> i32 {
    Q10_ONE
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
            advection: Q10_ONE,
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
            advection: Q10_ONE,
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
            advection: Q10_ONE,
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
            advection: Q10_ONE,
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
            advection: Q10_ONE,
        };

        ChemTable::new(vec![
            signal("signal_a", [90, 160, 255]),
            signal("signal_b", [120, 200, 255]),
            signal("signal_c", [70, 130, 220]),
            signal("signal_d", [150, 190, 240]),
            monomer("carbon", [70, 70, 80]),
            // **The three minerals move in three different ways, and that is the whole of what
            // makes them three niches rather than one scarcity three times over.**
            //
            // Nothing produces any of them (`docs/CHEMISTRY.md` §8): seeding, a `Flux` or a
            // leaching wall is the only way in, and what a world has is what it was given. So the
            // question each one poses to a cell is not "can I make this" but "can I get to it",
            // and the answer is set here.
            //
            // Nitrogen drifts. Its reservoir is dissolved and well mixed, so a patch drawn down
            // refills from its neighbourhood without anyone doing anything, and a current
            // carries it. Diffusion above the monomer default for exactly that: it is the one of
            // the three that comes to you.
            ChemicalDef {
                name: "nitrogen".to_string(),
                diffusion: Q10_ONE / 8,
                toxicity: 0,
                energy_yield: 0,
                structural: true,
                colour: [110, 150, 110],
                decay_to: None,
                decay_rate: 0,
                advection: Q10_ONE,
            },
            // Phosphorus does not move at all. Its cycle has no gas phase — the only primary
            // source is rock — which is why it is so often the ultimate limiting nutrient, and
            // here it means an outcrop is a *location* rather than a level. Zero on both axes, so
            // the only thing that carries phosphate is a cell that ate it, and colonising away
            // from a supply means taking it with you, in vacuoles, at the cost of slots.
            //
            // A patch stripped of it therefore does not heal by diffusion. It heals when
            // something dies there, which is the phosphorus cycle doing what it does. That is a
            // harsh mechanism on purpose and it is also the cheapest: `fluid.rs` skips both axes
            // for a chemical with nothing to do, so being immobile costs nothing at all.
            ChemicalDef {
                name: "phosphorus".to_string(),
                diffusion: 0,
                toxicity: 0,
                energy_yield: 0,
                structural: true,
                colour: [220, 180, 90],
                decay_to: None,
                decay_rate: 0,
                advection: 0,
            },
            // Silicon settles. Dissolved silicate is middling mobile, but what matters here is
            // where it *ends up*: a shell returns its silicon in full at the death of the cell
            // that grew it, and a low coupling to the flow keeps it near where that happened. A
            // bed of shells is what a slide of dead armoured cells should leave behind, and that
            // is diatom ooze, which is a real thing made the same way.
            ChemicalDef {
                name: "silicon".to_string(),
                diffusion: Q10_ONE / 32,
                toxicity: 0,
                energy_yield: 0,
                structural: true,
                colour: [180, 180, 200],
                decay_to: None,
                decay_rate: 0,
                advection: Q10_ONE / 4,
            },
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
            // Detritus: the particulate (SPEC §17.4), in the slot `CHEMISTRY.md` §2 listed as
            // "ammonia — filler". Nothing read it, nothing made it, and no pathway named it.
            //
            // Solid, expressed as behaviour rather than as a category, which is what §17.4 asks
            // for — and the behaviour that makes it particulate is the *advection*, not the
            // diffusion. It is carried by the flow at a third of the water's speed, so it lags a
            // current, drops out of a plume, and piles up where the dissolved fraction does not,
            // which is what makes somewhere worth sitting. It breaks down very slowly into the
            // structural chemical, so a grain nobody eats still becomes building material in the
            // end, a few hundred thousand ticks later.
            //
            // It diffuses *more* than carrion, which is the correction to the first version of
            // this entry. Detritus is suspended and travelling — that is the whole idea — while
            // a corpse is a deposit that stays where it fell and is worth swimming to. Making
            // the travelling one the least mobile thing in the table had them the wrong way
            // round, and `m8_ecology` said so.
            //
            // Not `structural` itself, for the reason carrion is not: a cell cannot build itself
            // out of a lump it has not taken apart. Getting the carbon out of it early is what a
            // filter is for, and that is the whole trade — wait for it to rot, or catch it.
            ChemicalDef {
                name: "detritus".to_string(),
                diffusion: Q10_ONE / 32,
                toxicity: 0,
                energy_yield: 0,
                structural: false,
                colour: [190, 170, 130],
                decay_to: Some(4),
                // One, and it has to be written as a literal.
                //
                // This was `Q10_ONE / 2048`, and `Q10_ONE` is 1024 — so the rate was **zero**,
                // and `World::decay_fluid` skips any chemical whose rate is not positive.
                // Detritus has never mineralised at all, in spite of the note above promising it
                // does "a few hundred thousand ticks later". A silent integer truncation turned
                // a documented mechanism into a no-op, and nothing noticed because until carrion
                // was routed here in the same commit, nothing in the engine produced any
                // detritus for the missing decay to act on.
                //
                // One is the floor a `Q10` rate can express: a 1024th of the plane per fluid
                // step, which is a half-life near seven hundred ticks. That is faster than the
                // comment's "hundreds of thousands" and cannot be made slower without changing
                // the unit — and it is still half the speed carrion rots at (2/1024), so the
                // chain keeps the order it should: a corpse breaks up faster than the particulate
                // it breaks into turns back to building material.
                decay_rate: 1,
                advection: Q10_ONE / 3,
            },
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
                advection: Q10_ONE,
            },
            // The oxidant, and the only reason it is not obvious is that it used to be called
            // `brine`. It is `ChemicalDef::inert` because it needs no engine semantics of its
            // own — no yield, no toxicity, not structural — but *inert* describes its
            // definition and not its role: `MetabolicChemistry::default` names index 14 as the
            // oxidant of all four pathways and as what photosynthesis produces alongside the
            // substrate, so it is load-bearing in every reaction this world runs.
            //
            // The name cost real time. Asked which chemical slots were free, the honest reading
            // of `inert("brine")` beside a comment calling it filler is "that one" — and taking
            // it would have broken every way of making a living on the slide. A thing named for
            // what it is cannot be mistaken for a thing named for what it lacks.
            //
            // Values are unchanged from the entry it replaces apart from the colour: `inert`
            // paints everything the same grey, which was fine when nothing listed the whole
            // table and is not now that the legend does.
            ChemicalDef {
                name: "oxygen".to_string(),
                diffusion: Q10_ONE / 8,
                toxicity: 0,
                energy_yield: 0,
                structural: false,
                colour: [150, 205, 225],
                decay_to: None,
                decay_rate: 0,
                advection: Q10_ONE,
            },
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
                // A corpse rots into *particulate*, not into breath.
                //
                // This was `Some(11)` — carbon dioxide — and that made structural matter a
                // one-way sink. Bodies are built from index 4 alone; half of every corpse
                // becomes carrion, and nothing anywhere in the default chemistry produces index
                // 4 except a death returning the other half. So the buildable pool only ever
                // shrank, and any world where carbon became the binding constraint was on a
                // countdown rather than at a carrying capacity. Measured on a 256-square slide
                // seeded lean: 16,591 cells at tick 6,000 and 373 at 30,000, with carbon down
                // from 552M to 25M while *sugar* rose to 11.1 billion — a population starving
                // for something to build with while sitting on four hundred times its remaining
                // carbon in a form it cannot build with.
                //
                // Detritus is the return path and it always was: `ecology`'s filter converts it
                // straight to structural matter, and it decays to index 4 on its own. It simply
                // had no producer — the only detritus in the world was what `the_tide` and
                // `the_drift` piped in abiotically through a `Flux`. Two halves of a
                // decomposition chain that had never been joined.
                //
                // Joined, the chain is: corpse → carrion, which a lysosome digests for *energy*;
                // carrion → detritus, which drifts and can be filtered; detritus → carbon, which
                // is *structure* again. Three stages, each slower than the last (Q10/512 here
                // against Q10/2048 there), each with its own consumer. It also gives the
                // holdfast's filter an endogenous food supply for the first time, which is what
                // makes sessile suspension feeding a living on every slide rather than on two.
                decay_to: Some(12),
                decay_rate: Q10_ONE / 512,
                advection: Q10_ONE,
            },
            // Dinitrogen: the inert pool, and the only chemical in the table that is inert on
            // purpose rather than for want of a mechanism.
            //
            // **Diffusion on, advection off.** A dissolved gas ought to go where the water goes,
            // so switching advection off is a deliberate departure and the exhaustible pool that
            // follows is the point: a diazotroph mat draws its local supply down faster than the
            // neighbourhood refills it, which makes the inert reservoir a fourth spatial scarcity
            // rather than a flat background. Diffusion stays on so an exhausted patch recovers
            // instead of scarring for the rest of the run.
            //
            // Measured before it was chosen (`docs/CHEMISTRY.md` §8): against a realistic
            // seven-plane slide this costs about 8% of the fluid step, where full mobility costs
            // 15% and immobility is free. `fluid.rs` gates each axis on being non-zero and then
            // does identical work whatever the value, so there is no cheap-because-slow option —
            // it is off, or it is most of the price.
            ChemicalDef {
                name: "dinitrogen".to_string(),
                diffusion: Q10_ONE / 12,
                toxicity: 0,
                energy_yield: 0,
                structural: false,
                colour: [120, 130, 160],
                decay_to: None,
                decay_rate: 0,
                advection: 0,
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

    /// Each species' coupling to the flow, in index order. See [`ChemicalDef::advection`].
    #[must_use]
    pub fn advection_rates(&self) -> [i32; CHEM_COUNT] {
        std::array::from_fn(|c| self.defs[c].advection.clamp(0, Q10_ONE))
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
        // Seventeen since dinitrogen joined the table, and these are the assertions that
        // change when it does: what an out-of-range chemical operand *means* is part of the ISA,
        // exactly as `BUILD 19` naming a different organelle is.
        assert_eq!(chem_index(0), 0);
        assert_eq!(chem_index(16), 16);
        assert_eq!(chem_index(CHEM_COUNT as i16), 0);
        // Not `CHEM_COUNT - 1`, which is what a table of sixteen trained the eye to expect:
        // `-1` is 65,535 unsigned and 65,535 is 3,855 x 17 exactly, so it lands on 0. Worth an
        // assertion of its own precisely because it is the kind of thing a widening changes
        // silently.
        assert_eq!(chem_index(-1), 0);
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
