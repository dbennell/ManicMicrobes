//! The world: substrate, ledger and the tick loop (SPEC §12).
//!
//! At M1 the world has no cells in it. The tick order of SPEC §12 is already the real one —
//! sense, execute, resolve, physics, fluid, bookkeeping — with the first four steps empty,
//! so that M2 fills them in rather than restructuring the loop around them.
//!
//! Everything the world does to itself conserves matter exactly. The only ways matter can
//! move at all are the fluid solver, which exchanges across edges and therefore conserves by
//! construction, and the explicit seed/evict paths, which go through the ledger.

use std::sync::Arc;

use crate::biology::{self, BiologyConfig, BiologyReport, Intervention};
use crate::cell::{CellArena, CellId, CellSeed};
use crate::chem::CHEM_COUNT;
use crate::fluid;
use crate::genome::GenomePool;
use crate::intent::{IntentBuffer, Pending};
use crate::ledger::{Ledger, LedgerBreach};
use crate::light::{decay_impulses, CurrentField, LightRegime};
use crate::metabolism::MetabolicReport;
use crate::scenario::{Barrier, Scenario, ScenarioError, Seeding};
use crate::state_hash::{StateHash, StateHasher};
use crate::substrate::Substrate;

/// The world's narrative counters, restored from a snapshot together.
///
/// A struct rather than nine positional arguments: the list gained one at each of the last two
/// milestones, and nine numbers in a row is a call nobody can read and anybody can transpose.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RestoredStory {
    pub events: Vec<crate::events::Event>,
    pub window_population: u32,
    pub window_at: u64,
    pub generations: u32,
    pub dominant: Option<crate::phylogeny::SpeciesId>,
    pub births_total: u64,
    pub foreign_injections_total: u64,
    pub forced_joins_total: u64,
    pub wounds_total: u64,
}

/// A running simulation.
#[derive(Clone, Debug)]
pub struct World {
    scenario: Scenario,
    substrate: Substrate,
    ledger: Ledger,
    tick: u64,
    /// Momentum injected locally by cilia (M3), decaying each fluid step. Separate from the
    /// prescribed current because it is state, not configuration.
    impulse_x: Vec<i32>,
    impulse_y: Vec<i32>,
    /// What the cilia are doing to the water *right now*, one entry per square.
    ///
    /// Rebuilt from the population every physics phase and consumed by `refresh_velocity` in the
    /// same tick, so it never carries anything across a tick boundary — scratch on the same
    /// terms as `slip` and `crowding`, and excluded from equality, hashing and snapshots for the
    /// same reason. `impulse_x/y` above is the other kind: a disturbance that outlives what made
    /// it. `ECONOMY.md` §14 is why a cilium's reaction has to be this one.
    stir_x: Vec<i32>,
    stir_y: Vec<i32>,
    /// Cached so a static light regime is not rewritten over the whole grid every step.
    light_written: bool,
    /// Whether the velocity field is already correct for the current impulse layer.
    ///
    /// The prescribed current is time-invariant, so with no cilia pushing on the water the
    /// velocity field is written once and never again. Rewriting it every step — a quarter of
    /// a million squares, each a couple of divisions — cost more than the fluid solver it was
    /// feeding.
    velocity_written: bool,
    /// Whether anything stirred the water on the *previous* tick.
    ///
    /// Not simulation state and deliberately not serialised: it is one bit of history that says
    /// whether `velocity_written` can be trusted, and `restore` sets it conservatively. See the
    /// invalidation in [`World::step`] for what it is for, and `restore` for why `true` there is
    /// both safe and enough to keep hard rule 7.
    stir_active: bool,
    /// How many squares carry a non-zero impulse. Zero means the velocity field cannot have
    /// moved since it was last written.
    active_impulses: u32,
    /// Diffusion and flow-coupling per chemical, cached from the scenario's table.
    fluid_rates: fluid::FluidRates,
    /// Working buffer for the fluid solver. Not state: it holds nothing between steps, and
    /// is excluded from equality, hashing and snapshots for that reason.
    /// The species archive and the tree over it (SPEC §10).
    archive: crate::phylogeny::Phylogeny,
    /// The world's newspaper: first occurrences and mass extinctions (SPEC §10.6).
    events: crate::events::EventLog,
    /// Births since the run began. Feeds the replication detector and nothing else.
    births_total: u64,
    /// Genome bytes written into another cell's nucleus, and junctions forced against a key
    /// mismatch. Counted as they happen, because neither leaves a trace a later scan could
    /// find — the byte is just a byte, and a forced junction looks like any other.
    foreign_injections_total: u64,
    forced_joins_total: u64,
    /// Spike wounds dealt since the run began, for the predation detector.
    wounds_total: u64,
    /// Constraint list, reused by the junction solver so it does not allocate per tick.
    /// Scratch: rebuilt from the junctions every solve.
    constraints: Vec<(u32, u32, i32)>,
    /// Connected components over hard junctions — which cells are one organism (SPEC §8.4).
    /// Scratch: derived entirely from the junctions, rebuilt when asked, so it is excluded
    /// from equality and from the hash the way `scratch` and `radii` are.
    components: crate::junction::Components,
    /// Reused between censuses so counting species does not allocate every time.
    census: std::collections::BTreeMap<crate::phylogeny::SpeciesId, u32>,
    scratch: crate::fluid::FluidScratch,
    /// Per-cell radii, reused by collision separation so it does not allocate per tick.
    /// Scratch like `scratch`: excluded from equality and from the hash.
    radii: Vec<i32>,
    /// The separation solver's own working memory, for the same reason and on the same terms.
    separation: crate::neighbours::SeparationScratch,
    /// Every cell's photosynthetic and respiratory capacity, worked out in parallel before the
    /// metabolic loop reads them. Scratch on the same terms as the two above.
    capacities: Vec<crate::metabolism::Capacities>,
    /// Where each cell stops compressing, permille of its own radius, from its wall and its
    /// turgor. Scratch on the same terms as `radii`: derived fresh every tick from state that is
    /// itself saved, so it is excluded from equality, hashing and snapshots.
    core_permille: Vec<i32>,
    /// How hard each cell is being pressed by cells it is not joined to, `POS`, as of this
    /// tick's separation pass. Scratch in the same sense: recomputed from positions every
    /// tick, so it is excluded from equality and from the hash.
    crowding: Vec<i32>,
    /// How stuck each cell is, `Q10`, one unit per neighbour bottomed out on its core. Derived
    /// every physics phase like `crowding`, and read by division a phase later — a cell that was
    /// wedged last tick is still wedged, and the alternative is running the collision pass twice.
    pressure: Vec<i32>,
    /// How fast the water is going past each cell, `Q10`. Written by the physics phase and read
    /// by filtering a phase later. Scratch like `crowding` and `pressure`: derived every tick,
    /// so excluded from equality, hashing and snapshots.
    slip: Vec<i32>,
    /// What each cell's cilia and holdfasts add up to, gathered in parallel just before the
    /// physics phase reads it. Scratch on the same terms as `slip`: derived every tick from the
    /// organelle loadout, so excluded from equality, hashing and snapshots.
    body_scan: Vec<crate::sensing::BodyScan>,
    /// What each cell's spikes, holdfasts and lysosomes add up to, gathered in parallel just
    /// before the ecology phase reads it. Scratch, on the same terms as `body_scan`.
    ecology_scan: Vec<crate::ecology::EcologyScan>,

    /// The population.
    cells: CellArena,
    /// Interned genomes, shared across the whole population.
    genomes: GenomePool,
    /// Costs, rates and mutation for the living half of the world.
    ///
    /// Starts as the scenario's and stays equal to it until somebody changes it mid-run, at
    /// which point the change is recorded in `interventions`.
    biology: BiologyConfig,
    /// Parameter changes made while the world was running, oldest first (M10.2).
    ///
    /// A run is reproducible from `(scenario, seed)` — I1 — and changing a parameter at tick
    /// forty thousand breaks that unless the change is part of the record. So it is part of the
    /// record. The alternative was to forbid mid-run edits, which makes every balancing
    /// experiment a cold start, and this world is nowhere near balanced.
    interventions: Vec<Intervention>,
    /// This tick's intents. Cleared at the start of every execute phase.
    intents: IntentBuffer,
    /// Deaths and births decided during resolve, applied during bookkeeping.
    pending: Pending,
    /// Reused between ticks to avoid an allocation per tick.
    starving: Vec<CellId>,
    /// Who is next to whom. Rebuilt each tick before sensing, so touch readings and collision
    /// resolution see the same neighbourhood.
    neighbours: crate::neighbours::NeighbourIndex,
    /// What the last tick did, for metrics.
    last_report: TickReport,
}

/// What one tick did, for the instrumentation of SPEC §13.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TickReport {
    pub population: u32,
    pub biology: BiologyReport,
    pub metabolism: MetabolicReport,
    pub physics: crate::sensing::PhysicsReport,
    pub ecology: crate::ecology::EcologyReport,
}

// `scratch` and `radii` are scratchpads, so two worlds that differ only in what happens to be
// left in them are the same world. Deriving `PartialEq` would make the snapshot round-trip
// test fail for a reason that means nothing.
impl PartialEq for World {
    fn eq(&self, other: &Self) -> bool {
        self.scenario == other.scenario
            && self.substrate == other.substrate
            && self.ledger == other.ledger
            && self.tick == other.tick
            && self.impulse_x == other.impulse_x
            && self.impulse_y == other.impulse_y
            && self.cells == other.cells
            && self.archive == other.archive
            && self.events == other.events
            && self.births_total == other.births_total
            && self.foreign_injections_total == other.foreign_injections_total
            && self.forced_joins_total == other.forced_joins_total
            && self.wounds_total == other.wounds_total
            // Both, though one implies the other: `biology` is the founding scenario's with the
            // interventions replayed over it. Comparing the derived value as well as the record
            // it comes from is what catches a restore that has the history right and applies it
            // wrongly, which is the failure that would otherwise look like a correct file.
            && self.biology == other.biology
            && self.interventions == other.interventions
    }
}

impl Eq for World {}

/// Move one fluid step's worth of matter across the boundary of the slide.
///
/// Everything here goes through the ledger, in both currencies, because both invariants are
/// identities rather than approximations. Matter (I4): what `add_chem` reports as *actually*
/// moved is what is recorded, so a source aimed at a barrier, or at a square already at the
/// quantity cap, records what it managed rather than what it intended. Energy (I5): matter that
/// is a metabolic substrate carries its latent energy with it, and an inflow of sulphide that
/// did not say so would show up as stored energy nobody let in.
///
/// A free function rather than a method so that the scenario can be read while the substrate is
/// written.
fn apply_flux(
    flux: &[crate::Flux],
    substrate: &mut Substrate,
    ledger: &mut Ledger,
    chem: &crate::chem::ChemTable,
    metabolism: &crate::metabolism::Metabolism,
) {
    let (w, h) = (substrate.width(), substrate.height());
    // Clamped to the slide rather than wrapped. `Substrate::index` reduces modulo the grid, so
    // a rectangle running off the right-hand edge would otherwise reappear on the left and put
    // an inlet at the outflow.
    let bounds = |x: u32, y: u32, width: u32, height: u32| {
        (
            x.min(w),
            y.min(h),
            x.saturating_add(width).min(w),
            y.saturating_add(height).min(h),
        )
    };
    for f in flux {
        match f {
            crate::Flux::Source {
                chemical,
                x,
                y,
                width,
                height,
                per_tick,
            } => {
                if *per_tick <= 0 {
                    continue;
                }
                let c = chemical % CHEM_COUNT;
                let (x0, y0, x1, y1) = bounds(*x, *y, *width, *height);
                let mut moved = 0i64;
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        moved += i64::from(substrate.add_chem(c, xx as i32, yy as i32, *per_tick));
                    }
                }
                if moved > 0 {
                    ledger.record_injected(c, moved);
                    ledger.import(metabolism.latent_in(chem, c, moved));
                }
            }
            crate::Flux::Drain {
                chemical,
                x,
                y,
                width,
                height,
                rate,
            } => {
                if *rate <= 0 {
                    continue;
                }
                let c = chemical % CHEM_COUNT;
                let rate = (*rate).min(crate::fixed::Q10_ONE);
                let (x0, y0, x1, y1) = bounds(*x, *y, *width, *height);
                let mut moved = 0i64;
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        let here = substrate.chem_at(c, xx as i32, yy as i32);
                        // At least one unit where there is anything at all, so a drain at a low
                        // rate is an outflow rather than a rounding error that never fires.
                        let take = crate::fixed::q10_scale(here, rate).max(i32::from(here > 0));
                        moved -=
                            i64::from(substrate.add_chem(c, xx as i32, yy as i32, -take.min(here)));
                    }
                }
                if moved > 0 {
                    ledger.record_drained(c, moved);
                    ledger.export(metabolism.latent_in(chem, c, moved));
                }
            }
        }
    }
}

