//! Who is next to whom (M3), and what to do when two cells are in the same place.
//!
//! # One index, two customers
//!
//! Touch sensing and collision resolution ask the same question — which cells are within reach
//! of this one — so they share one index rather than each walking the population. At M3 that
//! index is a bucket per substrate square, built by counting sort; M9 replaces it with
//! something tuned, behind the same query.
//!
//! # Determinism
//!
//! Buckets are built by counting sort and each bucket is filled in slot order, so the
//! neighbour list for a cell is the same list in the same order on every machine. That matters
//! more than it sounds: separation pushes cells apart pairwise, and pairwise pushes do not
//! commute. A neighbour list that came back in a different order would give a different world
//! (I1, I6).
//!
//! # Separation is not a physics engine
//!
//! Two cells that overlap are pushed apart along the line between them, a little, once per
//! tick. There is no restitution, no momentum exchange and no iteration to convergence,
//! because none of that is what the simulation is about — what it needs is that cells occupy
//! space, so that a crowded patch is crowded and a cell has a reason to leave it. Junction
//! constraints at M7 are where a real solver goes, and SPEC §8.4 is explicit that even that
//! one is two or three Gauss-Seidel iterations and no more.

use rayon::prelude::*;

use crate::cell::CellArena;
use crate::fixed::{pos_to_square, POS_ONE};
use crate::sensing::TouchReading;

/// How far apart two cells are pushed per tick when they overlap, as a fraction of the
/// overlap, in sixteenths.
///
/// Partial rather than complete: shoving two cells fully apart in one tick makes a crowd
/// explode, and a crowd that explodes is not a crowd.
const SEPARATION_STRENGTH: i32 = 4;

/// Cells indexed by the substrate square they stand on.
#[derive(Clone, Debug, Default)]
pub struct NeighbourIndex {
    /// Start of each square's run in `entries`, plus a final sentinel. `starts.len() == n + 1`.
    starts: Vec<u32>,
    /// Cell slots, grouped by square and ascending within each group.
    entries: Vec<u32>,
    width: i32,
    height: i32,
    /// Each cell's touch reading, gathered once per tick by [`Self::gather_touch`].
    ///
    /// `None` for a cell that was not gathered — either because the table is not built at all,
    /// or because the cell has no touch sensor to read it. [`Self::touch`] walks the
    /// neighbourhood for those, so an ungathered answer is still the right answer.
    ///
    /// Cleared by [`Self::rebuild`], because a reading describes where cells *were*: the
    /// physics phase rebuilds the index after moving everything, and a table left behind would
    /// be a stale answer that looked fresh.
    touch: Vec<Option<TouchReading>>,
    /// Cell radii in `POS`, hoisted for the gather.
    ///
    /// [`crate::biology::radius`] is an integer square root and the inner loop below would
    /// otherwise call it once per neighbour per cell. The same hoist `resolve_collisions`
    /// makes, for the same reason.
    radii: Vec<i32>,
}

impl NeighbourIndex {
    /// Rebuild from the population. Counting sort, so it is linear and its output does not
    /// depend on anything but the input.
    pub fn rebuild(&mut self, cells: &CellArena, width: u32, height: u32) {
        let n = (width as usize).saturating_mul(height as usize);
        self.width = width as i32;
        self.height = height as i32;
        self.starts.clear();
        self.starts.resize(n + 1, 0);
        self.entries.clear();
        // Whatever was gathered described the old positions. See the field's own note.
        self.touch.clear();

        // Count.
        for i in cells.iter() {
            let sq = self.square_of(cells, i);
            self.starts[sq + 1] = self.starts[sq + 1].saturating_add(1);
        }
        // Prefix-sum into offsets.
        for k in 1..=n {
            self.starts[k] = self.starts[k].saturating_add(self.starts[k - 1]);
        }
        self.entries.resize(cells.len(), 0);
        // Place, in slot order, so each bucket ends up ascending.
        let mut cursor = self.starts.clone();
        for i in cells.iter() {
            let sq = self.square_of(cells, i);
            let at = cursor[sq] as usize;
            if let Some(slot) = self.entries.get_mut(at) {
                *slot = i as u32;
            }
            cursor[sq] = cursor[sq].saturating_add(1);
        }
    }

