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
//! # Contact is not a physics engine
//!
//! Two cells that overlap are pushed apart along the line between them, a little, a few times
//! per tick. There is no restitution and no momentum exchange, because none of that is what the
//! simulation is about — what it needs is that cells occupy space, so that a crowded patch is
//! crowded and a cell has a reason to leave it. SPEC §8.4 is explicit that even the junction
//! solver is two or three Gauss-Seidel iterations and no more.
//!
//! # Cells compress; they do not interpenetrate freely, and they do not stay circles
//!
//! Contact is deliberately *not* a non-penetration constraint. Two cells in contact are allowed
//! to overlap, softly at first and then against a hard floor — see [`CORE_PERMILLE`] — so that
//! how deeply a cell is pressed into its neighbour is a reading of how hard it is being pressed.
//!
//! This is load-bearing for the picture as well as the physics, and it took a while to see why.
//! The renderer draws the flat wall between two cells by cutting each of them at the plane where
//! their outlines cross, which only exists where they overlap. Solve contact to convergence and
//! there is no overlap, so there is no wall — and non-overlapping circles cannot tile a plane, so
//! what you get is a bag of marbles with holes between them however good the shader is. A packed
//! tissue is cells pressed *into* one another. The overlap is the tissue.

use rayon::prelude::*;

use crate::cell::CellArena;
use crate::fixed::{pos_to_square, Q10_ONE, POS_ONE};
use crate::sensing::TouchReading;

/// The closest two cells may be pressed together, as a fraction of the distance at which their
/// outlines merely touch, in permille.
///
/// A cell is not a circle that happens to be solid; it is a bag of nearly incompressible water.
/// Pressed, it flattens against its neighbour and the two share a wall, and it goes on doing
/// that until there is nothing left to give. This is where nothing is left.
///
/// The same fraction as `mm_app::slide::MIN_FACE`, expressing the same idea from the other side:
/// every cell keeps an incompressible core of this much of its own radius. Here that is a floor
/// on how close two centres may come; there it is a floor on how deeply a cell may be cut. The
/// two must be changed together or the picture stops being a picture of the physics.
///
/// Not the same constraint, though, and neither makes the other redundant. For equal radii they
/// coincide exactly — at the distance where the cores touch, the plane through the crossing
/// outlines falls precisely at the core of each. For unequal radii they do not: a cell twice its
/// neighbour's radius has that plane past the smaller cell's *centre* while the two cores are
/// still apart, so respecting the core here does not stop the renderer cutting a small cell away.
pub const CORE_PERMILLE: i32 = 950;

/// The stiffest a cell can be, in the same permille. **A thousand: it does not overlap at all.**
///
/// This was 995, on the reasoning that two cells which cannot overlap share no wall, and SPEC §6.4
/// is explicit that the resting overlap *is* the tissue and that circles which do not overlap
/// cannot tile a plane. That reasoning has expired, and it is worth writing down why rather than
/// quietly changing the number.
///
/// It was true while every cell was drawn at `slide::PACKING` times its radius — 15% larger than
/// it is — so a pair had to physically overlap before their *drawn* outlines crossed enough to
/// share a face. A firm cell is now drawn at its true size (`slide::packing_for`), and a pair of
/// them resting exactly tangent is drawn exactly tangent: two circles, touching, no seam, no gap.
/// It needs no overlap to be drawn correctly, so there is nothing left to buy with the five
/// thousandths.
///
/// What the five thousandths cost is the thing that matters. Firmness in the *picture* can only
/// take a cell so far — measured, from 0.387 out of round to 0.167 — and the rest of the distance
/// is the pair genuinely not being pressed into one another. A core below tangency guarantees
/// they are. At a thousand, any overlap at all is past the core and takes the stiff response, so
/// a firm cell cannot be that close to another without pushing it away, which is the whole of
/// what "marble" means mechanically.
///
/// **A thousand was tried, put back, and then reached properly, and the route is the finding.**
///
/// The first attempt broke two things at once. `pressure` was normalised against the pair's own
/// band, `want - core`, which at tangency is zero — the guard skipped it, pressure was never
/// accumulated, and `split_pressure` and `growth_pressure` both silently stopped working. That is
/// fixed by normalising against a fixed reference band, which is also the right meaning: pressure
/// is how hard a cell is being squeezed, not how far through its own range it is.
///
/// The second was that with the core at tangency every contact takes the stiff branch, where it
/// had been the rare deep-penetration case, and the corrections were *summed* across a cell's
/// neighbours. A rigid pack buzzed at 110 thousandths of a square a tick against a soft one's 18.
/// `BiologyConfig::separation_relax` is the dial that fixes it, and an eighth is enough: 110 falls
/// to 9, which is quieter than the soft pack was before any of this.
///
/// A soft cell is unaffected: [`CORE_PERMILLE`] is still 950 and `rigidity_gain` is still zero by
/// default, so a tissue still tessellates and still shares its walls.
pub const CORE_PERMILLE_RIGID: i32 = 1000;

/// How firm a cell is *as the simulation sees it*, `Q10`: wall times turgor times the
/// scenario's `rigidity_gain`. Zero when the gain is, which is every world written before it.
#[must_use]
pub fn firmness(cells: &CellArena, i: usize, rates: &crate::metabolism::MetabolicRates) -> i32 {
    if rates.rigidity_gain <= 0 {
        return 0;
    }
    // Wall times turgor, and then the scenario's gain on top. See `biology::rigidity` for why the
    // renderer reads the same quantity *without* the gain: swell has no counterpart in the
    // simulation, and everything in this module does.
    crate::fixed::q10_scale(crate::biology::rigidity(cells, i, rates), rates.rigidity_gain)
        .clamp(0, crate::fixed::Q10_ONE)
}

/// Where one cell stops compressing, in permille of its own radius.
///
/// [`CORE_PERMILLE`] when `rigidity_gain` is zero, which is every world written before it, and
/// scaled towards [`CORE_PERMILLE_RIGID`] by [`firmness`] otherwise.
#[must_use]
pub fn core_permille(cells: &CellArena, i: usize, rates: &crate::metabolism::MetabolicRates) -> i32 {
    let rigidity = firmness(cells, i, rates);
    if rigidity <= 0 {
        return CORE_PERMILLE;
    }
    let span = (CORE_PERMILLE_RIGID - CORE_PERMILLE) as i64;
    CORE_PERMILLE + ((span * rigidity as i64) / crate::fixed::Q10_ONE as i64) as i32
}

/// How much of a contact's compression one relaxation pass takes out, in sixteenths.
///
/// Soft on purpose, and much softer than the value this replaced. Contact used to be a hard
/// non-penetration constraint driving every overlap to zero, which is the wrong target: cells
/// that are merely touching have no shared wall to draw, and circles that do not overlap cannot
/// tile a plane, so a crowd solved to convergence is a heap of discs with holes between them.
/// A gentle response lets an unloaded pair rest almost exactly touching and a loaded one sink in
/// proportionally, which is what flattens the contact into a face.
const CONTACT_STRENGTH: i32 = 1;

/// How much of the compression *past* the core one pass takes out, in sixteenths, on top of
/// [`CONTACT_STRENGTH`].
///
/// This is the whole mechanism: resistance is nearly free through the soft band and sixteen
/// times stiffer once the core is reached, so how deep a crowd settles is set by geometry rather
/// than by whatever the load happens to be. A crowd gets harder to squash the more it is
/// squashed.
///
/// Sixteen sixteenths, so one pass takes out a core penetration exactly once — each cell moves
/// half of it and the pair closes the whole of it. Not more: at thirty-two the pair separates by
/// twice what it was overlapping, which is an over-relaxed constraint, and an over-relaxed
/// constraint oscillates instead of settling. Modelled before it was written; at a load that
/// pins a pair to its core, this holds it within about 6% of the core and a softer 8/16 lets it
/// through by twice that.
///
/// Note that the core is stiff, not infinite, and [`MAX_SHOVE`] caps it further. A load heavy
/// enough will still press a pair through it — this bounds compression, it does not forbid it.
///
/// The response is continuous at the knee — at exactly the core this term contributes nothing —
/// so there is no step for the solver to chatter across.
const CORE_STRENGTH: i32 = 15;

/// The furthest one contact may move one cell in one pass, as a divisor of its own radius.
///
/// The guard rail, and the reason a rising stiffness is safe here when it was not before. A
/// stiff response to a deep overlap is a large number, and the deepest overlaps in the
/// simulation are not crowds at all — a daughter is placed within half a square of its parent,
/// which is far past the core. Clamping each shove means depth can buy stiffness without ever
/// buying a teleport.
///
/// Per shove rather than per tick, which is the substantive change. The budget this replaced was
/// a pool for the whole tick, so a cell wedged among eight neighbours spent it on the first two
/// or three contacts in slot order and the rest were silently skipped — the interior of a pack
/// could not resolve at all while its surface resolved perfectly. That is a starvation, not a
/// speed limit, and it is what a per-contact clamp gets right: eight neighbours may each push,
/// none of them may push far, and in a real pack they mostly cancel.
const MAX_SHOVE: i32 = 8;

/// How much of a barrier overlap one relaxation pass takes out, in sixteenths.
///
/// Sixteen, where a cell-cell contact is one: a wall is not a bag of water and there is no
/// tissue to be made by pressing into it. The soft band of [`CONTACT_STRENGTH`] exists so that
/// two cells share a face; a barrier has nothing to share, so the constraint is the whole
/// penetration, taken out at once and bounded only by [`MAX_SHOVE`].
///
/// That bound is what makes it safe rather than the strength being modest. One pass may move a
/// cell an eighth of its own radius, three passes three eighths — for the seeded ancestor about
/// a third of a square a tick, comfortably above `fluid::MAX_VELOCITY`. So a current cannot
/// drive a cell through a wall faster than the solver takes it back out, which is the property
/// that has to hold and the one [`barrier_correction`]'s test asserts directly.
const BARRIER_STRENGTH: i32 = 16;

/// Fraction of a touching cell's sliding velocity that survives a tick, `Q10`.
///
/// Cells are not ball bearings. Water alone is already syrup — see `sensing::DRAG_RETAIN` — but
/// drag acts on a cell moving through fluid, and two cells pressed against each other are a
/// different situation: there is a membrane dragging on a membrane, and it resists.
///
/// Without it the normal direction is handled and the tangential one is not, so a crowd under
/// load has no way to lock up: cells slide freely past their neighbours, the arrangement
/// reshuffles, and every shared wall in the picture is redrawn somewhere new each frame.
const CONTACT_FRICTION: i32 = crate::fixed::Q10_ONE / 4;

