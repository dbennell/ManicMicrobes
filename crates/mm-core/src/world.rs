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
    /// Cached so a static light regime is not rewritten over the whole grid every step.
    light_written: bool,
    /// Whether the velocity field is already correct for the current impulse layer.
    ///
    /// The prescribed current is time-invariant, so with no cilia pushing on the water the
    /// velocity field is written once and never again. Rewriting it every step — a quarter of
    /// a million squares, each a couple of divisions — cost more than the fluid solver it was
    /// feeding.
    velocity_written: bool,
    /// How many squares carry a non-zero impulse. Zero means the velocity field cannot have
    /// moved since it was last written.
    active_impulses: u32,
    diffusion_rates: [i32; CHEM_COUNT],
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
    /// How hard each cell is being pressed by cells it is not joined to, `POS`, as of this
    /// tick's separation pass. Scratch in the same sense: recomputed from positions every
    /// tick, so it is excluded from equality and from the hash.
    crowding: Vec<i32>,
    /// How stuck each cell is, `Q10`, one unit per neighbour bottomed out on its core. Derived
    /// every physics phase like `crowding`, and read by division a phase later — a cell that was
    /// wedged last tick is still wedged, and the alternative is running the collision pass twice.
    pressure: Vec<i32>,

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
        let diffusion_rates = scenario.chemicals.diffusion_rates();

        let mut world = World {
            scenario,
            substrate,
            ledger: Ledger::new(),
            tick: 0,
            impulse_x: vec![0; n],
            impulse_y: vec![0; n],
            light_written: false,
            velocity_written: false,
            active_impulses: 0,
            diffusion_rates,
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
            crowding: Vec::new(),
            pressure: Vec::new(),
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
        let barriers = self.scenario.barriers.clone();
        for b in &barriers {
            match b {
                Barrier::Square { x, y } => {
                    self.block(*x, *y);
                }
                Barrier::Rect {
                    x,
                    y,
                    width,
                    height,
                } => {
                    for dy in 0..*height {
                        for dx in 0..*width {
                            self.block(x.saturating_add(dx), y.saturating_add(dy));
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
                        let in_gap = i >= *gap_start && i < gap_start.saturating_add(*gap_len);
                        if in_gap {
                            continue;
                        }
                        if *vertical {
                            self.block(*at, i);
                        } else {
                            self.block(i, *at);
                        }
                    }
                }
            }
        }
    }

    /// Raise a barrier and tell the ledger about anything it destroyed.
    fn block(&mut self, x: u32, y: u32) {
        if x >= self.substrate.width() || y >= self.substrate.height() {
            return;
        }
        let evicted = self.substrate.set_blocked(x as i32, y as i32, true);
        self.ledger.record_evicted(&evicted);
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

    #[must_use]
    /// How wedged each cell is, `Q10`. See [`crate::neighbours::resolve_collisions`].
    ///
    /// Snapshot state rather than scratch, and the distinction cost a failed round-trip test to
    /// notice: it is written by the physics phase and read by *the next tick's* division, so a
    /// world restored without it lets through the first round of divisions the original refused.
    #[must_use]
    pub fn pressure(&self) -> &[i32] {
        &self.pressure
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
                self.biology.metabolism.catalogue.metabolism,
            );

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
            report.physics = crate::sensing::step_physics(
                &mut self.cells,
                &self.substrate,
                &mut self.impulse_x,
                &mut self.impulse_y,
                crate::sensing::BodyForces {
                    jitter: self.scenario.jitter,
                    gravity: self.scenario.gravity,
                },
                tick,
                seed,
            );
            if report.physics.moved != 0 || report.physics.energy_spent != 0 {
                // A cilium that pushed on the water changed the velocity field.
                self.velocity_written = false;
                self.active_impulses = self
                    .impulse_x
                    .iter()
                    .zip(self.impulse_y.iter())
                    .filter(|(x, y)| **x != 0 || **y != 0)
                    .count() as u32;
            }
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
            report.physics.separated = crate::neighbours::resolve_collisions(
                &mut self.cells,
                &mut self.neighbours,
                &mut self.radii,
                &mut self.crowding,
                &mut self.pressure,
            );
        }

        // 5. Fluid, at fluid_hz.
        let interval = self.scenario.fluid_interval.max(1) as u64;
        if self.tick.is_multiple_of(interval) {
            self.refresh_light();
            self.refresh_velocity();
            fluid::step(
                &mut self.substrate,
                &self.diffusion_rates,
                &mut self.scratch,
            );
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
            );
            self.pending.deaths.append(&mut self.starving);
            // 6b. Ecology: spikes wound, lysosomes digest (M8). Before deaths, so a cell
            //     wounded past its limit dies this tick rather than next — and after the
            //     neighbour index was rebuilt by the physics phase, so "touching" means where
            //     things are now.
            report.ecology = crate::ecology::step(
                &mut self.cells,
                &mut self.substrate,
                &self.neighbours,
                &self.crowding,
                &self.biology.ecology,
                &self.biology.metabolism.catalogue.metabolism,
                &mut self.ledger,
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

    /// Draw or erase a barrier, putting whatever was in the square somewhere it can go.
    ///
    /// `Substrate::set_blocked` evicts the square's contents and hands them back; dropping
    /// them on the floor would be a matter leak in a tool, which is the hardest kind to find
    /// because the ledger would blame the fluid. They go to the neighbours, and whatever will
    /// not fit is written off through the ledger so the books still balance exactly.
    pub fn set_barrier(&mut self, x: u32, y: u32, blocked: bool) {
        let evicted = self.substrate.set_blocked(x as i32, y as i32, blocked);
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
    /// Skipped entirely once written, unless a cilium has pushed on the water since. A
    /// prescribed current does not depend on the tick.
    fn refresh_velocity(&mut self) {
        if self.velocity_written {
            return;
        }
        self.velocity_written = true;
        let current: CurrentField = self.scenario.current.clone();
        // Take the impulse buffers out so the substrate can be borrowed mutably.
        let ix = std::mem::take(&mut self.impulse_x);
        let iy = std::mem::take(&mut self.impulse_y);
        current.apply(&mut self.substrate, &ix, &iy);
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
        chem_totals: [i64; CHEM_COUNT],
        evicted: [i64; CHEM_COUNT],
        energy_in: i64,
        energy_out: i64,
        energy_stored: i64,
        converted: i64,
        income: [i64; crate::ledger::TrophicSource::COUNT],
    ) {
        self.tick = tick;
        self.substrate.restore(planes, light, vx, vy, blocked);
        self.impulse_x = impulse_x;
        self.impulse_y = impulse_y;
        self.pressure = pressure;
        self.ledger.restore(
            chem_totals,
            evicted,
            energy_in,
            energy_out,
            energy_stored,
            converted,
            income,
        );
        // The light and velocity fields came from the snapshot, so neither may be overwritten
        // by a fresh evaluation on the next step.
        self.light_written = true;
        self.velocity_written = true;
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