    fn square_of(&self, cells: &CellArena, i: usize) -> usize {
        let x = pos_to_square(cells.x[i]).clamp(0, self.width.saturating_sub(1).max(0));
        let y = pos_to_square(cells.y[i]).clamp(0, self.height.saturating_sub(1).max(0));
        (y as usize) * (self.width.max(1) as usize) + x as usize
    }

    /// Cells in one square, ascending by slot.
    #[must_use]
    pub fn in_square(&self, sx: i32, sy: i32) -> &[u32] {
        if sx < 0 || sy < 0 || sx >= self.width || sy >= self.height {
            return &[];
        }
        let sq = (sy as usize) * (self.width as usize) + sx as usize;
        let (Some(from), Some(to)) = (self.starts.get(sq), self.starts.get(sq + 1)) else {
            return &[];
        };
        self.entries
            .get(*from as usize..*to as usize)
            .unwrap_or(&[])
    }

    /// The three horizontally-adjacent squares of one row, as a single run.
    ///
    /// Squares are laid out row-major and `starts` is a prefix sum over them, so `sx-1`, `sx`
    /// and `sx+1` in the same row are three *consecutive* buckets — which means they are one
    /// contiguous slice of `entries`, findable with one pair of lookups instead of three.
    ///
    /// Exactly the three buckets in the same order, including at the edges: clamping the run
    /// to the slide drops the squares that are off it, which is what the three separate
    /// lookups did by returning nothing for them.
    fn row_run(&self, sx: i32, sy: i32) -> &[u32] {
        if self.width <= 0 || sy < 0 || sy >= self.height {
            return &[];
        }
        let x0 = sx.saturating_sub(1).max(0);
        let x1 = sx.saturating_add(1).min(self.width - 1);
        if x0 > x1 {
            return &[];
        }
        let row = (sy as usize).saturating_mul(self.width as usize);
        let (Some(from), Some(to)) = (
            self.starts.get(row + x0 as usize),
            self.starts.get(row + x1 as usize + 1),
        ) else {
            return &[];
        };
        self.entries
            .get(*from as usize..*to as usize)
            .unwrap_or(&[])
    }