/// Speed below which a touching cell is treated as at rest, `Q10` squares per tick.
///
/// Static friction, and the thing that finally makes a loaded pack stop rather than merely
/// slow down. A steady body force re-accelerates a jammed cell every tick and the contacts undo
/// the position every tick, so the residual is one tick of acceleration — small, permanent, and
/// visible as a crowd that buzzes without going anywhere. A pile of sand under gravity does not
/// do this because grains in contact hold each other still until something exceeds the friction
/// between them.
///
/// Well below anything a cilium produces — a cilium at full power is hundreds of `Q10` — so this
/// stops a resting crowd without ever stopping a cell that is trying to swim.
const REST_SPEED: i32 = crate::fixed::Q10_ONE / 24;

/// Relaxation passes per tick.
///
/// Separation is a distance constraint — two cells must be at least their radii apart — and
/// SPEC §8.4 already asks for exactly this treatment on the other kind: position-based
/// dynamics, two to three Gauss-Seidel iterations a tick. Junctions got it and contacts did
/// not, and one pass at a quarter strength is not a solver, it is a nudge.
///
/// Three rather than more because it is the top of SPEC's range and because each pass is
/// another walk of every contact — the phase is already a quarter of the tick.
///
/// Note that passes are no longer what bounds compression; [`CORE_PERMILLE`] is. Under the hard
/// non-penetration constraint this replaced, pass count set how deep a crowd ended up, so the
/// number was load-bearing and undertuned. Now it only sets how quickly the crowd gets to a
/// depth the core already decides, which is a much less interesting job for a constant to have.
const SEPARATION_PASSES: usize = 3;

/// Cells indexed by the substrate square they stand on.
#[derive(Clone, Debug, Default)]
pub struct NeighbourIndex {
    /// Start of each square's run in `entries`, plus a final sentinel. `starts.len() == n + 1`.
    starts: Vec<u32>,
    /// Cell slots, grouped by square and ascending within each group.
    entries: Vec<u32>,
    width: i32,
    height: i32,
    /// The largest cell radius on the slide, `POS`. What sizes the neighbour search.
    max_radius: i32,
    /// How many squares out [`Self::around`] walks.
    ///
    /// Was a hard-coded one, and that was correct only while a cell was smaller than a substrate
    /// square. It is not: a cell of mass 44 has a radius of a whole square, so two of them sit
    /// about 1.65 squares apart and a three-by-three walk cannot see half of a cell's real
    /// neighbours. Contacts that are never found are never separated and never drawn with a
    /// shared wall, which is why a packed sheet came out as discs layered over one another with
    /// only three or four seams each where a tiled monolayer needs six.
    search: i32,
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

        // Sized from the population rather than assumed. Two cells interact out to the sum of
        // their radii, so the walk has to reach twice the largest radius, plus one square
        // because a cell sits somewhere *within* its square rather than at the corner.
        let mut max_radius = 0i32;
        for i in cells.iter() {
            max_radius = max_radius.max(crate::fixed::q10_to_pos(crate::biology::radius(cells, i)));
        }
        self.max_radius = max_radius;
        self.search = self.squares_for(max_radius.saturating_mul(2));

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
    /// How many squares a reach of `POS` units needs, rounded up, plus one for the offset of a
    /// cell within its own square.
    fn squares_for(&self, reach: i32) -> i32 {
        (reach.saturating_add(POS_ONE - 1) / POS_ONE).saturating_add(1)
    }

    fn row_run(&self, sx: i32, sy: i32, k: i32) -> &[u32] {
        if self.width <= 0 || sy < 0 || sy >= self.height {
            return &[];
        }
        let x0 = sx.saturating_sub(k).max(0);
        let x1 = sx.saturating_add(k).min(self.width - 1);
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
        self.within(sx, sy, self.search)
    }

    /// Every living slot, in the order the grid holds them: by square, row-major.
    ///
    /// The counting sort that builds the buckets leaves this spatially sorted for free, and it
    /// is what a separation pass should iterate rather than `0..capacity`. Cells near each other
    /// on the slide share neighbours, so consecutive entries here re-read the same arena rows
    /// while they are still in cache, where consecutive *slots* are wherever the free list
    /// happened to put them.
    ///
    /// The half of the space-filling-curve idea that is reachable without reordering the arena
    /// itself — which cannot be done, because `CellId` carries the slot, so a permutation would
    /// invalidate every junction, parent and archived reference in the world.
    ///
    /// Safe to iterate in any order only because separation is now Jacobi: each cell computes
    /// its own correction and writes nobody else, so which order the work happens in is not
    /// something the result can see.
    #[must_use]
    pub fn occupants(&self) -> &[u32] {
        &self.entries
    }

