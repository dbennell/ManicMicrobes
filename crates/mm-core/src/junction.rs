//! Junctions: how cells become organisms (SPEC §8, M7).
//!
//! # Two kinds, one table
//!
//! A **soft** junction is a channel. It moves chemicals, energy and genome bytes, constrains
//! nothing positionally, and breaks when the two cells drift beyond `soft_max_range`. It is
//! the conjugation channel, the synapse and the infection route, all the same mechanism.
//!
//! A **hard** junction is structure. It carries a distance constraint, and it is what
//! multicellularity is made of.
//!
//! # Junctions are symmetric
//!
//! When A joins B, both get a slot. SPEC does not say so outright, and the alternative — only
//! the initiator holds a handle — was considered and rejected, because it makes the target
//! unable to see a junction it is part of. A host that cannot perceive the parasite attached
//! to it cannot evolve to `LEAVE`, and SPEC §8.2 is explicit that host defence should be "a
//! real trade-off" and a "genuine Red Queen dynamic with a cost on both sides". A one-sided
//! junction has the cost on one side only.
//!
//! It also makes the physics honest: a distance constraint is a relationship between two
//! cells, and storing it once per side means the solver sees it twice and each cell's own
//! `LEAVE` means what it says.
//!
//! # The handle problem
//!
//! `JOIN` takes a *handle* naming the target. The handle a genome has is whatever its touch
//! sensor gave it — `TouchSensor` reading 1 is the nearest neighbour's slot — so a handle is
//! an arena slot index, read this tick and used this tick. That is deliberately fragile:
//! slots are reused, so a handle kept across ticks may name a different cell. Resolving one
//! therefore checks that the named slot is occupied and within reach, and refuses otherwise.
//! A genome cannot reach across the slide by inventing a number.
//!
//! # What this must never become
//!
//! Junctions do not couple to the fluid. No torque, no angular dynamics, no lever arms, no
//! backpressure — SPEC §8.4 and CLAUDE.md both say so, and both say it was decided
//! deliberately for performance. A cluster does not paddle. Cilia on one cell push *that*
//! cell and the constraints drag the rest, which is what makes colony locomotion emergent
//! rather than a special case in the engine.

use crate::cell::{CellArena, CellId};
use crate::fixed::{pos_to_square, POS_ONE};
use crate::state_hash::{StateHash, StateHasher};

/// Junction slots per cell.
///
/// Four rather than eight. A junction is 12 bytes, and SPEC §6.1 budgets 512 for a whole cell
/// — the VM alone is most of it. Four is enough for a chain, a branch or a small sheet, which
/// is what the acceptance tests need; a cell that wants more is describing a different design
/// than the one budgeted for.
pub const JUNCTIONS_PER_CELL: usize = 4;

/// Reduce a junction operand into range. Addressing wraps (hard rule 4).
#[inline(always)]
#[must_use]
pub const fn junction_index(idx: i16) -> usize {
    (idx as u16 as usize) % JUNCTIONS_PER_CELL
}

/// What a junction carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum JunctionKind {
    /// No junction here.
    #[default]
    None,
    /// A transfer channel. No positional constraint.
    Soft,
    /// Structure: a distance constraint.
    Hard,
}

impl JunctionKind {
    /// Decode a `JOIN` kind operand. Total: anything even is soft, anything odd is hard, so
    /// a mutation to the operand flips the kind rather than landing on an invalid value.
    #[inline]
    #[must_use]
    pub const fn from_operand(kind: i16) -> JunctionKind {
        if kind & 1 == 0 {
            JunctionKind::Soft
        } else {
            JunctionKind::Hard
        }
    }

    #[must_use]
    pub const fn is_some(self) -> bool {
        !matches!(self, JunctionKind::None)
    }
}

/// One end of a junction, as one cell sees it.
///
/// Twelve bytes. Both cells hold one, so a junction costs twenty-four bytes across the pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Junction {
    pub kind: JunctionKind,
    /// The cell at the other end. Generational, so a junction to a dead cell is detectable
    /// rather than a dangling index into a reused slot.
    pub other: CellId,
    /// Rest length for a hard junction, `POS` units. `JLEN` modulates it, which is muscle.
    pub rest: i32,
}

impl Default for Junction {
    fn default() -> Junction {
        Junction::empty()
    }
}

