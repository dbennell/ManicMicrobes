//! First-occurrence detectors and the world event log (SPEC §10.6).
//!
//! > This is the newspaper, and it is the single largest contributor to the simulation feeling
//! > like a story rather than a screensaver.
//!
//! # What a detector is allowed to be
//!
//! Each detector answers one question about the world as it is *this tick*, and the log
//! records the first tick at which the answer was yes, with the species and place it happened.
//! Detectors never look at history, so a detector cannot slowly convince itself of something;
//! either the thing is happening now or it is not.
//!
//! # Honesty about what cannot be detected yet
//!
//! SPEC §10.6 lists detectors for mechanisms that do not exist until M7 and M8: junctions,
//! `INJECT` into another cell, clusters, differentiation, signal relays, predation, dormancy.
//! Those are declared in [`Occurrence`] and **never fire**, because the events they describe
//! cannot happen yet.
//!
//! They are declared rather than omitted so that the log's shape is fixed now — the archive
//! format, the timeline UI and the NDJSON export all key off this enum, and adding variants
//! later is a schema change where filling in a detector is not. `Occurrence::detectable_now`
//! says which is which, and a test asserts that the undetectable ones stay silent, so nothing
//! can quietly start reporting a first predation before predation is implemented.

use crate::cell::CellArena;
use crate::organelle::OrganelleType;
use crate::phylogeny::SpeciesId;

/// Something that happened for the first time.
///
/// Ordering is the order they are checked and the order they appear in the timeline, which is
/// roughly the order they are expected to occur in a run.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Occurrence {
    /// A cell divided under its own genome's control.
    EndogenousReplication,
    /// A cell built a cilium and drove it.
    Motility,
    /// A cell carried both a chemosensor and a cilium — the machinery chemotaxis needs.
    ChemotacticMachinery,
    /// A cell carried both a photosensor and a cilium.
    PhototacticMachinery,
    /// A lineage passed this many speciation events from the seeded founder.
    Generations(u32),
    /// A species other than the seeded founder held more of the population than any other.
    NewDominantSpecies,
    /// Population fell by more than half inside the detection window.
    MassExtinction,

    /// A cell wrote genome bytes into another cell's nucleus. Parasitism — and nothing in the
    /// engine knows the word (SPEC §8.3).
    ForeignInjection,
    /// A transfer channel formed.
    SoftJunction,
    /// A structural junction formed. The beginning of multicellularity.
    HardJunction,
    /// A connected component reached this size.
    Cluster(u32),
    /// Two distinct organelle loadouts inside one component.
    DifferentiatedCluster,
    /// A chain of at least three hard junctions — a body long enough to relay through.
    SignalRelay,
    /// A junction forced against a key mismatch, paid for at the full penalty.
    KeyMismatchJunction,

    /// A cell wounded another with a spike. Predation — and as with parasitism, nothing in
    /// the engine knows the word: a spike does damage, damage kills, and death makes carrion.
    Predation,

    // --- Declared, not yet detectable. ---
    /// Requires a dormant state, which nothing implements.
    Dormancy,
}

impl Occurrence {
    /// Every kind of occurrence, once each, in timeline order.
    ///
    /// The two parameterised variants stand in with a representative threshold — this is for
    /// walking the *kinds*, as the timeline legend and the staleness tests do, not for
    /// enumerating every event a run can produce.
    pub const ALL: [Occurrence; 16] = [
        Occurrence::EndogenousReplication,
        Occurrence::Motility,
        Occurrence::ChemotacticMachinery,
        Occurrence::PhototacticMachinery,
        Occurrence::Generations(4),
        Occurrence::NewDominantSpecies,
        Occurrence::MassExtinction,
        Occurrence::ForeignInjection,
        Occurrence::SoftJunction,
        Occurrence::HardJunction,
        Occurrence::Cluster(4),
        Occurrence::DifferentiatedCluster,
        Occurrence::SignalRelay,
        Occurrence::KeyMismatchJunction,
        Occurrence::Predation,
        Occurrence::Dormancy,
    ];

