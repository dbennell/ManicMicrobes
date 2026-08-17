//! Who is still here, by descent (M8, SPEC §10.1 and §13).
//!
//! # Why a second census, when §13 already specifies one
//!
//! [`crate::ecology::TrophicMix`] is the **guild census** SPEC §13 asks for, and it is right: it
//! reads what cells are *built out of*, because there is no cell-type enum and there must not be
//! one. A cell with chloroplasts is a producer whatever it descends from.
//!
//! That makes it the wrong instrument for a question the ecology tests were asking it anyway —
//! *did the thing I seeded survive?* The two questions come apart in both directions, and both
//! were observed on `predator_introduction.ron` with `predator.mm`, `scavenger.mm` and
//! `ancestor.mm` seeded together:
//!
//! - **A guild is not a lineage.** At tick 5,000 the predator cohort held 110 cells and 104 of
//!   them carried a lysosome, so 104 predators were counted in the scavenger column. The mix read
//!   436 scavengers where the scavenger *lineage* held 376.
//! - **A guild survives a lineage's death.** By tick 15,000 the predator cohort was down to four
//!   cells and still falling; the predator column read four and satisfied `predators > 0`, which
//!   is what the trophic-structure acceptance test asserts. A two-cell remnant out of 3,600
//!   passes a test whose milestone text says "a stable predator–prey oscillation persists".
//!
//! Neither is a defect in the mix. They are the mix being asked to carry a claim about descent,
//! which it cannot: the analysis layer infers guilds *precisely so that* it does not need to know
//! who came from whom. The record of who came from whom is kept somewhere else, and it is exact.
//!
//! # The tree is free, so use it
//!
//! SPEC §10.1: *"Real biologists infer phylogeny from extant sequences because they have no
//! record of who descended from whom. We have a perfect record."* Every cell carries a species
//! id, every species carries its parent, and a seeded genome founds a root. So the founding
//! cohort of any living cell is the root of its species chain, and nothing has to be inferred.
//!
//! [`crate::arena`] has done this since M6 — `side_of` attributes a cell to one of two
//! competitors by exactly this walk — and its doc comment already argues the case against the
//! obvious alternative: **do not identify a lineage by the genome it is running.** `COPYB`
//! charges per byte and a cell that cannot pay skips the byte, so genomes drift even with
//! mutation switched off, and the first version of that test found 68 cells running two genomes
//! neither competitor had entered. Descent survives that; bytes do not.
//!
//! What is here is that walk generalised past two sides, given a guild census *per lineage*, and
//! made cheap enough to run on a full slide — `side_of` allocates a `Vec` per cell per sample,
//! which is fine for two cohorts of twelve and not for fifty thousand cells.
//!
//! # Derived, never stored
//!
//! Nothing in this module is world state. A [`Census`] is a pure function of the cell arena, the
//! archive and the cohort list, so hard rule 7 has no new surface to cover — the species ids it
//! reads round-trip already, because [`crate::phylogeny`] serialises them. A [`CensusLog`] is
//! held by whoever is running the simulation, for the same reason `World::prune_archive` is not
//! on a timer inside `step`: how much history is worth keeping is the caller's decision.

use std::collections::BTreeMap;

use crate::cell::CellArena;
use crate::ecology::TrophicMix;
use crate::organelle::OrganelleType;
use crate::phylogeny::{Phylogeny, SpeciesId};

/// One part in a thousand, matching [`crate::balance`] and [`TrophicMix::is_monoculture`].
pub const PERMILLE: u32 = 1000;

/// The share of its own peak a lineage must still hold at the end to count as having held.
///
/// **A quarter, and the argument is about oscillation.** A predator–prey system that works
/// *should* halve and recover — that is what acceptance 2 is asking to see — so a floor set
/// anywhere above a halving would call the healthy result a collapse and there would be no
/// setting of it that admitted success. A quarter is two consecutive halvings, which is outside
/// anything the tree has been observed to do and recover from.
///
/// It is deliberately generous to the lineage, because the case it exists to catch is not
/// marginal. `predator.mm` on `predator_introduction.ron` peaks at 110 cells around tick 5,000
/// and ends at 2 — eighteen parts in a thousand of its own peak, five doublings below this
/// floor. A test that cannot separate that from a working oscillation is not measuring anything,
/// and a floor that needed tuning to separate them would be measuring the floor.
pub const HELD_FLOOR_PERMILLE: u32 = 250;