impl Junction {
    #[must_use]
    pub const fn empty() -> Junction {
        Junction {
            kind: JunctionKind::None,
            other: CellId::NONE,
            rest: 0,
        }
    }

    #[must_use]
    pub const fn is_some(&self) -> bool {
        self.kind.is_some()
    }
}

impl StateHash for Junction {
    fn hash_state(&self, h: &mut StateHasher) {
        h.u8(match self.kind {
            JunctionKind::None => 0,
            JunctionKind::Soft => 1,
            JunctionKind::Hard => 2,
        });
        h.u64(self.other.ordering_key());
        h.i32(self.rest);
    }
}

/// The numbers that make junctions behave the way SPEC §8.2 describes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JunctionConfig {
    /// Energy to form a junction with a matching key, `Q10`. Meant to be nearly free.
    pub join_base_cost: i32,
    /// Extra energy per unit of the target's membrane investment when the key does *not*
    /// match, `Q10`. This is what makes consent economic rather than absolute.
    pub join_forced_penalty: i32,
    /// A soft junction breaks beyond this range, `POS` units.
    pub soft_max_range: i32,
    /// A hard junction breaks when stretched this far past its rest length, `POS` units.
    pub breaking_strain: i32,
    /// How much of the position error one Gauss-Seidel iteration corrects, `Q10`.
    pub stiffness: i32,
    /// Gauss-Seidel iterations per tick. SPEC §8.4 says two or three.
    pub iterations: u8,
    /// How far `JLEN` may move a rest length from its natural value, `POS` units.
    pub muscle_range: i32,
    /// Whether a failed `JOIN` leaks how close the key was.
    ///
    /// Off, and it must stay off by default: SPEC §8.2 is explicit that returning Hamming
    /// distance makes the key hill-climbable in about seven probes and parasitism trivial.
    /// It exists as a knob for anyone who wants to watch that happen deliberately.
    pub probe_leaks_distance: bool,
    /// Energy per unit transferred across a soft junction, `Q10`.
    pub transfer_cost: i32,
}

impl Default for JunctionConfig {
    fn default() -> JunctionConfig {
        JunctionConfig {
            // Cheap enough that a clonal colony is not paying to exist, which is the whole
            // point of SPEC §8.2's first claim.
            join_base_cost: crate::Q10_ONE / 2,
            // Against a membrane investment of ~24, a forced join costs about 48 energy where
            // a consensual one costs half of one. Two orders of magnitude, which is what makes
            // brute-forcing 128 keys a real decision rather than a formality.
            join_forced_penalty: crate::Q10_ONE * 2,
            soft_max_range: POS_ONE * 3,
            breaking_strain: POS_ONE * 2,
            // Well under one: over-correcting a Gauss-Seidel constraint makes a chain
            // oscillate, and a colony that vibrates is a colony that flies apart.
            stiffness: crate::Q10_ONE / 2,
            iterations: 3,
            muscle_range: POS_ONE,
            probe_leaks_distance: false,
            transfer_cost: crate::Q10_ONE / 64,
        }
    }
}

/// What `JOIN` costs, given whether the key matched.
///
/// Separate from the act of joining so the acceptance test can check the arithmetic directly
/// against the ledger, which is what M7's first acceptance test asks for.
#[must_use]
pub fn join_cost(config: &JunctionConfig, matched: bool, target_membrane: u8) -> i32 {
    if matched {
        config.join_base_cost
    } else {
        config
            .join_base_cost
            .saturating_add(crate::fixed::q10_scale(
                config.join_forced_penalty,
                crate::fixed::q10(target_membrane as i32),
            ))
    }
}

/// The first free junction slot on a cell, if it has one.
#[must_use]
pub fn free_slot(cells: &CellArena, i: usize) -> Option<usize> {
    cells.junctions(i).iter().position(|j| !j.is_some())
}

/// Whether two cells are already joined, and how.
#[must_use]
pub fn existing(cells: &CellArena, i: usize, other: CellId) -> Option<usize> {
    cells
        .junctions(i)
        .iter()
        .position(|j| j.is_some() && j.other == other)
}

/// Distance between two cells, `POS` units.
///
/// Octagonal, matching `neighbours::separation` — the same approximation everywhere, so a
/// junction that a cell can feel is a junction that can form.
#[inline]
#[must_use]
pub fn distance(cells: &CellArena, i: usize, j: usize) -> i32 {
    let dx = (cells.x[i] - cells.x[j]).abs();
    let dy = (cells.y[i] - cells.y[j]).abs();
    let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
    hi.saturating_add(lo / 2)
}

