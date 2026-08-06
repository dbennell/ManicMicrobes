//! The debugger (M6).
//!
//! > Breakpoints, single-step, run-to-tick, watch panes over stack, registers, RAM,
//! > organelles and junctions, on a selected live cell.
//!
//! # How it cannot interfere
//!
//! M6's second acceptance test is that stepping and breakpoints do not change the state hash
//! of a run. The way to pass a test like that is the way M4 passed its own: make the other
//! outcome unrepresentable rather than merely avoided.
//!
//! There are two halves, and each is safe for a different reason.
//!
//! **Breakpoints act on the viewer, not the world.** A breakpoint is a condition checked
//! *between* ticks; when it holds, the slide stops advancing. Pausing provably does not change
//! a world — `slide.rs` has the test — so a breakpoint cannot either. There is no
//! stop-in-the-middle-of-a-tick, because a tick is the simulation's atom and breaking it open
//! would be inventing a state the simulation never has.
//!
//! **Instruction stepping runs on a sandbox.** Picking a cell clones its VM and its genome
//! into a [`Sandbox`] which holds no reference to the world at all — it cannot write to one,
//! because it does not have one. Stepping the sandbox is faithful rather than approximate:
//! `stepping_one_instruction_at_a_time_is_the_same_as_running_the_budget` in `mm-core`'s
//! determinism tests proves that one instruction at a time reaches the same VM as the whole
//! budget at once, so what the debugger shows is what the cell really did.
//!
//! The cost of that honesty is that a sandbox is a *copy*: stepping it does not advance the
//! live cell, and the live cell will have moved on by the time you finish reading. That is the
//! correct trade. A debugger that could halt one cell mid-tick while the world ran on would be
//! showing a world that never existed.

use mm_core::config::VmConfig;
use mm_core::genome::Genome;
use mm_core::host::Host;
use mm_core::rng::RandCtx;
use mm_core::vm::Vm;
use mm_core::{CellId, World};
use std::sync::Arc;

/// A condition that stops the slide.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Breakpoint {
    /// When the world reaches this tick.
    AtTick(u64),
    /// When a specific cell's instruction pointer reaches a genome offset.
    CellReaches {
        cell: CellId,
        offset: u16,
    },
    /// When a cell dies.
    CellDies(CellId),
    /// When the population crosses a threshold, in either direction.
    PopulationBelow(usize),
    PopulationAbove(usize),
    /// When the species count changes — the phylogeny equivalent of a watchpoint.
    SpeciesCountReaches(usize),
}

impl Breakpoint {
    /// Whether this breakpoint holds, given the world as it is now.
    ///
    /// Takes `&World` and returns a `bool`. It has no way to change anything, and that is the
    /// entire safety argument — a breakpoint predicate that could write would be a breakpoint
    /// that changed the run it was watching.
    #[must_use]
    pub fn holds(&self, world: &World) -> bool {
        match self {
            Breakpoint::AtTick(t) => world.tick_count() >= *t,
            Breakpoint::CellReaches { cell, offset } => world
                .cells()
                .index(*cell)
                .is_some_and(|i| world.cells().vm[i].ip == *offset),
            Breakpoint::CellDies(cell) => world.cells().index(*cell).is_none(),
            Breakpoint::PopulationBelow(n) => world.cells().len() < *n,
            Breakpoint::PopulationAbove(n) => world.cells().len() > *n,
            Breakpoint::SpeciesCountReaches(n) => world.archive().living() >= *n,
        }
    }

    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Breakpoint::AtTick(t) => format!("tick {t}"),
            Breakpoint::CellReaches { cell, offset } => {
                format!("cell {} reaches offset {offset}", cell.ordering_key())
            }
            Breakpoint::CellDies(cell) => format!("cell {} dies", cell.ordering_key()),
            Breakpoint::PopulationBelow(n) => format!("population below {n}"),
            Breakpoint::PopulationAbove(n) => format!("population above {n}"),
            Breakpoint::SpeciesCountReaches(n) => format!("{n} living species"),
        }
    }
}

/// The set of breakpoints, and which one last fired.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Breakpoints {
    points: Vec<(Breakpoint, bool)>,
    /// The index of the breakpoint that stopped the run, if one did.
    tripped: Option<usize>,
}

impl Breakpoints {
    #[must_use]
    pub fn new() -> Breakpoints {
        Breakpoints::default()
    }