/// The population below which shares are not worth reporting.
///
/// Borrowed from [`TrophicMix::is_monoculture`] and for its reason: three surviving cells are not
/// a monoculture and not a community either, and a permille of a handful is noise with a decimal
/// point on it.
pub const MIN_POPULATION: u32 = 32;

/// A founding cohort — everything descended from one seeding.
///
/// `root` is read off the world at seeding time rather than assumed, because the archive assigns
/// ids in seeding order and a caller that hard-coded them would be depending on an implementation
/// detail. [`crate::World::place_community`] returns these.
///
/// # Two cohorts of the same genome
///
/// [`Phylogeny::found`] merges by fingerprint: twelve founders of one ancestor are one species,
/// which is what anybody reading the tree expects and what a scenario wants. It also means two
/// *arms* of the same genome resolve to one root and cannot be told apart here. That is what
/// [`Phylogeny::found_distinct`] and [`crate::World::spawn_cell_as_new_species`] are for, and it
/// is why the arena uses them — the first version of that test ended every same-genome match
/// 12–0 at tick one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cohort {
    /// What to call it in a report. Conventionally the genome's file name.
    pub label: String,
    /// The root species every member of this cohort chains to.
    pub root: SpeciesId,
    /// How many founders actually landed — a request is not a placement (see
    /// [`crate::World::place_inhabitants`]).
    pub founded: u32,
}

impl Cohort {
    #[must_use]
    pub fn new(label: impl Into<String>, root: SpeciesId, founded: u32) -> Cohort {
        Cohort {
            label: label.into(),
            root,
            founded,
        }
    }
}

/// The root of a species' ancestry chain, without allocating.
///
/// [`Phylogeny::ancestry`] builds a `Vec` to answer the same question, which is right for the
/// wiki showing a chain and wrong for a per-cell attribution over a full slide. Bounded by the
/// archive's size for the reason `ancestry` is: a species' parent always has a lower id, so a
/// cycle should be impossible, and a corrupt archive must not hang the caller anyway.
#[must_use]
pub fn root_of(archive: &Phylogeny, species: SpeciesId) -> SpeciesId {
    let mut at = species;
    let mut guard = archive.len().saturating_add(1);
    while guard > 0 {
        guard -= 1;
        match archive.get(at).and_then(|s| s.parent) {
            Some(parent) => at = parent,
            None => break,
        }
    }
    at
}

/// What one cohort holds at one moment.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CohortReading {
    pub label: String,
    pub root: SpeciesId,
    pub founded: u32,
    /// Living cells descended from this cohort's founders.
    pub cells: u32,
    /// How many distinct living species the cohort has been resolved into.
    ///
    /// One means it has not speciated. Worth reporting beside the population because a lineage
    /// that has thrown off six species and a lineage that is still its founding one are
    /// different results at the same head count.
    pub species: u32,
    /// The guild census of SPEC §13, taken *within* this lineage.
    ///
    /// This is the reading the food-web tests wanted and could not express: not "how many cells
    /// on the slide carry a spike" but "how many of the predator lineage still do".
    pub mix: TrophicMix,
}

impl CohortReading {
    /// This cohort's share of the whole population, in permille.
    #[must_use]
    pub fn share(&self, total: u32) -> u32 {
        if total == 0 {
            return 0;
        }
        (u64::from(self.cells) * u64::from(PERMILLE) / u64::from(total)) as u32
    }
}

/// One reading of the whole slide, by descent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Census {
    pub tick: u64,
    /// Every living cell, attributed or not.
    pub total: u32,
    /// One entry per cohort, in the order the cohorts were given.
    pub cohorts: Vec<CohortReading>,
    /// Living cells whose root is not any cohort's root.
    ///
    /// **Expected to be zero, and a bug in the caller when it is not.** Every cell chains to a
    /// root and every root is founded by a seeding, so a nonzero count means the cohort list is
    /// missing a seeding — a test that hand-spawned a cell after building the list, or a scenario
    /// whose `inhabitants` were placed by one path and censused against another. It is reported
    /// rather than folded into the total so that the mistake is visible instead of quietly
    /// deflating every share.
    pub unattributed: u32,
}

