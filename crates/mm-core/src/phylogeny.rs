//! Species, the tree, and what is remembered about a lineage (SPEC §10).
//!
//! # The tree is free, and it is a *species* tree
//!
//! SPEC §10.1 says the true tree needs no inference because every cell records its parent.
//! That is true of *living* cells, and it is all the arena keeps. §10.3 is equally explicit
//! that per-individual birth records must never be stored — at two hundred thousand cells with
//! fast turnover that is millions of events a minute, and the archive would be a storage leak
//! wearing a phylogeny costume.
//!
//! Both hold at once, and the resolution is that there are two trees:
//!
//! * The **individual** tree exists only among the living. `CellArena::parent` holds it, and a
//!   parent that has died and had its slot reused is simply gone. Nothing retains it.
//! * The **species** tree is the one that persists. It is small — thousands of nodes over
//!   millions of ticks, not millions — and it is what the wiki, the timeline and every
//!   acceptance test in M5 are about.
//!
//! So M5's "every cell's ancestry chain terminates at a founder" is read as: every living cell
//! belongs to a species, every species chains to a founding species, and no chain loops. That
//! is checkable, it is what the data supports, and the alternative reading would require the
//! exact storage the design rules forbid.
//!
//! # What a species costs
//!
//! One founder genome, one fingerprint, a name, some counters, and a bounded population curve.
//! The curve is the only thing that grows with time, and it decimates rather than growing:
//! when it fills, every other point is dropped and the sampling interval doubles. A species
//! alive for ten million ticks holds the same number of points as one alive for ten thousand,
//! at coarser resolution — which is exactly how anybody would want to read it, and is what
//! makes M5's storage bound reachable rather than a matter of hoping.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::genome::{fingerprint_distance, Genome};
use crate::names::{name, Binomial, Traits};

/// A species' index in the archive.
///
/// `u32` and never reused: an id that came back would make the tree ambiguous, and four
/// billion species is beyond any run that could be stored.
pub type SpeciesId = u32;

/// The species every seeded cell starts in.
pub const FOUNDER_SPECIES: SpeciesId = 0;

/// Population-curve points kept per species before decimation.
///
/// 64 points is enough to draw a legible curve at any width the wiki will show it, and small
/// enough that ten thousand species cost under ten megabytes of curve between them.
pub const CURVE_POINTS: usize = 64;

/// How different a newborn must be from its species founder to found a new species, in bits
/// of fingerprint Hamming distance (`0..=64`).
///
/// Unrelated genomes sit about 32 bits apart and a single byte moves about 3.6, so 12 is
/// roughly "a few dozen accumulated mutations" — far enough that the lineage has really
/// changed, near enough that it happens within a run. Below about 6 the noise floor of the
/// fingerprint starts founding species on nothing.
pub const DEFAULT_SPECIATION_THRESHOLD: u32 = 12;

/// Fingerprint distance at which two species are put in different genera, for display.
pub const DEFAULT_GENUS_THRESHOLD: u32 = 20;

/// Why a species stopped.
///
/// Inferred, and labelled as inferred: the simulation does not record causes, it records
/// numbers, and a cause is a reading of them. SPEC §10.5 asks for "an inferred cause" and this
/// is the honest amount of confidence to have.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Extinction {
    /// A descendant species replaced it — the lineage did not end, it was renamed.
    SucceededByDescendant(SpeciesId),
    /// Another species grew as this one shrank. Correlation, and named as such.
    Outcompeted(SpeciesId),
    /// It shrank while the whole slide shrank.
    MassExtinction,
    /// It never got going: died small, without ever reaching a population worth the name.
    NeverEstablished,
    /// None of the above fit.
    Unknown,
}

impl Extinction {
    /// A phrase for the wiki, in the register of SPEC §10.5's example prose.
    #[must_use]
    pub fn describe(&self, archive: &Phylogeny) -> String {
        let named = |id: SpeciesId| {
            archive
                .get(id)
                .map_or_else(|| format!("species {id}"), |s| s.name.abbreviated())
        };
        match self {
            Extinction::SucceededByDescendant(id) => {
                format!("succeeded by its own descendant {}", named(*id))
            }
            Extinction::Outcompeted(id) => format!("outcompeted by {}", named(*id)),
            Extinction::MassExtinction => "lost in a mass extinction".to_string(),
            Extinction::NeverEstablished => "never established".to_string(),
            Extinction::Unknown => "cause unclear".to_string(),
        }
    }
}

/// One point on a population curve.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CurvePoint {
    pub tick: u64,
    pub population: u32,
}

/// A bounded population history that coarsens instead of growing.
///
/// The alternative — one point per sample forever — is the exact per-individual-record
/// mistake in a different costume, and it is what M5's storage bound is written to catch.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Curve {
    points: Vec<CurvePoint>,
    /// Ticks between kept points. Doubles on every decimation.
    interval: u64,
    next_at: u64,
}

impl Curve {
    #[must_use]
    pub fn new(interval: u64) -> Curve {
        Curve {
            points: Vec::new(),
            interval: interval.max(1),
            next_at: 0,
        }
    }