/// Where the `k`th cell of a square spiral sits, relative to its centre.
///
/// Lifted out of `place_founders_at` unchanged, so `Placement::At` rings a square exactly the way
/// `at: Some(..)` used to: index zero is the centre, and each subsequent ring is walked edge by
/// edge. A count above one has to land *beside* its predecessor rather than inside it.
fn ring_offset(k: u32) -> (i32, i32) {
    let ring = (0..).find(|r| (2 * r + 1) * (2 * r + 1) > k).unwrap_or(0);
    if ring == 0 {
        return (0, 0);
    }
    let side = 2 * ring + 1;
    let along = k.saturating_sub((2 * ring - 1) * (2 * ring - 1));
    let per = side - 1;
    let (edge, step) = (along / per, (along % per) as i32);
    let r = ring as i32;
    match edge {
        0 => (-r + step, -r),
        1 => (r, -r + step),
        2 => (r - step, r),
        _ => (-r, r - step),
    }
}

impl World {
    /// Build a world from a scenario: allocate the grid, raise the barriers, seed the
    /// chemistry, and take the ledger's baseline.
    ///
    /// Order matters. Barriers go up *before* seeding, so that a scenario cannot place
    /// matter inside a wall and have it evicted a moment later; and the baseline is taken
    /// last, so it describes what the world actually holds.
    ///
    /// # Errors
    ///
    /// Bad grid dimensions, or an ISA version this engine cannot honour.
    pub fn new(scenario: Scenario) -> Result<World, ScenarioError> {
        let biology = scenario.biology.clone();
        scenario.check_isa()?;
        let substrate =
            Substrate::new(scenario.width, scenario.height).map_err(ScenarioError::Substrate)?;
        let n = substrate.len();
        let fluid_rates = fluid::FluidRates {
            diffusion: scenario.chemicals.diffusion_rates(),
            advection: scenario.chemicals.advection_rates(),
        };

        let mut world = World {
            scenario,
            substrate,
            ledger: Ledger::new(),
            tick: 0,
            impulse_x: vec![0; n],
            impulse_y: vec![0; n],
            stir_x: vec![0; n],
            stir_y: vec![0; n],
            light_written: false,
            velocity_written: false,
            stir_active: false,
            active_impulses: 0,
            fluid_rates,
            archive: crate::phylogeny::Phylogeny::new(),
            events: crate::events::EventLog::new(),
            births_total: 0,
            foreign_injections_total: 0,
            forced_joins_total: 0,
            wounds_total: 0,
            constraints: Vec::new(),
            components: crate::junction::Components::new(),
            census: std::collections::BTreeMap::new(),
            scratch: crate::fluid::FluidScratch::new(n),
            radii: Vec::new(),
            separation: crate::neighbours::SeparationScratch::default(),
            capacities: Vec::new(),
            core_permille: Vec::new(),
            crowding: Vec::new(),
            pressure: Vec::new(),
            slip: Vec::new(),
            body_scan: Vec::new(),
            ecology_scan: Vec::new(),
            cells: CellArena::new(),
            genomes: GenomePool::new(),
            biology,
            interventions: Vec::new(),
            intents: IntentBuffer::new(),
            pending: Pending::default(),
            starving: Vec::new(),
            neighbours: crate::neighbours::NeighbourIndex::default(),
            last_report: TickReport::default(),
        };

        world.raise_barriers();
        world.seed_chemistry();
        world.ledger.set_baseline(world.total_matter());
        world.refresh_light();
        world.refresh_velocity();
        world.rebaseline_energy();
        Ok(world)
    }

    fn raise_barriers(&mut self) {
        for (x, y) in self.barrier_squares() {
            self.block(x, y);
        }
        // Once, now that every square is placed. `block` defers it.
        self.substrate.rebuild_edge_masks();
    }

    /// Every square the scenario's barrier list covers, shapes expanded.
    ///
    /// One expansion, because there were nearly two: the editing tools need to know which
    /// squares an authored shape covers in order to erase one out of it, and a second copy of
    /// "what does a `WallWithGap` mean" is a copy that drifts from this one.
    fn barrier_squares(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for b in &self.scenario.barriers {
            match b {
                Barrier::Square { x, y } => out.push((*x, *y)),
                Barrier::Rect {
                    x,
                    y,
                    width,
                    height,
                } => {
                    for dy in 0..*height {
                        for dx in 0..*width {
                            out.push((x.saturating_add(dx), y.saturating_add(dy)));
                        }
                    }
                }
                Barrier::WallWithGap {
                    at,
                    vertical,
                    gap_start,
                    gap_len,
                } => {
                    let span = if *vertical {
                        self.substrate.height()
                    } else {
                        self.substrate.width()
                    };
                    for i in 0..span {
                        if i >= *gap_start && i < gap_start.saturating_add(*gap_len) {
                            continue;
                        }
                        out.push(if *vertical { (*at, i) } else { (i, *at) });
                    }
                }
            }
        }
        out
    }

    /// Raise one scenario barrier square, deferring the edge-mask rebuild.
    ///
    /// `raise_barriers` rebuilds once when it has finished. This had the same cost as the
    /// drawing tool did — the archipelago blocks a few hundred squares and rebuilt a whole
    /// slide's masks for each of them — and nobody noticed only because it happens once, at
    /// construction, before there is anything to watch.
    fn block(&mut self, x: u32, y: u32) {
        if x >= self.substrate.width() || y >= self.substrate.height() {
            return;
        }
        let evicted = self
            .substrate
            .set_blocked_deferred(x as i32, y as i32, true);
        self.ledger.record_evicted(&evicted);
    }

    /// One fluid step's worth of matter across the edge of the slide. See [`crate::Flux`].
    fn step_flux(&mut self) {
        if self.scenario.flux.is_empty() {
            return;
        }
        // Destructured rather than called through `&mut self`: the borrow checker's objection
        // is to the whole of `self` at once, and these are four disjoint fields.
        let World {
            scenario,
            substrate,
            ledger,
            biology,
            ..
        } = self;
        apply_flux(
            &scenario.flux,
            substrate,
            ledger,
            &scenario.chemicals,
            &biology.metabolism,
        );
    }

    fn seed_chemistry(&mut self) {
        let seeding = self.scenario.seeding.clone();
        let w = self.substrate.width();
        let h = self.substrate.height();
        for s in &seeding {
            match s {
                Seeding::Uniform {
                    chemical,
                    per_square,
                } => {
                    for y in 0..h as i32 {
                        for x in 0..w as i32 {
                            self.substrate.add_chem(*chemical, x, y, *per_square);
                        }
                    }
                }
                Seeding::Gradient {
                    chemical,
                    low,
                    high,
                    horizontal,
                } => {
                    for y in 0..h as i32 {
                        for x in 0..w as i32 {
                            let (num, den) = if *horizontal {
                                (x as i64, (w.saturating_sub(1)).max(1) as i64)
                            } else {
                                (y as i64, (h.saturating_sub(1)).max(1) as i64)
                            };
                            let v = *low as i64 + (*high as i64 - *low as i64) * num / den;
                            self.substrate
                                .add_chem(*chemical, x, y, crate::fixed::sat_i32(v));
                        }
                    }
                }
                Seeding::Spike {
                    chemical,
                    x,
                    y,
                    amount,
                } => {
                    self.substrate
                        .add_chem(*chemical, *x as i32, *y as i32, *amount);
                }
                Seeding::Patch {
                    chemical,
                    x,
                    y,
                    width,
                    height,
                    per_square,
                } => {
                    for dy in 0..*height {
                        for dx in 0..*width {
                            self.substrate.add_chem(
                                *chemical,
                                x.saturating_add(dx) as i32,
                                y.saturating_add(dy) as i32,
                                *per_square,
                            );
                        }
                    }
                }
            }
        }
        // Seeding is the world being given its contents, not the world creating matter, so
        // the baseline is taken afterwards rather than each placement being "injected".
    }

    #[inline]
    #[must_use]
    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    #[inline]
    #[must_use]
    pub fn substrate(&self) -> &Substrate {
        &self.substrate
    }

    #[inline]
    pub fn substrate_mut(&mut self) -> &mut Substrate {
        &mut self.substrate
    }

    #[inline]
    #[must_use]
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    #[inline]
    pub fn ledger_mut(&mut self) -> &mut Ledger {
        &mut self.ledger
    }

    #[inline]
    #[must_use]
    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    /// How wedged each cell is, `Q10`. See [`crate::neighbours::resolve_collisions`].
    ///
    /// Snapshot state rather than scratch, and the distinction cost a failed round-trip test to
    /// notice: it is written by the physics phase and read by *the next tick's* division, so a
    /// world restored without it lets through the first round of divisions the original refused.
    #[must_use]
    pub fn pressure(&self) -> &[i32] {
        &self.pressure
    }

    /// What the cilia are doing to the water this tick, one entry per square.
    ///
    /// Rebuilt from the population every physics phase, so unlike [`World::impulses`] it holds
    /// what is happening rather than what happened. See the field for why a cilium's reaction
    /// has to be the first kind.
    #[must_use]
    pub fn stir(&self) -> (&[i32], &[i32]) {
        (&self.stir_x, &self.stir_y)
    }

    pub fn impulses(&self) -> (&[i32], &[i32]) {
        (&self.impulse_x, &self.impulse_y)
    }

    /// Inject momentum at a square. This is the API cilia will use at M3.
    ///
    /// Momentum is not matter and is not conserved — it decays, which is what stops one flick
    /// of one cilium from stirring the slide forever.
    pub fn inject_impulse(&mut self, x: i32, y: i32, dx: i32, dy: i32) {
        let i = self.substrate.index(x, y);
        if self.substrate.blocked()[i] {
            return;
        }
        let limit = crate::fixed::Q10_ONE;
        let was = self.impulse_x[i] != 0 || self.impulse_y[i] != 0;
        self.impulse_x[i] =
            (self.impulse_x[i] as i64 + dx as i64).clamp(-(limit as i64), limit as i64) as i32;
        self.impulse_y[i] =
            (self.impulse_y[i] as i64 + dy as i64).clamp(-(limit as i64), limit as i64) as i32;
        let now = self.impulse_x[i] != 0 || self.impulse_y[i] != 0;
        if now && !was {
            self.active_impulses = self.active_impulses.saturating_add(1);
        } else if was && !now {
            self.active_impulses = self.active_impulses.saturating_sub(1);
        }
        self.velocity_written = false;
    }