    /// Whether the mechanism this describes exists in the engine yet.
    ///
    /// A detector for something unimplementable must never fire — a newspaper that reports
    /// events that did not happen is worse than one with fewer pages.
    #[must_use]
    pub fn detectable_now(&self) -> bool {
        // M7 filled in the junction half; M8 filled in predation. `Dormancy` is what is left,
        // and there is no dormant state to enter — SPEC lists it, nothing implements it, and a
        // detector that fired for it would be reporting fiction.
        !matches!(self, Occurrence::Dormancy)
    }

    /// A headline, in the register of a newspaper rather than a log file.
    #[must_use]
    pub fn headline(&self) -> String {
        match self {
            Occurrence::EndogenousReplication => "first self-replication".to_string(),
            Occurrence::Motility => "first motility".to_string(),
            Occurrence::ChemotacticMachinery => "first chemotactic machinery".to_string(),
            Occurrence::PhototacticMachinery => "first phototactic machinery".to_string(),
            Occurrence::Generations(n) => format!("lineage reached {n} generations"),
            Occurrence::NewDominantSpecies => "a new species took the slide".to_string(),
            Occurrence::MassExtinction => "mass extinction".to_string(),
            Occurrence::Predation => "first predation".to_string(),
            Occurrence::ForeignInjection => "first foreign injection".to_string(),
            Occurrence::SoftJunction => "first soft junction".to_string(),
            Occurrence::HardJunction => "first hard junction".to_string(),
            Occurrence::Cluster(n) => format!("first cluster of {n}"),
            Occurrence::DifferentiatedCluster => "first differentiated cluster".to_string(),
            Occurrence::SignalRelay => "first signal relay".to_string(),
            Occurrence::KeyMismatchJunction => "first forced junction".to_string(),
            Occurrence::Dormancy => "first dormancy".to_string(),
        }
    }
}

/// One entry in the world's event log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub tick: u64,
    pub what: Occurrence,
    pub species: SpeciesId,
    /// Where it happened, in substrate squares.
    pub x: i32,
    pub y: i32,
}

/// The world's newspaper: every first occurrence, in the order they happened.
///
/// Mass extinctions are the one thing recorded more than once — each is its own event, because
/// each is its own story. Everything else is a first and only a first.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct EventLog {
    events: Vec<Event>,
    /// Population at the start of the current window, for the mass-extinction detector.
    window_start_population: u32,
    window_started_at: u64,
    /// Generation milestones already reported, so `Generations(4)` fires once.
    generations_reported: u32,
    /// The species that most recently held the slide.
    dominant: Option<SpeciesId>,
}

/// Ticks over which a population drop is judged (SPEC §10.6: "within a window").
pub const MASS_EXTINCTION_WINDOW: u64 = 2_000;

/// Generation counts worth a headline. Powers of two, so the milestones thin out as they get
/// harder rather than arriving at a constant rate.
const GENERATION_MILESTONES: [u32; 7] = [4, 8, 16, 32, 64, 128, 256];

impl EventLog {
    #[must_use]
    pub fn new() -> EventLog {
        EventLog::default()
    }

    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// When something first happened, if it has.
    #[must_use]
    pub fn first(&self, what: Occurrence) -> Option<u64> {
        self.events.iter().find(|e| e.what == what).map(|e| e.tick)
    }

    fn already(&self, what: Occurrence) -> bool {
        self.events.iter().any(|e| e.what == what)
    }

    fn record(&mut self, tick: u64, what: Occurrence, species: SpeciesId, x: i32, y: i32) {
        debug_assert!(
            what.detectable_now(),
            "recorded {what:?}, whose mechanism does not exist yet"
        );
        self.events.push(Event {
            tick,
            what,
            species,
            x,
            y,
        });
    }