    /// Offer a sample. Kept only if one is due.
    pub fn sample(&mut self, tick: u64, population: u32) {
        if tick < self.next_at && !self.points.is_empty() {
            return;
        }
        self.points.push(CurvePoint { tick, population });
        self.next_at = tick.saturating_add(self.interval);
        if self.points.len() > CURVE_POINTS {
            self.decimate();
        }
    }

    /// Halve the resolution: keep every other point, double the interval.
    ///
    /// The last point is kept whatever its parity, so the most recent state of a species is
    /// never the thing that gets thrown away.
    fn decimate(&mut self) {
        let last = self.points.last().copied();
        let mut kept: Vec<CurvePoint> = self.points.iter().step_by(2).copied().collect();
        if let Some(last) = last {
            if kept.last().map(|p| p.tick) != Some(last.tick) {
                kept.push(last);
            }
        }
        self.points = kept;
        self.interval = self.interval.saturating_mul(2);
        // Re-derive when the next sample is due, at the *new* interval.
        //
        // Without this, `next_at` keeps the value `sample` computed a moment ago from the old
        // interval, and two facts that ought to agree stop agreeing: a restored curve derives
        // `next_at` from the interval it was saved with and lands somewhere else. That is what
        // broke the snapshot round-trip, and it is also just wrong — having decided to sample
        // half as often, the next sample should be twice as far away.
        if let Some(last) = self.points.last() {
            self.next_at = last.tick.saturating_add(self.interval);
        }
    }

    #[must_use]
    pub fn points(&self) -> &[CurvePoint] {
        &self.points
    }

    #[must_use]
    pub fn interval(&self) -> u64 {
        self.interval
    }

    /// Rebuild from a snapshot. Hard rule 7.
    ///
    /// `next_at` is derived rather than stored: it is always the last kept point plus the
    /// interval, so writing it would be storing a fact the other two already determine — and
    /// a stored copy is a copy that can disagree.
    pub fn restore(&mut self, points: Vec<CurvePoint>, interval: u64) {
        self.interval = interval.max(1);
        self.next_at = points
            .last()
            .map_or(0, |p| p.tick.saturating_add(self.interval));
        self.points = points;
    }
}

/// Everything retained about one species.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Species {
    pub id: SpeciesId,
    /// The species this one diverged from. `None` only for the seeded founder.
    pub parent: Option<SpeciesId>,
    pub name: Binomial,
    /// Display grouping, by fingerprint distance between founders.
    pub genus: SpeciesId,
    pub founded_tick: u64,
    pub founder_fingerprint: u64,
    /// The founder's genome. Full genomes are kept for founders only (SPEC §10.3).
    pub founder_genome: Arc<Genome>,
    /// What the founder was built out of, for the name and the wiki's description.
    pub traits: Traits,

    pub population: u32,
    pub peak_population: u32,
    pub peak_tick: u64,
    pub births: u64,
    pub deaths: u64,
    /// How many speciation events lie between this species and the seeded founder.
    pub depth: u32,
    pub extinct_tick: Option<u64>,
    pub extinction: Option<Extinction>,
    pub curve: Curve,
    /// Species that diverged directly from this one. Held so that a newly drifted cell can be
    /// offered its cousins before it is given a species of its own.
    pub children: Vec<SpeciesId>,
    /// Whether `traits` has been read off a member that finished building itself.
    pub traits_settled: bool,
}

impl Species {
    #[must_use]
    pub fn is_extinct(&self) -> bool {
        self.extinct_tick.is_some()
    }

    /// The wiki's one-paragraph description (SPEC §10.5).
    ///
    /// Built from the founder's organelle loadout and the species' own numbers, so it says
    /// only things that are true of the thing it describes.
    #[must_use]
    pub fn describe(&self, archive: &Phylogeny) -> String {
        use crate::organelle::OrganelleType;
        let mut parts: Vec<String> = Vec::new();

        let count = |k: OrganelleType| self.traits.counts.get(k as usize).copied().unwrap_or(0);
        let (chloro, mito) = (
            count(OrganelleType::Chloroplast),
            count(OrganelleType::Mitochondrion),
        );
        parts.push(
            match (chloro > 0, mito > 0) {
                (true, true) => "photo-autotroph with its own respiration",
                (true, false) => "obligate phototroph",
                (false, true) => "chemotroph",
                (false, false) => "neither photosynthesising nor respiring",
            }
            .to_string(),
        );

        let cilia = count(OrganelleType::Cilium);
        if cilia > 0 {
            parts.push(format!(
                "{cilia} cili{}",
                if cilia == 1 { "um" } else { "a" }
            ));
        }
        let sensors = count(OrganelleType::Chemosensor);
        if sensors > 0 {
            parts.push(format!("{sensors} chemosensor{}", plural(sensors)));
        }
        if count(OrganelleType::Photosensor) > 0 {
            parts.push("light-sensing".to_string());
        }
        if count(OrganelleType::Vacuole) > 0 {
            parts.push("enlarged storage".to_string());
        }
        parts.push(format!("{}-byte genome", self.traits.genome_len));

        let mut out = format!("{} — {}.", self.name.full(), parts.join(", "));
        if let Some(parent) = self.parent {
            let parent_name = archive
                .get(parent)
                .map_or_else(|| format!("species {parent}"), |s| s.name.abbreviated());
            out.push_str(&format!(
                " Diverged from {parent_name} at tick {}.",
                self.founded_tick
            ));
        } else {
            out.push_str(&format!(" Seeded at tick {}.", self.founded_tick));
        }
        if self.peak_population > 0 {
            out.push_str(&format!(
                " Peak population {} at tick {}.",
                self.peak_population, self.peak_tick
            ));
        }
        if let (Some(tick), Some(cause)) = (self.extinct_tick, self.extinction) {
            out.push_str(&format!(" Extinct at {tick}, {}.", cause.describe(archive)));
        }
        out
    }
}