    /// Advance one tick, in the fixed order of SPEC §12.
    pub fn step(&mut self) {
        let seed = self.scenario.seed;
        let tick = self.tick;
        let mut report = TickReport::default();

        if !self.cells.is_empty() {
            // 1. Sense. Light and chemistry are already in the substrate and nothing has moved
            //    since the last tick ended, so the execute phase reads those directly and
            //    read-only. What does have to be gathered is who is next to whom, because
            //    answering that by walking the population would be quadratic.
            self.neighbours
                .rebuild(&self.cells, self.substrate.width(), self.substrate.height());
            //    And what each cell can feel, which is the same question asked once per cell
            //    rather than once per sensor read. Nothing it depends on moves again until
            //    execute is over — see `gather_touch`.
            self.neighbours.gather_touch(&self.cells);

            // 2. Execute. Each cell runs its instruction budget and emits intents. No cell
            //    writes shared state, so no cell can observe another's turn.
            self.intents.begin_tick(self.cells.capacity());
            biology::execute(
                &mut self.cells,
                &self.substrate,
                &self.neighbours,
                &mut self.intents,
                &self.scenario.vm,
                tick,
                seed,
                self.biology.ecology.spike_damage,
                self.biology.ecology.em_range,
                self.biology.metabolism.catalogue.metabolism,
            );

            // 2b. Last tick's spending has been read by everything that was going to read it,
            //     and this tick's has not started. See `CellArena::emission`.
            self.cells.clear_emission();

            // 3. Resolve. Intents applied in slot order, which is cell-id order, so a
            //    contested square is allocated the same way on every machine.
            report.biology = biology::resolve(
                &mut self.cells,
                &mut self.substrate,
                &self.genomes,
                &self.intents,
                &self.biology,
                &self.scenario.chemicals,
                &mut self.ledger,
                &mut self.pending,
                &self.pressure,
                tick,
                seed,
            );

            // 4. Physics: thrust, Brownian jitter, drag and integration. Cells push on the
            //    water here, so it is sequential and in slot order like resolve.
            // The organelle sums the physics phase needs, on every core, before the phase that
            // needs them runs on one. See `sensing::BodyScan`.
            crate::sensing::scan_bodies_into(&self.cells, &mut self.body_scan);
            report.physics = crate::sensing::step_physics(
                &mut self.cells,
                &self.substrate,
                &mut self.stir_x,
                &mut self.stir_y,
                crate::sensing::BodyForces {
                    jitter: self.scenario.jitter,
                    gravity: self.scenario.gravity,
                },
                &mut self.slip,
                &self.body_scan,
                tick,
                seed,
            );
            // A cilium that pushed on the water changed the velocity field. Nothing else here
            // did.
            //
            // `report.physics.moved` used to be in this condition and is not a reason to
            // rewrite the field: it is the population's *total travel*, so Brownian jitter
            // alone made it non-zero every tick on every populated slide. The whole velocity
            // field was therefore rebuilt every fluid step on worlds with no cilia at all —
            // 0.59ms a tick at fifty thousand cells, spent writing back the numbers that were
            // already there. `PhysicsReport::stirred` is the flag meant for this and says so in
            // its own doc comment.
            //
            // It needs one bit of history beside it. `step_physics` refills the stir layer from
            // scratch every tick (`stir_x.fill(0)`), so the tick *after* the last cilium stops
            // beating is the one that zeroes the layer — a real change to the field, with
            // `stirred` already back at zero. Without `stir_active` the last thing that swam
            // would leave its current behind it forever, which is the same fault the empty-slide
            // branch below exists to prevent.
            let stirring = report.physics.energy_spent != 0 || report.physics.stirred != 0;
            if stirring || self.stir_active {
                self.velocity_written = false;
                self.active_impulses = self
                    .impulse_x
                    .iter()
                    .zip(self.impulse_y.iter())
                    .filter(|(x, y)| **x != 0 || **y != 0)
                    .count() as u32;
            }
            self.stir_active = stirring;
            if report.physics.energy_spent > 0 {
                self.ledger.dissipate(report.physics.energy_spent);
            }
            // 4b. Junctions (SPEC §8.4). Broken ends first, so the solver never pulls on a
            //     junction to a cell that is not there, then the distance constraints.
            //
            //     After physics and before collision separation, deliberately: cilia have
            //     already pushed the cells they are attached to, and the constraints now drag
            //     the rest. That ordering is the whole of "colony locomotion is emergent" —
            //     nothing in the engine moves a cluster as a unit.
            //     Solve first, then break what is left over. The other order looks natural —
            //     tidy up, then do the work — and is wrong: physics has just moved every cell,
            //     so a junction is routinely overstretched at this instant and the solver
            //     exists precisely to pull it back. Pruning first broke junctions the solver
            //     would have saved, and a cluster with one cilium tore itself apart within a
            //     few hundred ticks.
            //
            //     Breaking strain means "the constraint could not hold it", not "it was
            //     briefly stretched".
            report.physics.constraints = crate::junction::solve(
                &mut self.cells,
                &self.biology.junctions,
                &mut self.constraints,
            );
            report.physics.junctions_broken =
                crate::junction::prune(&mut self.cells, &self.biology.junctions);

            // Cells occupy space, so a crowded patch is crowded and there is a reason to
            // leave it. Rebuilt first because everything just moved.
            self.neighbours
                .rebuild(&self.cells, self.substrate.width(), self.substrate.height());
            // And barriers occupy space too. Passed as an empty slice when the slide has none,
            // so a scenario without barriers does not pay for the scan.
            let blocked: &[bool] = if self.substrate.has_barriers() {
                self.substrate.blocked()
            } else {
                &[]
            };
            report.physics.separated = crate::neighbours::resolve_collisions(
                &mut self.cells,
                &mut self.neighbours,
                &mut self.radii,
                &mut self.core_permille,
                &mut self.crowding,
                &mut self.pressure,
                blocked,
                &self.biology.metabolism.rates,
                self.biology.separation_relax,
                &mut self.separation,
            );
        } else {
            // Nobody is beating, and `step_physics` — which is what rebuilds the stir layer
            // every tick — did not run to say so. Without this the last thing that swam leaves
            // its current behind it forever, which is exactly what `stir` exists not to do.
            // The scan short-circuits on the first non-zero and then costs one pass of zeroes
            // per tick over a slide with nothing alive on it.
            if self.stir_x.iter().chain(self.stir_y.iter()).any(|v| *v != 0) {
                self.stir_x.fill(0);
                self.stir_y.fill(0);
                self.velocity_written = false;
            }
        }

        // 5. Fluid, at fluid_hz.
        let interval = self.scenario.fluid_interval.max(1) as u64;
        if self.tick.is_multiple_of(interval) {
            self.refresh_light();
            self.refresh_velocity();
            // Matter across the boundary before the water moves it, so a source at the head of
            // a channel is carried away on the same step it arrives rather than sitting in the
            // inlet for one.
            self.step_flux();
            fluid::step(&mut self.substrate, &self.fluid_rates, &mut self.scratch);
            self.decay_fluid();
            if self.active_impulses > 0 {
                decay_impulses(
                    &mut self.impulse_x,
                    &mut self.impulse_y,
                    self.scenario.impulse_retain,
                );
                self.active_impulses = self
                    .impulse_x
                    .iter()
                    .zip(self.impulse_y.iter())
                    .filter(|(x, y)| **x != 0 || **y != 0)
                    .count() as u32;
                self.velocity_written = false;
            }
        }

        // 6. Bookkeeping: metabolism, deaths, births, metrics.
        if !self.cells.is_empty() {
            self.starving.clear();
            report.metabolism = self.biology.metabolism.step(
                &mut self.cells,
                &self.substrate,
                &self.scenario.chemicals,
                &mut self.ledger,
                &mut self.starving,
                &self.pressure,
                &mut self.capacities,
            );
            self.pending.deaths.append(&mut self.starving);
            // 6b. Ecology: spikes wound, lysosomes digest (M8). Before deaths, so a cell
            //     wounded past its limit dies this tick rather than next — and after the
            //     neighbour index was rebuilt by the physics phase, so "touching" means where
            //     things are now.
            // As the physics phase does: the organelle sums first, on every core.
            crate::ecology::scan_into(&self.cells, &mut self.ecology_scan);
            report.ecology = crate::ecology::step(
                &mut self.cells,
                &mut self.substrate,
                &self.neighbours,
                &self.crowding,
                &self.slip,
                &self.biology.ecology,
                &self.biology.metabolism.catalogue.metabolism,
                &mut self.ledger,
                &self.ecology_scan,
            );

            let dead = biology::apply_deaths(
                &mut self.cells,
                &mut self.substrate,
                &self.biology,
                &mut self.ledger,
                &mut self.pending,
            );
            report.biology.deaths = dead.deaths;
            report.biology.to_carrion = dead.to_carrion;
            report.biology.births = biology::apply_births(
                &mut self.cells,
                &self.genomes,
                &mut self.pending,
                &mut self.archive,
                tick,
                seed,
            );
            self.births_total = self
                .births_total
                .saturating_add(u64::from(report.biology.births));
            self.foreign_injections_total = self
                .foreign_injections_total
                .saturating_add(u64::from(report.biology.foreign_injections));
            self.forced_joins_total = self
                .forced_joins_total
                .saturating_add(u64::from(report.biology.junctions_forced));
            self.wounds_total = self
                .wounds_total
                .saturating_add(u64::from(report.ecology.wounded));
        }

        // 7. The story: who is alive, who just stopped, and anything happening for the first
        //    time. Read-only over the world — nothing here can change a tick's outcome, which
        //    is why it runs last and why it may be sampled rather than run in full.
        self.observe(tick);

        report.population = self.cells.len() as u32;
        self.last_report = report;

        self.tick = self.tick.saturating_add(1);
        debug_assert!(
            self.ledger.check_energy().is_ok(),
            "energy accounting broke at tick {}",
            self.tick
        );
    }

    /// Turn unstable species in the water into what they decay to (SPEC §12, step 5).
    ///
    /// A balanced reaction like any other, so it goes through the ledger and the per-species
    /// claim stays exact. Without it a byproduct excreted into the water would be a permanent
    /// matter sink, and a world that respires would slowly turn into its own exhaust.
    fn decay_fluid(&mut self) {
        for c in 0..CHEM_COUNT {
            let def = self.scenario.chemicals.get(c);
            let (Some(into), rate) = (def.decay_to, def.decay_rate) else {
                continue;
            };
            let into = into % CHEM_COUNT;
            if rate <= 0 || into == c || !self.substrate.present()[c] {
                continue;
            }
            let moved = self.substrate.decay_plane(c, into, rate);
            if moved > 0 {
                self.ledger.convert(c, into, moved);
            }
        }
    }

    /// Total of each chemical over every compartment: fluid, cell interiors and cell mass.
    ///
    /// This is what I4 is about. Structural mass counts — matter built into a body has not
    /// left the world, it has left the pool the fluid can reach.
    #[must_use]
    pub fn total_matter(&self) -> [i64; CHEM_COUNT] {
        let fluid = self.substrate.total_chem();
        let interiors = self.cells.total_interior();
        let sc = self.biology.structural_chemical % CHEM_COUNT;
        let mut out: [i64; CHEM_COUNT] = std::array::from_fn(|c| fluid[c] + interiors[c]);
        for i in self.cells.iter() {
            out[sc] = out[sc].saturating_add(self.cells.mass[i] as i64);
        }
        out
    }

    /// Adopt the world's current stored energy as the ledger's baseline.
    ///
    /// What is in the world at tick zero was not absorbed from anywhere, so it counts as both
    /// `energy_in` and `energy_stored` and the identity starts balanced.
    fn rebaseline_energy(&mut self) {
        let stored = crate::metabolism::recompute_stored(
            &self.cells,
            &self.substrate,
            &self.scenario.chemicals,
            &self.biology.metabolism,
        );
        self.ledger.set_energy_baseline(stored);
    }

    /// Who is next to whom, as of the last time anything moved.
    ///
    /// Exposed so the renderer can ask which neighbours a cell is squashed against without
    /// walking the population itself. Read-only, like everything else the front end gets: the
    /// index is derived from positions and rebuilt by the tick, so there is nothing here to
    /// write back through.
    #[inline]
    #[must_use]
    pub fn neighbours(&self) -> &crate::neighbours::NeighbourIndex {
        &self.neighbours
    }

