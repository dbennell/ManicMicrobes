//! The cell arena (SPEC §6.1).
//!
//! Cells live in a struct-of-arrays arena, **not** in the Bevy ECS. Two hundred thousand
//! entities with birth and death every tick means constant archetype churn, and the renderer
//! must never be able to hold the simulation hostage to its own data layout — `mm-core` does
//! not know Bevy exists.
//!
//! # Identity
//!
//! IDs are generational slot-map keys. A slot is reused the moment its occupant dies, so a
//! stale [`CellId`] from last tick would otherwise silently address whoever moved in. The
//! generation counter makes that a lookup failure instead, which matters because junction
//! lists, parentage records and the renderer's selection all hold IDs across ticks.
//!
//! # Ordering
//!
//! Every pass over the population goes in slot order, and every contested resource is settled
//! in cell-id order (SPEC §12). That is not a stylistic choice: it is the whole of I6. A
//! `HashMap` iteration or a rayon completion order anywhere in this file would make results
//! depend on scheduling, and the failure would reproduce only sometimes.
//!
//! # Budget
//!
//! SPEC §6.1 budgets 512 bytes per cell for fixed state, excluding the shared genome. The
//! arena is a set of parallel `Vec`s rather than a `Vec<Cell>` so that a pass touching only
//! positions does not drag sixteen organelle slots through cache with it.

use std::sync::Arc;

use crate::chem::CHEM_COUNT;
use crate::genome::Genome;
use crate::organelle::{Organelle, OrganelleType, SLOT_COUNT};
use crate::state_hash::{StateHash, StateHasher};
use crate::vm::Vm;

/// A stable handle to a cell, valid only while that cell lives.
///
/// The generation is what makes it safe to keep one across ticks: a slot reused by a new cell
/// carries a different generation, so an old handle stops resolving rather than quietly
/// addressing a stranger.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CellId {
    slot: u32,
    generation: u32,
}

impl CellId {
    /// A handle that never resolves. Useful as "no parent" and "nothing selected".
    pub const NONE: CellId = CellId {
        slot: u32::MAX,
        generation: 0,
    };

    #[inline(always)]
    #[must_use]
    pub const fn slot(self) -> u32 {
        self.slot
    }

    #[inline(always)]
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }

    #[inline(always)]
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.slot == u32::MAX
    }

    /// A total order over cells, used wherever a contest has to be settled the same way on
    /// every machine and at every thread count (SPEC §12).
    #[inline(always)]
    #[must_use]
    pub const fn ordering_key(self) -> u64 {
        ((self.slot as u64) << 32) | self.generation as u64
    }
}

/// Everything one cell is, laid out as parallel arrays.
///
/// Adding a field here means adding it to [`CellArena::spawn`], to the snapshot format and to
/// [`StateHash`] — hard rule 7, and the reason those three are next to each other.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CellArena {
    /// Slot occupancy and generation. A free slot has `alive[i] == false` and keeps its
    /// generation so the next occupant gets a fresh one.
    alive: Vec<bool>,
    generation: Vec<u32>,
    /// Free slots, in the order they were freed. A `Vec` used as a stack rather than a set,
    /// because the order has to be reproducible.
    free: Vec<u32>,

    /// Position and velocity, `POS` fixed point in substrate-square units.
    pub x: Vec<i32>,
    pub y: Vec<i32>,
    pub vx: Vec<i32>,
    pub vy: Vec<i32>,

    /// Structural mass, `Q10`.
    pub mass: Vec<i32>,
    /// Stored energy, `Q10`.
    pub energy: Vec<i32>,
    /// Ticks since birth.
    pub age: Vec<u32>,
    /// Membrane damage, `Q10`. A cell dies when this exceeds its membrane investment.
    pub damage: Vec<i32>,

    /// Internal chemistry, `CHEM_COUNT` values per cell, `Q10`.
    pub interior: Vec<i32>,
    /// Organelle loadout, `SLOT_COUNT` per cell.
    pub slots: Vec<Organelle>,

    /// Per-cell VM state.
    pub vm: Vec<Vm>,
    /// The shared, interned genome.
    pub genome: Vec<Arc<Genome>>,
    /// The daughter buffer being filled by `COPYB`, if a `BUD` is in progress.
    pub daughter: Vec<Option<Vec<u8>>>,

    /// 7-bit receptor key (SPEC §8.2).
    pub key: Vec<u8>,
    /// Species assignment (M5).
    pub species: Vec<u32>,
    /// Who this cell divided from.
    pub parent: Vec<CellId>,
    pub birth_tick: Vec<u64>,

    /// How many slots are occupied.
    count: usize,
}