    pub fn add(&mut self, point: Breakpoint) {
        if !self.points.iter().any(|(p, _)| *p == point) {
            self.points.push((point, true));
        }
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.points.len() {
            self.points.remove(index);
            self.tripped = None;
        }
    }

    pub fn set_enabled(&mut self, index: usize, on: bool) {
        if let Some((_, enabled)) = self.points.get_mut(index) {
            *enabled = on;
        }
    }

    pub fn clear(&mut self) {
        self.points.clear();
        self.tripped = None;
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Breakpoint, bool)> {
        self.points.iter().map(|(p, on)| (p, *on))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Which breakpoint tripped, if the run is stopped at one.
    #[must_use]
    pub fn tripped(&self) -> Option<&Breakpoint> {
        self.tripped
            .and_then(|i| self.points.get(i))
            .map(|(p, _)| p)
    }

    pub fn rearm(&mut self) {
        self.tripped = None;
    }

    /// Check every enabled breakpoint against the world.
    ///
    /// Read-only over the world by signature, so this cannot be the thing that changes a run.
    #[must_use]
    pub fn check(&mut self, world: &World) -> bool {
        for (i, (point, enabled)) in self.points.iter().enumerate() {
            if *enabled && point.holds(world) {
                self.tripped = Some(i);
                return true;
            }
        }
        false
    }
}

/// A detached copy of one cell, for instruction-level stepping.
///
/// Holds no world. It cannot write to the simulation because it has nothing to write to, which
/// is a stronger guarantee than a promise not to.
#[derive(Clone, Debug)]
pub struct Sandbox {
    /// Which cell this was taken from, or `None` when it was built from an editor buffer and
    /// never had one (M10.8).
    pub cell: Option<CellId>,
    pub taken_at_tick: u64,
    pub genome: Arc<Genome>,
    pub vm: Vm,
    cfg: VmConfig,
    seed: u64,
    ordering_key: u64,
    /// Instructions run since the sandbox was taken.
    pub executed: u32,
    /// Instructions run in the current simulated tick, against the budget.
    pub in_tick: u32,
    /// What the genome has asked the world for since the sandbox was taken.
    pub asked: ScratchHost,
    /// How many times each gene has been reached by `EXPRESS`, indexed as
    /// `Genome::promoters` is.
    ///
    /// Counted by *watching*, not by instrumenting the VM: see [`Sandbox::step`]. Nothing in
    /// `mm-core` knows this is being counted, and nothing in the world does either.
    pub gene_hits: Vec<u32>,
}

/// What a genome asked the world for, in a world that answers nothing.
///
/// A [`NullHost`] that keeps a note. Every *read* is left at the trait's default and therefore
/// still answers zero — which is the sandbox's one honest limitation and the reason
/// [`Sandbox::step`] says so out loud. Every *write* is recorded instead of being thrown away.
///
/// Not `mm_core::host::RecordingHost`, which exists and looks like the right type, and is not:
/// its `eat` returns the full amount asked for. In a test of a replication loop that is a
/// convenience; in a debugger it is a lie, because it would show a scratch cell being fed by a
/// world that is not there. The distinction is exactly the one `Sandbox::step` was written to
/// preserve.
#[derive(Clone, Default, Debug)]
pub struct ScratchHost {
    /// `(param, type, slot)` per `BUILD`, in the order they were asked for.
    pub builds: Vec<(i16, i16, i16)>,
    pub tears: Vec<i16>,
    /// `(amount, chemical)` per `EAT`. Each returned nothing.
    pub eats: Vec<(i16, i16)>,
    pub emits: Vec<(i16, i16)>,
    /// How many times `BUD` was reached.
    pub buds: u32,
    /// How many times `SPLIT` was reached — **not** how many times it divided. A scratch cell
    /// has no world to divide into, and counting these as divisions is the one number a preview
    /// of a genome must not invent.
    pub splits: u32,
    /// Bytes written by `COPYB` towards a daughter that will never exist.
    pub copied: u32,
    /// How many times `INJECT` was reached: this genome writes to other cells' nuclei.
    pub injects: u32,
}

impl Host for ScratchHost {
    fn build(&mut self, param: i16, ty: i16, slot: i16) {
        self.builds.push((param, ty, slot));
    }

    fn tear(&mut self, slot: i16) {
        self.tears.push(slot);
    }

    fn eat(&mut self, amount: i16, chem: i16) -> i16 {
        self.eats.push((amount, chem));
        // Nothing. There is no water under a scratch cell.
        0
    }