    #[inline]
    #[must_use]
    pub fn cells(&self) -> &CellArena {
        &self.cells
    }

    #[inline]
    pub fn cells_mut(&mut self) -> &mut CellArena {
        &mut self.cells
    }

    #[inline]
    #[must_use]
    pub fn genomes(&self) -> &GenomePool {
        &self.genomes
    }

    #[inline]
    #[must_use]
    pub fn biology(&self) -> &BiologyConfig {
        &self.biology
    }

    /// Change the parameters the living half of the world runs on.
    ///
    /// Before the first tick this is scenario setup, so the scenario is updated to match: a
    /// world whose parameters differ from the scenario that describes it is the bug M10.2
    /// exists to fix, and it would be a poor showing to reintroduce it through the setter.
    ///
    /// After the first tick this is an *intervention* — a hand reaching into a running world —
    /// and it is recorded as one, so that `(scenario, seed, interventions)` still reproduces
    /// the run exactly and the timeline can say when somebody changed their mind.
    pub fn set_biology(&mut self, config: BiologyConfig) {
        if self.biology == config {
            return;
        }
        if self.tick == 0 {
            self.scenario.biology = config.clone();
        } else {
            self.interventions.push(Intervention {
                tick: self.tick,
                biology: config.clone(),
            });
        }
        self.biology = config;
    }

    /// Every parameter change made to this world while it was running, oldest first.
    ///
    /// The experiment log. Replaying the founding scenario's parameters and then these, in
    /// order, gives the configuration the world is running on now — which is how a snapshot
    /// restores it without storing it twice.
    #[inline]
    #[must_use]
    pub fn interventions(&self) -> &[Intervention] {
        &self.interventions
    }

    /// Restore a recorded intervention list, when resuming a snapshot.
    ///
    /// Applies the last one, since that is by definition the configuration in force.
    pub fn restore_interventions(&mut self, interventions: Vec<Intervention>) {
        if let Some(last) = interventions.last() {
            self.biology = last.biology.clone();
        }
        self.interventions = interventions;
    }

    /// What the last tick did.
    #[inline]
    #[must_use]
    pub fn report(&self) -> TickReport {
        self.last_report
    }

    /// Adopt whatever the world currently holds as the ledger's baseline.
    ///
    /// **Scenario setup only.** Placing a cell and then filling its cytoplasm by hand creates
    /// matter, which is correct at setup — the world is being given its contents — and a bug
    /// at any other time. Calling this mid-run would paper over exactly what I4 exists to
    /// catch, so it is a deliberate, named act rather than something that happens quietly.
    pub fn adopt_current_contents_as_baseline(&mut self) {
        self.ledger.set_baseline(self.total_matter());
        self.rebaseline_energy();
    }

    /// Place a cell on the slide and rebalance the energy baseline around it.
    ///
    /// # Errors
    ///
    /// A genome longer than the addressing limit.
    /// Spawn a cell that founds its own species even if an identical genome already has one.
    ///
    /// For arena mode, where the two sides may enter the same genome and must stay two teams.
    /// Everything else should use [`World::spawn_cell`], which merges — twelve seedings of one
    /// ancestor are one species, not twelve rivals.
    pub fn spawn_cell_as_new_species(&mut self, seed: CellSeed) -> CellId {
        let genome = Arc::clone(&seed.genome);
        let id = self.cells.spawn(seed);
        if let Some(i) = self.cells.index(id) {
            let traits = crate::names::Traits::of(self.cells.slots(i), genome.len());
            let species = self.archive.found_distinct(&genome, traits, self.tick);
            self.cells.species[i] = species;
        }
        self.ledger.set_baseline(self.total_matter());
        self.rebaseline_energy();
        id
    }

    pub fn spawn_cell(&mut self, seed: CellSeed) -> CellId {
        let genome = Arc::clone(&seed.genome);
        let id = self.cells.spawn(seed);
        // A seeded cell founds a species, or joins the one already founded for its genome —
        // twelve founders of one ancestor are one species, not twelve rivals. Its traits are
        // read after the caller has finished dressing it, so this registers the species now
        // and `observe` corrects the loadout once the cell has organelles.
        if let Some(i) = self.cells.index(id) {
            let traits = crate::names::Traits::of(self.cells.slots(i), genome.len());
            let species = self.archive.found(&genome, traits, self.tick);
            self.cells.species[i] = species;
        }
        self.ledger.set_baseline(self.total_matter());
        self.rebaseline_energy();
        id
    }

    /// Place `count` founders of one genome, spread over the slide. Returns how many landed.
    ///
    /// Both front ends had their own copy of this and the copies had drifted: `mm-cli` placed
    /// founders on a `w / (across + 1)` lattice with an empty cytoplasm, `mm-app` on a centred
    /// one with build material in it. The centred lattice is the better of the two — one
    /// founder lands in the middle of the slide rather than a quarter of the way into it — and
    /// the build material is not a nicety: without structural matter in the cytoplasm every
    /// `BUILD` is silently skipped, so a seeded cell runs the organelles it was given forever
    /// and a genome that works looks like a genome that does not.
    ///
    /// Spread rather than piled, and not only to look even. Sixteen founders far enough apart
    /// to grow independently for a while are sixteen experiments; sixteen in a heap are one,
    /// because they interbreed and compete from the first tick.
    ///
    /// The starting loadout is a policy and it is stated here rather than at each call site: a
    /// founder gets the organelles its build gene would otherwise spend many ticks affording,
    /// because bootstrapping from a bare membrane is a scenario in its own right and not
    /// something every run should have to survive first.
    ///
    /// Adopts the result as the ledger's baseline, because dressing a cytoplasm by hand adds
    /// matter to the world — that is what setting a scenario up means, and it is said out loud
    /// here rather than left for the conservation check to trip over.
    pub fn place_founders(&mut self, genome: &[u8], count: u32) -> u32 {
        self.place_founders_at(genome, count, None)
    }

    /// The same, at a named square rather than spread over the slide.
    ///
    /// `at` is where the *first* one goes; any beyond that spiral outward from it, because a
    /// count of four dropped on one square is one cell and three refusals. `None` is the grid
    /// spread [`World::place_founders`] describes.
    pub fn place_founders_at(&mut self, genome: &[u8], count: u32, at: Option<(u32, u32)>) -> u32 {
        let place = match at {
            Some((x, y)) => crate::Placement::At { x, y },
            None => crate::Placement::Spread,
        };
        self.place_inhabitants(genome, count, place)
    }

    /// Place founders in an arrangement, skipping the walls.
    ///
    /// The one implementation. Both front ends had their own copy of the old two-case version and
    /// **both of them dropped `Inhabitant::at` on the floor**, so a scenario that said where its
    /// inhabitants went was ignored by everything that could have read it. That is the drift the
    /// `place_founders` note below complains about, happening a second time, so the arrangement
    /// logic lives here now and the front ends pass a [`crate::Placement`] through.
    ///
    /// Returns how many actually landed, which may be fewer than asked for: a rectangle that is
    /// mostly wall has fewer free squares than a scenario may have requested, and reporting the
    /// shortfall is better than silently stacking founders on top of one another.
    pub fn place_inhabitants(
        &mut self,
        genome: &[u8],
        count: u32,
        place: crate::Placement,
    ) -> u32 {
        let mut taken = Vec::new();
        self.place_run(genome, count, place, 0, count, &mut taken)
    }

    /// Place several genomes over one region as a **single cohort**, sharing the slot sequence.
    ///
    /// # The bug this exists for
    ///
    /// A slot is a pure function of the placement, the index `k`, the total count and the seed —
    /// there is nothing in it that says *which* inhabitant is being placed. So two entries with
    /// the same placement and the same count were placed on **exactly the same squares**: the
    /// lattices coincide, and `Scatter` draws from `RandCtx::new(seed, k, 0)`, which is the same
    /// stream for both. Two species seeded into one region came out as one pile of pairs.
    ///
    /// It was unreachable until now — no shipped scenario has ever named two inhabitants — which
    /// is why it survived, and also why fixing it moves no acceptance number in the tree.
    ///
    /// The fix is not a per-inhabitant salt, which would only scatter them differently rather
    /// than fairly. `k` runs across the *whole cohort* and the lattice is sized to the whole
    /// cohort, so four of one genome and four of another are eight slots of one arrangement,
    /// interleaved. `taken` is shared too, so `Scatter`'s spacing holds between species and not
    /// only within one.
    ///
    /// Returns how many of each landed, in the order given.
    pub fn place_cohort(
        &mut self,
        members: &[(&[u8], u32)],
        place: crate::Placement,
    ) -> Vec<u32> {
        let total: u32 = members.iter().map(|(_, n)| *n).sum();
        let mut taken = Vec::new();
        let mut first = 0;
        let mut out = Vec::with_capacity(members.len());
        for (genome, count) in members {
            out.push(self.place_run(genome, *count, place, first, total, &mut taken));
            first += *count;
        }
        out
    }

    /// One member of a cohort: `k` runs `first..first + count` out of `total`.
    fn place_run(
        &mut self,
        genome: &[u8],
        count: u32,
        place: crate::Placement,
        first: u32,
        total: u32,
        taken: &mut Vec<(i32, i32)>,
    ) -> u32 {
        if count == 0 {
            return 0;
        }
        let structural = self.biology.structural_chemical;
        // Sized here, from the bytes, before `genome` is shadowed by the interned `Arc`. See
        // where it is used below for why it is not the fixed 40 it was.
        let nucleus = crate::biology::nucleus_param_for(genome.len()).max(40);
        let (w, h) = (
            self.substrate.width() as i32,
            self.substrate.height() as i32,
        );
        let seed = self.scenario.seed;
        let mut placed = 0;
        for k in first..first.saturating_add(count) {
            let Ok(genome) = self.genomes.intern(genome.to_vec()) else {
                continue;
            };
            let Some((px, py)) = self.slot(place, k, total, w, h, seed, taken) else {
                continue;
            };
            let id = self.spawn_cell(CellSeed {
                x: px,
                y: py,
                mass: crate::fixed::q10(30),
                energy: crate::fixed::q10(400),
                membrane: 24,
                key: 11,
                badge: 0,
                species: 0,
                parent: CellId::NONE,
                birth_tick: self.tick,
                genome,
            });
            let Some(i) = self.cells.index(id) else {
                continue;
            };
            // Big enough to hold the genome it is being handed, and never smaller than the 40
            // this was fixed at.
            //
            // A fixed 40 is 320 bytes, and **six of the twenty-one genomes in `genomes/` are
            // longer than that** — `stalker.mm` is 596. `CellHost::bud` returns zero when the
            // genome will not fit the nucleus, so those founders could not divide *at all* until
            // their own `#build` finished upgrading the nucleus, and a hunter that is paying for
            // a spike and two cilia may never afford to. It presents as a hand-placed organism
            // that hunts well, never breeds, and dies — which is what the microscope showed.
            //
            // The kit exists because "bootstrapping from a bare membrane is a scenario in its own
            // right and not something every run should have to survive first", and a nucleus that
            // silently forbids reproduction fails that for exactly the long genomes that need the
            // help most. Sizing it to the genome is the same policy applied properly.
            //
            // `max` rather than an exact fit, so that every genome which already fitted gets the
            // nucleus it has always been given and no result taken on one moves. Saturating into
            // a `u8` bounds it at 2,040 bytes, past which `bud` refuses and the inspector says so.
            self.cells.slots_mut(i)[1] = crate::organelle::Organelle::finished(
                crate::organelle::OrganelleType::Nucleus,
                nucleus,
            );
            self.cells.slots_mut(i)[2] = crate::organelle::Organelle::finished(
                crate::organelle::OrganelleType::Mitochondrion,
                50,
            );
            self.cells.slots_mut(i)[3] = crate::organelle::Organelle::finished(
                crate::organelle::OrganelleType::Chloroplast,
                60,
            );
            self.cells.interior_mut(i)[structural] = crate::fixed::q10(200);
            self.cells.interior_mut(i)[11] = crate::fixed::q10(40);
            self.cells.interior_mut(i)[14] = crate::fixed::q10(40);
            placed += 1;
        }
        self.adopt_current_contents_as_baseline();
        placed
    }