    /// Every cell in the nine squares around one, in a fixed order.
    ///
    /// Three runs rather than nine buckets. The order is unchanged — that is load-bearing, not
    /// incidental: separation pushes cells apart pairwise and pairwise pushes do not commute,
    /// so a different order here is a different world (I1, I6).
    pub fn around(&self, sx: i32, sy: i32) -> impl Iterator<Item = usize> + '_ {
        (-1..=1)
            .flat_map(move |dy| self.row_run(sx, sy.saturating_add(dy)))
            .map(|s| *s as usize)
    }

    /// Gather every touch reading the execute phase could ask for, once.
    ///
    /// # Why this is a phase and not a lookup
    ///
    /// `touch_reading` used to be called from `OGET`, which means once per *sensor read* rather
    /// than once per cell: a genome that looks at three organelles walked its neighbourhood
    /// three times, and each walk took an integer square root per neighbour. Worse, the reading
    /// was built eagerly for every sensor — a chemosensor paid for a neighbourhood walk and
    /// then read a chemical.
    ///
    /// Memoising it inside the host is not available: the execute phase runs cells in parallel
    /// and a shared cache filled on demand would make the result depend on which thread got
    /// there first, which is exactly what I6 forbids. So it is gathered up front instead, where
    /// each cell writes only its own slot and the order it happens in cannot be observed.
    ///
    /// # Why this is exact
    ///
    /// Between this call and the end of execute, nothing a reading depends on moves. Positions
    /// change in the physics phase, mass and occupancy in the bookkeeping phase, and both are
    /// after execute — genomes emit intents rather than writing the world. So a reading
    /// gathered here is the one the walk would have produced at the moment it was asked for,
    /// byte for byte, and the state hash proves it.
    pub fn gather_touch(&mut self, cells: &CellArena) {
        // Nothing on the slide can feel anything, which is the usual case — a touch sensor is
        // one of sixteen organelle types and most lineages never evolve one. Answered before
        // anything is allocated or sized, because sizing the two tables is a pair of memsets
        // over the whole population and that is not free at fifty thousand cells. It cost 10%
        // of the tick on a population that never reads the result.
        //
        // In parallel and short-circuiting, so the answer costs a scan of the organelle slots
        // divided by the core count, and usually far less because it stops at the first hit.
        let wanted = (0..cells.capacity())
            .into_par_iter()
            .any(|i| cells.occupied(i) && reads_touch(cells, i));
        if !wanted {
            self.touch.clear();
            return;
        }

        let capacity = cells.capacity();
        // Taken out so the parallel fill can hold `&*self` for `around`. The same borrow
        // dance `execute` does with the VM array.
        let mut touch = std::mem::take(&mut self.touch);
        let mut radii = std::mem::take(&mut self.radii);

        radii.clear();
        radii.resize(capacity, 0);
        for (i, r) in radii.iter_mut().enumerate() {
            if cells.occupied(i) {
                *r = crate::fixed::q10_to_pos(crate::biology::radius(cells, i));
            }
        }

        touch.clear();
        touch.resize(capacity, None);
        let index: &NeighbourIndex = self;
        let hoisted = &radii;
        touch.par_iter_mut().enumerate().for_each(|(i, slot)| {
            if cells.occupied(i) && reads_touch(cells, i) {
                *slot = Some(feel(cells, index, i, |j| {
                    hoisted.get(j).copied().unwrap_or(0)
                }));
            }
        });

        self.touch = touch;
        self.radii = radii;
    }

    /// Which neighbours are within reach of a cell, nearest first.
    ///
    /// The same walk the touch sensor makes, asking a different question: not "how crowded am
    /// I" but "who exactly is beside me, and where".
    ///
    /// `reach_permille` scales what counts as touching: 1000 is the bare radii, 1200 counts a
    /// neighbour whose centre is within a fifth further out than that. It exists because
    /// separation drives overlap to *zero* — it pushes every tick until `d >= ri + rj` — so at
    /// rest almost nothing overlaps and a caller asking only for genuine overlap would be told
    /// about nothing almost all of the time. A caller that wants to know who a cell is packed
    /// against, rather than who it is presently colliding with, has to ask slightly wider than
    /// the physics does.
    ///
    /// Per pair rather than as a flat distance, so it means the same thing for a large cell as
    /// for a small one. In permille because `mm-core` has no floats.
    ///
    /// Not gathered up front like [`Self::gather_touch`], because only the cells actually on
    /// screen need it and the renderer knows which those are.
    #[must_use]
    pub fn contacts(&self, cells: &CellArena, i: usize, reach_permille: i32) -> ContactSet {
        let mut out = ContactSet::default();
        if !cells.occupied(i) {
            return out;
        }
        let sx = pos_to_square(cells.x[i]);
        let sy = pos_to_square(cells.y[i]);
        let ri = crate::fixed::q10_to_pos(crate::biology::radius(cells, i));
        let reach = reach_permille.max(0) as i64;
        for j in self.around(sx, sy) {
            if j == i || !cells.occupied(j) {
                continue;
            }
            let rj = crate::fixed::q10_to_pos(crate::biology::radius(cells, j));
            let d = separation(cells, i, j);
            let touching = ((ri.saturating_add(rj) as i64 * reach) / 1000) as i32;
            let overlap = touching.saturating_sub(d);
            if overlap <= 0 {
                continue;
            }
            out.offer(Contact {
                dx: cells.x[j].saturating_sub(cells.x[i]),
                dy: cells.y[j].saturating_sub(cells.y[i]),
                radius: rj,
                overlap,
            });
        }
        out
    }

    /// One cell's touch reading: from the gathered table when it is there, by walking when not.
    #[must_use]
    pub fn touch(&self, cells: &CellArena, i: usize) -> TouchReading {
        match self.touch.get(i).copied().flatten() {
            Some(reading) => reading,
            None => touch_reading(cells, self, i),
        }
    }
}

/// How many neighbours a cell is reported as pressed against.
///
/// Four, like junctions. A cell with more than four neighbours deep enough to change its
/// outline has them behind one another, and the fifth stops making a visible difference.
pub const CONTACTS_PER_CELL: usize = 4;

/// A neighbour a cell is overlapping, for whoever draws it.
///
/// Facts, not presentation: which way the neighbour lies, how far, and how big it is. Where to
/// put the seam between two cells squashed together is a question about how it should *look*,
/// which belongs in `mm-app` and needs floats to answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Contact {
    /// Offset from this cell's centre to the neighbour's, `POS`.
    pub dx: i32,
    pub dy: i32,
    /// The neighbour's radius, `POS`.
    pub radius: i32,
    /// How far inside each other's reach the two are, `POS`. Always positive, and measured
    /// against whatever reach the caller asked for rather than against bare radii.
    pub overlap: i32,
}