impl Census {
    /// Read the slide, attributing every cell to the cohort it descends from.
    ///
    /// One pass over the cells. Species are resolved to roots through a memo, because a
    /// population of fifty thousand typically holds a few hundred species and walking each cell's
    /// chain independently would repeat the same walk thousands of times.
    #[must_use]
    pub fn take(tick: u64, cells: &CellArena, archive: &Phylogeny, cohorts: &[Cohort]) -> Census {
        // root species -> index into `cohorts`. Built from the cohort list rather than searched,
        // so two cohorts sharing a root (which `found` can produce — see `Cohort`) resolve to the
        // first consistently rather than by iteration order.
        let mut by_root: BTreeMap<SpeciesId, usize> = BTreeMap::new();
        for (k, c) in cohorts.iter().enumerate() {
            by_root.entry(c.root).or_insert(k);
        }

        let mut readings: Vec<CohortReading> = cohorts
            .iter()
            .map(|c| CohortReading {
                label: c.label.clone(),
                root: c.root,
                founded: c.founded,
                cells: 0,
                species: 0,
                mix: TrophicMix::default(),
            })
            .collect();

        // species -> resolved cohort index, memoised across cells.
        let mut memo: BTreeMap<SpeciesId, Option<usize>> = BTreeMap::new();
        // Which species each cohort has been seen holding, for the `species` count.
        let mut seen: BTreeMap<(usize, SpeciesId), ()> = BTreeMap::new();

        let mut total = 0u32;
        let mut unattributed = 0u32;
        for i in cells.iter() {
            total = total.saturating_add(1);
            let species = cells.species[i];
            let which = *memo
                .entry(species)
                .or_insert_with(|| by_root.get(&root_of(archive, species)).copied());
            let Some(k) = which else {
                unattributed = unattributed.saturating_add(1);
                continue;
            };
            let Some(r) = readings.get_mut(k) else {
                unattributed = unattributed.saturating_add(1);
                continue;
            };
            r.cells = r.cells.saturating_add(1);
            seen.insert((k, species), ());

            // The guild census of SPEC §13, restricted to this lineage. A cell may fall in more
            // than one column, because a mixotroph is a real thing.
            let mut producer = false;
            let mut predator = false;
            let mut scavenger = false;
            for o in cells.slots(i) {
                if !o.is_active() {
                    continue;
                }
                match o.kind {
                    OrganelleType::Chloroplast => producer = true,
                    OrganelleType::Spike => predator = true,
                    OrganelleType::Lysosome => scavenger = true,
                    _ => {}
                }
            }
            r.mix.producers += u32::from(producer);
            r.mix.predators += u32::from(predator);
            r.mix.scavengers += u32::from(scavenger);
            r.mix.osmotrophs += u32::from(!producer && !predator && !scavenger);
            r.mix.total += 1;
        }

        for ((k, _), ()) in &seen {
            if let Some(r) = readings.get_mut(*k) {
                r.species = r.species.saturating_add(1);
            }
        }

        Census {
            tick,
            total,
            cohorts: readings,
            unattributed,
        }
    }

    /// A cohort's reading by label.
    #[must_use]
    pub fn cohort(&self, label: &str) -> Option<&CohortReading> {
        self.cohorts.iter().find(|c| c.label == label)
    }

    /// Whether one *lineage* holds essentially the whole slide.
    ///
    /// M8's fourth acceptance test — "no scenario in the library collapses to a single strategy"
    /// — read this off [`TrophicMix::is_monoculture`], which cannot answer it. The founder kit
    /// [`crate::World::place_inhabitants`] hands out includes a finished chloroplast, so every
    /// seeded cell is a producer from tick zero: the producers column stands at 955–999 permille
    /// across fifteen of the eighteen library scenarios at 10,000 ticks, and the test would
    /// report the whole library as collapsed while measuring nothing but the kit.
    ///
    /// Lineage shares discriminate where guild columns do not. On the same food-web slide the
    /// ancestor cohort holds 877 permille at tick 40,000 — dominant, not degenerate — and the
    /// number moves when the ecology moves.
    #[must_use]
    pub fn is_monoculture(&self, threshold_permille: u32) -> bool {
        if self.total < MIN_POPULATION {
            return false;
        }
        self.cohorts
            .iter()
            .any(|c| c.share(self.total) >= threshold_permille)
    }

