//! The grid substrate (SPEC §7.4).
//!
//! Each square holds sixteen chemical quantities, a light value, a velocity vector and a
//! `blocked` flag for user-drawn barriers.
//!
//! # Layout
//!
//! Fields are stored one chemical at a time — `chem[c]` is a whole `width × height` plane —
//! rather than sixteen values interleaved per square. The fluid solver sweeps one chemical
//! across the whole grid at a time, so this is the layout that keeps its working set
//! contiguous: a 512×512 plane of `i32` is 1MB and stays in cache for the duration of its
//! sweep, where the interleaved layout would stream 16MB and touch every chemical's data to
//! do one chemical's work.
//!
//! # Barriers
//!
//! A barrier is a property of a *square*, but the solver needs it as a property of an
//! *edge*: a flux crosses between two squares, and it is legal only if both are open. So
//! [`Substrate`] keeps two precomputed edge masks, rebuilt whenever the barrier layout
//! changes. Barriers change rarely — a user draws them (M6) — and the masks are read sixteen
//! times per fluid step, once per chemical, so precomputing pays for itself many times over.

use crate::chem::CHEM_COUNT;
use crate::state_hash::{StateHash, StateHasher};

/// Most of one chemical a single square may hold.
///
/// Slightly under `i32::MAX`, and the slack is load-bearing. A diffusion update is a convex
/// combination of a square and its four neighbours, so it cannot exceed the largest of them —
/// but each of the four fluxes is truncated toward zero, and each truncation can leave the
/// result a unit higher than the exact combination. Sixteen units of headroom is far more
/// than the four that are reachable, and it buys the solver's inner loop the right to skip a
/// bounds check per edge, which is what lets it vectorise.
pub const MAX_QUANTITY: i32 = i32::MAX - 16;

/// Largest grid dimension. `u16` indices keep the per-square footprint down, and a grid
/// wider than this would exceed what a single machine can step at a useful rate anyway.
pub const MAX_DIM: u32 = 4096;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubstrateError {
    /// Zero or larger than [`MAX_DIM`].
    BadDimensions { width: u32, height: u32 },
}

impl std::fmt::Display for SubstrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubstrateError::BadDimensions { width, height } => write!(
                f,
                "substrate {width}x{height} is outside 1..={MAX_DIM} in each dimension"
            ),
        }
    }
}

impl std::error::Error for SubstrateError {}

/// The world's fluid, chemistry and barriers.
#[derive(Clone, Debug)]
pub struct Substrate {
    width: u32,
    height: u32,
    /// `chem[c][y * width + x]`, `Q10`. Never negative.
    chem: Vec<Vec<i32>>,
    /// Incident light, `Q10`. Prescribed by the light regime, not conserved.
    light: Vec<i32>,
    /// Velocity, `Q10` in squares per fluid step. The CFL limit bounds these to ±1 square.
    vx: Vec<i32>,
    vy: Vec<i32>,
    blocked: Vec<bool>,
    /// `open_x[y * width + x]` — true when the edge between `(x, y)` and `(x+1, y)` may
    /// carry flux. The last column is always false; there is no square to its right.
    open_x: Vec<bool>,
    /// `open_y[y * width + x]` — the edge between `(x, y)` and `(x, y+1)`.
    open_y: Vec<bool>,
    /// Whether any square is blocked. The solver reads the edge masks only when this is
    /// true; on a slide with no barriers they are a quarter of a megabyte of guaranteed
    /// `true` that it is faster not to look at.
    has_barriers: bool,
    /// Whether any chemical is present at all, per chemical.
    ///
    /// Exact rather than approximate, and cheap to keep so: nothing in the fluid creates
    /// matter, so a plane that is zero everywhere stays zero until something explicitly adds
    /// to it — and every such route goes through `add_chem`/`set_chem`/`restore`. A plane
    /// marked absent is skipped entirely, which is most of the table in most scenarios.
    ///
    /// The flag is allowed to be conservatively `true` for a plane that has since drained to
    /// zero; that costs a wasted sweep, never a wrong answer.
    present: [bool; CHEM_COUNT],
    /// Whether any square has a non-zero velocity. A still slide skips advection.
    has_flow: bool,
    /// Velocity at each x-edge and y-edge, `Q10`, clamped to the CFL limit.
    ///
    /// Derived from `vx`/`vy`, cached because the advection sweep reads them once per
    /// chemical — sixteen times per step — while the velocity field itself changes rarely: a
    /// prescribed current is time-invariant and cilia impulses are sparse. `i16` rather than
    /// `i32` because the CFL clamp puts them well inside its range, and halving the width
    /// halves what the sweep has to stream.
    edge_vx: Vec<i16>,
    edge_vy: Vec<i16>,
    /// Set when `vx`/`vy` change; cleared by `sync_edge_velocity`.
    edge_velocity_stale: bool,
}