    fn emit(&mut self, amount: i16, chem: i16) -> i16 {
        self.emits.push((amount, chem));
        0
    }

    fn bud(&mut self, _size: i16) -> i16 {
        self.buds = self.buds.saturating_add(1);
        0
    }

    fn copy_byte(&mut self, _dst: u16, _src: u8) {
        self.copied = self.copied.saturating_add(1);
    }

    fn split(&mut self) {
        self.splits = self.splits.saturating_add(1);
    }

    fn inject(&mut self, _jidx: i16, _dst: u16, _src: u8) -> i16 {
        self.injects = self.injects.saturating_add(1);
        0
    }
}

impl Sandbox {
    /// Take a copy of a live cell. `None` if it is not alive.
    #[must_use]
    pub fn of(world: &World, cell: CellId) -> Option<Sandbox> {
        let i = world.cells().index(cell)?;
        let genome = Arc::clone(&world.cells().genome[i]);
        Some(Sandbox {
            cell: Some(cell),
            taken_at_tick: world.tick_count(),
            gene_hits: vec![0; genome.promoters().len()],
            genome,
            vm: world.cells().vm[i].clone(),
            cfg: world.scenario().vm,
            seed: world.scenario().seed,
            ordering_key: cell.ordering_key(),
            executed: 0,
            in_tick: 0,
            asked: ScratchHost::default(),
        })
    }

    /// A scratch cell running a genome that is not in the world and never was (M10.8).
    ///
    /// The editor's buffer, assembled. `Sandbox::of` reads five things out of a `World` to build
    /// itself and none of them need a world to exist, which is why this is a constructor rather
    /// than new machinery — the stepping, the budget and the host are the same code, and the
    /// answer to "does this program run" must not depend on which of the two it came through.
    ///
    /// `None` if the bytes are not a genome — over the length a nucleus can hold, in practice.
    #[must_use]
    pub fn from_genome(bytes: &[u8], cfg: VmConfig, seed: u64) -> Option<Sandbox> {
        let genome = Arc::new(Genome::new(bytes.to_vec()).ok()?);
        Some(Sandbox {
            cell: None,
            taken_at_tick: 0,
            gene_hits: vec![0; genome.promoters().len()],
            genome,
            vm: Vm::new(),
            cfg,
            seed,
            // Some fixed key, so two runs of the same buffer are the same run. Zero is as good
            // as any: what it feeds is `RandCtx`, and a scratch cell has no neighbours to be
            // ordered against.
            ordering_key: 0,
            executed: 0,
            in_tick: 0,
            asked: ScratchHost::default(),
        })
    }

    /// Start again from the top with the same genome.
    pub fn reset(&mut self) {
        self.vm = Vm::new();
        self.executed = 0;
        self.in_tick = 0;
        self.asked = ScratchHost::default();
        self.gene_hits = vec![0; self.genome.promoters().len()];
    }

    /// The instruction budget one tick gives a cell.
    #[must_use]
    pub fn budget(&self) -> u32 {
        self.cfg.instr_per_tick as u32
    }

    /// Execute exactly one instruction.
    ///
    /// World-facing opcodes go to a [`ScratchHost`]: in a sandbox there is nothing to eat and
    /// nowhere to emit. So arithmetic, control flow and the stack are exact, and anything that
    /// reads the world reads zero. That limit is real and is why the panel says so — a
    /// debugger that invented plausible sensor readings would be worse than one that admitted
    /// it had none. What the host adds over a [`mm_core::host::NullHost`] is a note of what was
    /// *asked for*, which is a different thing from what was got and is the only one of the two
    /// a scratch cell knows.
    pub fn step(&mut self) -> bool {
        if self.vm.halted && self.in_tick > 0 {
            return false;
        }
        let ctx = RandCtx::new(self.seed, self.taken_at_tick, self.ordering_key);
        // What was about to run, so that where it went can be read afterwards.
        let was = self.next_op();
        let ran = self.vm.run(&self.genome, &self.cfg, &ctx, &mut self.asked, 1);
        if ran > 0 && was == Some(mm_core::isa::Op::Express) {
            self.note_expression();
        }
        self.executed = self.executed.saturating_add(ran);
        self.in_tick = self.in_tick.saturating_add(ran);
        if self.in_tick >= self.budget() {
            // A tick's budget is spent; start the next one the way `Vm::tick` would.
            self.in_tick = 0;
            self.vm.halted = false;
        }
        ran > 0
    }

