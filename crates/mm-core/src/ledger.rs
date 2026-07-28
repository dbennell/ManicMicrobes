//! Global matter and energy accounting (invariants I4 and I5).
//!
//! # Two different claims
//!
//! Matter is **conserved**: the sum of each chemical over fluid, cell interiors, structural
//! mass and corpses is invariant to the exact integer. Energy is **not** conserved — it
//! degrades — it is *accounted*: `energy_in == energy_out + Δenergy_stored`, exactly, in
//! integer units. Confusing the two would be a design error, so they are different types
//! here with different operations.
//!
//! # Why a ledger rather than a sum
//!
//! Recomputing the true total means touching every square of every chemical, which at
//! 512×512×16 is four million values and far too slow to do every tick. So the ledger
//! maintains running totals, and every route by which matter enters or leaves the fluid
//! reports what it moved.
//!
//! That makes the ledger a *claim*. [`crate::substrate::Substrate::total_chem`] is the
//! independent recomputation that checks it, and the acceptance tests compare the two. A
//! ledger that agreed with itself would prove nothing; the point is that two different
//! calculations agree.
//!
//! # Where matter can legitimately leave
//!
//! Nowhere, at M1. The fluid solver conserves by construction, so at this milestone the
//! ledger should never move at all after initialisation. From M2 matter moves between the
//! fluid and cell interiors, and both sides are inside the accounted total, so it still
//! never moves. The one genuine exit is a barrier raised over an occupied square, which
//! evicts what was there — [`Ledger::record_evicted`] is how that gets said out loud rather
//! than silently breaking I4.

use crate::chem::CHEM_COUNT;
use crate::state_hash::{StateHash, StateHasher};

/// Running totals for matter and energy.
///
/// `i64` throughout: four million squares of `i32` overflow an `i32` total by three orders of
/// magnitude, and a conservation test that silently wrapped would be worse than no test.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Ledger {
    /// Total of each chemical across every compartment.
    chem: [i64; CHEM_COUNT],
    /// Matter deliberately removed from the world, per chemical. Only barriers do this.
    evicted: [i64; CHEM_COUNT],
    /// Energy absorbed from light, cumulative.
    energy_in: i64,
    /// Energy dissipated as heat, cumulative.
    energy_out: i64,
    /// Energy currently held by living things.
    energy_stored: i64,
    /// Matter converted between species by metabolism, cumulative.
    converted: i64,
}

/// What a mismatch looked like, for a test failure that says something useful.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LedgerBreach {
    /// The ledger's claimed total and the recomputed total disagree.
    Matter {
        chemical: usize,
        claimed: i64,
        actual: i64,
    },
    /// `energy_in != energy_out + energy_stored`.
    Energy {
        energy_in: i64,
        energy_out: i64,
        energy_stored: i64,
    },
}

impl std::fmt::Display for LedgerBreach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerBreach::Matter {
                chemical,
                claimed,
                actual,
            } => write!(
                f,
                "chemical {chemical}: ledger claims {claimed}, world holds {actual} \
                 (drift {})",
                actual - claimed
            ),
            LedgerBreach::Energy {
                energy_in,
                energy_out,
                energy_stored,
            } => write!(
                f,
                "energy_in {energy_in} != energy_out {energy_out} + stored {energy_stored} \
                 (drift {})",
                energy_in - energy_out - energy_stored
            ),
        }
    }
}

impl std::error::Error for LedgerBreach {}

impl Ledger {
    #[must_use]
    pub fn new() -> Ledger {
        Ledger::default()
    }

    /// Adopt the world's current contents as the baseline. Called once, at initialisation.
    pub fn set_baseline(&mut self, totals: [i64; CHEM_COUNT]) {
        self.chem = totals;
    }

    #[must_use]
    pub fn chem_totals(&self) -> [i64; CHEM_COUNT] {
        self.chem
    }

    #[must_use]
    pub fn evicted(&self) -> [i64; CHEM_COUNT] {
        self.evicted
    }

    #[must_use]
    pub fn energy_in(&self) -> i64 {
        self.energy_in
    }

    #[must_use]
    pub fn energy_out(&self) -> i64 {
        self.energy_out
    }

    #[must_use]
    pub fn energy_stored(&self) -> i64 {
        self.energy_stored
    }