fn plural(n: u8) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// The species archive and the tree over it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Phylogeny {
    species: BTreeMap<SpeciesId, Species>,
    next_id: SpeciesId,
    pub speciation_threshold: u32,
    pub genus_threshold: u32,
    /// How often a population sample is offered to each curve.
    pub sample_interval: u64,
    /// Species pruned so far, for the archive's own accounting.
    pruned: u64,
    /// Speciation events, whether or not the species survived.
    forks: u64,
    /// Every name in use, so checking one is a lookup rather than a walk.
    ///
    /// Derived from `species` and rebuilt on restore, like `Species::children`. Held because
    /// the alternative is rebuilding a set of every name on every speciation — O(n) string
    /// formats per fork, which is quadratic in a run that produces a lot of species, and
    /// speciation happens on the birth path.
    taken_names: std::collections::BTreeSet<String>,
}

impl Default for Phylogeny {
    fn default() -> Phylogeny {
        Phylogeny::new()
    }
}

impl Phylogeny {
    #[must_use]
    pub fn new() -> Phylogeny {
        Phylogeny {
            species: BTreeMap::new(),
            next_id: 0,
            speciation_threshold: DEFAULT_SPECIATION_THRESHOLD,
            genus_threshold: DEFAULT_GENUS_THRESHOLD,
            sample_interval: 500,
            pruned: 0,
            forks: 0,
            taken_names: Default::default(),
        }
    }

    /// Found a root species unconditionally, even if an identical genome already has one.
    ///
    /// For arena mode (M6), where two competitors may enter the *same* genome and must still
    /// be two teams. [`Phylogeny::found`] merges them, which is right for a slide being seeded
    /// with twelve copies of one ancestor and wrong for a match — the first version of the
    /// arena used `found` and every match between identical genomes ended 12–0 at tick one,
    /// because both sides resolved to the same root and one of them was always checked first.
    pub fn found_distinct(&mut self, genome: &Arc<Genome>, traits: Traits, tick: u64) -> SpeciesId {
        self.insert(None, genome, traits, tick, 0)
    }

    /// Register the species a seeded cell belongs to, creating it on first sight.
    ///
    /// Seeded cells have no parent species, so this is the only way a root enters the tree.
    pub fn found(&mut self, genome: &Arc<Genome>, traits: Traits, tick: u64) -> SpeciesId {
        // A second seeding of the same genome joins the species already founded for it,
        // rather than founding an identical rival — twelve founders of one ancestor are one
        // species, which is what anybody looking at the tree would expect.
        if let Some(existing) = self.root_for_fingerprint(genome.fingerprint()) {
            return existing;
        }
        self.insert(None, genome, traits, tick, 0)
    }

    /// The root species already founded for a genome's fingerprint, if there is one.
    ///
    /// The identity [`Phylogeny::found`] merges on, made available to a caller that needs to ask
    /// the same question *after* seeding — which is how [`crate::World::place_community`] learns
    /// the root each founding genome resolved to without depending on the order ids are handed
    /// out in. Deterministic: `species` is a `BTreeMap`, so the first match is the lowest id
    /// (hard rule 6).
    #[must_use]
    pub fn root_for_fingerprint(&self, fingerprint: u64) -> Option<SpeciesId> {
        self.species
            .values()
            .find(|s| s.parent.is_none() && s.founder_fingerprint == fingerprint)
            .map(|s| s.id)
    }