    /// Where the `k`th founder of `count` goes, in `POS`, or `None` if there is nowhere for it.
    #[allow(clippy::too_many_arguments)]
    fn slot(
        &self,
        place: crate::Placement,
        k: u32,
        count: u32,
        w: i32,
        h: i32,
        seed: u64,
        taken: &mut Vec<(i32, i32)>,
    ) -> Option<(i32, i32)> {
        use crate::fixed::{pos, POS_ONE};
        let centre = |sx: i32, sy: i32| (pos(sx) + POS_ONE / 2, pos(sy) + POS_ONE / 2);
        // A lattice of `n` slots across `extent`, each in the middle of its own share of it.
        let lattice = |n: u32, extent: i32, i: u32| extent * (2 * i as i32 + 1) / (2 * n.max(1) as i32);
        // Columns for `Spread`: a square lattice, unchanged, because every acceptance number in
        // the tree was taken on it.
        let across = {
            let mut a = 1u32;
            while a * a < count {
                a += 1;
            }
            a
        };
        // Columns for a rectangle: shaped to the rectangle rather than always square.
        //
        // `Grid` over an 80x2 strip along a channel wall wants sixteen columns and one row, and a
        // square lattice gives it four of each — which stacks the founders in pairs and puts half
        // of them off the strip. Columns go as `sqrt(count * width / height)`, integer, clamped
        // to at least one and at most `count`, so a square rectangle still comes out square.
        let shaped = |rw: i32, rh: i32| -> u32 {
            if rh <= 0 {
                return count.max(1);
            }
            let want = (count as i64 * rw.max(1) as i64) / rh as i64;
            let mut a = 1u32;
            while (a as i64) * (a as i64) < want {
                a += 1;
            }
            a.clamp(1, count.max(1))
        };

        let (x, y) = match place {
            // Unchanged, to the position: every acceptance number in the tree was taken on this
            // lattice, and it is expressed in `POS` rather than as a square centre because that
            // is what it has always been.
            crate::Placement::Spread => (
                pos(lattice(across, w, k % across)),
                pos(lattice(across, h, k / across)),
            ),
            crate::Placement::At { x, y } => {
                let (dx, dy) = ring_offset(k);
                centre(
                    (x as i32 + dx).clamp(0, w - 1),
                    (y as i32 + dy).clamp(0, h - 1),
                )
            }
            crate::Placement::Grid {
                x,
                y,
                width,
                height,
            } => {
                let (rw, rh) = (width.max(1) as i32, height.max(1) as i32);
                let cols = shaped(rw, rh);
                let rows = count.div_ceil(cols).max(1);
                centre(
                    x as i32 + lattice(cols, rw, k % cols),
                    y as i32 + lattice(rows, rh, k / cols),
                )
            }
            crate::Placement::Hex {
                x,
                y,
                width,
                height,
            } => {
                let (rw, rh) = (width.max(1) as i32, height.max(1) as i32);
                let cols = shaped(rw, rh);
                let rows = count.div_ceil(cols).max(1);
                let (col, row) = (k % cols, k / cols);
                // Every other row shifted half a column step, which is the whole of a hex
                // lattice: the rows themselves are unchanged.
                let step = rw / cols.max(1) as i32;
                let offset = if row % 2 == 1 { step / 2 } else { 0 };
                centre(
                    x as i32 + lattice(cols, rw, col) + offset,
                    y as i32 + lattice(rows, rh, row),
                )
            }
            crate::Placement::Scatter {
                x,
                y,
                width,
                height,
                spacing,
            } => {
                let ctx = crate::rng::RandCtx::new(seed, u64::from(k), 0);
                let (rw, rh) = (width.max(1) as i32, height.max(1) as i32);
                // Bounded dart throwing. Sixty-four attempts, then the last candidate whatever
                // it is — a rectangle too small for the spacing asked for should crowd rather
                // than lose founders, and an unbounded search would not terminate at all.
                let mut chosen = None;
                for attempt in 0..64u64 {
                    let sx = x as i32
                        + ctx.draw_below(crate::rng::Purpose::Placement, attempt * 2, rw as u64)
                            as i32;
                    let sy = y as i32
                        + ctx.draw_below(crate::rng::Purpose::Placement, attempt * 2 + 1, rh as u64)
                            as i32;
                    chosen = Some((sx, sy));
                    let far_enough = spacing == 0
                        || taken.iter().all(|(tx, ty)| {
                            (tx - sx).abs() >= spacing as i32 || (ty - sy).abs() >= spacing as i32
                        });
                    if far_enough && !self.substrate.is_blocked(sx, sy) {
                        break;
                    }
                }
                let (sx, sy) = chosen?;
                taken.push((sx, sy));
                centre(sx.clamp(0, w - 1), sy.clamp(0, h - 1))
            }
        };

        self.off_the_walls(x.clamp(0, pos(w) - 1), y.clamp(0, pos(h) - 1), w, h)
    }