    /// Matter destroyed by raising a barrier over an occupied square.
    ///
    /// The only sanctioned way for matter to leave. It is subtracted from the total *and*
    /// recorded separately, so a run can always answer "how much did the user delete" rather
    /// than the deletion looking like a conservation bug.
    pub fn record_evicted(&mut self, evicted: &[i32; CHEM_COUNT]) {
        for (c, amount) in evicted.iter().enumerate() {
            self.chem[c] = self.chem[c].saturating_sub(*amount as i64);
            self.evicted[c] = self.evicted[c].saturating_add(*amount as i64);
        }
    }

    /// One species becoming another, in a balanced reaction.
    ///
    /// The only way a per-species total may move. Total matter is unchanged by construction —
    /// the same figure leaves one species and arrives in another — so I4 survives in its exact
    /// per-species form rather than being weakened to a sum. An unreported transmutation shows
    /// up in `check_matter` as drift, which is the point: metabolism has to say what it did.
    pub fn convert(&mut self, from: usize, to: usize, amount: i64) {
        if amount <= 0 {
            return;
        }
        let from = from % CHEM_COUNT;
        let to = to % CHEM_COUNT;
        if from == to {
            return;
        }
        self.chem[from] = self.chem[from].saturating_sub(amount);
        self.chem[to] = self.chem[to].saturating_add(amount);
        self.converted = self.converted.saturating_add(amount);
    }

    /// Total matter converted between species, cumulative. Instrumentation only.
    #[must_use]
    pub fn converted(&self) -> i64 {
        self.converted
    }

    /// Total matter across every species — the quantity no mechanism may move.
    #[must_use]
    pub fn total_matter(&self) -> i64 {
        self.chem.iter().sum()
    }

    /// Matter added to the world from outside — seeding a scenario, a tool dropping food in.
    ///
    /// Not something the simulation does to itself. If a mechanism ever needs this, that
    /// mechanism is creating matter, and the question is whether it should be.
    pub fn record_injected(&mut self, chemical: usize, amount: i32) {
        let c = chemical % CHEM_COUNT;
        self.chem[c] = self.chem[c].saturating_add(amount as i64);
    }

    /// Energy absorbed from light by a chloroplast (M2). Enters the accounts and is stored.
    pub fn absorb(&mut self, amount: i64) {
        if amount <= 0 {
            return;
        }
        self.energy_in = self.energy_in.saturating_add(amount);
        self.energy_stored = self.energy_stored.saturating_add(amount);
    }

    /// Energy leaving as heat: metabolic inefficiency, movement drag, maintenance.
    ///
    /// Clamped to what is actually stored — the world cannot dissipate energy it does not
    /// hold — and the amount actually dissipated is returned, because the caller has to
    /// deduct the same figure from whichever cell paid it.
    pub fn dissipate(&mut self, amount: i64) -> i64 {
        if amount <= 0 {
            return 0;
        }
        let actual = amount.min(self.energy_stored);
        self.energy_out = self.energy_out.saturating_add(actual);
        self.energy_stored = self.energy_stored.saturating_sub(actual);
        actual
    }

    /// Overwrite every field, for snapshot restoration.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn restore(
        &mut self,
        chem: [i64; CHEM_COUNT],
        evicted: [i64; CHEM_COUNT],
        energy_in: i64,
        energy_out: i64,
        energy_stored: i64,
        converted: i64,
    ) {
        self.chem = chem;
        self.evicted = evicted;
        self.energy_in = energy_in;
        self.energy_out = energy_out;
        self.energy_stored = energy_stored;
        self.converted = converted;
    }

    /// Adopt the world's current stored energy as the baseline.
    ///
    /// What was in the world at tick zero was not absorbed from anywhere, so it counts as
    /// both `energy_in` and `energy_stored` and the identity starts balanced.
    pub fn set_energy_baseline(&mut self, stored: i64) {
        self.energy_in = stored;
        self.energy_out = 0;
        self.energy_stored = stored;
    }

    /// Check I5. Called every tick in debug builds and by the acceptance tests.
    ///
    /// # Errors
    ///
    /// Returns the breach if the identity does not hold exactly.
    pub fn check_energy(&self) -> Result<(), LedgerBreach> {
        if self.energy_in == self.energy_out + self.energy_stored {
            Ok(())
        } else {
            Err(LedgerBreach::Energy {
                energy_in: self.energy_in,
                energy_out: self.energy_out,
                energy_stored: self.energy_stored,
            })
        }
    }

    /// Check I4 against an independent recomputation of the world's contents.
    ///
    /// # Errors
    ///
    /// Returns the first chemical whose claimed and actual totals differ. Not "within
    /// epsilon" — the difference must be zero.
    pub fn check_matter(&self, actual: &[i64; CHEM_COUNT]) -> Result<(), LedgerBreach> {
        for (c, claimed) in self.chem.iter().enumerate() {
            if *claimed != actual[c] {
                return Err(LedgerBreach::Matter {
                    chemical: c,
                    claimed: *claimed,
                    actual: actual[c],
                });
            }
        }
        Ok(())
    }
}