    /// Attribute an `EXPRESS` that has just run to the gene it reached.
    ///
    /// By watching, not by counting inside the VM. `EXPRESS` either found a promoter and jumped
    /// to its entry, or found none and fell through — so after the instruction has run, the
    /// pointer is either at some `promoters()[i].entry` or it is not. The rule that decided it
    /// is the real one, because it is the real one that just ran; nothing here re-implements the
    /// match, which is the mistake an approximate second definition in a front end always is.
    ///
    /// A promoter's entry is just past its own `GENE` and template, so no two share one, and
    /// this cannot attribute a hit to the wrong gene. Fall-through lands on the byte after the
    /// `EXPRESS` and its template, which is not an entry.
    fn note_expression(&mut self) {
        let at = self.vm.ip;
        if let Some(nth) = self
            .genome
            .promoters()
            .iter()
            .position(|p| p.entry == at)
        {
            if let Some(count) = self.gene_hits.get_mut(nth) {
                *count = count.saturating_add(1);
            }
        }
    }

    /// Run to the end of the current tick's budget, or until it halts.
    pub fn step_tick(&mut self) {
        let start = self.in_tick;
        for _ in start..self.budget() {
            if self.vm.halted {
                break;
            }
            if !self.step() {
                break;
            }
        }
        self.in_tick = 0;
        self.vm.halted = false;
    }

    /// Run until the instruction pointer reaches `offset`, or `limit` instructions pass.
    ///
    /// Bounded, because a genome is a loop and "run to here" on an offset that is never
    /// reached must return rather than hang the front-end.
    pub fn run_to(&mut self, offset: u16, limit: u32) -> bool {
        for _ in 0..limit {
            if self.vm.ip == offset {
                return true;
            }
            if !self.step() {
                return false;
            }
        }
        self.vm.ip == offset
    }

    /// The instruction about to run.
    #[must_use]
    pub fn next_op(&self) -> Option<mm_core::isa::Op> {
        let byte = *self.genome.bytes().get(self.vm.ip as usize)?;
        Some(mm_core::isa::Op::from_byte(byte))
    }

    /// How far the live cell has moved on since this copy was taken.
    ///
    /// Shown in the panel so nobody mistakes a sandbox for the live cell.
    #[must_use]
    pub fn ticks_behind(&self, world: &World) -> u64 {
        world.tick_count().saturating_sub(self.taken_at_tick)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_core::biology::BiologyConfig;
    use mm_core::cell::CellSeed;
    use mm_core::fixed::{pos, q10};
    use mm_core::light::CurrentField;
    use mm_core::{LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding};

    fn petri() -> Scenario {
        Scenario {
            name: "petri".to_string(),
            seed: 1,
            width: 48,
            height: 48,
            light: LightRegime::Uniform {
                intensity: mm_core::Q10_ONE,
            },
            current: CurrentField::Still,
            seeding: vec![
                Seeding::Uniform {
                    chemical: 11,
                    per_square: q10(400),
                },
                Seeding::Uniform {
                    chemical: 14,
                    per_square: q10(400),
                },
                Seeding::Uniform {
                    chemical: 4,
                    per_square: q10(400),
                },
            ],
            ..Scenario::default()
        }
    }

    fn living() -> World {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../genomes/ancestor.mm"
        ))
        .expect("the ancestor is in the repository");
        let bytes = mm_asm::assemble(&src).expect("assembles").bytes;
        let mut world = World::new(petri()).expect("world");
        world.set_biology(BiologyConfig {
            mutation: MutationRates::default(),
            ..BiologyConfig::default()
        });
        for k in 0..6u32 {
            let genome = world.genomes().intern(bytes.clone()).expect("genome");
            let id = world.spawn_cell(CellSeed {
                x: pos((6 + (k % 3) * 14) as i32),
                y: pos((6 + (k / 3) * 14) as i32),
                mass: q10(30),
                energy: q10(400),
                membrane: 24,
                key: 11,
                badge: 0,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome,
            });
            if let Some(i) = world.cells_mut().index(id) {
                let cells = world.cells_mut();
                cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
                cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
                cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
                cells.interior_mut(i)[11] = q10(40);
                cells.interior_mut(i)[14] = q10(40);
            }
        }
        world.adopt_current_contents_as_baseline();
        world
    }

