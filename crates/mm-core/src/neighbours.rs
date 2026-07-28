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

    /// Every cell in the nine squares around one, in a fixed order.
    pub fn around(&self, sx: i32, sy: i32) -> impl Iterator<Item = usize> + '_ {
        (-1..=1)
            .flat_map(move |dy| (-1..=1).map(move |dx| (dx, dy)))
            .flat_map(move |(dx, dy)| self.in_square(sx + dx, sy + dy))
            .map(|s| *s as usize)
    }
}

/// What one cell can feel around it (SPEC §6.2's touch sensor).
#[must_use]
pub fn touch_reading(cells: &CellArena, index: &NeighbourIndex, i: usize) -> TouchReading {
    let sx = pos_to_square(cells.x[i]);
    let sy = pos_to_square(cells.y[i]);
    let reach = crate::biology::radius(cells, i).saturating_mul(2);

    let mut contacts = 0i32;
    let mut nearest = i32::MAX;
    let mut nearest_slot = 0i32;
    let mut mass = 0i64;
    for j in index.around(sx, sy) {
        if j == i || !cells.occupied(j) {
            continue;
        }
        let d = separation(cells, i, j);
        let touching = crate::biology::radius(cells, j)
            .saturating_add(crate::biology::radius(cells, i))
            .saturating_add(reach);
        if d <= touching {
            contacts = contacts.saturating_add(1);
            mass = mass.saturating_add(cells.mass[j] as i64);
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

/// Distance between two cells, `POS` units.
///
/// Octagonal rather than Euclidean: exact in integers, monotonic in the true distance, and
/// nothing here needs the difference. A square root would need a float or a loop, and this is
/// on a path walked once per neighbour per cell per tick.
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
pub fn resolve_collisions(cells: &mut CellArena, index: &NeighbourIndex) -> u32 {
    let mut separated = 0u32;
    let width = index.width;
    let height = index.height;

    for i in 0..cells.capacity() {
        if !cells.occupied(i) {
            continue;
        }
        let sx = pos_to_square(cells.x[i]);
        let sy = pos_to_square(cells.y[i]);
        let ri = crate::biology::radius(cells, i);

        // `around` borrows the index and the push writes to cells, which are different
        // objects, so the neighbours can be walked directly.
        let neighbours: Vec<usize> = index.around(sx, sy).collect();
        for j in neighbours {
            // Each pair is handled once, by its lower slot, so the push is applied to both
            // sides of one decision rather than to two sides of two.
            if j <= i || !cells.occupied(j) {
                continue;
            }
            let rj = crate::biology::radius(cells, j);
            let want = ri.saturating_add(rj);
            let d = separation(cells, i, j);
            if d >= want {
                continue;
            }
            let overlap = want - d;
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
        let n = resolve_collisions(&mut cells, &index);
        assert_eq!(n, 1);
        assert!(
            separation(&cells, 0, 1) > before,
            "they were not pushed apart"
        );

        let (mut far, _p2) = arena(&[(pos(2), pos(2)), (pos(12), pos(12))]);
        let mut index2 = NeighbourIndex::default();
        index2.rebuild(&far, 16, 16);
        let positions: Vec<(i32, i32)> = far.iter().map(|i| (far.x[i], far.y[i])).collect();
        assert_eq!(resolve_collisions(&mut far, &index2), 0);
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
            resolve_collisions(&mut cells, &index);
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
            resolve_collisions(&mut cells, &index);
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
                resolve_collisions(&mut cells, &index);
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