    /// Look at the world and record anything happening for the first time.
    ///
    /// Called once per tick. Everything here is a scan over live cells, so it is deliberately
    /// one pass with early exits: once every machinery detector has fired, the per-cell loop
    /// is skipped entirely, which is what keeps M5's 5% budget reachable in a long run where
    /// all the firsts happened in the first thousand ticks.
    pub fn observe(&mut self, world: &WorldView<'_>, tick: u64) {
        self.observe_population(world, tick);
        self.observe_lineage(world, tick);

        self.observe_junctions(world, tick);

        // The per-cell scan, skipped once there is nothing left for it to find.
        let wanted = [
            Occurrence::Motility,
            Occurrence::ChemotacticMachinery,
            Occurrence::PhototacticMachinery,
        ];
        if wanted.iter().all(|w| self.already(*w)) {
            return;
        }
        for i in world.cells.iter() {
            let mut cilia = false;
            let mut chemo = false;
            let mut photo = false;
            for o in world.cells.slots(i) {
                if !o.is_active() {
                    continue;
                }
                match o.kind {
                    OrganelleType::Cilium => cilia = true,
                    OrganelleType::Chemosensor => chemo = true,
                    OrganelleType::Photosensor => photo = true,
                    _ => {}
                }
            }
            if !cilia {
                continue;
            }
            let species = world.cells.species[i];
            let (x, y) = (
                crate::fixed::pos_to_square(world.cells.x[i]),
                crate::fixed::pos_to_square(world.cells.y[i]),
            );
            // Motility means a cilium that is actually being driven, not merely owned: a
            // cell carrying an idle cilium has not moved and reporting it would be a lie.
            if !self.already(Occurrence::Motility)
                && world.cells.slots(i).iter().any(|o| {
                    o.kind == OrganelleType::Cilium
                        && o.is_active()
                        && crate::sensing::cilium_thrust(o) != 0
                })
            {
                self.record(tick, Occurrence::Motility, species, x, y);
            }
            if chemo && !self.already(Occurrence::ChemotacticMachinery) {
                self.record(tick, Occurrence::ChemotacticMachinery, species, x, y);
            }
            if photo && !self.already(Occurrence::PhototacticMachinery) {
                self.record(tick, Occurrence::PhototacticMachinery, species, x, y);
            }
            if wanted.iter().all(|w| self.already(*w)) {
                return;
            }
        }
    }

    /// Junctions, clusters and the things that only happen once cells are joined (SPEC §8).
    fn observe_junctions(&mut self, world: &WorldView<'_>, tick: u64) {
        use crate::junction::JunctionKind;

        if !self.already(Occurrence::ForeignInjection) && world.foreign_injections > 0 {
            let (species, x, y) = world.any_cell().unwrap_or((0, 0, 0));
            self.record(tick, Occurrence::ForeignInjection, species, x, y);
        }
        if !self.already(Occurrence::Predation) && world.wounds > 0 {
            let (species, x, y) = world.any_cell().unwrap_or((0, 0, 0));
            self.record(tick, Occurrence::Predation, species, x, y);
        }
        if !self.already(Occurrence::KeyMismatchJunction) && world.forced_joins > 0 {
            let (species, x, y) = world.any_cell().unwrap_or((0, 0, 0));
            self.record(tick, Occurrence::KeyMismatchJunction, species, x, y);
        }

        let want_soft = !self.already(Occurrence::SoftJunction);
        let want_hard = !self.already(Occurrence::HardJunction);
        if want_soft || want_hard {
            'scan: for i in world.cells.iter() {
                for j in world.cells.junctions(i) {
                    let what = match j.kind {
                        JunctionKind::Soft if want_soft => Occurrence::SoftJunction,
                        JunctionKind::Hard if want_hard => Occurrence::HardJunction,
                        _ => continue,
                    };
                    if self.already(what) {
                        continue;
                    }
                    let (x, y) = (
                        crate::fixed::pos_to_square(world.cells.x[i]),
                        crate::fixed::pos_to_square(world.cells.y[i]),
                    );
                    self.record(tick, what, world.cells.species[i], x, y);
                    if self.already(Occurrence::SoftJunction)
                        && self.already(Occurrence::HardJunction)
                    {
                        break 'scan;
                    }
                }
            }
        }