    #[test]
    fn taking_and_stepping_a_sandbox_cannot_touch_the_world() {
        // M6 acceptance 2, in the form that matters: the debugger's most invasive operation,
        // performed over and over, against a world running untouched alongside.
        let mut watched = living();
        let mut clean = living();
        for _ in 0..300 {
            watched.step();
            clean.step();
            if let Some(cell) = watched
                .cells()
                .iter()
                .next()
                .map(|i| watched.cells().id_at(i))
            {
                if let Some(mut sandbox) = Sandbox::of(&watched, cell) {
                    for _ in 0..40 {
                        sandbox.step();
                    }
                    sandbox.step_tick();
                    sandbox.run_to(3, 200);
                    let _ = sandbox.next_op();
                    let _ = sandbox.ticks_behind(&watched);
                }
            }
        }
        assert_eq!(
            watched.state_hash(),
            clean.state_hash(),
            "the debugger reached the simulation"
        );
        assert!(!watched.cells().is_empty());
    }

    #[test]
    fn checking_breakpoints_cannot_touch_the_world() {
        let mut watched = living();
        let mut clean = living();
        let cell = {
            let w = &watched;
            w.cells()
                .iter()
                .next()
                .map(|i| w.cells().id_at(i))
                .expect("a cell")
        };
        let mut points = Breakpoints::new();
        points.add(Breakpoint::AtTick(150));
        points.add(Breakpoint::CellReaches { cell, offset: 4 });
        points.add(Breakpoint::CellDies(cell));
        points.add(Breakpoint::PopulationAbove(8));
        points.add(Breakpoint::PopulationBelow(2));
        points.add(Breakpoint::SpeciesCountReaches(2));

        for _ in 0..300 {
            watched.step();
            clean.step();
            // Checked every tick, rearmed every tick: the most work a breakpoint set can do.
            let _ = points.check(&watched);
            points.rearm();
        }
        assert_eq!(
            watched.state_hash(),
            clean.state_hash(),
            "checking breakpoints changed the run"
        );
    }

    #[test]
    fn a_breakpoint_fires_when_its_condition_holds() {
        let mut world = living();
        let mut points = Breakpoints::new();
        points.add(Breakpoint::AtTick(50));
        let mut stopped_at = None;
        for _ in 0..200 {
            world.step();
            if points.check(&world) {
                stopped_at = Some(world.tick_count());
                break;
            }
        }
        assert_eq!(stopped_at, Some(50));
        assert_eq!(points.tripped(), Some(&Breakpoint::AtTick(50)));
    }

    #[test]
    fn a_disabled_breakpoint_does_not_fire() {
        let mut world = living();
        let mut points = Breakpoints::new();
        points.add(Breakpoint::AtTick(10));
        points.set_enabled(0, false);
        for _ in 0..50 {
            world.step();
            assert!(!points.check(&world), "a disabled breakpoint fired");
        }
    }

    #[test]
    fn a_sandbox_steps_the_instructions_the_cell_would_have_run() {
        let mut world = living();
        world.run(30);
        let cell = world
            .cells()
            .iter()
            .next()
            .map(|i| world.cells().id_at(i))
            .expect("a cell");
        let mut sandbox = Sandbox::of(&world, cell).expect("sandbox");
        let start_ip = sandbox.vm.ip;
        assert!(sandbox.next_op().is_some());
        assert!(sandbox.step(), "the first step did nothing");
        assert!(
            sandbox.vm.ip != start_ip || sandbox.executed == 1,
            "stepping neither moved nor counted"
        );
        assert_eq!(sandbox.executed, 1);
    }

    #[test]
    fn run_to_an_offset_that_never_comes_returns_rather_than_hanging() {
        let mut world = living();
        world.run(10);
        let cell = world
            .cells()
            .iter()
            .next()
            .map(|i| world.cells().id_at(i))
            .expect("a cell");
        let mut sandbox = Sandbox::of(&world, cell).expect("sandbox");
        // An offset past the end of the genome, which the ip can never equal because it wraps.
        let unreachable = u16::MAX;
        assert!(!sandbox.run_to(unreachable, 500));
        assert!(sandbox.executed <= 500);
    }

    #[test]
    fn a_sandbox_of_a_dead_cell_is_none() {
        let mut world = living();
        let cell = world
            .cells()
            .iter()
            .next()
            .map(|i| world.cells().id_at(i))
            .expect("a cell");
        world.cells_mut().despawn(cell);
        assert!(Sandbox::of(&world, cell).is_none());
    }