/// Two substrates are equal when they hold the same things, not when their caches agree.
///
/// `present`, `has_flow`, `has_barriers`, the edge masks and the edge velocities are all
/// derived, and two of them are deliberately *conservative*: `present` may stay true for a
/// plane that has since drained to zero, because clearing it would mean scanning a megabyte to
/// learn something that only costs a wasted sweep to get wrong. A derived `PartialEq` would
/// therefore report two identical worlds as different — which is exactly what it did, and it
/// took a snapshot round-trip failing on "some field is missing from the format" to notice,
/// when nothing was missing at all.
impl PartialEq for Substrate {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.chem == other.chem
            && self.light == other.light
            && self.vx == other.vx
            && self.vy == other.vy
            && self.blocked == other.blocked
    }
}

impl Eq for Substrate {}

impl Substrate {
    /// An empty substrate: no chemicals, no light, no flow, no barriers.
    pub fn new(width: u32, height: u32) -> Result<Substrate, SubstrateError> {
        if width == 0 || height == 0 || width > MAX_DIM || height > MAX_DIM {
            return Err(SubstrateError::BadDimensions { width, height });
        }
        let n = (width as usize).saturating_mul(height as usize);
        let mut s = Substrate {
            width,
            height,
            chem: vec![vec![0i32; n]; CHEM_COUNT],
            light: vec![0i32; n],
            vx: vec![0i32; n],
            vy: vec![0i32; n],
            blocked: vec![false; n],
            open_x: vec![false; n],
            open_y: vec![false; n],
            has_barriers: false,
            present: [false; CHEM_COUNT],
            has_flow: false,
            edge_vx: vec![0; n],
            edge_vy: vec![0; n],
            edge_velocity_stale: false,
        };
        s.rebuild_edge_masks();
        Ok(s)
    }