    /// A one-line-per-cohort table, in the house style of [`crate::balance`].
    #[must_use]
    pub fn report(&self) -> String {
        let mut out = format!("tick {}: {} cells", self.tick, self.total);
        if self.unattributed > 0 {
            out.push_str(&format!(
                " ({} UNATTRIBUTED — the cohort list is missing a seeding)",
                self.unattributed
            ));
        }
        out.push('\n');
        for c in &self.cohorts {
            out.push_str(&format!(
                "  {:<18} {:>7} cells {:>4}‰ of the slide, {:>3} species, from {:>3} founders \
                 (P{} Pr{} S{} O{})\n",
                c.label,
                c.cells,
                c.share(self.total),
                c.species,
                c.founded,
                c.mix.producers,
                c.mix.predators,
                c.mix.scavengers,
                c.mix.osmotrophs
            ));
        }
        out
    }
}

/// What became of a lineage over a run.
///
/// The verdict the ecology tests could not express. `> 0` cannot tell a thriving lineage from a
/// dying one, and every claim worth asserting about an ecosystem is about a trend.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fate {
    /// Never placed, or never sampled. Not a result — a setup mistake.
    NeverLived,
    /// Reached zero, at this tick.
    Extinct { at: u64, peak: u32 },
    /// Alive, but below [`HELD_FLOOR_PERMILLE`] of its own peak. A lineage on paper.
    Collapsed {
        peak: u32,
        peak_tick: u64,
        ended: u32,
    },
    /// Alive and still holding a quarter or more of its peak.
    Held { peak: u32, peak_tick: u64, ended: u32 },
}

impl Fate {
    /// Whether the lineage is still a going concern.
    ///
    /// `Collapsed` is deliberately *not* a survival: that is the whole distinction this type
    /// exists to draw, and folding it back in would restore the `> 0` test with more ceremony.
    #[must_use]
    pub fn held(&self) -> bool {
        matches!(self, Fate::Held { .. })
    }

    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Fate::NeverLived => "never lived — it was not placed, or never sampled".to_string(),
            Fate::Extinct { at, peak } => {
                format!("extinct at tick {at}, after peaking at {peak}")
            }
            Fate::Collapsed {
                peak,
                peak_tick,
                ended,
            } => format!(
                "collapsed: peaked at {peak} on tick {peak_tick} and ended at {ended}, which is \
                 {}‰ of its peak",
                if *peak == 0 {
                    0
                } else {
                    (u64::from(*ended) * u64::from(PERMILLE) / u64::from(*peak)) as u32
                }
            ),
            Fate::Held {
                peak,
                peak_tick,
                ended,
            } => format!("held: peaked at {peak} on tick {peak_tick} and ended at {ended}"),
        }
    }
}

/// A run's worth of censuses, and the verdicts that follow from them.
///
/// Driven by the caller, not by `World::step` — see the module header.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct CensusLog {
    samples: Vec<Census>,
}

impl CensusLog {
    #[must_use]
    pub fn new() -> CensusLog {
        CensusLog {
            samples: Vec::new(),
        }
    }

    pub fn push(&mut self, census: Census) {
        self.samples.push(census);
    }

    /// Take a reading and keep it. The common case.
    ///
    /// Returns nothing rather than a reference to what was just pushed: the obvious `last()`
    /// after a `push()` needs an `unwrap` to discharge an `Option` that cannot be `None`, and
    /// hard rule 3 forbids the construct throughout `mm-core` rather than case by case. A caller
    /// that wants the reading back asks for [`CensusLog::last`].
    pub fn sample(
        &mut self,
        tick: u64,
        cells: &CellArena,
        archive: &Phylogeny,
        cohorts: &[Cohort],
    ) {
        self.samples
            .push(Census::take(tick, cells, archive, cohorts));
    }