    /// Every cell a cell of `radius` could possibly be touching, in a fixed order.
    ///
    /// Two cells interact out to the sum of their radii, so this one has to reach its own
    /// radius plus the largest on the slide — not *twice* the largest, which is what
    /// [`Self::around`] assumes because it does not know who is asking.
    ///
    /// The difference is not small. A slide is mostly cells of about one size with a handful of
    /// outliers, and the search is a square: measured on a populated slide the median radius was
    /// 1.25 squares and the largest 7, so every cell was scanning 961 grid squares when its own
    /// reach needed 81. That is a twelvefold tax on the phase that *is* the tick — collision
    /// separation was 113% of a whole tick at sixty thousand cells — levied on the whole
    /// population by one big cell, four times a tick, because the walk runs once for touch and
    /// once per separation pass.
    ///
    /// Still `max_radius` and not the neighbour's actual radius, because the whole point of an
    /// index is not to have looked at the neighbour yet. What this removes is the half of the
    /// overestimate that was never about the caller at all.
    pub fn around_radius(&self, sx: i32, sy: i32, radius: i32) -> impl Iterator<Item = usize> + '_ {
        let reach = self.squares_for(radius.saturating_add(self.max_radius));
        self.within(sx, sy, reach.min(self.search))
    }

    /// Every cell within `k` squares, in a fixed order.
    pub fn within(&self, sx: i32, sy: i32, k: i32) -> impl Iterator<Item = usize> + '_ {
        let k = k.clamp(1, 64);
        (-k..=k)
            .flat_map(move |dy| self.row_run(sx, sy.saturating_add(dy), k))
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
    pub fn contacts(
        &self,
        cells: &CellArena,
        i: usize,
        reach_permille: i32,
        rates: &crate::metabolism::MetabolicRates,
    ) -> ContactSet {
        let mut out = ContactSet::default();
        if !cells.occupied(i) {
            return out;
        }
        let sx = pos_to_square(cells.x[i]);
        let sy = pos_to_square(cells.y[i]);
        let ri = crate::fixed::q10_to_pos(crate::biology::radius(cells, i));
        let reach = reach_permille.max(0) as i64;
        // The renderer looks further than the physics does, so it gets its own radius rather
        // than the index's default. A seam that is never looked for is a wall that is never
        // drawn, and the cell is then drawn lying over its neighbour instead.
        let k = self.squares_for(
            ((self.max_radius.saturating_mul(2) as i64 * reach) / 1000).min(i32::MAX as i64) as i32,
        );
        for j in self.within(sx, sy, k) {
            if j == i || !cells.occupied(j) {
                continue;
            }
            let rj = crate::fixed::q10_to_pos(crate::biology::radius(cells, j));
            let touching = ((ri.saturating_add(rj) as i64 * reach) / 1000) as i32;
            // The same metric the seam will be drawn from. Admitting a neighbour on one distance
            // and drawing it from another is what made seams flicker in and out.
            let d_sq = separation_sq(cells, i, j);
            if d_sq >= (touching as i64) * (touching as i64) {
                continue;
            }
            let d = d_sq.isqrt().min(i32::MAX as i64) as i32;
            let overlap = touching.saturating_sub(d);
            if overlap <= 0 {
                continue;
            }
            out.offer(Contact {
                dx: cells.x[j].saturating_sub(cells.x[i]),
                dy: cells.y[j].saturating_sub(cells.y[i]),
                radius: rj,
                mass: cells.mass[j],
                overlap,
                rigidity: cells.slots(j).first().map_or(0, |m| m.param as i32),
                joined: crate::junction::existing(cells, i, cells.id_at(j)).is_some(),
                firmness: crate::biology::rigidity(cells, j, rates),
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
/// Eight, which is more than a cell in a monolayer ever has.
///
/// Six is the number a packing of similar circles settles on, so eight is headroom — and
/// headroom is the whole point, because the cap binding is what breaks the picture. Two cells
/// only meet along one wall if *both* of them cut against the other, and a cell that has run
/// out of slots stops cutting for somebody who is still cutting for it. The result is one cell
/// laid over another with no shared wall at all, which is precisely what a crowd should never
/// look like. Cheaper to never reach the limit than to be clever about which contact to drop.
pub const CONTACTS_PER_CELL: usize = 24;

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
    /// The neighbour's mass, `Q10`.
    ///
    /// Carried so a caller that draws cells at a size of its own devising can work the
    /// neighbour out the same way it works itself out. Without it a front end that smooths the
    /// radius smooths only its *own* — and then the two cells of a pair compute their shared
    /// wall from different numbers and stop agreeing where it is, which is the one property the
    /// whole seam scheme rests on.
    pub mass: i32,
    /// How far inside each other's reach the two are, `POS`. Always positive, and measured
    /// against whatever reach the caller asked for rather than against bare radii.
    pub overlap: i32,
    /// How firmly the neighbour holds its own shape, `Q10`. See [`crate::biology::rigidity`].
    ///
    /// The neighbour's, not this cell's, and reported for the same reason `mass` is: a renderer
    /// that draws a firm cell smaller than a soft one has to work the *pair's* shared wall out
    /// from both sizes, or the two cells compute two different planes for one wall and are drawn
    /// overlapping. `rigidity` beside it is the membrane parameter alone, which decides which of
    /// two cells gives way; this is wall times turgor, which decides how large either is drawn.
    pub firmness: i32,

    /// Whether these two are joined, by a junction of either kind.
    ///
    /// Reported because being *stuck to* a neighbour and merely being pressed against one are
    /// two different situations and the picture should not be the same. A tissue shares its
    /// walls; a heap of separate bodies does not, however hard it is packed. See
    /// `mm_app::slide::squash_of`, which is the only caller and the whole reason this is here.
    ///
    /// Either kind, deliberately. A soft junction is a channel and a hard one is a strut, but
    /// both are a lineage having decided that the cell on the other end is part of it, and that
    /// is the question being asked.
    pub joined: bool,

    /// What the neighbour has invested in its membrane — slot zero's `param`.
    ///
    /// Reported because it is the nearest thing a cell has to a turgor: a cell that paid for a
    /// thick membrane holds its shape, and one that did not gives way. It already decides how
    /// much damage the cell can take before it fails, so it is a trait under selection rather
    /// than a number invented for the renderer, and letting it decide which of two cells
    /// deforms costs nothing new to evolve.
    pub rigidity: i32,
}

/// The neighbours pressing on one cell, deepest first.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ContactSet {
    found: [Contact; CONTACTS_PER_CELL],
    len: usize,
}

impl ContactSet {
    /// Keep this one, and the deepest if there is no room.
    ///
    /// Every neighbour within reach earns a slot while there are slots, and that plainness is
    /// deliberate. An earlier version merged neighbours lying in nearly the same direction, on
    /// the reasoning that the nearer one's seam hides the far one's. It does — for that cell.
    /// But the merge is decided from one side only, so the cell doing the merging stops
    /// cutting against a neighbour that is still cutting against it, and the two no longer
    /// agree where their wall is. One is drawn straight over the other with no shared edge,
    /// which looks exactly like the overlapping this whole mechanism exists to remove.
    ///
    /// Two cells meet along one wall only if both cut for the other. Anything that decides
    /// per-cell which contacts to keep can break that; the only safe cap is one high enough
    /// never to be reached.
    fn offer(&mut self, c: Contact) {
        if self.len < CONTACTS_PER_CELL {
            self.found[self.len] = c;
            self.len += 1;
            return;
        }
        // Full, and this one is from a direction nothing else covers. It earns a place only by
        // being deeper than the shallowest thing here.
        let mut worst = 0;
        for k in 1..self.len {
            if self.found[k].overlap < self.found[worst].overlap {
                worst = k;
            }
        }
        if c.overlap > self.found[worst].overlap {
            self.found[worst] = c;
        }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Contact] {
        self.found.get(..self.len).unwrap_or(&[])
    }
}

/// What one cell can feel around it (SPEC §6.2's touch sensor).
///
/// What a cell can see glowing around it, by band, with the direction it is coming from.
///
/// Computed on demand for the cell asking, not gathered for the population: the scan is a
/// square of side `2 * range + 1` squares, so the cost falls on the cell that wants to see
/// rather than on the world. A slide where nothing carries a photosensor pays nothing, and a
/// slide where everything does is a slide where seeing is worth it.
///
/// **Falloff is inverse-square**, which is what radiating into two dimensions from a point does,
/// with a floor of one square so a cell standing on top of another does not divide by nothing.
/// Without it a genome could not tell one loud neighbour from four distant ones, and the
/// gradient would point at the densest crowd rather than the brightest thing in it.
///
/// Accumulated in `i64` and saturated once at the end. Saturating addition is not associative,
/// so per-step saturation would make the answer depend on the order the neighbours came in —
/// which is an I6 violation that shows up only as two machines disagreeing.
#[must_use]
pub fn glow_reading(
    cells: &CellArena,
    index: &NeighbourIndex,
    i: usize,
    range: i32,
    band: usize,
) -> crate::sensing::ChemReading {
    let (sx, sy) = (
        crate::fixed::pos_to_square(cells.x[i]),
        crate::fixed::pos_to_square(cells.y[i]),
    );
    let (mut total, mut gx, mut gy) = (0i64, 0i64, 0i64);
    let reach = crate::fixed::pos(range.max(1));
    for j in index.within(sx, sy, range.max(1)) {
        if j == i || !cells.occupied(j) {
            continue;
        }
        let power = cells
            .emission
            .get(j)
            .and_then(|e| e.get(band))
            .copied()
            .unwrap_or(0);
        if power <= 0 {
            continue;
        }
        let (dx, dy) = (
            cells.x[j].saturating_sub(cells.x[i]) as i64,
            cells.y[j].saturating_sub(cells.y[i]) as i64,
        );
        // One square, squared, as the floor: closer than that and it is simply "touching".
        let one = crate::fixed::POS_ONE as i64;
        let d2 = (dx * dx + dy * dy).max(one * one);
        if d2 > (reach as i64) * (reach as i64) {
            continue;
        }
        let seen = (power as i64 * one * one) / d2;
        total += seen;
        // Weighted by what is seen rather than by what is there, so the gradient points at the
        // brightest thing rather than the nearest.
        gx += (seen * dx) / one;
        gy += (seen * dy) / one;
    }
    crate::sensing::ChemReading {
        concentration: crate::fixed::sat_i32(total),
        gradient_x: crate::fixed::sat_i32(gx),
        gradient_y: crate::fixed::sat_i32(gy),
    }
}

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
        // What the nearest neighbour is wearing (ISA 4). The engine does not know what a
        // friend is and this is how it stays that way: it reports a number and says nothing
        // about what the number means. Nothing when there is nobody to read.
        // Gated on `contacts`, not on `nearest_slot`, and the difference is not cosmetic:
        // `nearest_slot` starts at 0 rather than at a sentinel, so a cell touching nobody
        // reports slot 0 as its nearest neighbour. That is harmless for the `nearest` reading,
        // which exists to be handed to `JOIN` and `JOIN` checks the distance — and it is not
        // harmless here, because a lonely cell would read whatever slot 0 happens to be
        // wearing, which in a two-cell world is itself.
        badge: if contacts > 0 {
            cells.badge[nearest_slot as usize] as i16
        } else {
            0
        },
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

/// Distance between two cells, `POS` units. Euclidean, by integer square root.
///
/// It was octagonal — `max + min/2` — on the reasoning that it is monotonic in the true distance
/// and nothing needed the difference. Both halves of that turned out to be wrong.
///
/// The octagonal distance overestimates by 6% at 45° and not at all on the axes, so it is not one
/// metric but a direction-dependent family of them, and two things depended on the difference.
///
/// The solver settled a pair at a distance that depended on the angle between them, which was
/// tolerable while compression was bounded only by a starved budget and became the dominant
/// effect once [`CORE_PERMILLE`] made the whole band 5% wide — a 6% error inside a 5% band means
/// a crowd that can never settle, only rotate and be pushed around. It looks exactly like what it
/// is: cell boundaries fighting over where they belong.
///
/// And [`NeighbourIndex::contacts`] admitted a neighbour on this distance while handing back the
/// exact `dx, dy` for the renderer to draw the seam from. So a diagonal neighbour was rejected
/// where an equally close axial one was admitted, and cells jiggling across that inconsistent
/// threshold made seams appear and vanish between frames.
///
/// The square root is cheaper than it sounds, because [`separation_sq`] answers the common
/// question without one.
#[inline]
fn separation(cells: &CellArena, i: usize, j: usize) -> i32 {
    separation_sq(cells, i, j).isqrt().min(i32::MAX as i64) as i32
}

/// The square of [`separation`], which is the form most callers actually want.
///
/// Comparing `d² >= want²` answers "are these two apart?" without a square root, and that is the
/// answer for the overwhelming majority of pairs the solver looks at. Only the few that really do
/// overlap pay for the root.
#[inline]
fn separation_sq(cells: &CellArena, i: usize, j: usize) -> i64 {
    let dx = (cells.x[i] as i64) - (cells.x[j] as i64);
    let dy = (cells.y[i] as i64) - (cells.y[j] as i64);
    dx * dx + dy * dy
}

/// What one pass decided about one cell.
///
/// A cell's whole answer, computed from its neighbours without touching any of them. Returned
/// rather than applied so that a pass can compute every cell's answer at once and apply them
/// together — see the Jacobi loop in [`resolve_collisions`].
#[derive(Clone, Copy, Default, Debug)]
struct Correction {
    dx: i32,
    dy: i32,
    /// How far this cell is driven into cells it is not joined to, `POS`.
    crowding: i32,
    /// How stuck it is, `Q10`, one unit per neighbour bottomed out on its core.
    pressure: i32,
    /// Whether it is against anything at all, which is not the same as whether it moved: the
    /// inside of a pack is where the pushes cancel.
    touching: bool,
    /// Contacts seen, for the population-wide count.
    contacts: u32,
    /// The part of the correction that came from the *core* branch, kept apart so it can be
    /// under-relaxed on its own. See the note at the end of [`correction_for`].
    sdx: i32,
    sdy: i32,
    /// How many contacts contributed to it. Not the same as `contacts`: a pair resting in the
    /// soft band asks for nothing from the core.
    stiff_contacts: u32,
}

/// Push one cell out of any blocked square its body overlaps.
///
/// # Why this did not exist until now
///
/// `blocked` has been in the substrate since M1 and the fluid has always respected it — flux
/// across a blocked edge is zero, which is what makes a barrier conserve matter for free. Cells
/// were never told. A barrier stopped chemistry and light and let bodies straight through, so
/// every wall in every scenario was a wall for the water and a suggestion for everything alive
/// in it, and `scenarios/archipelago.ron` was fragmenting the fluid without fragmenting the
/// population it was written to isolate.
///
/// SPEC §17.1 asks barriers to do three things — reduce usable area, isolate populations, and
/// make an edge a different place to live from open water — and not one of them can happen
/// while a cell can swim through the wall.
///
/// # What it deliberately does not do
///
/// A barrier contact contributes a shove and sets `touching`. It does **not** contribute to
/// `crowding` or to `pressure`, and that is a decision rather than an omission. `crowding` is
/// what [`crate::ecology`] charges membrane damage for and `pressure` is what
/// [`crate::biology`] refuses divisions above, so counting a wall in either would make the
/// perimeter of a room a worse place to live than its middle. SPEC §17.1 wants the opposite:
/// a cell against a wall has fewer *neighbours* pressing on it, so the edge should be the
/// better address and the gradient that creates is the point of drawing rooms at all. A wall
/// is something to rest against, not something that crushes.
///
/// An empty `blocked` means a slide with no barriers on it, and the whole scan is skipped —
/// the common case, and a real grid is never zero squares.
fn barrier_correction(
    cells: &CellArena,
    blocked: &[bool],
    width: i32,
    height: i32,
    i: usize,
    ri: i32,
) -> (i32, i32, bool) {
    if blocked.is_empty() || ri <= 0 || width <= 0 || height <= 0 {
        return (0, 0, false);
    }
    let (cx, cy) = (cells.x[i], cells.y[i]);
    // Only the squares the body can actually reach. At the seeded radius that is three by
    // three; at `max_mass` it is six by six, which is the ceiling `biology::max_mass` exists
    // to keep on the collision phase generally.
    let lo_x = pos_to_square(cx - ri).max(0);
    let hi_x = pos_to_square(cx + ri).min(width - 1);
    let lo_y = pos_to_square(cy - ri).max(0);
    let hi_y = pos_to_square(cy + ri).min(height - 1);
    // The same per-contact clamp cell-cell contacts use, and for the same reason: depth may buy
    // stiffness but it may never buy a teleport.
    let cap = (ri / MAX_SHOVE).max(1);

    let (mut dx, mut dy, mut touching) = (0i32, 0i32, false);
    for sy in lo_y..=hi_y {
        for sx in lo_x..=hi_x {
            let idx = sy as usize * width as usize + sx as usize;
            if !blocked.get(idx).copied().unwrap_or(false) {
                continue;
            }
            let left = sx.saturating_mul(POS_ONE);
            let top = sy.saturating_mul(POS_ONE);
            let (right, bottom) = (left + POS_ONE, top + POS_ONE);
            // Closest point on the square to the centre — the standard disc-against-box test.
            let qx = cx.clamp(left, right);
            let qy = cy.clamp(top, bottom);

            let (ux, uy, penetration) = if qx != cx || qy != cy {
                // Centre outside the square, which is nearly always. Leave along the line from
                // the closest point, so a cell against a face leaves square to it and a cell on
                // a corner leaves diagonally.
                let (ox, oy) = ((cx - qx) as i64, (cy - qy) as i64);
                let d = (ox * ox + oy * oy).isqrt().max(1);
                if d >= ri as i64 {
                    continue;
                }
                (
                    (ox * POS_ONE as i64 / d) as i32,
                    (oy * POS_ONE as i64 / d) as i32,
                    ri - d as i32,
                )
            } else {
                // Centre *inside* the square. There is no line to leave along — the closest
                // point is the centre itself — so leave by whichever face is nearest, which is
                // the shortest way out and cannot pick a direction that drives deeper.
                //
                // Reachable despite the constraint, and it must be total rather than merely
                // unlikely: a barrier drawn on top of a standing cell by the drawing tool puts
                // one here immediately, and so does a daughter budded into a wall.
                let (dl, dr) = (cx - left, right - cx);
                let (dt, db) = (cy - top, bottom - cy);
                let least = dl.min(dr).min(dt).min(db);
                let (ux, uy) = if least == dl {
                    (-POS_ONE, 0)
                } else if least == dr {
                    (POS_ONE, 0)
                } else if least == dt {
                    (0, -POS_ONE)
                } else {
                    (0, POS_ONE)
                };
                (ux, uy, ri.saturating_add(least))
            };

            let shove = penetration.saturating_mul(BARRIER_STRENGTH) / 16;
            let allowed = shove.min(cap).max(0);
            if allowed <= 0 {
                continue;
            }
            touching = true;
            dx = dx.saturating_add((ux as i64 * allowed as i64 / POS_ONE as i64) as i32);
            dy = dy.saturating_add((uy as i64 * allowed as i64 / POS_ONE as i64) as i32);
        }
    }
    (dx, dy, touching)
}

/// Whether any part of this cell's body is against a barrier.
///
/// What a holdfast needs to know, and a strictly cheaper question than
/// [`barrier_correction`]'s: it stops at the first blocked square within reach instead of
/// accumulating a push from all of them. Kept beside that function rather than in
/// [`crate::sensing`] so the two answers can never disagree about what "against a wall" means.
#[must_use]
pub fn touches_barrier(
    cells: &CellArena,
    blocked: &[bool],
    width: i32,
    height: i32,
    i: usize,
    ri: i32,
) -> bool {
    if blocked.is_empty() || ri <= 0 || width <= 0 || height <= 0 {
        return false;
    }
    let (cx, cy) = (cells.x[i], cells.y[i]);
    let lo_x = pos_to_square(cx - ri).max(0);
    let hi_x = pos_to_square(cx + ri).min(width - 1);
    let lo_y = pos_to_square(cy - ri).max(0);
    let hi_y = pos_to_square(cy + ri).min(height - 1);
    for sy in lo_y..=hi_y {
        for sx in lo_x..=hi_x {
            if !blocked
                .get(sy as usize * width as usize + sx as usize)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            let left = sx.saturating_mul(POS_ONE);
            let top = sy.saturating_mul(POS_ONE);
            let (ox, oy) = (
                (cx - cx.clamp(left, left + POS_ONE)) as i64,
                (cy - cy.clamp(top, top + POS_ONE)) as i64,
            );
            if ox * ox + oy * oy < (ri as i64) * (ri as i64) {
                return true;
            }
        }
    }
    false
}

/// Solve one cell against its neighbourhood, reading everything and writing nothing.
///
/// The whole body of the old inner loop, with the two-sided writes turned into one cell's share.
/// Every quantity that used to be credited to both cells of a pair is now computed by each of
/// them about itself, which gives the same numbers — the geometry is symmetric — and removes the
/// only reason the pass had to be sequential.
#[allow(clippy::too_many_arguments)]
fn correction_for(
    cells: &CellArena,
    index: &NeighbourIndex,
    radii: &[i32],
    permille: &[i32],
    blocked: &[bool],
    i: usize,
    first_pass: bool,
    relax: i32,
) -> Correction {
    let mut out = Correction::default();
    if !cells.occupied(i) {
        return out;
    }
    let ri = radii[i];
    // The world before the neighbours. A barrier is immovable, so its shove is one-sided and
    // needs no pair to agree with — see [`barrier_correction`] for why it feeds only `dx`,
    // `dy` and `touching`.
    let (bdx, bdy, btouch) = barrier_correction(cells, blocked, index.width, index.height, i, ri);
    out.dx = bdx;
    out.dy = bdy;
    out.touching = btouch;
    let sx = pos_to_square(cells.x[i]);
    let sy = pos_to_square(cells.y[i]);
    for j in index.around_radius(sx, sy, ri) {
        if j == i || !cells.occupied(j) {
            continue;
        }
        let want = ri.saturating_add(radii[j]);
        // Tested on squares, so the pairs that are merely near each other — nearly all of them —
        // never pay for a square root.
        let d_sq = separation_sq(cells, i, j);
        if d_sq >= (want as i64) * (want as i64) {
            continue;
        }
        let d = d_sq.isqrt().min(i32::MAX as i64) as i32;
        // How far the pair is compressed, and how much of that is past the core.
        let squeeze = want - d;
        // Each cell contributes its own incompressible core, so a rigid cell pressed against a
        // limp one is the limp one that gives. Symmetric — both sides of the pair compute the
        // same number from the same two cells, which Jacobi separation requires — and exactly
        // equal to the expression it replaces when every cell is at `CORE_PERMILLE`.
        let core = ((ri as i64 * permille[i] as i64 + radii[j] as i64 * permille[j] as i64) / 1000)
            as i32;
        let crushed = (core - d).max(0);

        // Being crushed, charged to this cell — except by whatever it is joined to. An organism
        // is *meant* to hold its cells against each other, and billing it for that would make
        // being multicellular a way to die.
        //
        // Charged on the whole compression, which is safe now that [`CORE_PERMILLE`] bounds it at
        // a twentieth of the touching distance. It was briefly charged on core penetration only,
        // to stop an ordinary crowd being lethal back when a cell could legitimately rest more
        // than halfway inside its neighbour — with a core this tight nothing penetrates it and
        // that measure would read zero for every cell on the slide, quietly deleting crowding
        // pressure altogether.
        //
        // First pass only. The later passes are the same contacts relaxed further, not new ones,
        // and charging for each would make the price of being in a crowd depend on how many times
        // the solver looked at it.
        if first_pass && !joined(cells, i, j) {
            out.crowding = out.crowding.saturating_add(squeeze);
            // And how *stuck* the pair is, which is a different question from how deep they
            // overlap and the one that decides whether there is room to bud into.
            //
            // Zero where the two merely touch and `Q10_ONE` where they are bottomed out on their
            // cores and the solver has nothing left to give. Summed over contacts, so it rises
            // both with how many neighbours press and with how hard each presses — the
            // combination that means "pressed into a space too small" rather than merely
            // "surrounded". A cell ringed by neighbours all resting lightly is enclosed and not
            // under pressure: its whole neighbourhood still has somewhere to expand into.
            //
            // Normalised against the band rather than the radius, so it does not change when a
            // population shrinks: being jammed is being jammed at any size, and the size question
            // is [`crate::ecology`]'s to answer.
            // Against a **fixed reference band**, not this pair's own. The two are the same
            // number until a cell can be stiffer than the default, and then they part company
            // badly: a fully rigid pair has `want == core`, so its own band is zero, and the
            // guard below then skips it — pressure is never accumulated, nothing is ever
            // considered crowded, and `split_pressure` and `growth_pressure` both stop working.
            // Measured, that took a settled pack from 266 cells to 394 and tripled its jitter.
            //
            // It is also the right meaning. Pressure is *how hard this cell is being squeezed*,
            // and normalising it by the cell's own compressibility asks a different question —
            // how far through its personal range it is — so a stiff cell reads as jammed the
            // instant it touches anything and a soft one never does. Against a fixed reference,
            // a firm pair resting tangent is squeezed by nothing and correctly reads zero.
            let band = ((want as i64 * (1000 - CORE_PERMILLE) as i64) / 1000) as i32;
            if band > 0 {
                let one = crate::fixed::Q10_ONE as i64;
                let share = (((squeeze.max(0) as i64) * one) / band as i64).clamp(0, one) as i32;
                out.pressure = out.pressure.saturating_add(share);
            }
        }

        let (dx, dy) = (cells.x[i] - cells.x[j], cells.y[i] - cells.y[j]);
        // Exactly coincident cells have no line to push along, so they get a fixed nudge derived
        // from their slots — deterministic, and enough to break the tie. Antisymmetric in `i` and
        // `j`, so the two sides of the pair still choose opposite directions now that each
        // decides for itself.
        let (ux, uy) = if dx == 0 && dy == 0 {
            let away = if i < j { 1 } else { -1 };
            (
                if (i + j) % 2 == 0 { POS_ONE } else { -POS_ONE } * away,
                if (i / 2 + j / 2) % 2 == 0 {
                    POS_ONE
                } else {
                    -POS_ONE
                } * away,
            )
        } else {
            let scale = d.max(1);
            (
                (dx as i64 * POS_ONE as i64 / scale as i64) as i32,
                (dy as i64 * POS_ONE as i64 / scale as i64) as i32,
            )
        };
        // Soft everywhere, plus a second term that only exists inside the core. The sum is
        // continuous at the knee, because `crushed` is zero there.
        //
        // Halved once for the sixteenths and once more because both cells move, so the pair
        // closes by the full fraction while each travels half of it.
        let soft_push = squeeze.saturating_mul(CONTACT_STRENGTH);
        let stiff_push = crushed.saturating_mul(CORE_STRENGTH);
        let push = soft_push.saturating_add(stiff_push);
        let shove = push / 16 / 2;
        // Clamped per contact, not drawn from a per-tick pool. See [`MAX_SHOVE`]: the pool
        // starved the inside of a crowd, which is the one place that has to work.
        let cap = (ri / MAX_SHOVE).min(radii[j] / MAX_SHOVE).max(1);
        let allowed = shove.min(cap).max(0);
        if allowed <= 0 {
            continue;
        }
        out.touching = true;
        out.contacts = out.contacts.saturating_add(1);
        // Split by which branch asked for it, so the relaxation below can be aimed at the one that
        // needs it — but **only when there is a relaxation to aim**, because splitting one rounded
        // multiply into two is not a no-op. Two roundings instead of one is a few `POS` units per
        // contact, and in a system where a pack's arrangement is chaotic that is enough to
        // reshuffle every seeded acceptance result. At `relax == 0` this is the arithmetic that
        // shipped, digit for digit.
        if relax > 0 {
            let stiff = if push > 0 {
                ((allowed as i64 * stiff_push as i64) / push as i64) as i32
            } else {
                0
            };
            let soft = allowed - stiff;
            out.dx = out
                .dx
                .saturating_add((ux as i64 * soft as i64 / POS_ONE as i64) as i32);
            out.dy = out
                .dy
                .saturating_add((uy as i64 * soft as i64 / POS_ONE as i64) as i32);
            out.sdx = out
                .sdx
                .saturating_add((ux as i64 * stiff as i64 / POS_ONE as i64) as i32);
            out.sdy = out
                .sdy
                .saturating_add((uy as i64 * stiff as i64 / POS_ONE as i64) as i32);
            if stiff > 0 {
                out.stiff_contacts = out.stiff_contacts.saturating_add(1);
            }
        } else {
            out.dx = out
                .dx
                .saturating_add((ux as i64 * allowed as i64 / POS_ONE as i64) as i32);
            out.dy = out
                .dy
                .saturating_add((uy as i64 * allowed as i64 / POS_ONE as i64) as i32);
        }
    }

    // The stiff corrections, shared out among the contacts that asked for them.
    //
    // Each contact computes what would fix that pair on its own, and summing six of them moves a
    // cell six times as far as any one wanted. That is a textbook over-relaxation, and the
    // textbook fix is to divide by the number of constraints: the divisor is
    // `1 + (n - 1) * relax`, so zero is exactly the sum and `Q10_ONE` is exactly the average.
    //
    // **Only the stiff branch, and that is the whole of why this is safe.** The soft response is a
    // thirty-second of the overlap and six of them do not overshoot anything; it is the core's
    // sixteen-sixteenths that does, and for a cell of default stiffness that branch is the rare
    // deep-penetration case rather than every contact. So a soft population is left almost exactly
    // as it was — which matters, because relaxing *everything* was tried and it moved things that
    // have nothing to do with jitter: `sponge`'s filter feeder stopped out-gathering a cell with
    // no holdfast, and the settled population of a stress slide fell a fifth. A rigid population,
    // where the stiff branch fires on every contact, is where the correction belongs and is the
    // only place it lands.
    //
    // **The barrier's shove is held out of it entirely.** A wall is immovable and its correction is
    // one-sided — there is no pair to share it with — so dividing it by however many cells happen
    // to be touching lets a crowded cell sink into it. That broke the filter feeder outright the
    // first time this was written, for a different reason than the one above and with the same
    // symptom.
    let (mut sdx, mut sdy) = (out.sdx, out.sdy);
    if out.stiff_contacts > 1 && relax > 0 {
        let n = (out.stiff_contacts - 1) as i64;
        let divisor = (Q10_ONE as i64).saturating_add(n.saturating_mul(relax as i64)).max(1);
        sdx = ((sdx as i64 * Q10_ONE as i64) / divisor) as i32;
        sdy = ((sdy as i64 * Q10_ONE as i64) / divisor) as i32;
    }
    out.dx = out.dx.saturating_add(sdx);
    out.dy = out.dy.saturating_add(sdy);
    out
}

/// Resolve contacts, in slot order, with bounded compression.
///
/// Cells are not driven apart until they stop overlapping. They are allowed to compress, softly
/// at first and then very stiffly past [`CORE_PERMILLE`] of their touching distance, so an
/// unloaded pair rests very nearly tangent and a loaded one flattens against its neighbour by an
/// amount that depends on the load. That resting overlap is the point rather than an error: it is
/// the region the renderer cuts into a shared wall, and without it a crowd is a heap of circles
/// with holes between them, because circles do not tile a plane.
///
/// Returns how many pairs were moved, which is a cheap measure of how crowded the slide is and
/// the thing to watch if a population stops growing for reasons that are not food.
///
/// Also records two different things about being crowded, because they answer different
/// questions and this is the pass that already knows both.
///
/// `crowding` is how far each cell is driven into cells it is *not* joined to, in `POS`.
/// [`crate::ecology`] charges membrane damage for it.
///
/// `pressure` is how *stuck* each cell is, in `Q10`, summed over the same contacts: one unit per
/// neighbour that has bottomed out on its core, nothing for a neighbour merely resting against
/// it. [`crate::biology`] refuses divisions above a threshold of it. Enclosure alone is not the
/// signal — a cell ringed by neighbours with room to spread still has somewhere to put a
/// daughter, and only one whose neighbourhood has nothing left to give does not.
/// `blocked` is the substrate's barrier mask, one entry per square in row-major order. Pass an
/// empty slice for a slide with no barriers on it; see [`barrier_correction`].
/// Working memory for [`resolve_collisions`], kept between ticks.
///
/// The solver allocated four vectors the size of the arena on every call — two for what the
/// constraints moved each cell, one for what is touching anything, and one per pass for the
/// corrections `collect` gathered. At fifty thousand cells and three passes that is six
/// allocations and about a megabyte of zeroing a tick, for buffers whose contents never survive
/// the call that made them.
///
/// Scratch in the same sense as `World::radii` and `World::crowding`: derived fresh inside every
/// call, so two worlds that differ only in what is left here are the same world, and it is
/// excluded from equality, hashing and snapshots.
#[derive(Clone, Debug, Default)]
pub struct SeparationScratch {
    /// What the constraints ended up moving each cell, so velocity can be reconciled with it.
    push_x: Vec<i32>,
    push_y: Vec<i32>,
    /// Whether a cell is touching anything at all — not the same question as whether it *moved*.
    touching: Vec<bool>,
    /// One pass's corrections, gathered in slot order.
    deltas: Vec<(u32, Correction)>,
}

impl SeparationScratch {
    /// Empty every buffer and size it for `capacity` cells, keeping the allocations.
    fn begin(&mut self, capacity: usize) {
        self.push_x.clear();
        self.push_x.resize(capacity, 0);
        self.push_y.clear();
        self.push_y.resize(capacity, 0);
        self.touching.clear();
        self.touching.resize(capacity, false);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_collisions(
    cells: &mut CellArena,
    index: &NeighbourIndex,
    radii: &mut Vec<i32>,
    permille: &mut Vec<i32>,
    crowding: &mut Vec<i32>,
    pressure: &mut Vec<i32>,
    blocked: &[bool],
    rates: &crate::metabolism::MetabolicRates,
    relax: i32,
    scratch: &mut SeparationScratch,
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
    pressure.clear();
    pressure.resize(cells.capacity(), 0);
    // Where each cell stops compressing, hoisted for the same reason the radii are: it reads the
    // whole interior to work out the turgor, and the inner loop would otherwise redo that once
    // per pair. Exactly `CORE_PERMILLE` for every cell when `rigidity_gain` is zero.
    permille.clear();
    permille.reserve(cells.capacity());
    radii.clear();
    radii.reserve(cells.capacity());
    for i in 0..cells.capacity() {
        permille.push(if cells.occupied(i) {
            core_permille(cells, i, rates)
        } else {
            CORE_PERMILLE
        });
        radii.push(if cells.occupied(i) {
            crate::fixed::q10_to_pos(crate::biology::radius(cells, i))
        } else {
            0
        });
    }

    // What the constraints ended up moving each cell, so that velocity can be reconciled with it
    // once the passes are done, and whether a cell is touching anything at all — which is not the
    // same question as whether it was *moved*, because the interior of a pack is where the pushes
    // cancel. Friction has to key off contact, not motion, or it would apply everywhere except
    // the one place that needs it.
    //
    // Reused across ticks rather than allocated per call. See [`SeparationScratch`].
    scratch.begin(cells.capacity());
    let SeparationScratch {
        push_x,
        push_y,
        touching,
        deltas,
    } = scratch;

    for pass in 0..SEPARATION_PASSES {
        // Jacobi rather than Gauss-Seidel: every cell works out its own correction from where
        // its neighbours are at the start of the pass, and all the corrections are applied
        // together at the end of it. The old loop applied each shove the moment it computed it,
        // so a pair's answer depended on how many of its neighbours had already been visited —
        // correct, but only because the visiting happened in slot order on one thread.
        //
        // # Why this is the version that can use the machine
        //
        // Separation was the whole tick and it ran on one core while the rest sat idle. It could
        // not be parallelised as written, because a shove writes to both cells of a pair and two
        // threads holding overlapping pairs would race. Here each cell reads its neighbours and
        // writes only itself, so there is nothing to race on: the pass is a pure map over slots.
        //
        // # Why it stays bit-identical on any number of threads
        //
        // Because the simulation has no floats (CLAUDE.md rule 2). A cell sums its own
        // contributions in the fixed order of its own pair list, and `collect` puts the results
        // back in slot order, so the arithmetic a cell does is the same arithmetic in the same
        // sequence whatever the scheduler did. Rule 6 asks that outcomes not depend on rayon
        // scheduling or thread count; this satisfies it by construction rather than by the
        // solver happening to be single-threaded.
        //
        // The cost is convergence. Gauss-Seidel propagates a correction within the pass it was
        // made in and Jacobi does not, so the same number of passes relaxes a crowd slightly
        // less. That is a good trade here specifically because this solver is deliberately soft
        // already — `CONTACT_STRENGTH` in sixteenths over three passes, a gentle response chosen
        // so a pack rests at contact rather than being driven to zero overlap.
        // Walked in the grid's own order rather than by slot, which costs nothing and buys
        // locality: neighbouring cells share neighbours, so consecutive tasks re-read arena rows
        // that are still warm. Slot order scatters that, because a slot is wherever the free
        // list put it. Only legal because the pass is Jacobi — see `NeighbourIndex::occupants`.
        let first = pass == 0;
        let arena: &CellArena = cells;
        let radii_ref: &[i32] = radii;
        let perm_ref: &[i32] = permille;
        // Into the buffer rather than into a fresh `Vec`: `collect_into_vec` keeps the
        // allocation and writes in the same order `collect` would, so the arithmetic and its
        // sequence are untouched.
        index
            .occupants()
            .par_iter()
            .map(|&i| {
                (
                    i,
                    correction_for(
                        arena, index, radii_ref, perm_ref, blocked, i as usize, first, relax,
                    ),
                )
            })
            .collect_into_vec(deltas);

        let max_x = (width as i64 * POS_ONE as i64) - 1;
        let max_y = (height as i64 * POS_ONE as i64) - 1;
        for &(i, ref c) in deltas.iter() {
            let i = i as usize;
            if !cells.occupied(i) {
                continue;
            }
            if c.dx != 0 || c.dy != 0 {
                // What the constraint *did*, which at the slide edge is not what it asked for.
                //
                // The velocity reconciliation below exists to take out the part of a cell's
                // motion that the constraints had to undo, and it was being handed the intended
                // correction rather than the achieved one. Against a wall those differ
                // completely: the clamp eats the whole push, the cell does not move, and the
                // cell's velocity is then adjusted for a movement that never happened — every
                // tick, for as long as the crowd leans on the boundary.
                //
                // Measured on the packing bench with nothing else running: a pack floating free
                // is perfectly still, and the same pack squeezed onto a slide small enough to
                // reach the walls churned 2.12% of its pixels every tick.
                let (was_x, was_y) = (cells.x[i], cells.y[i]);
                cells.x[i] = ((was_x as i64) + c.dx as i64).clamp(0, max_x) as i32;
                cells.y[i] = ((was_y as i64) + c.dy as i64).clamp(0, max_y) as i32;
                if let Some(v) = push_x.get_mut(i) {
                    *v = v.saturating_add(cells.x[i].saturating_sub(was_x));
                }
                if let Some(v) = push_y.get_mut(i) {
                    *v = v.saturating_add(cells.y[i].saturating_sub(was_y));
                }
            }
            if c.touching {
                if let Some(t) = touching.get_mut(i) {
                    *t = true;
                }
            }
            if first {
                if let Some(v) = crowding.get_mut(i) {
                    *v = c.crowding;
                }
                if let Some(v) = pressure.get_mut(i) {
                    *v = c.pressure;
                }
                // Each pair is now seen from both of its sides, so the population's contact count
                // is twice the number of contacts. Halved rather than counted from one side,
                // because "from one side" is exactly the asymmetry this loop exists to remove.
                separated = separated.saturating_add(c.contacts);
            }
        }
    }
    separated /= 2;
    // Velocity, reconciled with what the constraints actually did.
    //
    // Without this the solver cannot win, and the packing bench showed it plainly: a settled
    // crowd had a mean speed of a fifteenth of a square per tick and was going nowhere. A cell
    // moves under whatever is pushing it — a current, a cilium — and the constraint then puts it
    // back, but the *velocity* that drove it there is untouched, so next tick it drives in again
    // and is put back again. Position-based dynamics has to close that loop or the two fight for
    // as long as the load lasts, which reads as every boundary in the picture jittering.
    //
    // Removed rather than reversed. Full PBD sets `v = (x - x_before) / dt`, which here would
    // hand the cell the whole correction as outgoing speed — that is restitution, and cells in
    // water do not bounce. So only the component of velocity *opposing* the correction is taken
    // out: a cell being pushed out of its neighbour loses exactly the part of its motion that was
    // driving it in, and keeps the part along the wall it is sliding on. Strictly removes energy,
    // so it cannot be the source of a new instability.
    for i in 0..cells.capacity() {
        if !cells.occupied(i) {
            continue;
        }
        if !touching.get(i).copied().unwrap_or(false) {
            continue;
        }
        let (cx, cy) = (
            push_x.get(i).copied().unwrap_or(0) as i64,
            push_y.get(i).copied().unwrap_or(0) as i64,
        );
        let mag_sq = cx * cx + cy * cy;
        if mag_sq > 0 {
            let dot = (cells.vx[i] as i64) * cx + (cells.vy[i] as i64) * cy;
            // Negative means the cell was driving into what pushed it; that part goes.
            // Positive means it was already going that way, and there is nothing to remove.
            if dot < 0 {
                cells.vx[i] = cells.vx[i].saturating_sub((dot * cx / mag_sq) as i32);
                cells.vy[i] = cells.vy[i].saturating_sub((dot * cy / mag_sq) as i32);
            }
        }
        // What is left is the cell sliding along its neighbours, which membranes resist — and how
        // *much* they resist is the thing firmness buys.
        //
        // A bag of fluid pressed into another bag of fluid has a wide, flattened, sticky contact
        // and drags badly; a hard round body touching another has very little contact and slips
        // past. So a limp cell keeps a quarter of its sliding speed, as everything did before
        // this, and a marble keeps almost all of it.
        //
        // This is what makes firmness a *choice* rather than a look. Being soft costs nothing and
        // is fine if you are an autotroph — sitting in a mat of your own kind in the light is the
        // whole plan. It is ruinous if you are trying to hunt, because the thing you are hunting
        // is inside a crowd and getting into a crowd, through it and out again is exactly the
        // manoeuvre a soft cell cannot do. The wall and the turgor are paid for in matter and
        // upkeep; what they buy is being able to move where everything else is stuck.
        //
        // Scaled by the cell's own firmness and not the pair's: a marble slides past a blob just
        // as well as past another marble, because the thing that drags is its own deformable
        // surface. Two blobs stick to each other twice as much as one blob and a marble do, which
        // falls out of both of them being scaled.
        let firm = firmness(cells, i, rates);
        let slip = CONTACT_FRICTION
            + (((Q10_ONE - CONTACT_FRICTION) as i64 * firm.clamp(0, Q10_ONE) as i64)
                / Q10_ONE as i64) as i32;
        cells.vx[i] = crate::fixed::q10_scale(cells.vx[i], slip);
        cells.vy[i] = crate::fixed::q10_scale(cells.vy[i], slip);
        // And below a threshold it is simply held: see `REST_SPEED`. A firm cell is harder to pin
        // this way too — static friction is the same contact area doing the same job — so the
        // threshold falls with the same factor, and a marble in a crowd is never quite held still.
        let pin = (REST_SPEED as i64 * (Q10_ONE - firm.clamp(0, Q10_ONE)) as i64
            / Q10_ONE as i64) as i32;
        if cells.vx[i]
            .saturating_abs()
            .saturating_add(cells.vy[i].saturating_abs())
            < pin
        {
            cells.vx[i] = 0;
            cells.vy[i] = 0;
        }
    }

    separated
}

#[cfg(test)]
mod tests {

    #[test]
    fn rigidity_is_off_by_default_and_never_softens_a_cell() {
        // The property that lets this ship with every earlier measurement intact: at the default
        // rate `core_permille` is the constant those measurements were taken against, to the
        // digit, for every cell whatever it is carrying.
        use crate::cell::{CellId, CellSeed};
        use crate::fixed::{pos, q10};
        use crate::genome::GenomePool;

        let pool = GenomePool::new();
        let mut cells = CellArena::new();
        let genome = pool.intern(vec![0x2E; 4]).expect("genome");
        let id = cells.spawn(CellSeed {
            x: pos(4),
            y: pos(4),
            mass: q10(30),
            energy: q10(1_000),
            membrane: 24,
            key: 11,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        let i = cells.index(id).expect("alive");

        let off = crate::metabolism::MetabolicRates::default();
        assert_eq!(off.rigidity_gain, 0, "rigidity is not off by default");
        for load in [0, 1, 64] {
            for c in 0..crate::chem::CHEM_COUNT {
                cells.interior_mut(i)[c] = q10(load);
            }
            assert_eq!(core_permille(&cells, i, &off), CORE_PERMILLE);
        }

        // Switched on, a cell is stiffer than the default and never softer, at any load and any
        // membrane, and it never passes the rigid ceiling.
        let on = crate::metabolism::MetabolicRates {
            rigidity_gain: crate::fixed::Q10_ONE * 16,
            ..crate::metabolism::MetabolicRates::default()
        };
        for load in [0, 1, 4, 16, 64, 4096] {
            for c in 0..crate::chem::CHEM_COUNT {
                cells.interior_mut(i)[c] = q10(load);
            }
            for membrane in [0u8, 24, 255] {
                cells.slots_mut(i)[crate::organelle::MEMBRANE_SLOT].param = membrane;
                let p = core_permille(&cells, i, &on);
                assert!(
                    (CORE_PERMILLE..=CORE_PERMILLE_RIGID).contains(&p),
                    "core {p} out of range at load {load}, membrane {membrane}"
                );
            }
        }
    }

    /// `resolve_collisions` with the two arguments every test here leaves at their defaults.
    ///
    /// Added when separation grew a per-cell core: the tests are about geometry, not about
    /// turgor, and `MetabolicRates::default` has `rigidity_gain` at zero, which makes
    /// `core_permille` exactly `CORE_PERMILLE` for every cell — the constant these tests were
    /// written against.
    #[allow(clippy::too_many_arguments)]
    fn separate(
        cells: &mut CellArena,
        index: &NeighbourIndex,
        radii: &mut Vec<i32>,
        crowding: &mut Vec<i32>,
        pressure: &mut Vec<i32>,
        blocked: &[bool],
        scratch: &mut SeparationScratch,
    ) -> u32 {
        resolve_collisions(
            cells,
            index,
            radii,
            &mut Vec::new(),
            crowding,
            pressure,
            blocked,
            &crate::metabolism::MetabolicRates::default(),
            crate::biology::BiologyConfig::default().separation_relax,
            scratch,
        )
    }

    use super::*;
    use crate::cell::{CellId, CellSeed};
    use crate::fixed::{pos, q10};
    use crate::genome::GenomePool;

    fn arena_of_mass(positions: &[(i32, i32)], mass: i32) -> (CellArena, GenomePool) {
        let pool = GenomePool::new();
        let mut cells = CellArena::new();
        for (x, y) in positions {
            cells.spawn(CellSeed {
                x: *x,
                y: *y,
                mass: q10(mass),
                energy: q10(100),
                membrane: 16,
                key: 0,
                badge: 0,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome: pool.intern(vec![0x2E]).unwrap(),
            });
        }
        (cells, pool)
    }

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
                badge: 0,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome: pool.intern(vec![0x2E]).unwrap(),
            });
        }
        (cells, pool)
    }

    /// The nine-bucket walk `around` replaced, kept as the thing to measure it against.
    /// The naive bucket-at-a-time walk of the `(2k+1)²` block, which the row runs must match.
    fn block_buckets(index: &NeighbourIndex, sx: i32, sy: i32, k: i32) -> Vec<usize> {
        (-k..=k)
            .flat_map(|dy| (-k..=k).map(move |dx| (dx, dy)))
            .flat_map(|(dx, dy)| index.in_square(sx + dx, sy + dy))
            .map(|s| *s as usize)
            .collect()
    }

    fn contact(dx: i32, dy: i32, overlap: i32) -> Contact {
        Contact {
            dx,
            dy,
            radius: 100,
            mass: 0,
            overlap,
            rigidity: 24,
            // These tests are about which neighbours are found and where their seams fall, which
            // is a question about geometry. Being joined changes only how a cell is drawn, and so
            // does being firm.
            joined: false,
            firmness: 0,
        }
    }

    #[test]
    fn every_neighbour_within_reach_keeps_its_own_seam() {
        // No merging, no cleverness: two cells share a wall only if both cut against the
        // other, and anything that drops a contact from one side alone breaks that.
        let mut set = ContactSet::default();
        set.offer(contact(100, 0, 40));
        set.offer(contact(105, 8, 10));
        assert_eq!(set.as_slice().len(), 2, "a contact was merged away");
    }

    #[test]
    fn the_six_directions_of_a_hexagonal_packing_all_survive() {
        let mut set = ContactSet::default();
        let r = 1000.0f64;
        for k in 0..6 {
            let a = std::f64::consts::TAU * k as f64 / 6.0;
            set.offer(contact((r * a.cos()) as i32, (r * a.sin()) as i32, 50 + k));
        }
        assert_eq!(
            set.as_slice().len(),
            6,
            "a hexagonal neighbourhood lost a side"
        );
    }

    #[test]
    fn past_the_cap_the_deepest_are_kept() {
        let mut set = ContactSet::default();
        for k in 0..CONTACTS_PER_CELL {
            set.offer(contact(1000, k as i32 * 7, 50));
        }
        set.offer(contact(1000, 99, 5));
        assert_eq!(set.as_slice().len(), CONTACTS_PER_CELL);
        assert!(
            set.as_slice().iter().all(|c| c.overlap == 50),
            "a shallower newcomer displaced a deeper seam"
        );
        set.offer(contact(1000, 99, 500));
        assert!(
            set.as_slice().iter().any(|c| c.overlap == 500),
            "a deeper newcomer was refused"
        );
    }

    #[test]
    fn row_runs_are_the_same_walk_as_bucket_at_a_time() {
        // The walk gathers each row of the neighbourhood as one contiguous run rather than a
        // bucket at a time. That is only sound if it is the same *sequence*, not merely
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

        // Over every radius the walk can be asked for, not just the one it used to hard-code.
        for k in 1..=4 {
            for sy in -2..=9 {
                for sx in -2..=9 {
                    let walked: Vec<usize> = index.within(sx, sy, k).collect();
                    assert_eq!(
                        walked,
                        block_buckets(&index, sx, sy, k),
                        "at ({sx}, {sy}) k={k}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_search_grows_with_the_cells_on_the_slide() {
        // The bug this replaced, as a test. A three-by-three walk is right only while a cell is
        // smaller than a substrate square, and these are not: a cell of mass 44 has a radius of
        // a whole square, so two of them rest about 1.65 squares apart and half of any cell's
        // real neighbours fall outside a fixed three-by-three. They were then never separated
        // and never given a shared wall, so a packed sheet drew as discs lying over each other
        // with three or four seams each where a tiled monolayer needs six.
        let (small, _p) = arena_of_mass(&[(pos(4), pos(4))], 1);
        let (big, _p2) = arena_of_mass(&[(pos(4), pos(4))], 4000);
        let mut a = NeighbourIndex::default();
        let mut b = NeighbourIndex::default();
        a.rebuild(&small, 32, 32);
        b.rebuild(&big, 32, 32);
        assert!(
            b.search > a.search,
            "a slide of large cells must be searched further than one of small cells: {} vs {}",
            b.search,
            a.search
        );
        // And it must cover the distance two of the largest can interact over, which is the
        // sum of their radii.
        let r = crate::fixed::q10_to_pos(crate::biology::radius(&big, 0));
        assert!(
            b.search * POS_ONE >= 2 * r,
            "search of {} squares does not reach two radii ({r} POS)",
            b.search
        );
    }

    #[test]
    fn a_one_square_slide_still_walks_itself() {
        // The degenerate clamp: every neighbour is the same square, and the run must not
        // reach past it into whatever the prefix sum has next.
        let (cells, _p) = arena(&[(pos(0), pos(0)), (pos(0), pos(0))]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 1, 1);
        assert_eq!(index.within(0, 0, 1).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(block_buckets(&index, 0, 0, 1), vec![0, 1]);
        // A square off the end still has the last real square as its neighbour, and always
        // did — the run is clamped, not truncated to nothing.
        for sx in -1..=2 {
            assert_eq!(
                index.within(sx, 0, 1).collect::<Vec<_>>(),
                block_buckets(&index, sx, 0, 1),
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
    fn a_neighbourhood_of_radius_one_is_the_nine_squares_around_a_cell() {
        let (cells, _p) = arena(&[
            (pos(5), pos(5)),
            (pos(6), pos(5)),
            (pos(4), pos(4)),
            (pos(8), pos(5)),
        ]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        let near: Vec<usize> = index.within(5, 5, 1).collect();
        assert!(near.contains(&0) && near.contains(&1) && near.contains(&2));
        assert!(!near.contains(&3), "three squares away is not adjacent");
    }

    #[test]
    fn overlapping_cells_are_pushed_apart_and_separated_ones_are_left_alone() {
        let (mut cells, _p) = arena(&[(pos(5), pos(5)), (pos(5) + 4, pos(5))]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        let before = separation(&cells, 0, 1);
        let n = separate(
            &mut cells,
            &mut index,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new(),
            &[],
            &mut SeparationScratch::default(),
        );
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
            separate(
                &mut far,
                &mut index2,
                &mut Vec::new(),
                &mut Vec::new(),
                &mut Vec::new(),
                &[],
            &mut SeparationScratch::default(),
        ),
            0
        );
        let after: Vec<(i32, i32)> = far.iter().map(|i| (far.x[i], far.y[i])).collect();
        assert_eq!(positions, after, "distant cells were moved");
    }

    /// Squeeze a pair together by `load` per cell per tick for long enough to settle, and report
    /// what separation they hold. The load stands in for whatever presses a real crowd together
    /// — a current, gravity, the weight of the cells further out.
    fn rest_under_load(load: i32) -> i32 {
        let (mut cells, _p) = arena(&[(pos(6), pos(6)), (pos(8), pos(6))]);
        let mut index = NeighbourIndex::default();
        for _ in 0..400 {
            cells.x[0] = cells.x[0].saturating_add(load);
            cells.x[1] = cells.x[1].saturating_sub(load);
            index.rebuild(&cells, 16, 16);
            separate(
                &mut cells,
                &mut index,
                &mut Vec::new(),
                &mut Vec::new(),
                &mut Vec::new(),
                &[],
            &mut SeparationScratch::default(),
        );
        }
        separation(&cells, 0, 1)
    }

    #[test]
    fn a_pressed_pair_settles_at_its_core_whatever_is_pressing_it() {
        // A cell is a bag of water, so how deeply it is pressed into a neighbour is *not* a
        // reading of how hard it is being pressed — past a very short soft band it simply stops,
        // and the picture is set by geometry rather than by pressure.
        //
        // This is the opposite of what an earlier version of this test asserted, and the change
        // is the point. When the core sat at 0.55 of touching there was a wide band to be graded
        // across, but a cell resting there has lost 69% of its area, which is not a cell. At an
        // area-preserving core the band is a twentieth of the touching distance, and everything
        // from the lightest load up pins to the same place.
        let want = 2 * crate::fixed::q10_to_pos(crate::biology::radius(&arena(&[(0, 0)]).0, 0));
        let core = ((want as i64 * CORE_PERMILLE as i64) / 1000) as i32;
        for load in [4, 16, 48] {
            let d = rest_under_load(load);
            assert!(
                (d - core).abs() * 20 <= want,
                "load {load} settled at {d}, not near the core at {core}"
            );
        }
        // And an unloaded pair is left very nearly tangent, so two cells that merely meet in
        // open water still read as two cells rather than as one flattened blob.
        assert!(
            rest_under_load(0) * 20 > want * 19,
            "an unpressed pair should barely compress"
        );
    }

    #[test]
    fn compression_stops_at_the_core() {
        // The bound, and the reason this is safe to make soft. Lean on a pair as hard as the
        // solver can be leaned on and it still does not collapse: past the core the response is
        // sixteen times stiffer, so the depth stops being a function of the load.
        //
        // `MIN_FACE` in the renderer cuts at the same fraction, and the two have to agree — a
        // cell drawn with a core it does not physically have is a cell drawn overlapping.
        let want = 2 * crate::fixed::q10_to_pos(crate::biology::radius(&arena(&[(0, 0)]).0, 0));
        let core = ((want as i64 * CORE_PERMILLE as i64) / 1000) as i32;
        for load in [48, 96] {
            let d = rest_under_load(load);
            // Still in contact, first. Without this the assertion below passes for the worst
            // possible reason: a load the solver cannot hold drives one cell clean through the
            // other, after which they are far apart and trivially "not compressed". Caught
            // exactly that way, at a load of 160.
            assert!(
                d < want,
                "load {load} pushed the pair apart entirely, to {d} — it did not compress at all"
            );
            // Not a hard floor — `MAX_SHOVE` caps how hard the core may shove back — so this
            // asserts the bound that is actually claimed: near the core, and nowhere near the
            // unbounded collapse a fixed-rate push gives under a steady squeeze.
            assert!(
                d * 4 > core * 3,
                "load {load} drove the pair to {d}, well past a core of {core}"
            );
        }
    }

    #[test]
    fn a_load_heavier_than_the_core_can_answer_pushes_through_it() {
        // The limit of the mechanism, asserted rather than left to be discovered. `MAX_SHOVE`
        // caps how hard a contact may shove per pass, so there is a load above which the core
        // simply loses. Recorded here because the number matters: if a current, a spike or a
        // junction ever pulls harder than this, cells will pass through one another and the
        // renderer will draw the result faithfully and look broken.
        let want = 2 * crate::fixed::q10_to_pos(crate::biology::radius(&arena(&[(0, 0)]).0, 0));
        let r = want / 2;
        // Three passes of at most `r / MAX_SHOVE` each, from both sides.
        let ceiling = 2 * SEPARATION_PASSES as i32 * (r / MAX_SHOVE);
        assert!(
            rest_under_load(ceiling / 2) < want,
            "a load inside the ceiling should still be held"
        );
        assert!(
            rest_under_load(ceiling * 2) >= want,
            "a load past the ceiling should be expected to break contact"
        );
    }

    #[test]
    fn a_cell_surrounded_on_all_sides_still_resolves_every_contact() {
        // The starvation this replaced. A per-tick displacement pool was spent in slot order, so
        // a cell with eight neighbours resolved the first two or three contacts and silently
        // skipped the rest — the surface of a pack behaved and the interior collapsed into it.
        //
        // Six neighbours on a ring, all overlapping the middle cell. Every one of them must end
        // up no deeper than the core; a pool would leave the later slots untouched.
        //
        // Six rather than eight because six is the most equal discs that fit around one in a
        // plane. Eight was asking for a configuration that does not exist — ring neighbours at
        // the core distance from the centre are closer than the core to *each other* — so the
        // solver was being failed for not achieving the impossible.
        let r = crate::fixed::q10_to_pos(crate::biology::radius(&arena(&[(0, 0)]).0, 0));
        let mut ring = vec![(pos(8), pos(8))];
        // Sixths of a turn, at half the distance they want. `(±1, ±2)` and `(±2, 0)` is a
        // hexagon to within a few percent, and there is no trigonometry in `mm-core`.
        for (dx, dy) in [(2, 0), (1, 2), (-1, 2), (-2, 0), (-1, -2), (1, -2)] {
            ring.push((pos(8) + dx * r / 2, pos(8) + dy * r / 2));
        }
        let (mut cells, _p) = arena(&ring);
        let mut index = NeighbourIndex::default();
        for _ in 0..400 {
            index.rebuild(&cells, 16, 16);
            separate(
                &mut cells,
                &mut index,
                &mut Vec::new(),
                &mut Vec::new(),
                &mut Vec::new(),
                &[],
            &mut SeparationScratch::default(),
        );
        }
        let want = 2 * r;
        let core = ((want as i64 * CORE_PERMILLE as i64) / 1000) as i32;
        for j in 1..=6 {
            let d = separation(&cells, 0, j);
            assert!(
                d * 4 > core * 3,
                "neighbour {j} was left at {d}, inside a core of {core} — starved of budget"
            );
        }
    }

    #[test]
    fn crowding_is_charged_for_overlap_and_only_for_overlap() {
        // Charged on the whole compression rather than on core penetration, which is safe only
        // because [`CORE_PERMILLE`] now bounds compression at a twentieth of the touching
        // distance. Measured against the core instead, every cell on a packed slide would read
        // exactly zero and crowding pressure would quietly cease to exist.
        let mut crowding = Vec::new();

        // Clear of each other: free.
        let r = crate::fixed::q10_to_pos(crate::biology::radius(&arena(&[(0, 0)]).0, 0));
        let (mut apart, _p) = arena(&[(pos(6), pos(6)), (pos(6) + 3 * r, pos(6))]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&apart, 16, 16);
        separate(
            &mut apart,
            &mut index,
            &mut Vec::new(),
            &mut crowding,
            &mut Vec::new(),
            &[],
            &mut SeparationScratch::default(),
        );
        assert_eq!(
            crowding.iter().filter(|c| **c > 0).count(),
            0,
            "a cell was billed for a neighbour it is not touching"
        );

        // Driven well inside the core: charged, to both sides.
        let (mut crushed, _p2) = arena(&[(pos(6), pos(6)), (pos(6) + r / 4, pos(6))]);
        let mut index2 = NeighbourIndex::default();
        index2.rebuild(&crushed, 16, 16);
        separate(
            &mut crushed,
            &mut index2,
            &mut Vec::new(),
            &mut crowding,
            &mut Vec::new(),
            &[],
            &mut SeparationScratch::default(),
        );
        assert!(
            crowding[0] > 0 && crowding[1] > 0,
            "a crushed pair was not billed: {crowding:?}"
        );
    }

    #[test]
    fn pressure_reads_how_stuck_a_cell_is_not_how_many_neighbours_it_has() {
        // The distinction the division gate rests on. A pair resting against each other is a
        // contact and not a predicament: there is still somewhere for them to go, so neither is
        // under pressure. A pair driven onto their cores has nothing left to give.
        let r = crate::biology::radius(&arena(&[(pos(6), pos(6))]).0, 0);
        let r = crate::fixed::q10_to_pos(r);

        // Just touching — a contact, but nothing is pushing back.
        let (mut resting, _) = arena(&[(pos(6), pos(6)), (pos(6) + 2 * r - 8, pos(6))]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&resting, 16, 16);
        let mut light = Vec::new();
        separate(
            &mut resting,
            &mut index,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut light,
            &[],
            &mut SeparationScratch::default(),
        );

        // Driven onto the core.
        let (mut wedged, _) = arena(&[(pos(6), pos(6)), (pos(6) + r / 2, pos(6))]);
        let mut index2 = NeighbourIndex::default();
        index2.rebuild(&wedged, 16, 16);
        let mut heavy = Vec::new();
        separate(
            &mut wedged,
            &mut index2,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut heavy,
            &[],
            &mut SeparationScratch::default(),
        );

        assert!(
            heavy[0] > light[0],
            "being wedged read no worse than resting: {heavy:?} against {light:?}"
        );
        assert!(
            heavy[0] <= crate::fixed::Q10_ONE,
            "one bottomed-out neighbour scored more than one neighbour's worth: {heavy:?}"
        );
        // Symmetric, like the crowding it rides along with — both sides of a contact are stuck.
        assert_eq!(heavy[0], heavy[1]);
    }

    #[test]
    fn exactly_coincident_cells_do_not_get_stuck() {
        // Two cells at the same point have no line to push along. Without a tie-break they
        // would sit on top of each other forever, which is how a crowd becomes a singularity.
        let (mut cells, _p) = arena(&[(pos(8), pos(8)), (pos(8), pos(8))]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        for _ in 0..20 {
            separate(
                &mut cells,
                &mut index,
                &mut Vec::new(),
                &mut Vec::new(),
                &mut Vec::new(),
                &[],
            &mut SeparationScratch::default(),
        );
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
            separate(
                &mut cells,
                &mut index,
                &mut Vec::new(),
                &mut Vec::new(),
                &mut Vec::new(),
                &[],
            &mut SeparationScratch::default(),
        );
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
                separate(
                    &mut cells,
                    &mut index,
                    &mut Vec::new(),
                    &mut Vec::new(),
                    &mut Vec::new(),
                    &[],
            &mut SeparationScratch::default(),
        );
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

    /// A grid of the given size with one square blocked.
    fn wall_at(w: i32, h: i32, bx: i32, by: i32) -> Vec<bool> {
        let mut blocked = vec![false; (w * h) as usize];
        blocked[(by * w + bx) as usize] = true;
        blocked
    }

    #[test]
    fn a_cell_overlapping_a_barrier_is_pushed_out_of_it() {
        // Centre just left of the blocked square at (6, 5), overlapping its left face.
        let (mut cells, _p) = arena(&[(pos(6) - 8, pos(5) + POS_ONE / 2)]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        let before = cells.x[0];
        separate(
            &mut cells,
            &mut index,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new(),
            &wall_at(16, 16, 6, 5),
            &mut SeparationScratch::default(),
        );
        assert!(
            cells.x[0] < before,
            "a cell overlapping a wall was not pushed away from it: {before} -> {}",
            cells.x[0]
        );
        assert_eq!(cells.y[0], pos(5) + POS_ONE / 2, "pushed along the face");
    }

    #[test]
    fn a_cell_standing_inside_a_barrier_leaves_by_the_nearest_face() {
        // The degenerate case the closest-point test cannot answer, and it is reachable: the
        // drawing tool can put a barrier on top of a standing cell. Placed nearest the top
        // face, so that is the way out.
        let (mut cells, _p) = arena(&[(pos(6) + POS_ONE / 2, pos(5) + POS_ONE / 8)]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        separate(
            &mut cells,
            &mut index,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new(),
            &wall_at(16, 16, 6, 5),
            &mut SeparationScratch::default(),
        );
        assert!(
            cells.y[0] < pos(5) + POS_ONE / 8,
            "a cell inside a wall did not leave by its nearest face"
        );
        assert_eq!(cells.x[0], pos(6) + POS_ONE / 2, "and not sideways");
    }

    #[test]
    fn a_slide_with_no_barriers_is_untouched_by_the_barrier_pass() {
        // The empty-slice path has to be exactly inert, or every scenario without barriers
        // silently changes trajectory. Same arrangement, solved with and without a mask of all
        // false, compared bit for bit.
        let arrangement = [(pos(5), pos(5)), (pos(5) + 4, pos(5)), (pos(9), pos(9))];
        let (mut empty, _p) = arena(&arrangement);
        let (mut clear, _q) = arena(&arrangement);
        let mut i1 = NeighbourIndex::default();
        let mut i2 = NeighbourIndex::default();
        i1.rebuild(&empty, 16, 16);
        i2.rebuild(&clear, 16, 16);
        let a = separate(
            &mut empty,
            &mut i1,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new(),
            &[],
            &mut SeparationScratch::default(),
        );
        let b = separate(
            &mut clear,
            &mut i2,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new(),
            &vec![false; 16 * 16],
            &mut SeparationScratch::default(),
        );
        assert_eq!(a, b);
        for i in 0..arrangement.len() {
            assert_eq!((empty.x[i], empty.y[i]), (clear.x[i], clear.y[i]));
            assert_eq!((empty.vx[i], empty.vy[i]), (clear.vx[i], clear.vy[i]));
        }
    }

    #[test]
    fn a_barrier_contact_is_not_charged_as_crowding_or_pressure() {
        // SPEC §17.1: an edge is meant to be a *better* address than the middle of a room,
        // because a cell against a wall has fewer neighbours pressing on it. Counting the wall
        // as a neighbour would invert exactly that, and it would do it silently — `crowding`
        // is membrane damage and `pressure` refuses divisions.
        let (mut cells, _p) = arena(&[(pos(6) - 8, pos(5) + POS_ONE / 2)]);
        let mut index = NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        let (mut crowding, mut pressure) = (Vec::new(), Vec::new());
        separate(
            &mut cells,
            &mut index,
            &mut Vec::new(),
            &mut crowding,
            &mut pressure,
            &wall_at(16, 16, 6, 5),
            &mut SeparationScratch::default(),
        );
        assert_eq!(crowding[0], 0, "a wall wounded a cell resting against it");
        assert_eq!(pressure[0], 0, "a wall stopped a cell dividing");
    }
}