impl StateHash for Ledger {
    fn hash_state(&self, h: &mut StateHasher) {
        for v in self.chem {
            h.u64(v as u64);
        }
        for v in self.evicted {
            h.u64(v as u64);
        }
        h.u64(self.energy_in as u64);
        h.u64(self.energy_out as u64);
        h.u64(self.energy_stored as u64);
        h.u64(self.converted as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_ledger_balances() {
        assert!(Ledger::new().check_energy().is_ok());
        assert!(Ledger::new().check_matter(&[0; CHEM_COUNT]).is_ok());
    }

    #[test]
    fn absorbing_and_dissipating_keeps_the_identity() {
        let mut l = Ledger::new();
        for i in 1..2000i64 {
            l.absorb(i * 7);
            l.dissipate(i * 3);
            l.check_energy().unwrap();
        }
        assert!(l.energy_in() > 0);
        assert!(l.energy_out() > 0);
        assert!(l.energy_stored() > 0);
    }

    #[test]
    fn energy_cannot_be_dissipated_that_was_never_absorbed() {
        // Otherwise energy_out would run ahead of energy_in and the identity would break by
        // exactly the amount the world invented.
        let mut l = Ledger::new();
        l.absorb(100);
        assert_eq!(
            l.dissipate(250),
            100,
            "dissipation is clamped to what is held"
        );
        assert_eq!(l.energy_stored(), 0);
        l.check_energy().unwrap();
        assert_eq!(l.dissipate(1), 0);
        l.check_energy().unwrap();
    }

    #[test]
    fn absorbing_a_negative_amount_does_nothing() {
        let mut l = Ledger::new();
        l.absorb(-500);
        assert_eq!(l.energy_in(), 0);
        assert_eq!(l.dissipate(-500), 0);
        l.check_energy().unwrap();
    }

    #[test]
    fn eviction_is_recorded_rather_than_hidden() {
        let mut l = Ledger::new();
        l.set_baseline([1000; CHEM_COUNT]);
        let mut evicted = [0i32; CHEM_COUNT];
        evicted[3] = 250;
        l.record_evicted(&evicted);

        assert_eq!(l.chem_totals()[3], 750);
        assert_eq!(l.evicted()[3], 250);
        // The world now holds 750, and the ledger agrees — the loss is accounted, not lost.
        let mut actual = [1000i64; CHEM_COUNT];
        actual[3] = 750;
        l.check_matter(&actual).unwrap();
    }

    #[test]
    fn a_matter_breach_names_the_chemical_and_the_drift() {
        let mut l = Ledger::new();
        l.set_baseline([500; CHEM_COUNT]);
        let mut actual = [500i64; CHEM_COUNT];
        actual[9] = 499;
        let breach = l.check_matter(&actual).unwrap_err();
        assert_eq!(
            breach,
            LedgerBreach::Matter {
                chemical: 9,
                claimed: 500,
                actual: 499
            }
        );
        assert!(breach.to_string().contains("drift -1"));
    }

    #[test]
    fn totals_survive_magnitudes_that_would_overflow_an_i32() {
        let mut l = Ledger::new();
        // 4M squares each holding a billion: 4e15, three orders past i32.
        l.set_baseline([4_000_000i64 * 1_000_000_000; CHEM_COUNT]);
        assert_eq!(l.chem_totals()[0], 4_000_000_000_000_000);
        l.check_matter(&[4_000_000_000_000_000; CHEM_COUNT])
            .unwrap();
    }
}
