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
use mm_core::host::NullHost;
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
    /// Which cell this was taken from, and when.
    pub cell: CellId,
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
}

impl Sandbox {
    /// Take a copy of a live cell. `None` if it is not alive.
    #[must_use]
    pub fn of(world: &World, cell: CellId) -> Option<Sandbox> {
        let i = world.cells().index(cell)?;
        Some(Sandbox {
            cell,
            taken_at_tick: world.tick_count(),
            genome: Arc::clone(&world.cells().genome[i]),
            vm: world.cells().vm[i].clone(),
            cfg: world.scenario().vm,
            seed: world.scenario().seed,
            ordering_key: cell.ordering_key(),
            executed: 0,
            in_tick: 0,
        })
    }

    /// The instruction budget one tick gives a cell.
    #[must_use]
    pub fn budget(&self) -> u32 {
        self.cfg.instr_per_tick as u32
    }

    /// Execute exactly one instruction.
    ///
    /// World-facing opcodes go to a [`NullHost`]: in a sandbox there is nothing to eat and
    /// nowhere to emit. So arithmetic, control flow and the stack are exact, and anything that
    /// reads the world reads zero. That limit is real and is why the panel says so — a
    /// debugger that invented plausible sensor readings would be worse than one that admitted
    /// it had none.
    pub fn step(&mut self) -> bool {
        if self.vm.halted && self.in_tick > 0 {
            return false;
        }
        let ctx = RandCtx::new(self.seed, self.taken_at_tick, self.ordering_key);
        let mut host = NullHost;
        let ran = self.vm.run(&self.genome, &self.cfg, &ctx, &mut host, 1);
        self.executed = self.executed.saturating_add(ran);
        self.in_tick = self.in_tick.saturating_add(ran);
        if self.in_tick >= self.budget() {
            // A tick's budget is spent; start the next one the way `Vm::tick` would.
            self.in_tick = 0;
            self.vm.halted = false;
        }
        ran > 0
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