    #[must_use]
    pub fn samples(&self) -> &[Census] {
        &self.samples
    }

    #[must_use]
    pub fn last(&self) -> Option<&Census> {
        self.samples.last()
    }

    /// The population series for one cohort, by label.
    #[must_use]
    pub fn series(&self, label: &str) -> Vec<(u64, u32)> {
        self.samples
            .iter()
            .filter_map(|s| s.cohort(label).map(|c| (s.tick, c.cells)))
            .collect()
    }

    /// What became of one cohort.
    ///
    /// Extinction is reported at the **first** sample holding zero, not the last, so a lineage
    /// that died at tick 15,000 is not recorded as dying at the end of a run that carried on for
    /// another 85,000 ticks. A cohort that comes back from zero is therefore reported as extinct,
    /// which is correct: nothing in this engine resurrects, so a zero followed by a nonzero can
    /// only mean the sampling interval was coarser than the lineage's own lifetime, and that is
    /// worth failing over rather than smoothing away.
    #[must_use]
    pub fn fate(&self, label: &str) -> Fate {
        let series = self.series(label);
        if series.is_empty() {
            return Fate::NeverLived;
        }
        let mut peak = 0u32;
        let mut peak_tick = 0u64;
        for (tick, n) in &series {
            if *n > peak {
                peak = *n;
                peak_tick = *tick;
            }
        }
        if let Some((at, _)) = series.iter().find(|(_, n)| *n == 0) {
            return Fate::Extinct { at: *at, peak };
        }
        // Non-empty and no zero, so the last entry exists and is positive.
        let ended = series.last().map_or(0, |(_, n)| *n);
        let floor = u64::from(peak) * u64::from(HELD_FLOOR_PERMILLE) / u64::from(PERMILLE);
        if u64::from(ended) < floor {
            Fate::Collapsed {
                peak,
                peak_tick,
                ended,
            }
        } else {
            Fate::Held {
                peak,
                peak_tick,
                ended,
            }
        }
    }