/// Break every junction whose other end has died or drifted out of range.
///
/// Returns how many were broken. Run once a tick, before the constraint solve, so the solver
/// never sees a junction to a cell that is not there.
pub fn prune(cells: &mut CellArena, config: &JunctionConfig) -> u32 {
    let mut broken = 0;
    for i in 0..cells.capacity() {
        if !cells.occupied(i) {
            continue;
        }
        for slot in 0..JUNCTIONS_PER_CELL {
            let junction = cells.junctions(i)[slot];
            if !junction.is_some() {
                continue;
            }
            let gone = match cells.index(junction.other) {
                None => true,
                Some(other) => {
                    let d = distance(cells, i, other);
                    match junction.kind {
                        JunctionKind::Soft => d > config.soft_max_range,
                        JunctionKind::Hard => {
                            d > junction.rest.saturating_add(config.breaking_strain)
                        }
                        JunctionKind::None => true,
                    }
                }
            };
            if gone {
                cells.junctions_mut(i)[slot] = Junction::empty();
                broken += 1;
            }
        }
    }
    broken
}

/// Solve the hard-junction distance constraints (SPEC §8.4).
///
/// Position-based dynamics: a few Gauss-Seidel passes, each moving both cells a fraction of
/// the way towards satisfying the constraint, weighted by mass so a heavy cell moves less than
/// a light one hanging off it.
///
/// # Determinism
///
/// Gauss-Seidel is order-dependent by nature — each constraint sees the positions the previous
/// ones left. So the order is slot order over cells and then junction slot, which is the same
/// on every machine and at every thread count (I1, I6). It is deliberately *not* parallelised:
/// a parallel Jacobi sweep would be order-independent but converges differently, and the two
/// would not agree.
///
/// Returns how many constraints were solved, for the metrics and the performance gate.
pub fn solve(
    cells: &mut CellArena,
    config: &JunctionConfig,
    scratch: &mut Vec<(u32, u32, i32)>,
) -> u32 {
    // Gather the constraints once, then iterate over the list.
    //
    // The obvious loop walks the arena inside each Gauss-Seidel pass, re-checking occupancy,
    // re-scanning four junction slots per cell and re-resolving a generational id per junction
    // — three times over, for work whose answer cannot change between passes. At fifty
    // thousand junctions that measured 9.5% of the tick against SPEC §8.4's estimate of one to
    // two.
    //
    // Gathering first also fixes the order once. Gauss-Seidel is order-dependent by nature —
    // each constraint sees what the previous ones left — so the order has to be identical on
    // every machine and at every thread count (I1, I6), and a list built once in slot order is
    // easier to be sure of than three walks that must agree.
    scratch.clear();
    for i in 0..cells.capacity() {
        if !cells.occupied(i) {
            continue;
        }
        for slot in 0..JUNCTIONS_PER_CELL {
            let junction = cells.junctions(i)[slot];
            if junction.kind != JunctionKind::Hard {
                continue;
            }
            let Some(j) = cells.index(junction.other) else {
                continue;
            };
            // Each pair once, by its lower slot. Solving from both ends would apply the
            // correction twice and double the effective stiffness, which is how a chain
            // starts to ring.
            if j <= i {
                continue;
            }
            scratch.push((i as u32, j as u32, junction.rest));
        }
    }

    for _ in 0..config.iterations.max(1) {
        for (i, j, rest) in scratch.iter() {
            apply_constraint(cells, *i as usize, *j as usize, *rest, config);
        }
    }
    scratch.len() as u32
}