        // Clusters need the components, which the caller may not have built.
        let Some(components) = &world.components else {
            return;
        };
        // Milestones, not every size: a headline for every cluster size from four to sixty-four
        // would bury the ones that matter.
        for size in [4u32, 16, 64] {
            let what = Occurrence::Cluster(size);
            if self.already(what) || components.largest() < size {
                continue;
            }
            let (species, x, y) = world.any_cell().unwrap_or((0, 0, 0));
            self.record(tick, what, species, x, y);
        }
        // A chain of three hard junctions is a body long enough for a signal to relay along.
        // Measured as a component of at least four, which is what three links make.
        if !self.already(Occurrence::SignalRelay) && components.largest() >= 4 {
            let (species, x, y) = world.any_cell().unwrap_or((0, 0, 0));
            self.record(tick, Occurrence::SignalRelay, species, x, y);
        }
    }

    /// Replication, dominance and mass extinction, all of which read counts rather than cells.
    fn observe_population(&mut self, world: &WorldView<'_>, tick: u64) {
        let population = world.cells.len() as u32;

        if !self.already(Occurrence::EndogenousReplication) && world.births_so_far > 0 {
            let (species, x, y) = world.any_cell().unwrap_or((0, 0, 0));
            self.record(tick, Occurrence::EndogenousReplication, species, x, y);
        }

        // Mass extinction: more than half gone inside the window. The window restarts from
        // the current population whenever it closes, so a slow decline over a hundred thousand
        // ticks is not reported as a catastrophe — which is right, because it is not one.
        if tick.saturating_sub(self.window_started_at) >= MASS_EXTINCTION_WINDOW {
            if self.window_start_population >= 8 && population * 2 < self.window_start_population {
                let (species, x, y) = world.any_cell().unwrap_or((0, 0, 0));
                self.record(tick, Occurrence::MassExtinction, species, x, y);
            }
            self.window_start_population = population;
            self.window_started_at = tick;
        } else if self.window_start_population == 0 {
            self.window_start_population = population;
            self.window_started_at = tick;
        }
    }

    /// Milestones that come off the species archive rather than off the cells.
    fn observe_lineage(&mut self, world: &WorldView<'_>, tick: u64) {
        let Some(archive) = world.archive else {
            return;
        };
        // Deepest living lineage.
        let deepest = archive
            .iter()
            .filter(|s| s.population > 0)
            .map(|s| s.depth)
            .max()
            .unwrap_or(0);
        for milestone in GENERATION_MILESTONES {
            if deepest >= milestone && self.generations_reported < milestone {
                let species = archive
                    .iter()
                    .filter(|s| s.population > 0 && s.depth >= milestone)
                    .map(|s| s.id)
                    .next()
                    .unwrap_or(0);
                self.generations_reported = milestone;
                self.record(tick, Occurrence::Generations(milestone), species, 0, 0);
            }
        }

        // A change of hands: whichever species holds the most cells, when that is somebody
        // new. Reported once per change rather than once ever, because "who is winning" is a
        // running story and not a single event.
        if let Some(top) = archive
            .iter()
            .filter(|s| s.population > 0)
            .max_by_key(|s| (s.population, std::cmp::Reverse(s.id)))
        {
            if self.dominant.is_some() && self.dominant != Some(top.id) {
                self.events.push(Event {
                    tick,
                    what: Occurrence::NewDominantSpecies,
                    species: top.id,
                    x: 0,
                    y: 0,
                });
            }
            if self.dominant != Some(top.id) {
                self.dominant = Some(top.id);
            }
        }
    }

    /// Restore from a snapshot. Hard rule 7.
    pub fn restore(
        &mut self,
        events: Vec<Event>,
        window_start_population: u32,
        window_started_at: u64,
        generations_reported: u32,
        dominant: Option<SpeciesId>,
    ) {
        self.events = events;
        self.window_start_population = window_start_population;
        self.window_started_at = window_started_at;
        self.generations_reported = generations_reported;
        self.dominant = dominant;
    }

    #[must_use]
    pub fn window_state(&self) -> (u32, u64, u32, Option<SpeciesId>) {
        (
            self.window_start_population,
            self.window_started_at,
            self.generations_reported,
            self.dominant,
        )
    }

    pub fn hash_into(&self, h: &mut crate::state_hash::StateHasher) {
        h.u32(self.window_start_population);
        h.u64(self.window_started_at);
        h.u32(self.generations_reported);
        h.u32(self.dominant.unwrap_or(u32::MAX));
        for e in &self.events {
            h.u64(e.tick);
            h.u32(e.species);
            h.i32(e.x);
            h.i32(e.y);
        }
    }
}