    #[test]
    fn a_sandbox_says_how_stale_it_is() {
        let mut world = living();
        world.run(20);
        let cell = world
            .cells()
            .iter()
            .next()
            .map(|i| world.cells().id_at(i))
            .expect("a cell");
        let sandbox = Sandbox::of(&world, cell).expect("sandbox");
        assert_eq!(sandbox.ticks_behind(&world), 0);
        world.run(35);
        assert_eq!(sandbox.ticks_behind(&world), 35);
    }
}

#[cfg(test)]
mod scratch_tests {
    use super::*;

    /// Two genes and a driver that expresses each once. `genomes/expression.mm` in miniature.
    const TWO_GENES: &str = "\
        EXPRESS #forage\n\
        EXPRESS #excrete\n\
        HALT\n\
        GENE    #forage\n\
        IMM     20\n\
        IMM     3\n\
        EAT\n\
        DROP\n\
        RET\n\
        GENE    #excrete\n\
        IMM     4\n\
        IMM     9\n\
        EMIT\n\
        DROP\n\
        RET\n";

    fn scratch(src: &str) -> Sandbox {
        let built = mm_asm::assemble(src).expect("the test genome assembles");
        Sandbox::from_genome(&built.bytes, VmConfig::default(), 1)
            .expect("the test genome is a genome")
    }

    #[test]
    fn a_genome_runs_without_a_world_behind_it() {
        let mut s = scratch(TWO_GENES);
        assert_eq!(s.cell, None, "a scratch cell is not a cell in the world");
        for _ in 0..40 {
            s.step();
        }
        assert!(s.executed > 0, "nothing ran");
    }

    #[test]
    fn expressing_a_gene_is_attributed_to_that_gene() {
        // The claim the panel makes. Two genes, one EXPRESS each, so after enough steps both
        // tallies are non-zero — and there are exactly two of them, because there are exactly
        // two GENE headers.
        let mut s = scratch(TWO_GENES);
        assert_eq!(s.gene_hits.len(), 2, "the promoter table is the wrong size");
        for _ in 0..60 {
            s.step();
        }
        assert!(
            s.gene_hits.iter().all(|n| *n > 0),
            "some gene was never reached: {:?}",
            s.gene_hits
        );
    }

    #[test]
    fn nothing_is_attributed_to_a_genome_with_no_genes() {
        // EXPRESS with no promoter to find falls through, and falling through must not be
        // counted as reaching anything.
        let mut s = scratch("EXPRESS #nothing\nHALT\n");
        assert!(s.gene_hits.is_empty());
        for _ in 0..20 {
            s.step();
        }
        assert!(s.gene_hits.is_empty());
    }

    #[test]
    fn a_scratch_cell_is_fed_by_nothing() {
        // The one honest limitation, and the reason this host is not `RecordingHost`: that one
        // returns the full amount asked for, which would show a cell being fed by a world that
        // is not there.
        let mut s = scratch(TWO_GENES);
        for _ in 0..60 {
            s.step();
        }
        assert!(!s.asked.eats.is_empty(), "EAT was never reached");
        // It asked for twenty of chemical three, and the stack got nothing back.
        assert!(s.asked.eats.iter().any(|(amount, chem)| *amount == 20 && *chem == 3));
    }

    #[test]
    fn what_it_asked_to_build_is_what_it_asked_for() {
        let mut s = scratch("IMM 7\nIMM 2\nIMM 1\nBUILD\nHALT\n");
        for _ in 0..10 {
            s.step();
        }
        assert_eq!(s.asked.builds.len(), 1, "{:?}", s.asked.builds);
    }

    #[test]
    fn reaching_split_is_not_dividing() {
        // The number the design would have called "divisions". A scratch cell has no world to
        // divide into; what happened is that SPLIT was reached.
        let mut s = scratch("SPLIT\nSPLIT\nHALT\n");
        for _ in 0..10 {
            s.step();
        }
        assert_eq!(s.asked.splits, 2);
    }

    #[test]
    fn a_reset_run_is_the_same_run_again() {
        // Two runs of one buffer have to agree, or "does this program run" depends on how many
        // times you have asked.
        let mut s = scratch(TWO_GENES);
        for _ in 0..60 {
            s.step();
        }
        let (first_hits, first_eats) = (s.gene_hits.clone(), s.asked.eats.clone());
        s.reset();
        assert_eq!(s.executed, 0);
        assert!(s.gene_hits.iter().all(|n| *n == 0));
        for _ in 0..60 {
            s.step();
        }
        assert_eq!(s.gene_hits, first_hits);
        assert_eq!(s.asked.eats, first_eats);
    }
}