    /// Every cohort's fate, in the order of the last census.
    #[must_use]
    pub fn fates(&self) -> Vec<(String, Fate)> {
        self.last()
            .map(|c| {
                c.cohorts
                    .iter()
                    .map(|r| (r.label.clone(), self.fate(&r.label)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The whole run as a table, for a test that has just failed to print.
    #[must_use]
    pub fn report(&self) -> String {
        let mut out = String::new();
        for s in &self.samples {
            out.push_str(&s.report());
        }
        out.push_str("fates:\n");
        for (label, fate) in self.fates() {
            out.push_str(&format!("  {label:<18} {}\n", fate.describe()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genome::Genome;
    use crate::names::Traits;
    use std::sync::Arc;

    fn traits() -> Traits {
        Traits::default()
    }

    fn genome(bytes: Vec<u8>) -> Arc<Genome> {
        Arc::new(Genome::new(bytes).expect("legal genome"))
    }

    #[test]
    fn a_root_is_its_own_root() {
        let mut p = Phylogeny::new();
        let root = p.found(&genome(vec![1, 2, 3, 4]), traits(), 0);
        assert_eq!(root_of(&p, root), root);
    }

    #[test]
    fn a_descendant_resolves_to_its_founder_however_deep() {
        let mut p = Phylogeny::new();
        // Every drift forks, so the chain is built by the real path rather than by hand: with a
        // threshold of zero, `on_birth` returns the parent species only for a fingerprint it
        // matches exactly, and forks for anything else.
        p.speciation_threshold = 0;
        let root = p.found(&genome(vec![1, 2, 3, 4, 5, 6, 7, 8]), traits(), 0);
        let mut at = root;
        let mut depth = 0;
        for k in 1..=8u8 {
            let bytes = vec![k, k ^ 0x5A, k.wrapping_mul(37), k ^ 0xF0, 9, 8, 7, k];
            let child = p.on_birth(at, &genome(bytes), traits(), u64::from(k));
            if child != at {
                depth += 1;
                at = child;
            }
        }
        assert!(depth >= 4, "expected a chain, got {depth} forks");
        assert_ne!(at, root, "the chain should have moved off its root");
        assert_eq!(root_of(&p, at), root);
    }

    #[test]
    fn an_unknown_species_is_its_own_root() {
        // A pruned or never-registered id must not hang or panic the walk.
        let p = Phylogeny::new();
        assert_eq!(root_of(&p, 9999), 9999);
    }

    #[test]
    fn the_floor_admits_a_halving_and_refuses_two() {
        let mut log = CensusLog::new();
        let cohorts = vec![Cohort::new("a", 0, 4)];
        // Peak 100, ended 50 — one halving, which a working oscillation does.
        for (tick, n) in [(0u64, 10u32), (1, 100), (2, 50)] {
            log.push(synthetic(tick, &cohorts, &[n]));
        }
        assert!(
            log.fate("a").held(),
            "a halving must not read as a collapse: {}",
            log.fate("a").describe()
        );

        // Peak 100, ended 20 — below a quarter.
        let mut log = CensusLog::new();
        for (tick, n) in [(0u64, 10u32), (1, 100), (2, 20)] {
            log.push(synthetic(tick, &cohorts, &[n]));
        }
        assert!(
            matches!(log.fate("a"), Fate::Collapsed { .. }),
            "two halvings must read as a collapse: {}",
            log.fate("a").describe()
        );
    }

    #[test]
    fn extinction_is_dated_at_the_first_zero_not_the_last() {
        let mut log = CensusLog::new();
        let cohorts = vec![Cohort::new("a", 0, 4)];
        for (tick, n) in [(0u64, 10u32), (100, 40), (200, 0), (300, 0)] {
            log.push(synthetic(tick, &cohorts, &[n]));
        }
        match log.fate("a") {
            Fate::Extinct { at, peak } => {
                assert_eq!(at, 200, "dated at the first zero");
                assert_eq!(peak, 40);
            }
            other => panic!("expected extinction, got {}", other.describe()),
        }
    }

    #[test]
    fn a_cohort_that_was_never_placed_says_so() {
        let log = CensusLog::new();
        assert_eq!(log.fate("nobody"), Fate::NeverLived);
    }

    #[test]
    fn a_share_is_of_the_whole_slide_including_the_unattributed() {
        let cohorts = vec![Cohort::new("a", 0, 4), Cohort::new("b", 1, 4)];
        let mut c = synthetic(0, &cohorts, &[250, 250]);
        c.total = 1000;
        c.unattributed = 500;
        // 250 of 1000, not 250 of the 500 that were attributed.
        assert_eq!(c.cohorts[0].share(c.total), 250);
    }

    #[test]
    fn a_dominant_lineage_is_not_a_monoculture_until_it_crosses_the_line() {
        let cohorts = vec![Cohort::new("a", 0, 4), Cohort::new("b", 1, 4)];
        let mut c = synthetic(0, &cohorts, &[877, 123]);
        c.total = 1000;
        assert!(
            !c.is_monoculture(950),
            "877‰ is dominance, not collapse — this is the food-web ancestor at tick 40,000"
        );
        let mut c = synthetic(0, &cohorts, &[960, 40]);
        c.total = 1000;
        assert!(c.is_monoculture(950));
    }

    #[test]
    fn a_handful_of_survivors_is_never_a_monoculture() {
        let cohorts = vec![Cohort::new("a", 0, 4)];
        let mut c = synthetic(0, &cohorts, &[8]);
        c.total = 8;
        assert!(!c.is_monoculture(950), "eight cells is not a result");
    }

    /// A census with the populations written in by hand, for testing the verdicts rather than
    /// the attribution. The attribution is tested against a real world in
    /// `tests/lineage_census.rs`, where there are actual cells to attribute.
    fn synthetic(tick: u64, cohorts: &[Cohort], counts: &[u32]) -> Census {
        let mut total = 0;
        let readings = cohorts
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let n = counts.get(k).copied().unwrap_or(0);
                total += n;
                CohortReading {
                    label: c.label.clone(),
                    root: c.root,
                    founded: c.founded,
                    cells: n,
                    species: u32::from(n > 0),
                    mix: TrophicMix::default(),
                }
            })
            .collect();
        Census {
            tick,
            total,
            cohorts: readings,
            unattributed: 0,
        }
    }
}