    /// Decide which species a newborn belongs to (SPEC §10.3).
    ///
    /// Returns the parent's species unless the daughter has drifted past the threshold, in
    /// which case a new species is founded and parented to it.
    pub fn on_birth(
        &mut self,
        parent_species: SpeciesId,
        genome: &Arc<Genome>,
        traits: Traits,
        tick: u64,
    ) -> SpeciesId {
        let Some(parent) = self.species.get(&parent_species) else {
            // The parent's species was pruned or never existed. Founding a root here keeps
            // every cell attributable, which acceptance 1 requires, rather than leaving an
            // orphan pointing at nothing.
            return self.found(genome, traits, tick);
        };
        let fingerprint = genome.fingerprint();
        let distance = fingerprint_distance(fingerprint, parent.founder_fingerprint);
        if distance <= self.speciation_threshold {
            return parent_species;
        }

        // It has drifted out of its parent species. Before founding a new one, look for a
        // sibling species it belongs in.
        //
        // Without this step, speciation is unusable. A lineage drifts, and then *every*
        // descendant that crosses the threshold founds its own species, because each is
        // measured only against the original founder and never against the cousins that
        // crossed the same line a moment earlier. The first run of this produced 7,684 species
        // from one ancestor in eight thousand ticks, nearly all of them with a peak population
        // of one — not speciation, just a counter incrementing.
        //
        // Searching the parent's existing children fixes it: a cell that has drifted the same
        // way its cousins did joins them. Bounded by how many children one species has, which
        // is small once this rule is in force — the churn it prevents was the thing that made
        // it large.
        let mut best: Option<(u32, SpeciesId)> = None;
        for sibling in parent.children.iter().copied() {
            let Some(s) = self.species.get(&sibling) else {
                continue;
            };
            let d = fingerprint_distance(fingerprint, s.founder_fingerprint);
            if d <= self.speciation_threshold && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, sibling));
            }
        }
        if let Some((_, sibling)) = best {
            return sibling;
        }

        let depth = parent.depth.saturating_add(1);
        self.forks += 1;
        let id = self.insert(Some(parent_species), genome, traits, tick, depth);
        if let Some(p) = self.species.get_mut(&parent_species) {
            p.children.push(id);
        }
        id
    }

    fn insert(
        &mut self,
        parent: Option<SpeciesId>,
        genome: &Arc<Genome>,
        traits: Traits,
        tick: u64,
        depth: u32,
    ) -> SpeciesId {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let fingerprint = genome.fingerprint();
        // Named from the fingerprint *and* the id, so two species founded from
        // indistinguishable genomes still get different names.
        let lineage = fingerprint ^ (id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let genus = self.genus_for(fingerprint, parent);
        let binomial = self.unique_name(lineage, &traits);
        let species = Species {
            id,
            parent,
            name: binomial,
            genus,
            founded_tick: tick,
            founder_fingerprint: fingerprint,
            founder_genome: Arc::clone(genome),
            traits,
            population: 0,
            peak_population: 0,
            peak_tick: tick,
            births: 0,
            deaths: 0,
            depth,
            extinct_tick: None,
            extinction: None,
            curve: Curve::new(self.sample_interval),
            children: Vec::new(),
            traits_settled: false,
        };
        self.taken_names.insert(species.name.full());
        self.species.insert(id, species);
        id
    }

    /// Generate a name nobody else has.
    ///
    /// The syllable tables are finite — thirty-two stems, eight endings, a dozen or so
    /// epithets — so collisions are not rare, they are expected: a seventeen-species run
    /// produced three distinct lineages all called *Membraopsis mixtus*. Two wiki pages with
    /// the same title is not a cosmetic problem, it is a wiki that cannot be navigated.
    ///
    /// Re-rolled with a walking salt rather than suffixed with a number, so a disambiguated
    /// name is still a name and not *Membraopsis mixtus 2*. Bounded, because a slide with
    /// enough species will eventually exhaust the tables, and at that point a numeric suffix
    /// is the honest answer — it says "there are more species here than the language has
    /// words for", which is true.
    fn unique_name(&self, lineage: u64, traits: &Traits) -> Binomial {
        let mut salt = lineage;
        for _ in 0..64 {
            let candidate = name(salt, traits);
            if !self.taken_names.contains(&candidate.full()) {
                return candidate;
            }
            salt = crate::rng::mix64(salt);
        }
        let mut fallback = name(lineage, traits);
        for n in 2u32..u32::MAX {
            let candidate = format!("{} {n}", fallback.epithet);
            if !self
                .taken_names
                .contains(&format!("{} {candidate}", fallback.genus))
            {
                fallback.epithet = candidate;
                break;
            }
        }
        fallback
    }

    /// Which genus a new species joins: its parent's, unless it has drifted past the deeper
    /// threshold, in which case it starts its own.
    fn genus_for(&self, fingerprint: u64, parent: Option<SpeciesId>) -> SpeciesId {
        let Some(parent) = parent.and_then(|p| self.species.get(&p)) else {
            return self.next_id.saturating_sub(1);
        };
        if fingerprint_distance(fingerprint, parent.founder_fingerprint) > self.genus_threshold {
            self.next_id.saturating_sub(1)
        } else {
            parent.genus
        }
    }

    /// Record a birth into a species.
    pub fn record_birth(&mut self, id: SpeciesId) {
        if let Some(s) = self.species.get_mut(&id) {
            s.births = s.births.saturating_add(1);
        }
    }

    /// Record a death out of a species.
    pub fn record_death(&mut self, id: SpeciesId) {
        if let Some(s) = self.species.get_mut(&id) {
            s.deaths = s.deaths.saturating_add(1);
        }
    }

    /// Adopt a living member's organelle loadout as the species' own, once one has expressed
    /// the genome.
    ///
    /// A species is founded at a birth, and a newborn has a membrane and nothing else — it
    /// spends the next few hundred ticks building what its genome describes. So the loadout
    /// read at founding is an empty cell, and a name and a description derived from it say
    /// "neither photosynthesising nor respiring" about a cell full of chloroplasts. The first
    /// run of this named the seeded ancestor exactly that.
    ///
    /// The loadout is therefore settled later, from a member that has finished building, and
    /// the name is regenerated with it. Once settled it never moves again — a species whose
    /// name drifted with its members would not be a name.
    pub fn settle_traits(&mut self, id: SpeciesId, traits: Traits) {
        let Some(species) = self.species.get(&id) else {
            return;
        };
        if species.traits_settled || traits.counts.iter().all(|c| *c == 0) {
            return;
        }
        // Regenerating the name must keep it unique, and the species' own current name must
        // not count as taken — otherwise it would collide with itself and walk off to a
        // different one for no reason.
        let lineage = species.founder_fingerprint ^ (id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let previous = species.name.full();
        let mut salt = lineage;
        let mut chosen = name(salt, &traits);
        for _ in 0..64 {
            let full = chosen.full();
            // Its own current name does not count as taken, or it would collide with itself
            // and walk off to a different one for no reason.
            if full == previous || !self.taken_names.contains(&full) {
                break;
            }
            salt = crate::rng::mix64(salt);
            chosen = name(salt, &traits);
        }
        self.taken_names.remove(&previous);
        self.taken_names.insert(chosen.full());
        let Some(species) = self.species.get_mut(&id) else {
            return;
        };
        species.traits = traits;
        species.name = chosen;
        species.traits_settled = true;
    }

    /// Update live populations and the curves from a census.
    ///
    /// Takes counts rather than walking the arena, so the caller can gather them in whatever
    /// pass it is already making. Species absent from the census are at zero.
    pub fn census(&mut self, counts: &BTreeMap<SpeciesId, u32>, tick: u64) {
        for (id, species) in &mut self.species {
            let n = counts.get(id).copied().unwrap_or(0);
            species.population = n;
            if n > species.peak_population {
                species.peak_population = n;
                species.peak_tick = tick;
            }
            if n > 0 {
                species.curve.sample(tick, n);
                // A species can come back from zero if the census missed it while its only
                // members were unborn intents. Clearing the mark keeps the archive honest.
                species.extinct_tick = None;
                species.extinction = None;
            } else if species.extinct_tick.is_none() && species.births > 0 {
                species.curve.sample(tick, 0);
                species.extinct_tick = Some(tick);
            }
        }
        // Causes are inferred in a second pass, because inferring them needs to see every
        // species' population at this tick, including the ones that had not been updated yet
        // when the first pass reached the one that died.
        self.infer_causes(tick);
    }

    /// Work out why anything that just went extinct did so.
    fn infer_causes(&mut self, tick: u64) {
        let newly: Vec<SpeciesId> = self
            .species
            .values()
            .filter(|s| s.extinct_tick == Some(tick) && s.extinction.is_none())
            .map(|s| s.id)
            .collect();
        if newly.is_empty() {
            return;
        }
        // The biggest live species, and the biggest live descendant of each casualty.
        let biggest = self
            .species
            .values()
            .filter(|s| s.population > 0)
            .max_by_key(|s| s.population)
            .map(|s| (s.id, s.population));
        let total_live: u64 = self.species.values().map(|s| u64::from(s.population)).sum();

        for id in newly {
            let peak = self.species.get(&id).map_or(0, |s| s.peak_population);
            let descendant = self
                .species
                .values()
                .filter(|s| s.parent == Some(id) && s.population > 0)
                .max_by_key(|s| s.population)
                .map(|s| s.id);
            let cause = if let Some(child) = descendant {
                Extinction::SucceededByDescendant(child)
            } else if peak < 4 {
                Extinction::NeverEstablished
            } else if total_live == 0 {
                Extinction::MassExtinction
            } else if let Some((other, n)) = biggest {
                if other != id && u64::from(n) * 4 > total_live {
                    Extinction::Outcompeted(other)
                } else {
                    Extinction::Unknown
                }
            } else {
                Extinction::Unknown
            };
            if let Some(s) = self.species.get_mut(&id) {
                s.extinction = Some(cause);
            }
        }
    }

    /// Drop extinct branches that carry no story (SPEC §10.3).
    ///
    /// Kept: anything alive, anything with living descendants, and anything that ever reached
    /// `keep_above` members — a species that mattered stays in the wiki after extinction,
    /// which is the whole point of the wiki. Dropped: the overwhelming majority, which are
    /// lineages that forked, stayed tiny and died.
    ///
    /// Returns how many were removed.
    pub fn prune(&mut self, keep_above: u32) -> usize {
        // A species with a surviving descendant has to stay, or the survivor's ancestry chain
        // would terminate at nothing and acceptance 1 would be violated by the pruner itself.
        let mut needed: std::collections::BTreeSet<SpeciesId> = Default::default();
        for s in self.species.values() {
            if s.population > 0 || s.peak_population >= keep_above {
                let mut at = Some(s.id);
                while let Some(id) = at {
                    if !needed.insert(id) {
                        break;
                    }
                    at = self.species.get(&id).and_then(|s| s.parent);
                }
            }
        }
        let before = self.species.len();
        self.species.retain(|id, _| needed.contains(id));
        // A pruned species must not survive as a dangling child link, or a later birth would
        // be offered a cousin that no longer exists.
        for s in self.species.values_mut() {
            s.children.retain(|c| needed.contains(c));
        }
        let removed = before - self.species.len();
        // A pruned species' name is free again.
        self.taken_names = self.species.values().map(|s| s.name.full()).collect();
        self.pruned = self.pruned.saturating_add(removed as u64);
        removed
    }

    #[must_use]
    pub fn get(&self, id: SpeciesId) -> Option<&Species> {
        self.species.get(&id)
    }

    /// Every species, ascending by id — which is also founding order.
    pub fn iter(&self) -> impl Iterator<Item = &Species> {
        self.species.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.species.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.species.is_empty()
    }

    #[must_use]
    pub fn living(&self) -> usize {
        self.species.values().filter(|s| s.population > 0).count()
    }

    #[must_use]
    pub fn pruned(&self) -> u64 {
        self.pruned
    }

    #[must_use]
    pub fn forks(&self) -> u64 {
        self.forks
    }

    #[must_use]
    pub fn next_id(&self) -> SpeciesId {
        self.next_id
    }

    /// Restore an archive from a snapshot. Round-tripping is hard rule 7.
    ///
    /// `children` is rebuilt from the parent links rather than being stored: it is entirely
    /// determined by them, and a stored copy is a copy that can disagree with the thing it
    /// duplicates. The order is by ascending id, which is the order it was built in, so a
    /// restored archive resolves sibling matches identically to the run it came from.
    pub fn restore(&mut self, species: Vec<Species>, next_id: SpeciesId, pruned: u64, forks: u64) {
        self.species = species.into_iter().map(|s| (s.id, s)).collect();
        for s in self.species.values_mut() {
            s.children.clear();
        }
        let links: Vec<(SpeciesId, SpeciesId)> = self
            .species
            .values()
            .filter_map(|s| s.parent.map(|p| (p, s.id)))
            .collect();
        for (parent, child) in links {
            if let Some(p) = self.species.get_mut(&parent) {
                p.children.push(child);
            }
        }
        self.taken_names = self.species.values().map(|s| s.name.full()).collect();
        self.next_id = next_id;
        self.pruned = pruned;
        self.forks = forks;
    }

    /// The chain from a species up to its root, nearest first.
    ///
    /// Bounded by the archive's size, so a cycle — which should be impossible, since a species'
    /// parent always has a lower id — cannot hang the caller.
    #[must_use]
    pub fn ancestry(&self, id: SpeciesId) -> Vec<SpeciesId> {
        let mut out = Vec::new();
        let mut at = Some(id);
        let mut guard = self.species.len() + 1;
        while let Some(current) = at {
            out.push(current);
            if guard == 0 {
                break;
            }
            guard -= 1;
            at = self.species.get(&current).and_then(|s| s.parent);
        }
        out
    }

    pub fn hash_into(&self, h: &mut crate::state_hash::StateHasher) {
        h.u32(self.next_id);
        h.u64(self.pruned);
        h.u64(self.forks);
        for s in self.species.values() {
            h.u32(s.id);
            h.u32(s.parent.unwrap_or(u32::MAX));
            h.u64(s.founded_tick);
            h.u64(s.founder_fingerprint);
            h.u32(s.population);
            h.u32(s.peak_population);
            h.u64(s.peak_tick);
            h.u64(s.births);
            h.u64(s.deaths);
            h.u32(s.depth);
            h.u64(s.extinct_tick.unwrap_or(u64::MAX));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organelle::{Organelle, OrganelleType};

    fn genome(bytes: Vec<u8>) -> Arc<Genome> {
        Arc::new(Genome::new(bytes).expect("genome"))
    }

    fn traits() -> Traits {
        Traits::of(&[Organelle::finished(OrganelleType::Chloroplast, 40)], 200)
    }

    /// A genome `bits` fingerprint-bits away from `base`, found by mutating until it is.
    fn drifted(base: &[u8], want: u32) -> Vec<u8> {
        let mut g = base.to_vec();
        let base_fp = crate::genome::simhash(base);
        for i in 0..g.len() {
            if fingerprint_distance(base_fp, crate::genome::simhash(&g)) >= want {
                break;
            }
            g[i] = g[i].wrapping_add(97).wrapping_mul(3);
        }
        g
    }

    #[test]
    fn seeding_the_same_ancestor_twice_founds_one_species() {
        let mut p = Phylogeny::new();
        let g = genome(vec![7u8; 200]);
        let a = p.found(&g, traits(), 0);
        let b = p.found(&g, traits(), 0);
        assert_eq!(a, b);
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn an_unmutated_daughter_stays_in_its_species() {
        let mut p = Phylogeny::new();
        let g = genome(vec![7u8; 200]);
        let founder = p.found(&g, traits(), 0);
        for tick in 0..100 {
            assert_eq!(p.on_birth(founder, &g, traits(), tick), founder);
        }
        assert_eq!(p.len(), 1, "an identical copy founded a new species");
    }

    #[test]
    fn drifting_far_enough_founds_a_new_species_parented_to_the_old() {
        let mut p = Phylogeny::new();
        let base = vec![7u8; 200];
        let g = genome(base.clone());
        let founder = p.found(&g, traits(), 0);

        let far = genome(drifted(&base, p.speciation_threshold + 6));
        assert!(
            fingerprint_distance(far.fingerprint(), g.fingerprint()) > p.speciation_threshold,
            "the test's own drifted genome is not far enough to speciate"
        );
        let new = p.on_birth(founder, &far, traits(), 500);
        assert_ne!(new, founder);
        let s = p.get(new).expect("the new species exists");
        assert_eq!(s.parent, Some(founder));
        assert_eq!(s.founded_tick, 500);
        assert_eq!(s.depth, 1);
    }

    #[test]
    fn ancestry_terminates_at_a_root_and_does_not_loop() {
        let mut p = Phylogeny::new();
        let base = vec![7u8; 200];
        let mut current = genome(base.clone());
        let mut species = p.found(&current, traits(), 0);
        let mut bytes = base;
        for step in 0..6 {
            bytes = drifted(&bytes, p.speciation_threshold + 6);
            current = genome(bytes.clone());
            species = p.on_birth(species, &current, traits(), step * 100);
        }
        let chain = p.ancestry(species);
        assert_eq!(*chain.last().expect("a chain"), 0, "did not reach the root");
        let unique: std::collections::BTreeSet<SpeciesId> = chain.iter().copied().collect();
        assert_eq!(unique.len(), chain.len(), "the chain repeats: {chain:?}");
        assert!(p.get(0).expect("root").parent.is_none());
    }

    #[test]
    fn the_curve_stays_bounded_however_long_a_species_lives() {
        // The storage bound in miniature. A species alive for ten million ticks must not cost
        // more than one alive for ten thousand.
        let mut c = Curve::new(100);
        for tick in (0..10_000_000u64).step_by(100) {
            c.sample(tick, 1000);
        }
        assert!(
            c.points().len() <= CURVE_POINTS + 1,
            "curve grew to {} points",
            c.points().len()
        );
        assert!(c.interval() > 100, "the interval never coarsened");
        // And it still covers the whole life, not just the start or the end.
        let first = c.points().first().expect("points").tick;
        let last = c.points().last().expect("points").tick;
        assert!(
            first < 100_000,
            "the curve lost its beginning: starts at {first}"
        );
        assert!(last > 9_000_000, "the curve lost its end: ends at {last}");
    }

    #[test]
    fn extinction_is_noticed_and_given_a_cause() {
        let mut p = Phylogeny::new();
        let g = genome(vec![7u8; 200]);
        let a = p.found(&g, traits(), 0);
        p.record_birth(a);
        let mut counts = BTreeMap::new();
        counts.insert(a, 50u32);
        p.census(&counts, 100);
        assert!(!p.get(a).expect("a").is_extinct());

        counts.insert(a, 0);
        p.census(&counts, 200);
        let s = p.get(a).expect("a");
        assert_eq!(s.extinct_tick, Some(200));
        assert!(s.extinction.is_some(), "extinct with no cause inferred");
        assert_eq!(s.peak_population, 50);
        assert_eq!(s.peak_tick, 100);
    }

    #[test]
    fn a_species_succeeded_by_its_own_descendant_says_so() {
        let mut p = Phylogeny::new();
        let base = vec![7u8; 200];
        let g = genome(base.clone());
        let parent = p.found(&g, traits(), 0);
        p.record_birth(parent);
        let child_genome = genome(drifted(&base, p.speciation_threshold + 6));
        let child = p.on_birth(parent, &child_genome, traits(), 100);
        p.record_birth(child);

        let mut counts = BTreeMap::new();
        counts.insert(parent, 100u32);
        counts.insert(child, 10u32);
        p.census(&counts, 200);

        counts.insert(parent, 0);
        counts.insert(child, 400);
        p.census(&counts, 300);
        assert_eq!(
            p.get(parent).expect("parent").extinction,
            Some(Extinction::SucceededByDescendant(child))
        );
    }

    #[test]
    fn pruning_keeps_what_a_survivor_needs_to_chain_through() {
        // The pruner must never orphan a living species, or it would break the invariant that
        // acceptance 1 checks — and it would be the pruner, not the simulation, that broke it.
        let mut p = Phylogeny::new();
        let base = vec![7u8; 200];
        let mut bytes = base.clone();
        let mut current = genome(base);
        let mut species = p.found(&current, traits(), 0);
        let mut chain = vec![species];
        for step in 0..5u64 {
            bytes = drifted(&bytes, p.speciation_threshold + 6);
            current = genome(bytes.clone());
            species = p.on_birth(species, &current, traits(), step * 100);
            p.record_birth(species);
            chain.push(species);
        }
        // Only the newest is alive, and none of the intermediates ever got big.
        let mut counts = BTreeMap::new();
        counts.insert(species, 500u32);
        p.census(&counts, 1000);

        p.prune(1000);
        for id in &chain {
            assert!(
                p.get(*id).is_some(),
                "pruned {id}, which the survivor chains through"
            );
        }
        let ancestry = p.ancestry(species);
        assert_eq!(*ancestry.last().expect("root"), 0);
    }

    #[test]
    fn pruning_removes_dead_ends_that_never_amounted_to_anything() {
        let mut p = Phylogeny::new();
        let base = vec![7u8; 200];
        let root_genome = genome(base.clone());
        let root = p.found(&root_genome, traits(), 0);
        p.record_birth(root);

        let mut bytes = base;
        for step in 0..8u64 {
            bytes = drifted(&bytes, p.speciation_threshold + 6);
            let dead_end = p.on_birth(root, &genome(bytes.clone()), traits(), step * 10);
            p.record_birth(dead_end);
        }
        let before = p.len();
        let mut counts = BTreeMap::new();
        counts.insert(root, 900u32);
        p.census(&counts, 500);

        let removed = p.prune(100);
        assert!(removed > 0, "nothing was pruned out of {before} species");
        assert!(p.get(root).is_some(), "pruned the living root");
    }

    #[test]
    fn a_wiki_description_says_only_true_things() {
        let mut p = Phylogeny::new();
        let t = Traits::of(
            &[
                Organelle::finished(OrganelleType::Chloroplast, 60),
                Organelle::finished(OrganelleType::Cilium, 30),
                Organelle::finished(OrganelleType::Cilium, 30),
            ],
            240,
        );
        let id = p.found(&genome(vec![3u8; 240]), t, 0);
        let text = p.get(id).expect("species").describe(&p);
        assert!(text.contains("2 cilia"), "{text}");
        assert!(text.contains("240-byte genome"), "{text}");
        assert!(text.contains("Seeded at tick 0"), "{text}");
        assert!(
            !text.contains("chemosensor"),
            "claimed a sensor it lacks: {text}"
        );
    }
}

/// NDJSON export of the whole species archive, for offline analysis (M5).
///
/// One object per species, one per line, plus one per event — the same stream shape the metric
/// export uses and for the same reasons: it can be tailed while the run is going, truncated
/// without corrupting what came before, and fed to anything line-oriented.
///
/// Serialised by hand rather than through serde, matching `metrics.rs`. The schema is a stable
/// contract that offline analysis depends on, and writing it longhand keeps the field names
/// visible in one place.
pub mod export {
    use super::{Extinction, Phylogeny, Species};
    use crate::events::{Event, EventLog};

    /// Escape a string for a JSON field. Species names come from a fixed table so they cannot
    /// contain anything exotic, but the export is a public contract and a name that broke the
    /// parser would be discovered by whoever was relying on it, not by us.
    fn quote(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    fn cause_json(cause: Option<Extinction>) -> String {
        match cause {
            None => "null".to_string(),
            Some(Extinction::SucceededByDescendant(id)) => {
                format!(r#"{{"kind":"succeeded_by_descendant","species":{id}}}"#)
            }
            Some(Extinction::Outcompeted(id)) => {
                format!(r#"{{"kind":"outcompeted","species":{id}}}"#)
            }
            Some(Extinction::MassExtinction) => r#"{"kind":"mass_extinction"}"#.to_string(),
            Some(Extinction::NeverEstablished) => r#"{"kind":"never_established"}"#.to_string(),
            Some(Extinction::Unknown) => r#"{"kind":"unknown"}"#.to_string(),
        }
    }

    /// One species as a JSON object.
    ///
    /// The founder genome goes out as hex rather than base64: it is the thing someone will
    /// want to paste into the disassembler, and hex survives every pipeline unchanged.
    #[must_use]
    pub fn species_json(s: &Species) -> String {
        let genome: String = s
            .founder_genome
            .bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let curve: Vec<String> = s
            .curve
            .points()
            .iter()
            .map(|p| format!("[{},{}]", p.tick, p.population))
            .collect();
        let counts: Vec<String> = s.traits.counts.iter().map(|c| c.to_string()).collect();
        format!(
            concat!(
                r#"{{"record":"species","id":{},"parent":{},"genus_group":{},"#,
                r#""genus":{},"epithet":{},"#,
                r#""founded_tick":{},"fingerprint":"{:016x}","depth":{},"#,
                r#""population":{},"peak_population":{},"peak_tick":{},"#,
                r#""births":{},"deaths":{},"extinct_tick":{},"extinction":{},"#,
                r#""organelles":[{}],"genome_len":{},"founder_genome":"{}","#,
                r#""curve_interval":{},"curve":[{}]}}"#
            ),
            s.id,
            s.parent.map_or("null".to_string(), |p| p.to_string()),
            s.genus,
            quote(&s.name.genus),
            quote(&s.name.epithet),
            s.founded_tick,
            s.founder_fingerprint,
            s.depth,
            s.population,
            s.peak_population,
            s.peak_tick,
            s.births,
            s.deaths,
            s.extinct_tick.map_or("null".to_string(), |t| t.to_string()),
            cause_json(s.extinction),
            counts.join(","),
            s.traits.genome_len,
            genome,
            s.curve.interval(),
            curve.join(","),
        )
    }

    /// One event as a JSON object.
    #[must_use]
    pub fn event_json(e: &Event) -> String {
        format!(
            r#"{{"record":"event","tick":{},"what":{},"species":{},"x":{},"y":{}}}"#,
            e.tick,
            quote(&e.what.headline()),
            e.species,
            e.x,
            e.y
        )
    }

    /// The whole archive as NDJSON: every species, then every event, in order.
    #[must_use]
    pub fn archive_ndjson(archive: &Phylogeny, log: &EventLog) -> String {
        let mut out = String::new();
        for s in archive.iter() {
            out.push_str(&species_json(s));
            out.push('\n');
        }
        for e in log.events() {
            out.push_str(&event_json(e));
            out.push('\n');
        }
        out
    }
}