impl CellArena {
    #[must_use]
    pub fn new() -> CellArena {
        CellArena::default()
    }

    /// With room for `capacity` cells reserved up front.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> CellArena {
        let mut a = CellArena::new();
        a.reserve(capacity);
        a
    }

    fn reserve(&mut self, n: usize) {
        self.alive.reserve(n);
        self.generation.reserve(n);
        self.x.reserve(n);
        self.y.reserve(n);
        self.vx.reserve(n);
        self.vy.reserve(n);
        self.mass.reserve(n);
        self.energy.reserve(n);
        self.age.reserve(n);
        self.damage.reserve(n);
        self.interior.reserve(n * CHEM_COUNT);
        self.slots.reserve(n * SLOT_COUNT);
        self.vm.reserve(n);
        self.genome.reserve(n);
        self.daughter.reserve(n);
        self.key.reserve(n);
        self.species.reserve(n);
        self.parent.reserve(n);
        self.birth_tick.reserve(n);
    }

    /// Living cells.
    #[inline(always)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.count
    }

    #[inline(always)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Slots allocated, living or not. Iteration bounds.
    #[inline(always)]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.alive.len()
    }

    #[inline(always)]
    #[must_use]
    pub fn is_alive(&self, id: CellId) -> bool {
        let s = id.slot as usize;
        s < self.alive.len() && self.alive[s] && self.generation[s] == id.generation
    }

    /// The slot index for a live handle, or `None` if it has gone stale.
    #[inline(always)]
    #[must_use]
    pub fn index(&self, id: CellId) -> Option<usize> {
        if self.is_alive(id) {
            Some(id.slot as usize)
        } else {
            None
        }
    }

    /// The handle for a slot, whether or not it is occupied.
    #[inline(always)]
    #[must_use]
    pub fn id_at(&self, slot: usize) -> CellId {
        CellId {
            slot: slot as u32,
            generation: self.generation.get(slot).copied().unwrap_or(0),
        }
    }

    /// Whether a slot holds a living cell.
    #[inline(always)]
    #[must_use]
    pub fn occupied(&self, slot: usize) -> bool {
        self.alive.get(slot).copied().unwrap_or(false)
    }

    /// Every living cell, in slot order — which is the order everything else uses too.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.alive.len()).filter(move |s| self.alive[*s])
    }

    /// Every living cell's handle, in slot order.
    pub fn ids(&self) -> impl Iterator<Item = CellId> + '_ {
        self.iter().map(move |s| self.id_at(s))
    }

    /// Create a cell. Reuses a free slot if there is one, so the arena does not grow while
    /// the population is stable however fast it turns over.
    pub fn spawn(&mut self, seed: CellSeed) -> CellId {
        let slot = match self.free.pop() {
            Some(s) => {
                let i = s as usize;
                self.generation[i] = self.generation[i].wrapping_add(1);
                self.alive[i] = true;
                self.write(i, seed);
                s
            }
            None => {
                let i = self.alive.len();
                self.alive.push(true);
                self.generation.push(0);
                self.x.push(0);
                self.y.push(0);
                self.vx.push(0);
                self.vy.push(0);
                self.mass.push(0);
                self.energy.push(0);
                self.age.push(0);
                self.damage.push(0);
                self.interior.extend(std::iter::repeat_n(0, CHEM_COUNT));
                self.slots
                    .extend(std::iter::repeat_n(Organelle::empty(), SLOT_COUNT));
                self.vm.push(Vm::new());
                self.genome.push(Arc::clone(&seed.genome));
                self.daughter.push(None);
                self.key.push(0);
                self.species.push(0);
                self.parent.push(CellId::NONE);
                self.birth_tick.push(0);
                self.write(i, seed);
                i as u32
            }
        };
        self.count = self.count.saturating_add(1);
        CellId {
            slot,
            generation: self.generation[slot as usize],
        }
    }

    fn write(&mut self, i: usize, seed: CellSeed) {
        self.x[i] = seed.x;
        self.y[i] = seed.y;
        self.vx[i] = 0;
        self.vy[i] = 0;
        self.mass[i] = seed.mass;
        self.energy[i] = seed.energy;
        self.age[i] = 0;
        self.damage[i] = 0;
        self.interior_mut(i).fill(0);
        let slots = self.slots_mut(i);
        slots.fill(Organelle::empty());
        slots[0] = Organelle::finished(OrganelleType::Membrane, seed.membrane);
        self.vm[i] = Vm::new();
        self.genome[i] = seed.genome;
        self.daughter[i] = None;
        self.key[i] = seed.key & 0x7F;
        self.species[i] = seed.species;
        self.parent[i] = seed.parent;
        self.birth_tick[i] = seed.birth_tick;
    }

    /// Remove a cell. Its slot is reused by the next birth, with a fresh generation.
    ///
    /// Returns false for a handle that had already gone stale, so a double-free is a
    /// no-op rather than a corruption.
    pub fn despawn(&mut self, id: CellId) -> bool {
        let Some(i) = self.index(id) else {
            return false;
        };
        self.alive[i] = false;
        // Drop the genome reference immediately: a dead cell holding the last `Arc` would keep
        // a whole lineage's bytes alive until its slot happened to be reused.
        self.daughter[i] = None;
        self.free.push(i as u32);
        self.count = self.count.saturating_sub(1);
        true
    }

    #[inline(always)]
    #[must_use]
    pub fn interior(&self, i: usize) -> &[i32] {
        let base = i * CHEM_COUNT;
        &self.interior[base..base + CHEM_COUNT]
    }

    #[inline(always)]
    pub fn interior_mut(&mut self, i: usize) -> &mut [i32] {
        let base = i * CHEM_COUNT;
        &mut self.interior[base..base + CHEM_COUNT]
    }

    #[inline(always)]
    #[must_use]
    pub fn slots(&self, i: usize) -> &[Organelle] {
        let base = i * SLOT_COUNT;
        &self.slots[base..base + SLOT_COUNT]
    }

    #[inline(always)]
    pub fn slots_mut(&mut self, i: usize) -> &mut [Organelle] {
        let base = i * SLOT_COUNT;
        &mut self.slots[base..base + SLOT_COUNT]
    }

    /// The loadout as a fixed array, for the catalogue's upkeep sum.
    #[must_use]
    pub fn loadout(&self, i: usize) -> [Organelle; SLOT_COUNT] {
        let mut out = [Organelle::empty(); SLOT_COUNT];
        out.copy_from_slice(self.slots(i));
        out
    }

    /// Total of each chemical held inside every living cell.
    ///
    /// Part of the conserved total (I4): matter inside a cell has not left the world, it has
    /// only left the fluid.
    #[must_use]
    pub fn total_interior(&self) -> [i64; CHEM_COUNT] {
        let mut out = [0i64; CHEM_COUNT];
        for i in self.iter() {
            for (c, v) in self.interior(i).iter().enumerate() {
                out[c] = out[c].saturating_add(*v as i64);
            }
        }
        out
    }

    /// Total stored energy across the population, for checking against the ledger.
    #[must_use]
    pub fn total_energy(&self) -> i64 {
        self.iter().map(|i| self.energy[i] as i64).sum()
    }

    /// Distinct genomes currently referenced. Instrumentation for M9's interning statistics.
    #[must_use]
    pub fn distinct_genomes(&self) -> usize {
        let mut hashes: Vec<u64> = self.iter().map(|i| self.genome[i].hash()).collect();
        hashes.sort_unstable();
        hashes.dedup();
        hashes.len()
    }
}