    #[inline(always)]
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[inline(always)]
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Number of squares.
    #[inline(always)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.light.len()
    }

    #[inline(always)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Square index for a coordinate, wrapping both axes.
    ///
    /// The grid is a closed box for *flux* — nothing crosses the outer boundary, which is
    /// what keeps matter conserved — but *addressing* wraps like every other index in the
    /// simulation (SPEC §3), so no coordinate a genome or a tool produces is illegal.
    #[inline(always)]
    #[must_use]
    pub fn index(&self, x: i32, y: i32) -> usize {
        let w = self.width as i32;
        let h = self.height as i32;
        let xi = x.rem_euclid(w) as usize;
        let yi = y.rem_euclid(h) as usize;
        yi * self.width as usize + xi
    }

    #[inline(always)]
    #[must_use]
    pub fn chem_plane(&self, c: usize) -> &[i32] {
        &self.chem[c % CHEM_COUNT]
    }

    /// Mutable access to a whole plane.
    ///
    /// Marks the chemical present, because the caller may write anything into it and the
    /// substrate cannot see what.
    #[inline(always)]
    pub fn chem_plane_mut(&mut self, c: usize) -> &mut [i32] {
        let c = c % CHEM_COUNT;
        self.present[c] = true;
        &mut self.chem[c]
    }

    /// Which chemicals may be present anywhere. See [`Substrate::present`].
    #[inline(always)]
    #[must_use]
    pub fn present(&self) -> [bool; CHEM_COUNT] {
        self.present
    }

    /// Whether any square is blocked.
    #[inline(always)]
    #[must_use]
    pub fn has_barriers(&self) -> bool {
        self.has_barriers
    }

    /// Whether anything is flowing.
    #[inline(always)]
    #[must_use]
    pub fn has_flow(&self) -> bool {
        self.has_flow
    }

    /// Set one square's velocity, `Q10` squares per step, clamped to the CFL limit.
    #[inline]
    pub fn set_velocity(&mut self, x: i32, y: i32, u: i32, v: i32) {
        let i = self.index(x, y);
        if self.blocked[i] {
            return;
        }
        let limit = crate::fixed::Q10_ONE;
        let u = u.clamp(-limit, limit);
        let v = v.clamp(-limit, limit);
        self.vx[i] = u;
        self.vy[i] = v;
        self.edge_velocity_stale = true;
        if u != 0 || v != 0 {
            self.has_flow = true;
        }
    }

    /// Recompute [`Substrate::has_flow`] from the velocity field. Call after writing the
    /// field in bulk through [`Substrate::velocity_mut`].
    pub fn refresh_flow(&mut self) {
        self.has_flow = self.vx.iter().chain(self.vy.iter()).any(|v| *v != 0);
        self.edge_velocity_stale = true;
    }

    /// Rebuild the cached edge velocities if the field has moved since they were built.
    ///
    /// Called at the top of a fluid step. On a slide with a prescribed current and no cilia
    /// this runs once and never again.
    pub fn sync_edge_velocity(&mut self) {
        if !self.edge_velocity_stale {
            return;
        }
        self.edge_velocity_stale = false;
        let w = self.width as usize;
        let h = self.height as usize;
        let limit = crate::fluid::MAX_VELOCITY;
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                self.edge_vx[i] = if x + 1 < w {
                    // Mean of the two squares an edge separates, computed in i64 and divided
                    // so the rounding is symmetric under negation: a flow reversed must be a
                    // flow of the same magnitude.
                    let m = (self.vx[i] as i64 + self.vx[i + 1] as i64) / 2;
                    m.clamp(-(limit as i64), limit as i64) as i16
                } else {
                    0
                };
                self.edge_vy[i] = if y + 1 < h {
                    let m = (self.vy[i] as i64 + self.vy[i + w] as i64) / 2;
                    m.clamp(-(limit as i64), limit as i64) as i16
                } else {
                    0
                };
            }
        }
    }

    /// Cached edge velocities. Call [`Substrate::sync_edge_velocity`] first.
    #[must_use]
    pub fn edge_velocity(&self) -> (&[i16], &[i16]) {
        (&self.edge_vx, &self.edge_vy)
    }

    #[must_use]
    pub fn chem_planes(&self) -> &[Vec<i32>] {
        &self.chem
    }

    /// Quantity of chemical `c` at a square, `Q10`.
    #[inline(always)]
    #[must_use]
    pub fn chem_at(&self, c: usize, x: i32, y: i32) -> i32 {
        let i = self.index(x, y);
        self.chem[c % CHEM_COUNT][i]
    }

    /// Add to a square, saturating and never going below zero. Returns the amount actually
    /// applied, which is what the ledger must be told about.
    ///
    /// Every route by which matter enters or leaves the fluid goes through here, so that
    /// I4 is a property of the type rather than of each caller remembering to be careful. A
    /// blocked square accepts nothing.
    #[inline]
    pub fn add_chem(&mut self, c: usize, x: i32, y: i32, delta: i32) -> i32 {
        let i = self.index(x, y);
        if self.blocked[i] {
            return 0;
        }
        let c = c % CHEM_COUNT;
        let plane = &mut self.chem[c];
        let before = plane[i];
        let after = (before as i64 + delta as i64).clamp(0, MAX_QUANTITY as i64) as i32;
        plane[i] = after;
        if after > 0 {
            self.present[c] = true;
        }
        after - before
    }

    /// Set a square outright, clamping to non-negative. Returns the change, for the ledger.
    #[inline]
    pub fn set_chem(&mut self, c: usize, x: i32, y: i32, value: i32) -> i32 {
        let i = self.index(x, y);
        if self.blocked[i] {
            return 0;
        }
        let c = c % CHEM_COUNT;
        let plane = &mut self.chem[c];
        let before = plane[i];
        let after = value.clamp(0, MAX_QUANTITY);
        plane[i] = after;
        if after > 0 {
            self.present[c] = true;
        }
        after - before
    }

    #[inline(always)]
    #[must_use]
    pub fn light(&self) -> &[i32] {
        &self.light
    }

    #[inline(always)]
    pub fn light_mut(&mut self) -> &mut [i32] {
        &mut self.light
    }

    #[inline(always)]
    #[must_use]
    pub fn light_at(&self, x: i32, y: i32) -> i32 {
        let i = self.index(x, y);
        self.light[i]
    }

    #[inline(always)]
    #[must_use]
    pub fn velocity(&self) -> (&[i32], &[i32]) {
        (&self.vx, &self.vy)
    }

    #[inline(always)]
    pub fn velocity_mut(&mut self) -> (&mut [i32], &mut [i32]) {
        (&mut self.vx, &mut self.vy)
    }

    #[inline(always)]
    #[must_use]
    pub fn velocity_at(&self, x: i32, y: i32) -> (i32, i32) {
        let i = self.index(x, y);
        (self.vx[i], self.vy[i])
    }

    #[inline(always)]
    #[must_use]
    pub fn blocked(&self) -> &[bool] {
        &self.blocked
    }

    #[inline(always)]
    #[must_use]
    pub fn is_blocked(&self, x: i32, y: i32) -> bool {
        let i = self.index(x, y);
        self.blocked[i]
    }

    #[inline(always)]
    #[must_use]
    pub fn open_x(&self) -> &[bool] {
        &self.open_x
    }

    #[inline(always)]
    #[must_use]
    pub fn open_y(&self) -> &[bool] {
        &self.open_y
    }

    /// Raise or clear a barrier.
    ///
    /// Raising one evicts whatever the square held: a barrier is solid, and matter cannot be
    /// left sealed inside it where the fluid could never reach it again. The evicted amounts
    /// are returned per chemical so the caller can account for them — they have left the
    /// world, and I4 requires somebody to say so.
    pub fn set_blocked(&mut self, x: i32, y: i32, blocked: bool) -> [i32; CHEM_COUNT] {
        let i = self.index(x, y);
        let mut evicted = [0i32; CHEM_COUNT];
        if self.blocked[i] == blocked {
            return evicted;
        }
        self.blocked[i] = blocked;
        if blocked {
            for (c, plane) in self.chem.iter_mut().enumerate() {
                evicted[c] = plane[i];
                plane[i] = 0;
            }
            self.vx[i] = 0;
            self.vy[i] = 0;
            self.edge_velocity_stale = true;
        }
        self.rebuild_edge_masks();
        evicted
    }

    /// [`Substrate::set_blocked`] without the edge-mask rebuild.
    ///
    /// The rebuild walks every square on the slide, so doing it per square makes blocking `n`
    /// squares cost `n * width * height` — a quarter of a million operations each at 512×512.
    /// That was invisible while a barrier was one square per click and stops being invisible
    /// the moment a brush stamps eighty of them at a time.
    ///
    /// **The caller must call [`Substrate::rebuild_edge_masks`] before the next fluid step**,
    /// or the solver will keep fluxing across an edge that is now a wall. Nothing here can
    /// enforce that, which is why this is not the one to reach for: use `World::set_barriers`,
    /// which batches and rebuilds for you and is the only thing in the crate that calls this.
    pub fn set_blocked_deferred(&mut self, x: i32, y: i32, blocked: bool) -> [i32; CHEM_COUNT] {
        let i = self.index(x, y);
        let mut evicted = [0i32; CHEM_COUNT];
        if self.blocked[i] == blocked {
            return evicted;
        }
        self.blocked[i] = blocked;
        if blocked {
            for (c, plane) in self.chem.iter_mut().enumerate() {
                evicted[c] = plane[i];
                plane[i] = 0;
            }
            self.vx[i] = 0;
            self.vy[i] = 0;
            self.edge_velocity_stale = true;
        }
        evicted
    }

    /// Recompute the edge masks from the barrier layout. Call after any bulk change to
    /// `blocked`.
    pub fn rebuild_edge_masks(&mut self) {
        let w = self.width as usize;
        let h = self.height as usize;
        self.has_barriers = self.blocked.iter().any(|b| *b);
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let here = !self.blocked[i];
                self.open_x[i] = here && x + 1 < w && !self.blocked[i + 1];
                self.open_y[i] = here && y + 1 < h && !self.blocked[i + w];
            }
        }
    }

    /// Overwrite every field, for snapshot restoration. Rebuilds the derived edge masks so
    /// they cannot disagree with the barriers they were restored alongside.
    pub(crate) fn restore(
        &mut self,
        chem: Vec<Vec<i32>>,
        light: Vec<i32>,
        vx: Vec<i32>,
        vy: Vec<i32>,
        blocked: Vec<bool>,
    ) {
        self.chem = chem;
        self.light = light;
        self.vx = vx;
        self.vy = vy;
        self.blocked = blocked;
        self.rebuild_edge_masks();
        self.present = std::array::from_fn(|c| self.chem[c].iter().any(|v| *v != 0));
        self.refresh_flow();
        self.sync_edge_velocity();
    }

    /// The light field alongside the barrier flags, for writing a light regime without
    /// copying a quarter of a megabyte of flags to satisfy the borrow checker.
    pub fn light_and_blocked_mut(&mut self) -> (&mut [i32], &[bool]) {
        (&mut self.light, &self.blocked)
    }

    /// The velocity field alongside the barrier flags. Marks the derived edge velocities
    /// stale, because the caller is about to write the field in bulk.
    pub fn velocity_and_blocked_mut(&mut self) -> (&mut [i32], &mut [i32], &[bool]) {
        self.edge_velocity_stale = true;
        (&mut self.vx, &mut self.vy, &self.blocked)
    }

    /// The same, plus the cached edge velocities that advection reads.
    #[allow(clippy::type_complexity)]
    pub(crate) fn planes_masks_and_edge_velocity(
        &mut self,
    ) -> (&mut [Vec<i32>], &[bool], &[bool], &[i16], &[i16]) {
        (
            &mut self.chem,
            &self.open_x,
            &self.open_y,
            &self.edge_vx,
            &self.edge_vy,
        )
    }

    /// Move a fraction of one species into another, everywhere at once.
    ///
    /// Exactly conservative by the same argument as the fluid solver: the figure subtracted
    /// from one plane is the figure added to the other, square by square, so nothing rounds
    /// away. Returns the total moved, which is what the ledger has to be told.
    pub fn decay_plane(&mut self, from: usize, to: usize, rate: i32) -> i64 {
        let from = from % CHEM_COUNT;
        let to = to % CHEM_COUNT;
        if from == to || rate <= 0 {
            return 0;
        }
        let rate = rate.min(crate::fixed::Q10_ONE);
        let n = self.light.len();
        let mut total = 0i64;
        // Split the borrow: `from` and `to` are different planes, so both can be touched.
        let (lo, hi) = if from < to { (from, to) } else { (to, from) };
        let (head, tail) = self.chem.split_at_mut(hi);
        let (src, dst) = if from < to {
            (&mut head[lo], &mut tail[0])
        } else {
            (&mut tail[0], &mut head[lo])
        };
        for i in 0..n {
            let held = src[i];
            if held <= 0 {
                continue;
            }
            // Bounded by what is there and by what the destination has room for, so a full
            // square cannot make the two sides disagree.
            let headroom = MAX_QUANTITY.saturating_sub(dst[i]).max(0);
            let moved = crate::fixed::q10_scale(held, rate).min(held).min(headroom);
            if moved <= 0 {
                continue;
            }
            src[i] = held - moved;
            dst[i] += moved;
            total += moved as i64;
        }
        if total > 0 {
            self.present[to] = true;
        }
        total
    }

    /// The exact total of each chemical over the whole grid.
    ///
    /// This is the *check*, not the accounting: the ledger claims a total and this recomputes
    /// it from the field. `i64` because 4 million squares of `i32` would overflow one.
    #[must_use]
    pub fn total_chem(&self) -> [i64; CHEM_COUNT] {
        std::array::from_fn(|c| self.chem[c].iter().map(|v| *v as i64).sum())
    }

    /// Whether any square holds a negative quantity. Should never be true: the solver's
    /// fluxes are bounded by what their donor square holds.
    #[must_use]
    pub fn any_negative(&self) -> bool {
        self.chem.iter().flatten().any(|v| *v < 0)
    }

    /// Whether any blocked square holds anything. Should never be true.
    #[must_use]
    pub fn any_matter_inside_a_barrier(&self) -> bool {
        self.blocked
            .iter()
            .enumerate()
            .any(|(i, b)| *b && self.chem.iter().any(|plane| plane[i] != 0))
    }
}