/// The neighbours pressing on one cell, deepest first.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ContactSet {
    found: [Contact; CONTACTS_PER_CELL],
    len: usize,
}

impl ContactSet {
    /// Keep this one if it is deeper than something already held.
    fn offer(&mut self, c: Contact) {
        let mut at = self.len.min(CONTACTS_PER_CELL);
        // Ordered by depth, so the shallowest falls off the end when a deeper one arrives.
        while at > 0 && self.found[at - 1].overlap < c.overlap {
            if at < CONTACTS_PER_CELL {
                self.found[at] = self.found[at - 1];
            }
            at -= 1;
        }
        if at < CONTACTS_PER_CELL {
            self.found[at] = c;
            self.len = (self.len + 1).min(CONTACTS_PER_CELL);
        }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Contact] {
        self.found.get(..self.len).unwrap_or(&[])
    }
}

/// What one cell can feel around it (SPEC §6.2's touch sensor).
///
/// Walks the neighbourhood. Prefer [`NeighbourIndex::touch`], which reads the gathered table
/// when there is one; this is what that falls back to, and what fills the table.
#[must_use]
pub fn touch_reading(cells: &CellArena, index: &NeighbourIndex, i: usize) -> TouchReading {
    feel(cells, index, i, |j| {
        crate::fixed::q10_to_pos(crate::biology::radius(cells, j))
    })
}

/// The touch rule itself, over whatever supplies the radii.
///
/// One definition, two callers: [`touch_reading`] computes each radius as it goes, and
/// [`NeighbourIndex::gather_touch`] hands it a hoisted table. Written once because the two must
/// agree exactly — this is what a genome reads, so a discrepancy is not a rendering artefact,
/// it is a different simulation.
#[inline]
fn feel(
    cells: &CellArena,
    index: &NeighbourIndex,
    i: usize,
    radius_of: impl Fn(usize) -> i32,
) -> TouchReading {
    let sx = pos_to_square(cells.x[i]);
    let sy = pos_to_square(cells.y[i]);
    // The sensing cell's own radius does not change while it is looking around.
    let ri = radius_of(i);
    let reach = ri.saturating_mul(2);

    let mut contacts = 0i32;
    let mut nearest = i32::MAX;
    let mut nearest_slot = 0i32;
    let mut mass = 0i64;
    for j in index.around(sx, sy) {
        if j == i || !cells.occupied(j) {
            continue;
        }
        let d = separation(cells, i, j);
        let touching = radius_of(j).saturating_add(ri).saturating_add(reach);
        if d <= touching {
            contacts = contacts.saturating_add(1);
            mass = mass.saturating_add(cells.mass[j] as i64);
            // Strictly less, so ties go to the earlier neighbour in `around`'s fixed order.
            // That order is part of the result, not an accident of it (I1, I6).
            if d < nearest {
                nearest = d;
                nearest_slot = j as i32;
            }
        }
    }
    TouchReading {
        contacts: crate::fixed::sat_i16(contacts),
        // The nearest neighbour's slot, so a genome has a handle to `JOIN` at (M7).
        nearest: crate::fixed::sat_i16(nearest_slot),
        contact_mass: crate::fixed::sat_i16((mass / crate::fixed::Q10_ONE as i64) as i32),
    }
}

/// Whether a cell has anything that would read a touch reading.
///
/// Only [`crate::OrganelleType::TouchSensor`] does — a chemosensor or a photosensor ignores
/// it entirely — so gathering for a cell without one is a neighbourhood walk nobody collects.
/// In a population that does not sense touch at all, which is most of them, this makes the
/// gather free.
#[inline]
fn reads_touch(cells: &CellArena, i: usize) -> bool {
    cells
        .slots(i)
        .iter()
        .any(|o| o.kind == crate::organelle::OrganelleType::TouchSensor)
}

/// Whether two cells hold a junction to each other.
///
/// Asked of the lower slot's list only. A junction is recorded on both ends, so one look is
/// enough and the second would find the same answer.
#[inline]
fn joined(cells: &CellArena, i: usize, j: usize) -> bool {
    let other = cells.id_at(j);
    cells.junctions(i).iter().any(|k| k.other == other)
}