    /// The nearest free square to a position, or `None` if everything within reach is wall.
    ///
    /// A founder placed inside a barrier is a founder that cannot move, cannot divide into any
    /// direction that is not also wall, and is usually dead — and it is invisible in a scenario
    /// file, because the file says a count and a rectangle and not which squares those turned out
    /// to be. So the arrangement is a request and this is what honours it.
    fn off_the_walls(&self, x: i32, y: i32, w: i32, h: i32) -> Option<(i32, i32)> {
        use crate::fixed::{pos, pos_to_square, POS_ONE};
        let (sx, sy) = (pos_to_square(x), pos_to_square(y));
        if !self.substrate.is_blocked(sx, sy) {
            return Some((x, y));
        }
        // Outward in square rings, and in a fixed order within each ring, so which free square a
        // displaced founder lands on is a property of the scenario and not of anything else.
        for radius in 1..=8i32 {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx.abs() != radius && dy.abs() != radius {
                        continue;
                    }
                    let (nx, ny) = (sx + dx, sy + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    if !self.substrate.is_blocked(nx, ny) {
                        return Some((pos(nx) + POS_ONE / 2, pos(ny) + POS_ONE / 2));
                    }
                }
            }
        }
        None
    }

    /// Connected components over hard junctions, rebuilt from the current junctions.
    ///
    /// Takes `&mut self` because the union-find compresses paths as it answers, which is what
    /// makes it near-constant. It reads junctions and writes only its own scratch, so it is
    /// not part of the world's identity — see the `PartialEq` note.
    pub fn components(&mut self) -> &mut crate::junction::Components {
        self.components.rebuild(&self.cells);
        &mut self.components
    }

    /// The species archive and the tree over it.
    #[must_use]
    pub fn archive(&self) -> &crate::phylogeny::Phylogeny {
        &self.archive
    }

    pub fn archive_mut(&mut self) -> &mut crate::phylogeny::Phylogeny {
        &mut self.archive
    }

    /// The world's newspaper.
    #[must_use]
    pub fn events(&self) -> &crate::events::EventLog {
        &self.events
    }

    #[must_use]
    pub fn births_total(&self) -> u64 {
        self.births_total
    }

    #[must_use]
    pub fn foreign_injections_total(&self) -> u64 {
        self.foreign_injections_total
    }

    #[must_use]
    pub fn forced_joins_total(&self) -> u64 {
        self.forced_joins_total
    }

    #[must_use]
    pub fn wounds_total(&self) -> u64 {
        self.wounds_total
    }

    /// Census the population, update the archive, and look for firsts.
    ///
    /// # Why this is sampled rather than run every tick
    ///
    /// M5's gate is that phylogeny and metrics cost under 5% of tick time at a hundred
    /// thousand cells. A census is a walk over every cell — the same order as the tick itself,
    /// with a much smaller constant, but not free — and a population curve does not want
    /// per-tick resolution anyway. So it runs on the archive's sample interval, and the
    /// detectors run with it.
    ///
    /// Births and deaths are *not* sampled: those are counted as they happen, in
    /// `apply_births` and `apply_deaths`, so no birth is ever missed by a census that did not
    /// happen to fall on it.
    fn observe(&mut self, tick: u64) {
        let due = tick.is_multiple_of(self.archive.sample_interval);
        if !due {
            return;
        }
        self.census.clear();
        // One pass: count each species, and remember one member of each that has finished
        // building itself, so the archive can settle what the species is actually made of.
        // The member with the most organelles is chosen rather than the first, because the
        // first is as likely as not to be a newborn that is still a bare membrane.
        let mut exemplar: std::collections::BTreeMap<crate::phylogeny::SpeciesId, (usize, usize)> =
            Default::default();
        for i in self.cells.iter() {
            let species = self.cells.species[i];
            *self.census.entry(species).or_insert(0) += 1;
            let built = self.cells.slots(i).iter().filter(|o| o.is_active()).count();
            let entry = exemplar.entry(species).or_insert((0, i));
            if built > entry.0 {
                *entry = (built, i);
            }
        }
        for (species, (built, i)) in exemplar {
            if built > 1 {
                let traits =
                    crate::names::Traits::of(self.cells.slots(i), self.cells.genome[i].len());
                self.archive.settle_traits(species, traits);
            }
        }
        self.archive.census(&self.census, tick);
        // Components are rebuilt here rather than inside the detectors, because the wiki and
        // the renderer want them too and rebuilding twice would be paying twice.
        self.components.rebuild(&self.cells);
        let view = crate::events::WorldView {
            cells: &self.cells,
            archive: Some(&self.archive),
            births_so_far: self.births_total,
            foreign_injections: self.foreign_injections_total,
            forced_joins: self.forced_joins_total,
            wounds: self.wounds_total,
            components: Some(&mut self.components),
        };
        self.events.observe(&view, tick);
    }

    /// Rebuild the archive's narrative half from a snapshot. Hard rule 7.
    ///
    /// Takes a struct rather than nine positional arguments. It has gained one at each of the
    /// last two milestones and would gain more at the next, and nine `u64`s in a row is a
    /// call nobody can read and anybody can transpose.
    pub fn restore_story(&mut self, story: RestoredStory) {
        self.events.restore(
            story.events,
            story.window_population,
            story.window_at,
            story.generations,
            story.dominant,
        );
        self.births_total = story.births_total;
        self.foreign_injections_total = story.foreign_injections_total;
        self.forced_joins_total = story.forced_joins_total;
        self.wounds_total = story.wounds_total;
    }

    /// Drop extinct branches that carry no story (SPEC §10.3).
    ///
    /// Not on a timer inside `step`: how much history is worth keeping is a decision for
    /// whoever is running the simulation, and a headless sweep that wants every dead end has
    /// as good a claim as a viewer that wants a legible tree. `mm-cli` prunes on a schedule.
    pub fn prune_archive(&mut self, keep_above: u32) -> usize {
        self.archive.prune(keep_above)
    }

    /// Kill a cell, returning everything it held to the water (M6's tweezers).
    ///
    /// The simulation's own death path, not a second one. A tool that removed a cell by
    /// clearing its slot would destroy the matter inside it, and the conservation check would
    /// start failing in a way that pointed at the physics rather than at the tool.
    pub fn kill_cell(&mut self, cell: CellId) {
        if self.cells.index(cell).is_none() {
            return;
        }
        self.pending.deaths.push(cell);
        biology::apply_deaths(
            &mut self.cells,
            &mut self.substrate,
            &self.biology,
            &mut self.ledger,
            &mut self.pending,
        );
    }

    /// Put matter into a square from outside the world. Returns what actually landed.
    ///
    /// The tool half of [`crate::Flux`]. A brush is a source that fires once where you point it
    /// rather than every step where the scenario says, and it needs exactly the same care: what
    /// `add_chem` reports is what is recorded, so painting onto a barrier or onto a square at
    /// the quantity cap records what it managed. Matter that is a metabolic substrate brings its
    /// latent energy with it (I5), which is why this cannot be `substrate_mut().add_chem` at the
    /// call site — that spelling compiles, works, and puts the world's books out by exactly the
    /// energy in whatever you painted.
    pub fn inject(&mut self, chemical: usize, x: i32, y: i32, amount: i32) -> i32 {
        if amount <= 0 {
            return 0;
        }
        let c = chemical % CHEM_COUNT;
        let moved = self.substrate.add_chem(c, x, y, amount);
        if moved > 0 {
            self.ledger.record_injected(c, i64::from(moved));
            let latent =
                self.biology
                    .metabolism
                    .latent_in(&self.scenario.chemicals, c, i64::from(moved));
            self.ledger.import(latent);
            self.note_seeding(c, x, y, moved);
        }
        moved
    }

    /// Keep `scenario.seeding` in step with chemistry put on the slide by hand.
    ///
    /// The same argument as `record_barriers`: chemistry painted into the substrate and nowhere
    /// else is chemistry that vanishes when the scenario is saved and opened again, and what a
    /// scenario *is* is a recipe for a slide — so anything a hand did to the slide belongs in it.
    ///
    /// Merged per square rather than appended, so leaning on the brush over one patch is one
    /// entry that grows and not ten thousand that say the same thing.
    fn note_seeding(&mut self, c: usize, x: i32, y: i32, delta: i32) {
        let (w, h) = (self.substrate.width(), self.substrate.height());
        if x < 0 || y < 0 || x as u32 >= w || y as u32 >= h {
            return;
        }
        let (x, y) = (x as u32, y as u32);
        if let Some(i) = self.scenario.seeding.iter().position(|s| {
            matches!(s, crate::Seeding::Spike { chemical, x: sx, y: sy, .. }
                if *chemical == c && *sx == x && *sy == y)
        }) {
            if let crate::Seeding::Spike { amount, .. } = &mut self.scenario.seeding[i] {
                *amount = amount.saturating_add(delta);
                if *amount <= 0 {
                    self.scenario.seeding.remove(i);
                }
            }
            return;
        }
        if delta > 0 {
            self.scenario.seeding.push(crate::Seeding::Spike {
                chemical: c,
                x,
                y,
                amount: delta,
            });
        }
    }

    /// Take matter out of a square, off the slide. Returns what actually left.
    ///
    /// The eraser, and the other half of the books. Not "delete": matter does not stop existing,
    /// it leaves — which is the same claim [`crate::Flux::Drain`] makes and is accounted the same
    /// way, down to the energy going out with it.
    pub fn extract(&mut self, chemical: usize, x: i32, y: i32, amount: i32) -> i32 {
        if amount <= 0 {
            return 0;
        }
        let c = chemical % CHEM_COUNT;
        let here = self.substrate.chem_at(c, x, y);
        let moved = -self.substrate.add_chem(c, x, y, -amount.min(here));
        if moved > 0 {
            self.ledger.record_drained(c, i64::from(moved));
            let latent =
                self.biology
                    .metabolism
                    .latent_in(&self.scenario.chemicals, c, i64::from(moved));
            self.ledger.export(latent);
            self.note_seeding(c, x, y, -moved);
        }
        moved
    }

    /// Wash the whole slide to one level of one chemical, and say so in the recipe.
    ///
    /// Not the paint tool run over every square. [`Self::inject`] records a [`Seeding::Spike`]
    /// per square, so a 270-square slide washed in carbon would write 72,900 entries into a file
    /// whose entire job is to be read by a person. [`Seeding::Uniform`] is one line and has been
    /// in the format since M1; there was no way to reach it without a text editor, which is why
    /// "start this world short of carbon" — the first thing anybody wants from a scenario — was
    /// the one thing the editor could not express.
    ///
    /// A **level and not a dose**: each square is set to `per_square` rather than having it
    /// added, so dragging the number down takes matter off the slide as readily as raising it
    /// puts matter on. Everything else the recipe said about this chemical goes with it — spikes
    /// painted by hand, a gradient, a patch — because a wash with those still listed underneath
    /// is a file that reopens as a different slide than the one you were looking at when you
    /// saved it.
    ///
    /// Both directions go through the ledger, in both currencies (I4, I5), for the same reason
    /// `inject` cannot be `substrate_mut().set_chem` at the call site.
    ///
    /// Barrier squares are left alone, which is what makes this agree with reloading the file:
    /// `World::new` raises barriers before it seeds, so `add_chem` skips them there exactly as
    /// `set_chem` skips them here.
    ///
    /// Returns the net matter moved — positive onto the slide, negative off it.
    pub fn set_uniform_seeding(&mut self, chemical: usize, per_square: i32) -> i64 {
        let c = chemical % CHEM_COUNT;
        let level = per_square.max(0);
        let (w, h) = (self.substrate.width(), self.substrate.height());
        let mut net: i64 = 0;
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                net += i64::from(self.substrate.set_chem(c, x, y, level));
            }
        }

        let moved = net.unsigned_abs() as i64;
        let latent = self
            .biology
            .metabolism
            .latent_in(&self.scenario.chemicals, c, moved);
        if net > 0 {
            self.ledger.record_injected(c, moved);
            self.ledger.import(latent);
        } else if net < 0 {
            self.ledger.record_drained(c, moved);
            self.ledger.export(latent);
        }

        self.scenario.seeding.retain(|s| {
            !matches!(s,
                Seeding::Uniform { chemical, .. }
                | Seeding::Gradient { chemical, .. }
                | Seeding::Spike { chemical, .. }
                | Seeding::Patch { chemical, .. } if *chemical == c)
        });
        if level > 0 {
            self.scenario.seeding.push(Seeding::Uniform {
                chemical: c,
                per_square: level,
            });
        }
        net
    }

    /// What one square of `chemical` is seeded with, if the recipe says so uniformly.
    ///
    /// The read half of [`Self::set_uniform_seeding`], so an editor can show what is set rather
    /// than sampling a square and hoping it is representative — a square under a barrier, or one
    /// a cell has just eaten out of, is not.
    #[must_use]
    pub fn uniform_seeding(&self, chemical: usize) -> Option<i32> {
        let c = chemical % CHEM_COUNT;
        self.scenario.seeding.iter().find_map(|s| match s {
            Seeding::Uniform {
                chemical,
                per_square,
            } if *chemical == c => Some(*per_square),
            _ => None,
        })
    }

    /// Write a hand-placed founder into the scenario's inhabitant list.
    ///
    /// Placing a cell puts it on the slide; this is what makes the slide able to *say* so, and
    /// the two are separate because they are separate claims. `place_founders_at` changes the
    /// world; this changes the recipe.
    ///
    /// Note what a scenario is, because it is easy to expect otherwise: it is a starting
    /// condition and not a state. Saving one mid-run writes down the founders that were placed,
    /// not the population that grew from them — for the world as it stands there is
    /// `Snapshot`. A repeat placement of the same genome on the same square adds to the count
    /// rather than making a second entry, so leaning on the button reads as "twelve here".
    pub fn note_inhabitant(&mut self, genome: &str, count: u32, place: crate::Placement) {
        if count == 0 {
            return;
        }
        if let Some(existing) = self
            .scenario
            .inhabitants
            .iter_mut()
            .find(|i| i.genome == genome && i.place == place)
        {
            existing.count = existing.count.saturating_add(count);
            return;
        }
        self.scenario.inhabitants.push(crate::Inhabitant {
            genome: genome.to_string(),
            count,
            place,
        });
    }

    /// Place founders **and** write them into the recipe, as one act.
    ///
    /// The two halves were two calls, and nothing held them together: the front end placed with
    /// [`World::place_founders_at`] — which only ever builds `Placement::At` — and recorded with
    /// [`World::note_inhabitant`], which until now only ever wrote `Placement::At` back. So the
    /// four arrangements [`crate::Placement`] offers were reachable from a hand-written file and
    /// from nothing else, and the editor's only way to say "sixteen founders spread over this
    /// rectangle" was sixteen separate entries.
    ///
    /// One call, one `Placement`, used for both: **what was drawn is what reopens, by
    /// construction** rather than by two callers agreeing.
    ///
    /// The count recorded is how many actually *landed*, not how many were asked for — a
    /// rectangle that is mostly wall has fewer free squares than a request may name, and a recipe
    /// that says twelve because twelve fitted reopens as the slide you were looking at.
    pub fn seed_inhabitant(
        &mut self,
        genome: &str,
        bytes: &[u8],
        count: u32,
        place: crate::Placement,
    ) -> u32 {
        self.seed_cohort(&[(genome, bytes, count)], place)
            .first()
            .copied()
            .unwrap_or(0)
    }

    /// Several genomes over one region, placed as a cohort and written into the recipe.
    ///
    /// What "two of these and three of those, mixed" is: [`World::place_cohort`] interleaves them
    /// rather than stacking them, and each becomes its own [`crate::Inhabitant`] entry — same
    /// placement, different genome — so the file says exactly what was drawn.
    ///
    /// **Reopening that file has to place them as a cohort too**, or the entries are placed one
    /// at a time and land on top of each other again. [`World::place_recipe`] is the way in for
    /// whoever opens a scenario, and it groups by placement so this round-trips.
    pub fn seed_cohort(
        &mut self,
        members: &[(&str, &[u8], u32)],
        place: crate::Placement,
    ) -> Vec<u32> {
        let bytes: Vec<(&[u8], u32)> = members.iter().map(|(_, b, n)| (*b, *n)).collect();
        let placed = self.place_cohort(&bytes, place);
        for ((genome, _, _), landed) in members.iter().zip(placed.iter()) {
            self.note_inhabitant(genome, *landed, place);
        }
        placed
    }

    /// Place everyone the recipe names, grouping neighbours that share a placement into cohorts.
    ///
    /// **The way to open a scenario.** Placing the entries one at a time is what stacks two
    /// species seeded into the same region — see [`World::place_cohort`] — and grouping is what
    /// makes a slide drawn with mixed founders reopen as the slide that was drawn.
    ///
    /// Neighbours rather than all-alike, so the grouping is a property of the file's order and
    /// not of a search: two entries written next to each other are one act, and two written at
    /// opposite ends of the list are two.
    ///
    /// The genomes are resolved by the caller, because `mm-core` has no filesystem and no
    /// assembler. Returns how many landed in all.
    pub fn place_recipe(&mut self, members: &[(&[u8], u32, crate::Placement)]) -> u32 {
        let mut total = 0;
        let mut i = 0;
        while i < members.len() {
            let place = members[i].2;
            let mut j = i;
            while j < members.len() && members[j].2 == place {
                j += 1;
            }
            let cohort: Vec<(&[u8], u32)> =
                members[i..j].iter().map(|(b, n, _)| (*b, *n)).collect();
            total += self.place_cohort(&cohort, place).iter().sum::<u32>();
            i = j;
        }
        total
    }

    /// Who the recipe says lives here.
    #[must_use]
    pub fn inhabitants(&self) -> &[crate::Inhabitant] {
        &self.scenario.inhabitants
    }

    /// Take an entry out of the recipe.
    ///
    /// **The recipe only.** Cells already on the slide are not touched, for the same reason
    /// [`World::remove_flux`] leaves behind the matter its source already delivered: this
    /// changes what the world will be built from, not what it currently is. Undoing a placement
    /// properly would mean unspawning a cell *through the ledger* — the exact inverse of
    /// `spawn_cell`, matter and energy both — and `kill_cell` is not that: it returns the body
    /// to the water, which is a death and not an undo.
    ///
    /// Out of range is `None` rather than a panic: the caller is a list drawn from a snapshot of
    /// this vector a frame ago, and the world may have moved on.
    pub fn remove_inhabitant(&mut self, index: usize) -> Option<crate::Inhabitant> {
        (index < self.scenario.inhabitants.len())
            .then(|| self.scenario.inhabitants.remove(index))
    }

    /// The sources and drains in force. See [`crate::Flux`].
    #[must_use]
    pub fn flux(&self) -> &[crate::Flux] {
        &self.scenario.flux
    }

    /// Add a source or a drain to a running world.
    pub fn add_flux(&mut self, f: crate::Flux) {
        self.scenario.flux.push(f);
    }

    /// Remove one, by position in [`World::flux`]. Returns it if it was there.
    pub fn remove_flux(&mut self, i: usize) -> Option<crate::Flux> {
        (i < self.scenario.flux.len()).then(|| self.scenario.flux.remove(i))
    }

    /// Draw or erase a barrier, putting whatever was in the square somewhere it can go.
    ///
    /// `Substrate::set_blocked` evicts the square's contents and hands them back; dropping
    /// them on the floor would be a matter leak in a tool, which is the hardest kind to find
    /// because the ledger would blame the fluid. They go to the neighbours, and whatever will
    /// not fit is written off through the ledger so the books still balance exactly.
    pub fn set_barrier(&mut self, x: u32, y: u32, blocked: bool) {
        self.set_barriers(&[(x, y)], blocked);
    }

    /// Draw or erase many barrier squares as one edit.
    ///
    /// The same thing [`World::set_barrier`] does, `n` times, with the fluid's edge masks
    /// rebuilt **once** at the end rather than once per square. That rebuild walks the whole
    /// slide, so the per-square version costs `n * width * height` — fine for the one square a
    /// click used to draw, and a stall of seconds for a brush ten squares across dragged over a
    /// 512×512 slide.
    ///
    /// Squares already in the asked-for state cost nothing: the deferred setter returns before
    /// it touches anything, which is what makes overlapping brush stamps along a stroke cheap.
    ///
    /// Order is the caller's and it matters for the books rather than for the result: blocking
    /// a square spills its contents into its neighbours, so a neighbour blocked later in the
    /// same batch spills what it was just given as well. Matter is conserved either way.
    pub fn set_barriers(&mut self, squares: &[(u32, u32)], blocked: bool) {
        for &(x, y) in squares {
            self.place_barrier(x, y, blocked);
        }
        self.substrate.rebuild_edge_masks();
        self.record_barriers(squares, blocked);
    }

    /// Keep `scenario.barriers` in step with the substrate.
    ///
    /// Without this a wall drawn in the front end exists on the slide and nowhere else, so
    /// saving the scenario and opening it again gives you back a slide with no walls on it —
    /// which reads as the save being broken rather than as the wall never having been part of
    /// what a scenario is. A freehand stroke becomes a run of `Barrier::Square`, which is
    /// verbose in the file and exactly right: an authored `Rect` stays a `Rect` until somebody
    /// draws over it, and the file says what is actually there.
    fn record_barriers(&mut self, squares: &[(u32, u32)], blocked: bool) {
        for &(x, y) in squares {
            let listed = self.scenario.barriers.iter().position(
                |b| matches!(b, crate::Barrier::Square { x: bx, y: by } if *bx == x && *by == y),
            );
            match (blocked, listed) {
                (true, None) => self.scenario.barriers.push(crate::Barrier::Square { x, y }),
                (false, Some(i)) => {
                    self.scenario.barriers.remove(i);
                }
                _ => {}
            }
        }
        if !blocked {
            // A square in the middle of an authored `Rect` or `WallWithGap` cannot be taken out
            // of the list by deleting an entry, so when the eraser lands inside a shape the
            // whole list is flattened to squares and the erased ones dropped. Only then: a
            // scenario that ships rectangles keeps them until somebody actually rubs one out.
            let flatten = squares.iter().any(|&(x, y)| self.inside_a_shape(x, y));
            if flatten {
                self.scenario.barriers = self
                    .barrier_squares()
                    .into_iter()
                    .map(|(x, y)| crate::Barrier::Square { x, y })
                    .collect();
                self.scenario
                    .barriers
                    .retain(|b| !matches!(b, crate::Barrier::Square { x, y } if squares.contains(&(*x, *y))));
            }
        }
    }

    /// Whether this square is covered by a barrier entry that is not a lone square.
    fn inside_a_shape(&self, x: u32, y: u32) -> bool {
        self.scenario
            .barriers
            .iter()
            .any(|b| !matches!(b, crate::Barrier::Square { .. }))
            && self.barrier_squares().contains(&(x, y))
    }

    fn place_barrier(&mut self, x: u32, y: u32, blocked: bool) {
        let evicted = self
            .substrate
            .set_blocked_deferred(x as i32, y as i32, blocked);
        let mut unplaced = [0i32; CHEM_COUNT];
        for (c, amount) in evicted.iter().enumerate() {
            if *amount <= 0 {
                continue;
            }
            let mut left = *amount;
            // Outward in rings, the same way a corpse finds somewhere to go.
            'placed: for ring in 1..4i32 {
                for dy in -ring..=ring {
                    for dx in -ring..=ring {
                        if dx.abs() != ring && dy.abs() != ring {
                            continue;
                        }
                        left -= self
                            .substrate
                            .add_chem(c, x as i32 + dx, y as i32 + dy, left);
                        if left <= 0 {
                            break 'placed;
                        }
                    }
                }
            }
            if left > 0 {
                // Nowhere to put it: walled in on every side. Recorded as evicted rather than
                // silently dropped, which is what the ledger's eviction column is for.
                unplaced[c] = left;
            }
        }
        if unplaced.iter().any(|v| *v > 0) {
            self.ledger.record_evicted(&unplaced);
        }
        self.velocity_written = false;
    }

    /// Advance many ticks.
    pub fn run(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.step();
        }
    }

    /// Rewrite the light field if the regime needs it.
    fn refresh_light(&mut self) {
        if self.light_written && !self.scenario.light.is_time_varying() {
            return;
        }
        let regime = self.scenario.light.clone();
        regime.apply(&mut self.substrate, self.tick);
        self.light_written = true;
    }

    /// Rewrite the velocity field from the prescribed current plus the impulse layer.
    ///
    /// Skipped entirely once written, unless a cilium has pushed on the water since — or the
    /// current is one of the kinds that *does* depend on the tick, which
    /// [`CurrentField::is_time_varying`] answers. Every variant but the tide is a function of
    /// position alone and is written once.
    fn refresh_velocity(&mut self) {
        if self.velocity_written && !self.scenario.current.is_time_varying() {
            return;
        }
        self.velocity_written = true;
        let current: CurrentField = self.scenario.current.clone();
        // Take the impulse buffers out so the substrate can be borrowed mutably.
        let ix = std::mem::take(&mut self.impulse_x);
        let iy = std::mem::take(&mut self.impulse_y);
        current.apply(&mut self.substrate, &ix, &iy, &self.stir_x, &self.stir_y, self.tick);
        self.impulse_x = ix;
        self.impulse_y = iy;
    }

    /// Replace the population, for [`crate::snapshot::Snapshot`].
    pub(crate) fn restore_cells(
        &mut self,
        cells: Vec<(u32, Option<crate::cell::RestoredCell>)>,
        free: Vec<u32>,
    ) {
        self.cells.restore(cells, free);
    }

    /// Overwrite every piece of world state at once, for [`crate::snapshot::Snapshot`].
    ///
    /// Deliberately blunt and deliberately crate-private: I7 says a restored world must be
    /// bit-identical, and the way to be sure of that is for restoration to set *everything*
    /// rather than to patch selected fields and hope the rest was already right. Hard rule 7
    /// — if you add state, extend the serialisation in the same commit — means adding a
    /// parameter here, which is a compile error at both call sites until it is handled.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn restore(
        &mut self,
        tick: u64,
        planes: Vec<Vec<i32>>,
        light: Vec<i32>,
        vx: Vec<i32>,
        vy: Vec<i32>,
        blocked: Vec<bool>,
        impulse_x: Vec<i32>,
        impulse_y: Vec<i32>,
        pressure: Vec<i32>,
        ledger: crate::ledger::LedgerState,
    ) {
        self.tick = tick;
        self.substrate.restore(planes, light, vx, vy, blocked);
        self.impulse_x = impulse_x;
        self.impulse_y = impulse_y;
        self.pressure = pressure;
        self.ledger.restore(ledger);
        // The light and velocity fields came from the snapshot, so neither may be overwritten
        // by a fresh evaluation on the next step.
        self.light_written = true;
        self.velocity_written = true;
        // But the *stir* layer did not: it is rebuilt from the cilia every tick and so is not
        // in the file, while the velocity field that came out of the file may still carry the
        // contribution it had when the snapshot was taken. `true` forces exactly one refresh on
        // the next fluid step, which reconverges the restored world onto the uninterrupted one.
        //
        // This is why the flag can stay out of the snapshot without breaking hard rule 7. The
        // forced refresh is idempotent: `CurrentField::apply` writes base + impulse + stir from
        // the layers as they now are, so on a world where nothing had stirred it recomputes the
        // identical field, and on one where something had it writes the field the uninterrupted
        // world would have had. Either way the two agree from the first step, which is what
        // `resuming_matches_running_straight_through` checks.
        self.stir_active = true;
        self.active_impulses = self
            .impulse_x
            .iter()
            .zip(self.impulse_y.iter())
            .filter(|(x, y)| **x != 0 || **y != 0)
            .count() as u32;
    }

    /// Change the light regime mid-run — an authored scenario event (M8).
    pub fn set_light(&mut self, regime: LightRegime) {
        self.scenario.light = regime;
        self.light_written = false;
    }

    /// Change the prescribed current mid-run.
    pub fn set_current(&mut self, current: CurrentField) {
        self.scenario.current = current;
        self.velocity_written = false;
    }

    /// Check I4 against an independent recomputation.
    ///
    /// # Errors
    ///
    /// The first chemical whose ledger total and actual total differ.
    pub fn check_matter(&self) -> Result<(), LedgerBreach> {
        self.ledger.check_matter(&self.total_matter())
    }

    /// Check I5.
    ///
    /// # Errors
    ///
    /// The energy identity, if it does not hold exactly.
    pub fn check_energy(&self) -> Result<(), LedgerBreach> {
        self.ledger.check_energy()
    }

    /// Everything the invariants promise, checked at once. Used by the acceptance tests and
    /// available to any caller that wants to be sure.
    ///
    /// # Errors
    ///
    /// The first invariant that does not hold.
    pub fn check_invariants(&self) -> Result<(), LedgerBreach> {
        self.check_matter()?;
        self.check_energy()?;
        Ok(())
    }

    /// The rolling world-state hash (I1). Computed on demand rather than every tick: it
    /// touches every square of every chemical, which is four million values on a 512×512
    /// grid and far too much to do inside a 2ms step budget.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        self.hash_state(&mut h);
        h.finish()
    }
}