impl StateHash for Substrate {
    fn hash_state(&self, h: &mut StateHasher) {
        h.u32(self.width);
        h.u32(self.height);
        for plane in &self.chem {
            for v in plane {
                h.i32(*v);
            }
        }
        for v in &self.light {
            h.i32(*v);
        }
        for v in &self.vx {
            h.i32(*v);
        }
        for v in &self.vy {
            h.i32(*v);
        }
        for b in &self.blocked {
            h.bool(*b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::q10;

    fn grid() -> Substrate {
        Substrate::new(8, 6).unwrap()
    }

    #[test]
    fn dimensions_are_validated() {
        assert!(Substrate::new(0, 4).is_err());
        assert!(Substrate::new(4, 0).is_err());
        assert!(Substrate::new(MAX_DIM + 1, 4).is_err());
        assert!(Substrate::new(1, 1).is_ok());
    }

    #[test]
    fn addressing_wraps_on_both_axes() {
        let s = grid();
        assert_eq!(s.index(0, 0), 0);
        assert_eq!(s.index(8, 0), 0);
        assert_eq!(s.index(-1, 0), 7);
        assert_eq!(s.index(0, -1), 5 * 8);
        assert_eq!(s.index(-9, -7), s.index(7, 5));
        for x in -100..100 {
            for y in -100..100 {
                assert!(s.index(x, y) < s.len());
            }
        }
    }

    #[test]
    fn quantities_never_go_negative() {
        let mut s = grid();
        s.add_chem(0, 1, 1, q10(5));
        let applied = s.add_chem(0, 1, 1, -q10(9));
        assert_eq!(s.chem_at(0, 1, 1), 0);
        assert_eq!(applied, -q10(5), "the ledger is told what actually moved");
        assert!(!s.any_negative());
    }

    #[test]
    fn additions_saturate_rather_than_overflow() {
        let mut s = grid();
        s.add_chem(0, 0, 0, i32::MAX);
        let applied = s.add_chem(0, 0, 0, i32::MAX);
        assert_eq!(s.chem_at(0, 0, 0), MAX_QUANTITY);
        assert_eq!(applied, 0);
        // The headroom below i32::MAX is what the fluid solver's inner loop relies on.
        const { assert!(MAX_QUANTITY < i32::MAX) };
    }

    #[test]
    fn edge_masks_follow_the_barriers() {
        let mut s = grid();
        // Every interior edge is open to start with.
        assert!(s.open_x()[0]);
        assert!(s.open_y()[0]);
        // The far column and row have no neighbour beyond them.
        assert!(!s.open_x()[s.index(7, 0)]);
        assert!(!s.open_y()[s.index(0, 5)]);

        s.set_blocked(3, 2, true);
        assert!(
            !s.open_x()[s.index(2, 2)],
            "edge into the barrier from the left"
        );
        assert!(
            !s.open_x()[s.index(3, 2)],
            "edge out of the barrier to the right"
        );
        assert!(!s.open_y()[s.index(3, 1)]);
        assert!(!s.open_y()[s.index(3, 2)]);
        assert!(s.open_x()[s.index(0, 2)], "unrelated edges stay open");
    }

    #[test]
    fn raising_a_barrier_evicts_and_reports_what_was_there() {
        let mut s = grid();
        s.add_chem(3, 4, 4, q10(7));
        s.add_chem(9, 4, 4, q10(2));
        let evicted = s.set_blocked(4, 4, true);
        assert_eq!(evicted[3], q10(7));
        assert_eq!(evicted[9], q10(2));
        assert_eq!(evicted.iter().filter(|v| **v != 0).count(), 2);
        assert!(!s.any_matter_inside_a_barrier());
        // and a barrier accepts nothing afterwards
        assert_eq!(s.add_chem(3, 4, 4, q10(1)), 0);
        assert_eq!(s.chem_at(3, 4, 4), 0);
    }

    #[test]
    fn clearing_a_barrier_reopens_its_edges_and_evicts_nothing() {
        let mut s = grid();
        s.set_blocked(4, 4, true);
        let evicted = s.set_blocked(4, 4, false);
        assert!(evicted.iter().all(|v| *v == 0));
        assert!(s.open_x()[s.index(3, 4)]);
    }

    #[test]
    fn totals_are_exact_over_a_large_grid() {
        let mut s = Substrate::new(64, 64).unwrap();
        for x in 0..64 {
            for y in 0..64 {
                s.add_chem(2, x, y, i32::MAX / 2);
            }
        }
        let expected = 64i64 * 64 * (i32::MAX / 2) as i64;
        assert_eq!(s.total_chem()[2], expected, "i64 accumulation, not i32");
    }
}