/// One distance constraint, mass-weighted.
fn apply_constraint(cells: &mut CellArena, i: usize, j: usize, rest: i32, config: &JunctionConfig) {
    let dx = cells.x[i] - cells.x[j];
    let dy = cells.y[i] - cells.y[j];
    let d = distance(cells, i, j);
    if d == 0 {
        // Exactly coincident: no line to push along. Nudged apart deterministically by slot,
        // the same tie-break collision separation uses.
        let push = if i % 2 == 0 {
            POS_ONE / 8
        } else {
            -POS_ONE / 8
        };
        cells.x[i] = cells.x[i].saturating_add(push);
        cells.x[j] = cells.x[j].saturating_sub(push);
        return;
    }
    let error = d - rest;
    if error == 0 {
        return;
    }

    // How much of the error to take out this pass.
    let correction = crate::fixed::q10_scale(error, config.stiffness);
    if correction == 0 {
        return;
    }

    // Mass weighting: the lighter cell moves further. Shares sum to one, so the pair's centre
    // of mass does not drift — a constraint that moved both cells equally would let a heavy
    // cell be dragged around by a light one, and a chain would creep.
    let (mi, mj) = (cells.mass[i].max(1) as i64, cells.mass[j].max(1) as i64);
    let total = mi + mj;
    let share_i = ((mj * 1024) / total) as i32;
    let share_j = 1024 - share_i;

    let move_along = |delta: i32, share: i32| -> i32 {
        // The component of the correction along this axis, scaled by how much of the total
        // separation this axis accounts for.
        let along = (delta as i64 * correction as i64) / d.max(1) as i64;
        ((along * share as i64) / 1024) as i32
    };

    let (cx, cy) = (move_along(dx, share_i), move_along(dy, share_i));
    let (ox, oy) = (move_along(dx, share_j), move_along(dy, share_j));
    cells.x[i] = cells.x[i].saturating_sub(cx);
    cells.y[i] = cells.y[i].saturating_sub(cy);
    cells.x[j] = cells.x[j].saturating_add(ox);
    cells.y[j] = cells.y[j].saturating_add(oy);
}

/// Connected components over hard junctions — "which cells constitute one organism".
///
/// # Rebuilt rather than incremental
///
/// SPEC §8.4 asks for an incremental union-find updated on join and dissolve. This rebuilds
/// each time it is asked. The reason is that incremental union-find does not support deletion
/// without either a rollback log or a rebuild anyway, and junctions break constantly — every
/// death, every drift out of range. A structure that must be rebuilt whenever anything is
/// removed is a rebuild with extra steps and an extra way to be wrong.
///
/// The cost is linear with near-constant path compression, and it is only paid when something
/// asks. If the performance gate ever says otherwise, the incremental version is the fix and
/// this comment is where to start.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Components {
    /// Parent per arena slot. A root points at itself.
    parent: Vec<u32>,
    /// Members per root, only populated after `rebuild`.
    sizes: Vec<u32>,
}

impl Components {
    #[must_use]
    pub fn new() -> Components {
        Components::default()
    }

    /// Rebuild from the arena's hard junctions.
    pub fn rebuild(&mut self, cells: &CellArena) {
        let n = cells.capacity();
        self.parent.clear();
        self.parent.extend(0..n as u32);
        self.sizes.clear();
        self.sizes.resize(n, 0);

        for i in 0..n {
            if !cells.occupied(i) {
                continue;
            }
            for junction in cells.junctions(i) {
                if junction.kind != JunctionKind::Hard {
                    continue;
                }
                if let Some(j) = cells.index(junction.other) {
                    self.union(i, j);
                }
            }
        }
        for i in 0..n {
            if cells.occupied(i) {
                let root = self.find(i);
                self.sizes[root] = self.sizes[root].saturating_add(1);
            }
        }
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] as usize != i {
            // Path halving: every lookup shortens the chain it walked.
            let grandparent = self.parent[self.parent[i] as usize];
            self.parent[i] = grandparent;
            i = grandparent as usize;
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        // Lower slot wins, so the root of a component does not depend on the order joins
        // happened in — two worlds with the same junctions get the same components.
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        self.parent[hi] = lo as u32;
    }

    /// Which component a cell belongs to, as its root slot.
    #[must_use]
    pub fn component_of(&mut self, i: usize) -> usize {
        if i < self.parent.len() {
            self.find(i)
        } else {
            i
        }
    }

    /// How many cells are in a cell's component, including itself.
    #[must_use]
    pub fn size_of(&mut self, i: usize) -> u32 {
        let root = self.component_of(i);
        self.sizes.get(root).copied().unwrap_or(0)
    }

    /// The largest component on the slide.
    #[must_use]
    pub fn largest(&self) -> u32 {
        self.sizes.iter().copied().max().unwrap_or(0)
    }

