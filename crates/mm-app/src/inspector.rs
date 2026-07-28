//! The cell inspector (SPEC §14, M4).
//!
//! > Cell inspector showing live registers, stack, organelle slots, internal chemistry,
//! > junctions and current species.
//!
//! A read-only transcript of one cell, taken the same way a [`crate::slide::Frame`] is taken:
//! everything is copied out, so the panel that displays it holds no borrow of the world and
//! cannot write back through one. Inspecting is looking, and looking is free.
//!
//! The stack is presented **bottom-to-top in the order the VM would pop**, not in the order
//! the underlying array happens to be laid out. The data stack is circular (SPEC §3), so the
//! raw array is rotated by however many pushes have happened and reading it directly shows a
//! stack that appears to shuffle itself between ticks. Anyone debugging a genome needs
//! `[.., under, top]`, so that is what this produces.

use mm_core::cell::{CellArena, CellId};
use mm_core::chem::CHEM_COUNT;
use mm_core::organelle::{OrganelleType, SLOT_COUNT};
use mm_core::vm::{CALL_STACK_LEN, DATA_STACK_LEN, RAM_WORDS, REGISTER_COUNT};
use mm_core::World;

/// One organelle slot, as the panel shows it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotView {
    pub index: usize,
    pub kind: OrganelleType,
    pub param: u8,
    pub control: [i16; 2],
    /// Build progress: `None` once finished, otherwise the bytes still owed.
    pub remaining_build: Option<u16>,
    pub active: bool,
}

/// Everything worth knowing about one cell, copied out.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Inspection {
    pub id: CellId,
    pub species: u32,
    pub parent: CellId,
    pub birth_tick: u64,
    pub age: u32,
    pub x: i32,
    pub y: i32,

    pub mass: i32,
    pub energy: i32,
    pub damage: i32,
    pub key: u8,

    pub genome_len: usize,
    pub genome_hash: u64,
    /// Nucleus copy fidelity, `Q10`. The evolvable mutation rate, made visible.
    pub fidelity: i32,

    /// Data stack, deepest first, top last.
    pub stack: Vec<i16>,
    /// Return addresses, outermost first.
    pub call_stack: Vec<u16>,
    pub registers: [i16; REGISTER_COUNT],
    pub ram: [i16; RAM_WORDS],
    pub ip: u16,
    /// Copy pointers and counter — the state of a division in progress.
    pub pa: u16,
    pub pb: u16,
    pub ln: u16,
    pub halted: bool,

    pub slots: [SlotView; SLOT_COUNT],
    pub interior: [i32; CHEM_COUNT],
}

impl Inspection {
    /// Take a reading of one cell, or `None` if it is not alive.
    #[must_use]
    pub fn of(world: &World, id: CellId) -> Option<Inspection> {
        let cells = world.cells();
        let i = cells.index(id)?;
        Some(Inspection::of_slot(world, cells, i))
    }

    fn of_slot(world: &World, cells: &CellArena, i: usize) -> Inspection {
        let vm = &cells.vm[i];
        let mut slots = [SlotView {
            index: 0,
            kind: OrganelleType::Membrane,
            param: 0,
            control: [0; 2],
            remaining_build: None,
            active: false,
        }; SLOT_COUNT];
        for (n, o) in cells.slots(i).iter().enumerate() {
            slots[n] = SlotView {
                index: n,
                kind: o.kind,
                param: o.param,
                control: o.control,
                remaining_build: (o.remaining_build > 0).then_some(o.remaining_build),
                active: o.is_active(),
            };
        }
        let mut interior = [0i32; CHEM_COUNT];
        interior.copy_from_slice(cells.interior(i));

        Inspection {
            id: cells.id_at(i),
            species: cells.species[i],
            parent: cells.parent[i],
            birth_tick: cells.birth_tick[i],
            age: cells.age[i],
            x: cells.x[i],
            y: cells.y[i],
            mass: cells.mass[i],
            energy: cells.energy[i],
            damage: cells.damage[i],
            key: cells.key[i],
            genome_len: cells.genome[i].len(),
            genome_hash: cells.genome[i].hash(),
            fidelity: mm_core::biology::nucleus_fidelity(cells, i),
            stack: unwind(&vm.data, vm.dsp, vm.dlen, DATA_STACK_LEN),
            call_stack: unwind(&vm.call, vm.csp, vm.clen, CALL_STACK_LEN),
            registers: vm.regs,
            ram: vm.ram,
            ip: vm.ip,
            pa: vm.pa,
            pb: vm.pb,
            ln: vm.ln,
            halted: vm.halted,
            slots,
            interior: {
                let _ = world;
                interior
            },
        }
    }
}