impl StateHash for World {
    fn hash_state(&self, h: &mut StateHasher) {
        h.u64(self.tick);
        self.scenario.hash_state(h);
        self.substrate.hash_state(h);
        self.ledger.hash_state(h);
        self.cells.hash_state(h);
        for v in &self.impulse_x {
            h.i32(*v);
        }
        for v in &self.impulse_y {
            h.i32(*v);
        }
        // The archive and the log are world state: a species founded, a name given, an event
        // recorded. Two runs that diverged only in their phylogeny would be two different
        // worlds, and leaving these out of the hash would let that divergence go unnoticed.
        self.archive.hash_into(h);
        self.events.hash_into(h);
        h.u64(self.births_total);
        h.u64(self.foreign_injections_total);
        h.u64(self.forced_joins_total);
        h.u64(self.wounds_total);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::q10;

    #[test]
    fn a_world_starts_balanced() {
        let w = World::new(Scenario::stress(32, 24)).unwrap();
        w.check_invariants().unwrap();
        assert!(w.substrate().total_chem().iter().any(|t| *t > 0));
    }

    #[test]
    fn barriers_go_up_before_seeding() {
        // Otherwise a scenario would place matter into a square that is about to be walled
        // off, and the eviction would look exactly like a conservation bug.
        let s = Scenario {
            width: 8,
            height: 8,
            barriers: vec![Barrier::Square { x: 4, y: 4 }],
            seeding: vec![Seeding::Uniform {
                chemical: 0,
                per_square: q10(100),
            }],
            ..Scenario::default()
        };
        let w = World::new(s).unwrap();
        assert_eq!(w.substrate().chem_at(0, 4, 4), 0);
        assert_eq!(w.ledger().evicted()[0], 0, "nothing had to be destroyed");
        // 63 open squares, not 64
        assert_eq!(w.ledger().chem_totals()[0], 63 * q10(100) as i64);
        w.check_invariants().unwrap();
    }

    /// The reason the call exists: the recipe stays readable.
    #[test]
    fn a_uniform_wash_is_one_line_in_the_recipe_and_not_four_thousand() {
        let mut w = World::new(Scenario {
            width: 64,
            height: 64,
            ..Scenario::default()
        })
        .unwrap();
        // Painted by hand first, so the wash has spikes to clear as well as a level to set.
        for x in 0..10 {
            w.inject(4, x, 0, q10(500));
        }
        assert_eq!(w.scenario().seeding.len(), 10, "ten spikes, one per square");

        w.set_uniform_seeding(4, q10(40));
        let of_carbon: Vec<_> = w
            .scenario()
            .seeding
            .iter()
            .filter(|s| matches!(s, Seeding::Uniform { chemical: 4, .. }
                | Seeding::Gradient { chemical: 4, .. }
                | Seeding::Spike { chemical: 4, .. }
                | Seeding::Patch { chemical: 4, .. }))
            .collect();
        assert_eq!(
            of_carbon.len(),
            1,
            "4096 squares should be one entry, and the hand-painted spikes should be gone"
        );
        assert_eq!(
            of_carbon[0],
            &Seeding::Uniform {
                chemical: 4,
                per_square: q10(40)
            }
        );
        w.check_invariants().unwrap();
    }

    /// The claim the whole thing rests on: what you washed is what the file gives back.
    #[test]
    fn a_washed_slide_is_what_reopening_its_scenario_gives() {
        // With a barrier on it, because that is where the two paths could disagree: `World::new`
        // raises barriers before it seeds, and the wash has to skip the same squares.
        let mut w = World::new(Scenario {
            width: 16,
            height: 16,
            barriers: vec![Barrier::Rect {
                x: 3,
                y: 3,
                width: 4,
                height: 2,
            }],
            ..Scenario::default()
        })
        .unwrap();
        w.set_uniform_seeding(4, q10(40));
        w.set_uniform_seeding(11, q10(400));
        w.check_invariants().unwrap();

        let reopened = World::new(w.scenario().clone()).unwrap();
        for c in [4, 11] {
            assert_eq!(
                reopened.substrate().chem_plane(c),
                w.substrate().chem_plane(c),
                "chemical {c} came back different from what was saved"
            );
        }
        assert_eq!(reopened.substrate().chem_at(4, 4, 3), 0, "under the wall");
    }

    /// A level, not a dose. Setting it lower has to take matter *off* the slide, through the
    /// books, or the drag that lowers it is a leak.
    #[test]
    fn washing_down_takes_matter_off_the_slide_and_the_books_agree() {
        let mut w = World::new(Scenario {
            width: 16,
            height: 16,
            seeding: vec![Seeding::Uniform {
                chemical: 4,
                per_square: q10(400),
            }],
            ..Scenario::default()
        })
        .unwrap();
        let before: i64 = w.total_matter()[4];

        let net = w.set_uniform_seeding(4, q10(40));
        assert!(net < 0, "lowering the level should have removed matter");
        assert_eq!(w.total_matter()[4], before + net);
        assert_eq!(w.substrate().chem_at(4, 7, 7), q10(40));
        w.check_invariants().unwrap();

        // And to nothing at all, which drops the entry rather than writing `per_square: 0`.
        w.set_uniform_seeding(4, 0);
        assert_eq!(w.total_matter()[4], 0);
        assert!(w.uniform_seeding(4).is_none());
        assert!(w
            .scenario()
            .seeding
            .iter()
            .all(|s| !matches!(s, Seeding::Uniform { chemical: 4, .. })));
        w.check_invariants().unwrap();
    }

    #[test]
    fn stepping_conserves_matter_exactly() {
        // Per-species totals may move, but only through a balanced reaction that reported
        // itself — peroxide decomposing in the water is one. What may never move is the total
        // across every species, and what must always agree is the ledger's claim.
        let mut w = World::new(Scenario::stress(24, 20)).unwrap();
        let before: i64 = w.total_matter().iter().sum();
        for tick in 0..2000 {
            w.step();
            w.check_invariants()
                .unwrap_or_else(|e| panic!("at tick {tick}: {e}"));
            assert_eq!(
                w.total_matter().iter().sum::<i64>(),
                before,
                "total matter moved at tick {tick}"
            );
        }
    }

    #[test]
    fn the_fluid_interval_is_honoured() {
        let s = Scenario {
            width: 32,
            height: 32,
            fluid_interval: 10,
            seeding: vec![Seeding::Spike {
                chemical: 0,
                x: 16,
                y: 16,
                amount: q10(1_000_000),
            }],
            ..Scenario::default()
        };
        let mut w = World::new(s).unwrap();
        // Tick 0 runs the fluid: the interval counts from the start of the run, so the first
        // tick is a fluid tick.
        let before = w.state_hash();
        w.step();
        let after_first = w.state_hash();
        assert_ne!(after_first, before, "tick 0 should have run the fluid");

        // Ticks 1..=9 are not fluid ticks, so nothing about the world moves except the clock.
        for _ in 0..9 {
            w.step();
            assert_eq!(
                w.substrate().chem_plane(0),
                {
                    let mut reference = World::new(w.scenario().clone()).unwrap();
                    reference.step();
                    reference.substrate().chem_plane(0).to_vec()
                },
                "the substrate moved on a non-fluid tick"
            );
        }
        assert_eq!(w.tick_count(), 10);

        // Tick 10 is.
        let before_tenth = w.substrate().chem_plane(0).to_vec();
        w.step();
        assert_ne!(w.substrate().chem_plane(0), before_tenth.as_slice());
    }

    #[test]
    fn a_time_varying_light_regime_is_rewritten_and_a_static_one_is_not() {
        let mut day = World::new(Scenario {
            width: 8,
            height: 8,
            light: LightRegime::DayNight {
                period_ticks: 64,
                day: 1024,
                night: 0,
            },
            ..Scenario::default()
        })
        .unwrap();
        let dark = day.substrate().light_at(0, 0);
        day.run(32);
        assert!(day.substrate().light_at(0, 0) > dark, "day never came");

        let mut still = World::new(Scenario {
            width: 8,
            height: 8,
            light: LightRegime::Uniform { intensity: 500 },
            ..Scenario::default()
        })
        .unwrap();
        still.run(100);
        assert_eq!(still.substrate().light_at(0, 0), 500);
    }

    /// The same property for the current, and it is the one a tide silently loses without.
    ///
    /// `refresh_velocity` wrote the prescribed field once and skipped it thereafter, on the
    /// reasoning — true of every variant before `Tidal` — that a current does not depend on the
    /// tick. A tide written once is a constant current, and nothing about the picture or the
    /// matter ledger would have said so.
    #[test]
    fn a_tide_is_rewritten_every_tick_and_a_constant_current_is_not() {
        let tide = |w: &World| w.substrate().velocity_at(4, 4).0;
        let mut sea = World::new(Scenario {
            width: 8,
            height: 8,
            current: crate::light::CurrentField::Tidal {
                period_ticks: 64,
                peak_vx: crate::fluid::MAX_VELOCITY,
                peak_vy: 0,
                cycle_ticks: 1,
                neap: crate::fixed::Q10_ONE,
            },
            fluid_interval: 1,
            ..Scenario::default()
        })
        .unwrap();
        // Slack at tick zero, flooding a quarter period in, ebbing the other way at three
        // quarters. Reading the substrate, not the regime, so this is the field cells actually
        // swim in rather than the arithmetic that produced it.
        assert_eq!(tide(&sea), 0, "a tide starts at slack water");
        sea.run(16);
        let flood = tide(&sea);
        assert!(flood > 0, "the tide never came in: {flood}");
        sea.run(32);
        let ebb = tide(&sea);
        assert!(ebb < 0, "the tide never turned: {ebb}");

        let mut river = World::new(Scenario {
            width: 8,
            height: 8,
            current: crate::light::CurrentField::Uniform { vx: 100, vy: 0 },
            fluid_interval: 1,
            ..Scenario::default()
        })
        .unwrap();
        river.run(100);
        assert_eq!(
            river.substrate().velocity_at(4, 4).0,
            100,
            "a constant current is still constant"
        );
    }

    #[test]
    fn impulses_move_matter_and_then_fade() {
        let s = Scenario {
            width: 16,
            height: 4,
            seeding: vec![Seeding::Spike {
                chemical: 0,
                x: 2,
                y: 2,
                amount: q10(10_000),
            }],
            impulse_retain: crate::fixed::Q10_ONE / 2,
            ..Scenario::default()
        };
        let mut w = World::new(s).unwrap();
        let before = w.substrate().total_chem();
        for _ in 0..8 {
            for y in 0..4 {
                for x in 0..16 {
                    w.inject_impulse(x, y, crate::fixed::Q10_ONE, 0);
                }
            }
            w.step();
        }
        assert!(
            w.substrate().chem_at(0, 2, 2) < q10(10_000),
            "nothing moved"
        );
        assert_eq!(w.substrate().total_chem(), before, "and nothing was lost");

        w.run(64);
        let (ix, _) = w.impulses();
        assert!(ix.iter().all(|v| *v == 0), "impulses stirred forever");
    }

    #[test]
    fn an_impulse_into_a_barrier_is_ignored() {
        let mut w = World::new(Scenario {
            width: 8,
            height: 8,
            barriers: vec![Barrier::Square { x: 3, y: 3 }],
            ..Scenario::default()
        })
        .unwrap();
        w.inject_impulse(3, 3, 500, 500);
        let (ix, iy) = w.impulses();
        let i = w.substrate().index(3, 3);
        assert_eq!((ix[i], iy[i]), (0, 0));
    }

    #[test]
    fn a_wall_with_a_gap_leaves_exactly_the_gap() {
        let w = World::new(Scenario {
            width: 16,
            height: 16,
            barriers: vec![Barrier::WallWithGap {
                at: 8,
                vertical: true,
                gap_start: 6,
                gap_len: 2,
            }],
            ..Scenario::default()
        })
        .unwrap();
        for y in 0..16 {
            let open = (6..8).contains(&y);
            assert_eq!(!w.substrate().is_blocked(8, y), open, "row {y}");
        }
    }

    #[test]
    fn the_state_hash_moves_with_the_world_and_not_otherwise() {
        let mut a = World::new(Scenario::stress(16, 16)).unwrap();
        let b = World::new(Scenario::stress(16, 16)).unwrap();
        assert_eq!(a.state_hash(), b.state_hash(), "same scenario, same state");
        a.step();
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn two_worlds_from_one_scenario_stay_identical() {
        let mut a = World::new(Scenario::stress(20, 17)).unwrap();
        let mut b = World::new(Scenario::stress(20, 17)).unwrap();
        for _ in 0..500 {
            a.step();
            b.step();
            assert_eq!(
                a.state_hash(),
                b.state_hash(),
                "diverged at tick {}",
                a.tick
            );
        }
    }
}