/// What the detectors are allowed to see.
///
/// A narrow view rather than `&World`, so a detector cannot reach anything that would let it
/// change the simulation, and so this module does not depend on `world`.
#[derive(Debug)]
pub struct WorldView<'a> {
    pub cells: &'a CellArena,
    pub archive: Option<&'a crate::phylogeny::Phylogeny>,
    /// Births since the run began, for the replication detector.
    pub births_so_far: u64,
    /// Foreign injections so far. Counted as they happen in `resolve`, because a byte written
    /// into a neighbour leaves no trace afterwards that a scan could find.
    pub foreign_injections: u64,
    /// Junctions forced against a key mismatch so far, for the same reason.
    pub forced_joins: u64,
    /// Spike wounds dealt so far. Counted as they happen: a wound is damage on a cell and
    /// leaves nothing afterwards to say what caused it.
    pub wounds: u64,
    /// Connected components, if the caller has them. Rebuilt outside the detectors because
    /// they are wanted by the wiki and the renderer too.
    pub components: Option<&'a mut crate::junction::Components>,
}

impl WorldView<'_> {
    /// Any living cell's species and position, for events that need somewhere to point at.
    fn any_cell(&self) -> Option<(SpeciesId, i32, i32)> {
        self.cells.iter().next().map(|i| {
            (
                self.cells.species[i],
                crate::fixed::pos_to_square(self.cells.x[i]),
                crate::fixed::pos_to_square(self.cells.y[i]),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_that_cannot_happen_yet_claims_to_be_detectable() {
        // The list shrank at M7, which is the point of having it: junctions, clusters and
        // foreign injection all became real, so they moved from one half to the other and
        // this test had to be updated deliberately rather than drifting.
        // Everything except dormancy is implemented as of M8.
        for what in Occurrence::ALL {
            assert_eq!(
                what.detectable_now(),
                !matches!(what, Occurrence::Dormancy),
                "{what:?} disagrees with what the engine actually implements"
            );
        }
    }

    #[test]
    fn all_lists_every_kind_of_occurrence() {
        // `ALL` is what the timeline legend walks and what the staleness tests filter, so a
        // variant missing from it is a variant nothing checks. The match below does not compile
        // if a variant is added, which is the point of writing it out rather than counting.
        for what in Occurrence::ALL {
            let seen = match what {
                Occurrence::EndogenousReplication
                | Occurrence::Motility
                | Occurrence::ChemotacticMachinery
                | Occurrence::PhototacticMachinery
                | Occurrence::Generations(_)
                | Occurrence::NewDominantSpecies
                | Occurrence::MassExtinction
                | Occurrence::ForeignInjection
                | Occurrence::SoftJunction
                | Occurrence::HardJunction
                | Occurrence::Cluster(_)
                | Occurrence::DifferentiatedCluster
                | Occurrence::SignalRelay
                | Occurrence::KeyMismatchJunction
                | Occurrence::Predation
                | Occurrence::Dormancy => 1,
            };
            assert_eq!(seen, 1);
        }
        // And no duplicates, so the count is a real count.
        let mut sorted = Occurrence::ALL;
        sorted.sort_unstable();
        let mut deduped = sorted.to_vec();
        deduped.dedup();
        assert_eq!(
            deduped.len(),
            Occurrence::ALL.len(),
            "ALL repeats a variant"
        );
        assert!(
            !Occurrence::ALL.iter().all(|o| o.detectable_now()),
            "if everything is detectable, the silence test has nothing left to check"
        );
    }

    #[test]
    fn every_occurrence_has_a_headline() {
        // The log is the newspaper. A variant with no headline would print as a debug string
        // in the timeline UI.
        for what in [
            Occurrence::EndogenousReplication,
            Occurrence::Motility,
            Occurrence::Cluster(16),
            Occurrence::Generations(8),
            Occurrence::Dormancy,
        ] {
            assert!(!what.headline().is_empty());
            assert!(
                !what.headline().contains("Occurrence"),
                "debug-formatted headline"
            );
        }
    }

    #[test]
    fn a_first_is_recorded_once() {
        let mut log = EventLog::new();
        log.record(10, Occurrence::Motility, 1, 3, 4);
        assert!(log.already(Occurrence::Motility));
        assert_eq!(log.first(Occurrence::Motility), Some(10));
        assert_eq!(log.first(Occurrence::Predation), None);
    }
}