/// Distance between two cells, `POS` units.
///
/// Octagonal rather than Euclidean: exact in integers, monotonic in the true distance, and
/// nothing here needs the difference. A Euclidean distance would need a square root, and this
/// is on a path walked once per neighbour per cell per tick.
#[inline]
fn separation(cells: &CellArena, i: usize, j: usize) -> i32 {
    let dx = (cells.x[i] - cells.x[j]).abs();
    let dy = (cells.y[i] - cells.y[j]).abs();
    let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
    hi.saturating_add(lo / 2)
}

/// Push overlapping cells apart, once, in slot order.
///
/// Returns how many pairs were separated, which is a cheap measure of how crowded the slide
/// is and the thing to watch if a population stops growing for reasons that are not food.
///
/// Also records how hard each cell is being pressed, into `crowding`, in `POS`: the total
/// overlap with cells it is *not* joined to. Measured here because this is the pass that
/// already knows, and separation resolves only a fraction of an overlap per tick — so what is
/// left is a real and persistent state of being crushed rather than an instant of contact.
/// [`crate::ecology`] is what charges for it.
pub fn resolve_collisions(
    cells: &mut CellArena,
    index: &NeighbourIndex,
    radii: &mut Vec<i32>,
    crowding: &mut Vec<i32>,
) -> u32 {
    let mut separated = 0u32;
    let width = index.width;
    let height = index.height;

    // Radius is an integer square root, so it is not free, and the inner loop below would
    // otherwise recompute the same neighbour's radius once per pair it takes part in. Computed
    // once per cell here instead. Exactly equivalent: radius depends only on mass, and this
    // function moves cells without changing what they weigh.
    //
    // Converted to `POS` on the way in. `radius` is `Q10` and `separation` is `POS` — both
    // measure squares, at different scales — and comparing them directly meant a cell counted
    // anything within *seven* squares as overlapping instead of about one and three quarters.
    // A bug since M3, found at M7 because a junction could not pull two cells closer than the
    // separation was shoving them apart.
    crowding.clear();
    crowding.resize(cells.capacity(), 0);
    radii.clear();
    radii.reserve(cells.capacity());
    for i in 0..cells.capacity() {
        radii.push(if cells.occupied(i) {
            crate::fixed::q10_to_pos(crate::biology::radius(cells, i))
        } else {
            0
        });
    }

    for i in 0..cells.capacity() {
        if !cells.occupied(i) {
            continue;
        }
        let sx = pos_to_square(cells.x[i]);
        let sy = pos_to_square(cells.y[i]);
        let ri = radii[i];

        // `around` borrows the index and the push writes to cells, which are different
        // objects, so the neighbours are walked directly. Collecting them first would be one
        // heap allocation per cell per tick, which at fifty thousand cells was a third of the
        // tick on its own.
        for j in index.around(sx, sy) {
            // Each pair is handled once, by its lower slot, so the push is applied to both
            // sides of one decision rather than to two sides of two.
            if j <= i || !cells.occupied(j) {
                continue;
            }
            let want = ri.saturating_add(radii[j]);
            let d = separation(cells, i, j);
            if d >= want {
                continue;
            }
            let overlap = want - d;
            // Being crushed, charged to both sides — except by whatever this cell is joined
            // to. An organism is *meant* to hold its cells against each other, and billing it
            // for that would make being multicellular a way to die.
            if !joined(cells, i, j) {
                if let Some(c) = crowding.get_mut(i) {
                    *c = c.saturating_add(overlap);
                }
                if let Some(c) = crowding.get_mut(j) {
                    *c = c.saturating_add(overlap);
                }
            }
            let (dx, dy) = (cells.x[i] - cells.x[j], cells.y[i] - cells.y[j]);
            // Exactly coincident cells have no line to push along, so they get a fixed
            // nudge derived from their slots — deterministic, and enough to break the tie.
            let (ux, uy) = if dx == 0 && dy == 0 {
                (
                    if (i + j) % 2 == 0 { POS_ONE } else { -POS_ONE },
                    if (i / 2 + j) % 2 == 0 {
                        POS_ONE
                    } else {
                        -POS_ONE
                    },
                )
            } else {
                let scale = separation(cells, i, j).max(1);
                (
                    (dx as i64 * POS_ONE as i64 / scale as i64) as i32,
                    (dy as i64 * POS_ONE as i64 / scale as i64) as i32,
                )
            };
            let shove = overlap.saturating_mul(SEPARATION_STRENGTH) / 16 / 2;
            let px = (ux as i64 * shove as i64 / POS_ONE as i64) as i32;
            let py = (uy as i64 * shove as i64 / POS_ONE as i64) as i32;
            let max_x = (width as i64 * POS_ONE as i64) - 1;
            let max_y = (height as i64 * POS_ONE as i64) - 1;
            cells.x[i] = ((cells.x[i] as i64) + px as i64).clamp(0, max_x) as i32;
            cells.y[i] = ((cells.y[i] as i64) + py as i64).clamp(0, max_y) as i32;
            cells.x[j] = ((cells.x[j] as i64) - px as i64).clamp(0, max_x) as i32;
            cells.y[j] = ((cells.y[j] as i64) - py as i64).clamp(0, max_y) as i32;
            separated = separated.saturating_add(1);
        }
    }
    separated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{CellId, CellSeed};
    use crate::fixed::{pos, q10};
    use crate::genome::GenomePool;

    fn arena(positions: &[(i32, i32)]) -> (CellArena, GenomePool) {
        let pool = GenomePool::new();
        let mut cells = CellArena::new();
        for (x, y) in positions {
            cells.spawn(CellSeed {
                x: *x,
                y: *y,
                mass: q10(40),
                energy: q10(100),
                membrane: 16,
                key: 0,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome: pool.intern(vec![0x2E]).unwrap(),
            });
        }
        (cells, pool)
    }

    /// The nine-bucket walk `around` replaced, kept as the thing to measure it against.
    fn nine_buckets(index: &NeighbourIndex, sx: i32, sy: i32) -> Vec<usize> {
        (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
            .flat_map(|(dx, dy)| index.in_square(sx + dx, sy + dy))
            .map(|s| *s as usize)
            .collect()
    }

    #[test]
    fn three_row_runs_are_the_same_walk_as_nine_buckets() {
        // `around` gathers each row of the neighbourhood as one contiguous run rather than
        // three separate buckets. That is only sound if it is the same *sequence*, not merely
        // the same set: separation pushes cells apart pairwise and pairwise pushes do not
        // commute, so a reordering here is a different world (I1, I6).
        //
        // Checked over a crowded slide and past every edge, because the runs are clamped and
        // clamping is where an off-by-one would hide.
        let mut positions = Vec::new();
        for x in 0..8i32 {
            for y in 0..8i32 {
                positions.push((pos(x), pos(y)));
                if (x + y) % 3 == 0 {
                    positions.push((pos(x), pos(y)));
                }
            }
        }
        let (cells, _p) = arena(&positions);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 8, 8);

        for sy in -2..=9 {
            for sx in -2..=9 {
                let walked: Vec<usize> = index.around(sx, sy).collect();
                assert_eq!(walked, nine_buckets(&index, sx, sy), "at ({sx}, {sy})");
            }
        }
    }

    #[test]
    fn a_one_square_slide_still_walks_itself() {
        // The degenerate clamp: every neighbour is the same square, and the run must not
        // reach past it into whatever the prefix sum has next.
        let (cells, _p) = arena(&[(pos(0), pos(0)), (pos(0), pos(0))]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 1, 1);
        assert_eq!(index.around(0, 0).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(nine_buckets(&index, 0, 0), vec![0, 1]);
        // A square off the end still has the last real square as its neighbour, and always
        // did — the run is clamped, not truncated to nothing.
        for sx in -1..=2 {
            assert_eq!(
                index.around(sx, 0).collect::<Vec<_>>(),
                nine_buckets(&index, sx, 0),
                "at ({sx}, 0)"
            );
        }
    }

    #[test]
    fn the_index_groups_cells_by_square_in_slot_order() {
        let (cells, _p) = arena(&[
            (pos(3), pos(3)),
            (pos(9), pos(9)),
            (pos(3), pos(3)),
            (pos(3), pos(3)),
        ]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        assert_eq!(index.in_square(3, 3), &[0, 2, 3]);
        assert_eq!(index.in_square(9, 9), &[1]);
        assert_eq!(index.in_square(5, 5), &[] as &[u32]);
        assert_eq!(
            index.in_square(-1, 0),
            &[] as &[u32],
            "off the slide is empty"
        );
        assert_eq!(index.in_square(99, 99), &[] as &[u32]);
    }

    #[test]
    fn the_index_survives_an_empty_population() {
        let (cells, _p) = arena(&[]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 8, 8);
        assert_eq!(index.around(4, 4).count(), 0);
    }

    #[test]
    fn a_neighbourhood_is_the_nine_squares_around_a_cell() {
        let (cells, _p) = arena(&[
            (pos(5), pos(5)),
            (pos(6), pos(5)),
            (pos(4), pos(4)),
            (pos(8), pos(5)),
        ]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        let near: Vec<usize> = index.around(5, 5).collect();
        assert!(near.contains(&0) && near.contains(&1) && near.contains(&2));
        assert!(!near.contains(&3), "three squares away is not adjacent");
    }

    #[test]
    fn overlapping_cells_are_pushed_apart_and_separated_ones_are_left_alone() {
        let (mut cells, _p) = arena(&[(pos(5), pos(5)), (pos(5) + 4, pos(5))]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        let before = separation(&cells, 0, 1);
        let n = resolve_collisions(&mut cells, &index, &mut Vec::new(), &mut Vec::new());
        assert_eq!(n, 1);
        assert!(
            separation(&cells, 0, 1) > before,
            "they were not pushed apart"
        );

        let (mut far, _p2) = arena(&[(pos(2), pos(2)), (pos(12), pos(12))]);
        let mut index2 = NeighbourIndex::default();
        index2.rebuild(&far, 16, 16);
        let positions: Vec<(i32, i32)> = far.iter().map(|i| (far.x[i], far.y[i])).collect();
        assert_eq!(
            resolve_collisions(&mut far, &index2, &mut Vec::new(), &mut Vec::new()),
            0
        );
        let after: Vec<(i32, i32)> = far.iter().map(|i| (far.x[i], far.y[i])).collect();
        assert_eq!(positions, after, "distant cells were moved");
    }

    #[test]
    fn exactly_coincident_cells_do_not_get_stuck() {
        // Two cells at the same point have no line to push along. Without a tie-break they
        // would sit on top of each other forever, which is how a crowd becomes a singularity.
        let (mut cells, _p) = arena(&[(pos(8), pos(8)), (pos(8), pos(8))]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        for _ in 0..20 {
            resolve_collisions(&mut cells, &index, &mut Vec::new(), &mut Vec::new());
            index.rebuild(&cells, 16, 16);
        }
        assert!(
            separation(&cells, 0, 1) > 0,
            "coincident cells never separated"
        );
    }

    #[test]
    fn separation_keeps_cells_on_the_slide() {
        let (mut cells, _p) = arena(&[(0, 0), (1, 0), (2, 0)]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        for _ in 0..50 {
            resolve_collisions(&mut cells, &index, &mut Vec::new(), &mut Vec::new());
            index.rebuild(&cells, 16, 16);
            for i in cells.iter() {
                assert!(cells.x[i] >= 0 && cells.x[i] < 16 * POS_ONE);
                assert!(cells.y[i] >= 0 && cells.y[i] < 16 * POS_ONE);
            }
        }
    }

    #[test]
    fn separation_is_deterministic() {
        let run = || {
            let (mut cells, _p) = arena(&[
                (pos(5), pos(5)),
                (pos(5) + 3, pos(5) + 2),
                (pos(5) + 1, pos(5) + 4),
                (pos(6), pos(5)),
            ]);
            let mut index = NeighbourIndex::default();
            for _ in 0..10 {
                index.rebuild(&cells, 16, 16);
                resolve_collisions(&mut cells, &index, &mut Vec::new(), &mut Vec::new());
            }
            cells
                .iter()
                .map(|i| (cells.x[i], cells.y[i]))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn a_touch_sensor_counts_what_is_next_to_it() {
        let (cells, _p) = arena(&[
            (pos(5), pos(5)),
            (pos(5) + 8, pos(5)),
            (pos(5), pos(5) + 8),
            (pos(14), pos(14)),
        ]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        let r = touch_reading(&cells, &index, 0);
        assert_eq!(
            r.contacts, 2,
            "should feel the two beside it and not the far one"
        );
        assert!(r.contact_mass > 0);

        let lonely = touch_reading(&cells, &index, 3);
        assert_eq!(lonely.contacts, 0);
        assert_eq!(lonely.contact_mass, 0);
    }
}