/// Read a circular stack out in pop order: deepest entry first, top of stack last.
fn unwind<T: Copy>(buf: &[T], sp: u8, len: u8, capacity: usize) -> Vec<T> {
    let live = (len as usize).min(capacity).min(buf.len());
    if live == 0 || capacity == 0 {
        return Vec::new();
    }
    // `sp` indexes the top. Walk back `live` entries and read forward, so the caller gets the
    // order they would pop in reversed — which is the order a stack is drawn in.
    (0..live)
        .rev()
        .filter_map(|back| {
            let at = (sp as usize).wrapping_sub(back) % capacity;
            buf.get(at).copied()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_core::fixed::{pos, q10};
    use mm_core::{CellSeed, Organelle, Scenario};

    fn world_with_a_cell() -> (World, CellId) {
        let mut world = World::new(Scenario::stress(16, 16)).unwrap();
        let genome = world.genomes().intern(vec![0u8; 32]).unwrap();
        let id = world.spawn_cell(CellSeed {
            x: pos(4),
            y: pos(5),
            mass: q10(20),
            energy: q10(100),
            membrane: 16,
            key: 7,
            species: 3,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        if let Some(i) = world.cells_mut().index(id) {
            world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
            world.cells_mut().interior_mut(i)[8] = q10(5);
        }
        world.adopt_current_contents_as_baseline();
        (world, id)
    }

    #[test]
    fn an_inspection_describes_the_cell_it_was_taken_from() {
        let (world, id) = world_with_a_cell();
        let v = Inspection::of(&world, id).expect("the cell is alive");
        assert_eq!(v.id, id);
        assert_eq!(v.species, 3);
        assert_eq!(v.key, 7);
        assert_eq!(v.genome_len, 32);
        assert_eq!(v.energy, q10(100));
        assert_eq!(v.interior[8], q10(5));
        assert_eq!(v.slots[1].kind, OrganelleType::Nucleus);
        assert_eq!(v.slots[1].param, 40);
        assert!(
            v.slots[1].remaining_build.is_none(),
            "it was built finished"
        );
        assert_eq!(v.fidelity, mm_core::Q10_ONE, "a fresh nucleus is at full");
    }

    #[test]
    fn inspecting_a_dead_cell_finds_nothing() {
        let (mut world, id) = world_with_a_cell();
        world.cells_mut().despawn(id);
        assert_eq!(Inspection::of(&world, id), None);
    }

    #[test]
    fn inspecting_does_not_change_the_world() {
        let (mut world, id) = world_with_a_cell();
        world.step();
        let before = world.state_hash();
        for _ in 0..100 {
            let _ = Inspection::of(&world, id);
        }
        assert_eq!(world.state_hash(), before);
    }

    #[test]
    fn the_stack_reads_top_last() {
        // The data stack is circular, so the raw array is rotated by however many pushes have
        // happened. What the panel must show is push order, whatever the rotation is.
        let capacity = 8usize;
        let buf: Vec<i16> = (0..capacity as i16).collect();
        // Three live entries with the top at index 1: pushes landed on 7, 0, 1.
        assert_eq!(unwind(&buf, 1, 3, capacity), vec![7, 0, 1]);
        // Not wrapped: top at 4, four live.
        assert_eq!(unwind(&buf, 4, 4, capacity), vec![1, 2, 3, 4]);
        // Empty and over-full both behave.
        assert_eq!(unwind(&buf, 4, 0, capacity), Vec::<i16>::new());
        assert_eq!(unwind(&buf, 0, 200, capacity).len(), capacity);
    }
}