/// What a new cell starts with.
#[derive(Clone, Debug)]
pub struct CellSeed {
    pub x: i32,
    pub y: i32,
    pub mass: i32,
    pub energy: i32,
    pub membrane: u8,
    pub key: u8,
    pub species: u32,
    pub parent: CellId,
    pub birth_tick: u64,
    pub genome: Arc<Genome>,
}

impl StateHash for CellArena {
    fn hash_state(&self, h: &mut StateHasher) {
        // Slot order, not iteration order of anything hash-based (I6). Dead slots contribute
        // their generation so that a world which has churned differently hashes differently.
        h.u64(self.count as u64);
        h.u64(self.alive.len() as u64);
        for i in 0..self.alive.len() {
            h.bool(self.alive[i]);
            h.u32(self.generation[i]);
            if !self.alive[i] {
                continue;
            }
            h.i32(self.x[i]);
            h.i32(self.y[i]);
            h.i32(self.vx[i]);
            h.i32(self.vy[i]);
            h.i32(self.mass[i]);
            h.i32(self.energy[i]);
            h.u32(self.age[i]);
            h.i32(self.damage[i]);
            for v in self.interior(i) {
                h.i32(*v);
            }
            for o in self.slots(i) {
                o.hash_state(h);
            }
            self.vm[i].hash_state(h);
            h.u64(self.genome[i].hash());
            match &self.daughter[i] {
                Some(bytes) => {
                    h.bool(true);
                    h.bytes(bytes);
                }
                None => h.bool(false),
            }
            h.u8(self.key[i]);
            h.u32(self.species[i]);
            h.u64(self.parent[i].ordering_key());
            h.u64(self.birth_tick[i]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genome::GenomePool;

    fn seed(pool: &GenomePool) -> CellSeed {
        CellSeed {
            x: 0,
            y: 0,
            mass: 1000,
            energy: 500,
            membrane: 32,
            key: 5,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome: pool.intern(vec![0x2E]).unwrap(),
        }
    }

    #[test]
    fn a_stale_handle_stops_resolving_rather_than_addressing_a_stranger() {
        // The whole reason for a generation counter. Junction lists, parentage and the
        // renderer's selection all hold ids across ticks.
        let pool = GenomePool::new();
        let mut a = CellArena::new();
        let first = a.spawn(seed(&pool));
        assert!(a.is_alive(first));

        a.despawn(first);
        assert!(!a.is_alive(first));
        assert_eq!(a.index(first), None);

        let second = a.spawn(seed(&pool));
        assert_eq!(second.slot(), first.slot(), "the slot should be reused");
        assert_ne!(second.generation(), first.generation());
        assert!(
            !a.is_alive(first),
            "the old handle must not resolve to the new cell"
        );
        assert!(a.is_alive(second));
    }

    #[test]
    fn despawning_twice_is_a_no_op() {
        let pool = GenomePool::new();
        let mut a = CellArena::new();
        let id = a.spawn(seed(&pool));
        assert!(a.despawn(id));
        assert!(
            !a.despawn(id),
            "a double free must not corrupt the free list"
        );
        assert_eq!(a.len(), 0);
        assert_eq!(a.free.len(), 1);
    }

    #[test]
    fn slots_are_reused_so_a_stable_population_does_not_grow_the_arena() {
        let pool = GenomePool::new();
        let mut a = CellArena::new();
        let mut live: Vec<CellId> = (0..64).map(|_| a.spawn(seed(&pool))).collect();
        assert_eq!(a.capacity(), 64);

        for _ in 0..10_000 {
            let victim = live.remove(0);
            a.despawn(victim);
            live.push(a.spawn(seed(&pool)));
        }
        assert_eq!(a.len(), 64);
        assert_eq!(
            a.capacity(),
            64,
            "ten thousand births reused sixty-four slots"
        );
    }

    #[test]
    fn a_new_cell_starts_clean_in_a_reused_slot() {
        // Otherwise a daughter would inherit the previous occupant's chemistry, organelles
        // and half-finished daughter buffer.
        let pool = GenomePool::new();
        let mut a = CellArena::new();
        let first = a.spawn(seed(&pool));
        let i = a.index(first).unwrap();
        a.interior_mut(i)[3] = 9999;
        a.slots_mut(i)[4] = Organelle::finished(OrganelleType::Chloroplast, 200);
        a.daughter[i] = Some(vec![1, 2, 3]);
        a.age[i] = 500;
        a.despawn(first);

        let second = a.spawn(seed(&pool));
        let j = a.index(second).unwrap();
        assert_eq!(a.interior(j)[3], 0);
        assert_eq!(a.slots(j)[4], Organelle::empty());
        assert_eq!(a.daughter[j], None);
        assert_eq!(a.age[j], 0);
        assert_eq!(
            a.slots(j)[0].kind,
            OrganelleType::Membrane,
            "slot 0 is always the membrane"
        );
    }

    #[test]
    fn iteration_is_in_slot_order_and_skips_the_dead() {
        let pool = GenomePool::new();
        let mut a = CellArena::new();
        let ids: Vec<CellId> = (0..8).map(|_| a.spawn(seed(&pool))).collect();
        a.despawn(ids[2]);
        a.despawn(ids[5]);
        assert_eq!(a.iter().collect::<Vec<_>>(), vec![0, 1, 3, 4, 6, 7]);
        assert_eq!(a.len(), 6);
    }

    #[test]
    fn ordering_keys_are_a_total_order() {
        let pool = GenomePool::new();
        let mut a = CellArena::new();
        let ids: Vec<CellId> = (0..32).map(|_| a.spawn(seed(&pool))).collect();
        let mut keys: Vec<u64> = ids.iter().map(|i| i.ordering_key()).collect();
        let sorted = {
            let mut k = keys.clone();
            k.sort_unstable();
            k
        };
        assert_eq!(keys, sorted, "slot order is already id order");
        keys.dedup();
        assert_eq!(keys.len(), 32, "ids must be distinct");
    }

    #[test]
    fn interior_totals_count_every_living_cell_and_no_dead_ones() {
        let pool = GenomePool::new();
        let mut a = CellArena::new();
        let ids: Vec<CellId> = (0..4).map(|_| a.spawn(seed(&pool))).collect();
        for (n, id) in ids.iter().enumerate() {
            let i = a.index(*id).unwrap();
            a.interior_mut(i)[7] = 100 * (n as i32 + 1);
        }
        assert_eq!(a.total_interior()[7], 100 + 200 + 300 + 400);
        a.despawn(ids[1]);
        assert_eq!(a.total_interior()[7], 100 + 300 + 400);
    }

    #[test]
    fn the_state_hash_notices_every_field() {
        let pool = GenomePool::new();
        let mut a = CellArena::new();
        let id = a.spawn(seed(&pool));
        let i = a.index(id).unwrap();
        let base = a.state_hash();

        type Mutate = Box<dyn Fn(&mut CellArena)>;
        let mutations: Vec<Mutate> = vec![
            Box::new(|a: &mut CellArena| a.x[0] += 1),
            Box::new(|a: &mut CellArena| a.y[0] += 1),
            Box::new(|a: &mut CellArena| a.vx[0] += 1),
            Box::new(|a: &mut CellArena| a.vy[0] += 1),
            Box::new(|a: &mut CellArena| a.mass[0] += 1),
            Box::new(|a: &mut CellArena| a.energy[0] += 1),
            Box::new(|a: &mut CellArena| a.age[0] += 1),
            Box::new(|a: &mut CellArena| a.damage[0] += 1),
            Box::new(|a: &mut CellArena| a.interior_mut(0)[0] += 1),
            Box::new(|a: &mut CellArena| a.slots_mut(0)[1].param = 9),
            Box::new(|a: &mut CellArena| a.vm[0].pa += 1),
            Box::new(|a: &mut CellArena| a.daughter[0] = Some(vec![7])),
            Box::new(|a: &mut CellArena| a.key[0] = 33),
            Box::new(|a: &mut CellArena| a.species[0] = 4),
            Box::new(|a: &mut CellArena| a.birth_tick[0] = 12),
        ];
        for (n, mutate) in mutations.iter().enumerate() {
            let mut copy = a.clone();
            mutate(&mut copy);
            assert_ne!(
                copy.state_hash(),
                base,
                "mutation {n} did not reach the state hash, so it would not reach a snapshot"
            );
        }
        let _ = i;
    }

    #[test]
    fn distinct_genomes_counts_what_interning_saved() {
        let pool = GenomePool::new();
        let mut a = CellArena::new();
        for _ in 0..100 {
            a.spawn(seed(&pool));
        }
        assert_eq!(a.len(), 100);
        assert_eq!(
            a.distinct_genomes(),
            1,
            "a clonal population shares one genome"
        );
    }
}