    /// Every component with at least `min` members, as (root, size), ascending by root.
    #[must_use]
    pub fn clusters(&self, min: u32) -> Vec<(usize, u32)> {
        self.sizes
            .iter()
            .enumerate()
            .filter(|(_, n)| **n >= min)
            .map(|(root, n)| (root, *n))
            .collect()
    }
}

/// The centre of a cluster, for rendering and for the tweezers.
#[must_use]
pub fn cluster_centre(cells: &CellArena, components: &mut Components, root: usize) -> (i32, i32) {
    let mut sx = 0i64;
    let mut sy = 0i64;
    let mut n = 0i64;
    for i in cells.iter() {
        if components.component_of(i) == root {
            sx += cells.x[i] as i64;
            sy += cells.y[i] as i64;
            n += 1;
        }
    }
    if n == 0 {
        return (0, 0);
    }
    ((sx / n) as i32, (sy / n) as i32)
}

/// Whether a cluster holds two or more distinct organelle loadouts — differentiation.
///
/// The measure M7's sixth acceptance test is about, and the one the `DifferentiatedCluster`
/// detector has been declaring since M5 without being able to fire.
///
/// A loadout is the multiset of organelle types present, ignoring size and control inputs:
/// two cells that both carry one chloroplast are the same kind of cell even if one's is
/// bigger. Anything finer would call a colony differentiated because two members happened to
/// be mid-build.
#[must_use]
pub fn distinct_loadouts(cells: &CellArena, components: &mut Components, root: usize) -> usize {
    let mut seen: std::collections::BTreeSet<[u8; crate::organelle::SLOT_COUNT]> =
        Default::default();
    for i in cells.iter() {
        if components.component_of(i) != root {
            continue;
        }
        let mut counts = [0u8; crate::organelle::SLOT_COUNT];
        for o in cells.slots(i) {
            if o.is_active() {
                let k = o.kind as usize % crate::organelle::SLOT_COUNT;
                counts[k] = counts[k].saturating_add(1);
            }
        }
        seen.insert(counts);
    }
    seen.len()
}

/// Resolve a `JOIN` handle to an arena slot.
///
/// A handle is whatever the touch sensor gave the genome — an arena slot index. Checked
/// rather than trusted: the slot must be occupied, must not be the caller, and must be within
/// touching distance. A genome cannot reach across the slide by inventing a number, and a
/// handle held over from a previous tick names whoever is in that slot now, which is why one
/// should not be.
#[must_use]
pub fn resolve_handle(cells: &CellArena, i: usize, handle: i16, reach: i32) -> Option<usize> {
    let j = (handle as u16 as usize) % cells.capacity().max(1);
    if j == i || !cells.occupied(j) {
        return None;
    }
    if distance(cells, i, j) > reach {
        return None;
    }
    Some(j)
}

/// How far a cell can reach to form a junction, `POS` units.
///
/// Its own radius plus the target's, plus a margin — the same "touching" test the touch sensor
/// uses, so a genome that can feel a neighbour can join it.
#[must_use]
pub fn reach(cells: &CellArena, i: usize) -> i32 {
    // `radius` is `Q10`; a reach is `POS`. Converted rather than added straight, which is what
    // the first version did — cells could join from eight squares away.
    crate::fixed::q10_to_pos(crate::biology::radius(cells, i).saturating_mul(2))
        .saturating_add(POS_ONE)
}

/// Where a junction should be drawn, in substrate squares.
#[must_use]
pub fn endpoints(cells: &CellArena, i: usize, j: usize) -> ((i32, i32), (i32, i32)) {
    (
        (pos_to_square(cells.x[i]), pos_to_square(cells.y[i])),
        (pos_to_square(cells.x[j]), pos_to_square(cells.y[j])),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::CellSeed;
    use crate::fixed::{pos, q10};
    use crate::genome::{Genome, GenomePool};
    use std::sync::Arc;

    fn arena() -> (CellArena, GenomePool) {
        (CellArena::new(), GenomePool::new())
    }

    fn spawn(cells: &mut CellArena, pool: &GenomePool, x: i32, y: i32, mass: i32) -> CellId {
        let genome: Arc<Genome> = pool.intern(vec![0x2E; 8]).expect("genome");
        cells.spawn(CellSeed {
            x: pos(x),
            y: pos(y),
            mass: q10(mass),
            energy: q10(100),
            membrane: 24,
            key: 11,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        })
    }

    fn join(cells: &mut CellArena, a: usize, b: usize, kind: JunctionKind, rest: i32) {
        let (ida, idb) = (cells.id_at(a), cells.id_at(b));
        let sa = free_slot(cells, a).expect("a slot");
        cells.junctions_mut(a)[sa] = Junction {
            kind,
            other: idb,
            rest,
        };
        let sb = free_slot(cells, b).expect("a slot");
        cells.junctions_mut(b)[sb] = Junction {
            kind,
            other: ida,
            rest,
        };
    }

    #[test]
    fn a_matching_key_is_nearly_free_and_a_forced_join_is_not() {
        // M7 acceptance 1's arithmetic, checked directly. SPEC §8.2: consent is economic.
        let config = JunctionConfig::default();
        let consensual = join_cost(&config, true, 24);
        let forced = join_cost(&config, false, 24);
        assert_eq!(consensual, config.join_base_cost);
        assert!(
            forced > consensual * 50,
            "a forced join costs {forced} against {consensual} consensual; that is not a \
             deterrent"
        );
        // And it scales with what the target invested in its membrane: a well-defended cell
        // is dearer to force.
        assert!(join_cost(&config, false, 64) > join_cost(&config, false, 16));
    }

    #[test]
    fn a_hard_junction_pulls_two_cells_to_its_rest_length() {
        let (mut cells, pool) = arena();
        let a = spawn(&mut cells, &pool, 10, 10, 30);
        let b = spawn(&mut cells, &pool, 20, 10, 30);
        let (ia, ib) = (cells.index(a).unwrap(), cells.index(b).unwrap());
        let rest = POS_ONE * 4;
        join(&mut cells, ia, ib, JunctionKind::Hard, rest);

        let config = JunctionConfig::default();
        let before = distance(&cells, ia, ib);
        assert!(before > rest);
        for _ in 0..40 {
            solve(&mut cells, &config, &mut Vec::new());
        }
        let after = distance(&cells, ia, ib);
        assert!(
            (after - rest).abs() < POS_ONE / 2,
            "the constraint settled at {after} against a rest length of {rest}"
        );
    }

    #[test]
    fn a_light_cell_moves_further_than_a_heavy_one() {
        // Mass weighting. Without it a heavy cell is dragged about by a light one and a chain
        // creeps across the slide.
        let (mut cells, pool) = arena();
        let heavy = spawn(&mut cells, &pool, 10, 10, 200);
        let light = spawn(&mut cells, &pool, 20, 10, 5);
        let (ih, il) = (cells.index(heavy).unwrap(), cells.index(light).unwrap());
        let (hx0, lx0) = (cells.x[ih], cells.x[il]);
        join(&mut cells, ih, il, JunctionKind::Hard, POS_ONE * 4);

        solve(&mut cells, &JunctionConfig::default(), &mut Vec::new());
        let heavy_moved = (cells.x[ih] - hx0).abs();
        let light_moved = (cells.x[il] - lx0).abs();
        assert!(
            light_moved > heavy_moved * 3,
            "light moved {light_moved}, heavy moved {heavy_moved}; mass weighting is not \
             working"
        );
    }

    #[test]
    fn the_solver_does_not_ring() {
        // Over-correction makes a chain oscillate instead of settling. A colony that vibrates
        // flies apart, and the stiffness default exists to stop it.
        let (mut cells, pool) = arena();
        let a = spawn(&mut cells, &pool, 10, 10, 30);
        let b = spawn(&mut cells, &pool, 30, 10, 30);
        let (ia, ib) = (cells.index(a).unwrap(), cells.index(b).unwrap());
        let rest = POS_ONE * 4;
        join(&mut cells, ia, ib, JunctionKind::Hard, rest);

        let config = JunctionConfig::default();
        let mut previous = distance(&cells, ia, ib);
        let mut reversals = 0;
        let mut direction = 0i32;
        for _ in 0..60 {
            solve(&mut cells, &config, &mut Vec::new());
            let d = distance(&cells, ia, ib);
            let step = (d - previous).signum();
            if step != 0 {
                if direction != 0 && step != direction {
                    reversals += 1;
                }
                direction = step;
            }
            previous = d;
        }
        assert!(
            reversals <= 1,
            "the constraint reversed direction {reversals} times; it is ringing"
        );
    }

    #[test]
    fn a_junction_to_a_dead_cell_is_pruned() {
        let (mut cells, pool) = arena();
        let a = spawn(&mut cells, &pool, 10, 10, 30);
        let b = spawn(&mut cells, &pool, 11, 10, 30);
        let (ia, ib) = (cells.index(a).unwrap(), cells.index(b).unwrap());
        join(&mut cells, ia, ib, JunctionKind::Hard, POS_ONE);
        cells.despawn(b);

        let broken = prune(&mut cells, &JunctionConfig::default());
        assert_eq!(broken, 1);
        assert!(!cells.junctions(ia)[0].is_some());
    }

    #[test]
    fn a_soft_junction_breaks_when_the_two_drift_apart() {
        let (mut cells, pool) = arena();
        let a = spawn(&mut cells, &pool, 10, 10, 30);
        let b = spawn(&mut cells, &pool, 11, 10, 30);
        let (ia, ib) = (cells.index(a).unwrap(), cells.index(b).unwrap());
        join(&mut cells, ia, ib, JunctionKind::Soft, 0);
        let config = JunctionConfig::default();
        assert_eq!(
            prune(&mut cells, &config),
            0,
            "it broke while still in reach"
        );

        cells.x[ib] = pos(40);
        assert_eq!(prune(&mut cells, &config), 2, "both ends should break");
        assert!(!cells.junctions(ia)[0].is_some());
        assert!(!cells.junctions(ib)[0].is_some());
    }

    #[test]
    fn a_hard_junction_breaks_when_stretched_past_its_strain() {
        let (mut cells, pool) = arena();
        let a = spawn(&mut cells, &pool, 10, 10, 30);
        let b = spawn(&mut cells, &pool, 11, 10, 30);
        let (ia, ib) = (cells.index(a).unwrap(), cells.index(b).unwrap());
        join(&mut cells, ia, ib, JunctionKind::Hard, POS_ONE);
        let config = JunctionConfig::default();
        assert_eq!(prune(&mut cells, &config), 0);
        // Well past rest + breaking_strain.
        cells.x[ib] = pos(30);
        assert_eq!(prune(&mut cells, &config), 2);
    }

    #[test]
    fn a_chain_is_one_component_and_a_gap_makes_two() {
        let (mut cells, pool) = arena();
        let ids: Vec<CellId> = (0..6)
            .map(|k| spawn(&mut cells, &pool, 10 + k * 2, 10, 30))
            .collect();
        let slots: Vec<usize> = ids.iter().map(|id| cells.index(*id).unwrap()).collect();
        // Two chains of three.
        join(&mut cells, slots[0], slots[1], JunctionKind::Hard, POS_ONE);
        join(&mut cells, slots[1], slots[2], JunctionKind::Hard, POS_ONE);
        join(&mut cells, slots[3], slots[4], JunctionKind::Hard, POS_ONE);
        join(&mut cells, slots[4], slots[5], JunctionKind::Hard, POS_ONE);

        let mut components = Components::new();
        components.rebuild(&cells);
        assert_eq!(components.size_of(slots[0]), 3);
        assert_eq!(components.size_of(slots[3]), 3);
        assert_ne!(
            components.component_of(slots[0]),
            components.component_of(slots[3])
        );
        assert_eq!(components.largest(), 3);
        assert_eq!(components.clusters(3).len(), 2);
    }

    #[test]
    fn a_soft_junction_does_not_make_a_component() {
        // Components are over *hard* junctions: a conjugation channel does not make two cells
        // one organism, and a parasite is not part of its host.
        let (mut cells, pool) = arena();
        let a = spawn(&mut cells, &pool, 10, 10, 30);
        let b = spawn(&mut cells, &pool, 11, 10, 30);
        let (ia, ib) = (cells.index(a).unwrap(), cells.index(b).unwrap());
        join(&mut cells, ia, ib, JunctionKind::Soft, 0);

        let mut components = Components::new();
        components.rebuild(&cells);
        assert_eq!(components.size_of(ia), 1);
        assert_ne!(components.component_of(ia), components.component_of(ib));
    }

    #[test]
    fn components_do_not_depend_on_the_order_joins_happened_in() {
        // Two arenas with the same junctions made in opposite orders must agree, or the
        // phylogeny and the wiki would disagree about what an organism is between runs.
        let build = |forward: bool| {
            let (mut cells, pool) = arena();
            let ids: Vec<CellId> = (0..4)
                .map(|k| spawn(&mut cells, &pool, 10 + k * 2, 10, 30))
                .collect();
            let s: Vec<usize> = ids.iter().map(|id| cells.index(*id).unwrap()).collect();
            let pairs = [(0, 1), (1, 2), (2, 3)];
            if forward {
                for (a, b) in pairs {
                    join(&mut cells, s[a], s[b], JunctionKind::Hard, POS_ONE);
                }
            } else {
                for (a, b) in pairs.iter().rev() {
                    join(&mut cells, s[*a], s[*b], JunctionKind::Hard, POS_ONE);
                }
            }
            let mut c = Components::new();
            c.rebuild(&cells);
            (0..4).map(|k| c.component_of(s[k])).collect::<Vec<usize>>()
        };
        assert_eq!(build(true), build(false));
    }

    #[test]
    fn differentiation_is_two_kinds_of_cell_in_one_cluster() {
        use crate::organelle::{Organelle, OrganelleType};
        let (mut cells, pool) = arena();
        let a = spawn(&mut cells, &pool, 10, 10, 30);
        let b = spawn(&mut cells, &pool, 11, 10, 30);
        let (ia, ib) = (cells.index(a).unwrap(), cells.index(b).unwrap());
        join(&mut cells, ia, ib, JunctionKind::Hard, POS_ONE);

        let mut components = Components::new();
        components.rebuild(&cells);
        let root = components.component_of(ia);
        // Same loadout: one kind of cell.
        cells.slots_mut(ia)[1] = Organelle::finished(OrganelleType::Chloroplast, 40);
        cells.slots_mut(ib)[1] = Organelle::finished(OrganelleType::Chloroplast, 40);
        assert_eq!(distinct_loadouts(&cells, &mut components, root), 1);
        // Different size is still the same kind of cell.
        cells.slots_mut(ib)[1] = Organelle::finished(OrganelleType::Chloroplast, 90);
        assert_eq!(
            distinct_loadouts(&cells, &mut components, root),
            1,
            "a bigger chloroplast is not a different kind of cell"
        );
        // A different organelle is.
        cells.slots_mut(ib)[1] = Organelle::finished(OrganelleType::Cilium, 40);
        assert_eq!(distinct_loadouts(&cells, &mut components, root), 2);
    }

    #[test]
    fn a_handle_cannot_reach_across_the_slide() {
        let (mut cells, pool) = arena();
        let a = spawn(&mut cells, &pool, 10, 10, 30);
        let near = spawn(&mut cells, &pool, 11, 10, 30);
        let far = spawn(&mut cells, &pool, 60, 60, 30);
        let ia = cells.index(a).unwrap();
        let (inear, ifar) = (cells.index(near).unwrap(), cells.index(far).unwrap());
        let r = reach(&cells, ia);

        assert_eq!(resolve_handle(&cells, ia, inear as i16, r), Some(inear));
        assert_eq!(
            resolve_handle(&cells, ia, ifar as i16, r),
            None,
            "a genome reached a cell it cannot touch"
        );
        assert_eq!(
            resolve_handle(&cells, ia, ia as i16, r),
            None,
            "a cell joined itself"
        );
        // An arbitrary number wraps into range rather than being out of bounds, and then
        // fails the occupancy or distance test like any other.
        let _ = resolve_handle(&cells, ia, i16::MAX, r);
        let _ = resolve_handle(&cells, ia, -1, r);
    }

    #[test]
    fn the_kind_operand_is_total() {
        // Every byte sequence is a legal program, so every operand value has to mean
        // something. A mutation flips the kind rather than landing on an invalid junction.
        assert_eq!(JunctionKind::from_operand(0), JunctionKind::Soft);
        assert_eq!(JunctionKind::from_operand(1), JunctionKind::Hard);
        assert_eq!(JunctionKind::from_operand(-1), JunctionKind::Hard);
        assert_eq!(JunctionKind::from_operand(i16::MAX), JunctionKind::Hard);
        assert_eq!(JunctionKind::from_operand(i16::MIN), JunctionKind::Soft);
    }

    #[test]
    fn junction_indices_wrap_rather_than_going_out_of_range() {
        for idx in [0i16, 3, 4, 100, -1, i16::MIN, i16::MAX] {
            assert!(junction_index(idx) < JUNCTIONS_PER_CELL);
        }
    }
}
