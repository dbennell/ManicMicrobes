//! Diffusion and advection (SPEC §7.4).
//!
//! # Conservation is structural
//!
//! Invariant I4 says the total of each chemical is invariant *to the exact integer*, and a
//! test runs a million ticks and requires zero drift — not "within epsilon", which is not
//! conservation. The only way to hold that under integer arithmetic is to make it impossible
//! to violate rather than to correct it afterwards.
//!
//! So matter moves in two stages. First, what crosses every edge is computed from a field
//! nobody is writing to; then every square is updated from the four edges around it:
//!
//! ```text
//! fx[i] = flux across the edge to the right of square i
//! fy[i] = flux across the edge below square i
//! cell[i] += fx[i-1] - fx[i] + fy[i-w] - fy[i]
//! ```
//!
//! Every flux is computed once, subtracted from one square and added to the other, so summing
//! over the plane every term cancels and only the boundary matters — and the boundary is
//! closed. However the flux is rounded, and it is rounded, the same integer leaves one square
//! and arrives in the next. There is nothing to reconcile afterwards and no ordering of edges
//! that could lose a unit.
//!
//! # Why flux-then-apply rather than checkerboard phasing
//!
//! SPEC §7.4 suggests red/black phasing to keep fluxes from racing. Separating the stages
//! achieves the same thing — a flux is read from squares nobody is writing, then applied to
//! squares nobody else is touching — and is better on three counts:
//!
//! * **It vectorises**, and on this kernel that is the whole performance story. Phasing means
//!   visiting every second square and doing two dependent read-modify-writes there, which no
//!   autovectoriser will touch. An in-place rolling sweep is worse still: it touches the field
//!   half as many times but carries a serial dependency down the line, and measured on the
//!   512×512 gate it runs at half the speed of this despite moving half the memory. Every
//!   loop here is unit-stride over contiguous slices with no loop-carried dependency, which is
//!   the shape a compiler turns into SIMD without being asked.
//! * **The update is simultaneous, so it is isotropic.** Under phasing, a square that gained
//!   in the even phase carries that gain into the odd phase, and the result depends on which
//!   parity a square happens to have. Splitting the axes has the same problem one level up: a
//!   spike spreads further along whichever axis went second. Here both axes are computed from
//!   the same field and applied together, so a spike spreads the same distance in all four
//!   directions — exactly, not approximately.
//! * **There is no parity to get wrong at a band boundary**, which is the one place a phased
//!   sweep is easy to break and hard to notice.
//!
//! I6 holds for a stronger reason than phasing would give: the stages contain no ordering at
//! all, so rayon may split them however it likes.
//!
//! # Non-negativity and headroom
//!
//! A square must never go negative — the fluid would owe matter it does not have — and must
//! never exceed `i32::MAX`, which would wrap. Because the update is simultaneous in both
//! axes, a square gives to and receives from all four neighbours at once, so:
//!
//! * **Rates are capped at a quarter.** Diffusion at [`MAX_DIFFUSION`] makes the update the
//!   convex combination `(1-4r)·a + r·(a↑+a↓+a←+a→)`, which cannot leave the range of its own
//!   inputs. Advection at [`MAX_VELOCITY`] takes at most a quarter of the donor per edge, so a
//!   flow diverging in all four directions takes at most all of it. Truncation toward zero
//!   only ever makes a flux smaller, so both bounds survive it.
//! * **Inbound flux is capped by the receiver's headroom**, a quarter of it per edge. The
//!   convex-combination argument bounds diffusion, but advection has no such bound: a flow
//!   converging from four sides adds to a square without taking anything out, and four full
//!   squares converging on one would need five times the range. Capping the flux — rather than
//!   saturating on arrival — is what keeps I4 intact, because the capped value is the one
//!   subtracted from the donor *and* the one added to the receiver.

use rayon::prelude::*;

use crate::chem::{CHEM_COUNT, MAX_DIFFUSION};
use crate::fixed::{q10_scale, Q10_ONE};
use crate::substrate::Substrate;

/// Fastest a chemical may travel: a quarter of a square per fluid step.
///
/// The CFL condition says matter may not cross more than one square per step. A quarter, not
/// one, because the update is simultaneous in both axes: a square donates across all four of
/// its edges at once, and four quarters are exactly all of it. This is the standard explicit
/// stability limit in two dimensions.
pub const MAX_VELOCITY: i32 = Q10_ONE / 4;

/// Rows per rayon task.
const ROWS_PER_TASK: usize = 8;

/// Below this many squares, splitting a sweep costs more than the work it saves.
///
/// The serial and parallel paths do exactly the same arithmetic, so which one runs is
/// invisible in the result: this is a scheduling choice, and I6 says scheduling choices may
/// not be observable.
const PARALLEL_THRESHOLD: usize = 8192;

/// Working buffers for the flux stage.
///
/// Two planes of `i32`, reused across every chemical and both operators. Owned by the caller
/// rather than by the substrate because they are not state: they hold nothing between steps,
/// contribute nothing to the state hash, and must not appear in a snapshot.
#[derive(Clone, Debug, Default)]
pub struct FluidScratch {
    fx: Vec<i32>,
    fy: Vec<i32>,
}

impl FluidScratch {
    #[must_use]
    pub fn new(squares: usize) -> FluidScratch {
        FluidScratch {
            fx: vec![0; squares],
            fy: vec![0; squares],
        }
    }

    fn fit(&mut self, squares: usize) {
        if self.fx.len() != squares {
            self.fx.resize(squares, 0);
            self.fy.resize(squares, 0);
        }
    }
}

/// The per-chemical rates a sweep needs, together.
///
/// A struct rather than two `&[i32; CHEM_COUNT]` arguments side by side: they are the same type
/// and mean opposite things, so the positional version is a transposition nobody would see —
/// the world would still conserve matter exactly, and every chemical would move at the wrong
/// speed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FluidRates {
    /// Fraction of the difference between neighbours crossing per step, `Q10`.
    pub diffusion: [i32; CHEM_COUNT],
    /// How strongly the flow carries each species, `Q10`. See [`crate::chem::ChemicalDef`].
    pub advection: [i32; CHEM_COUNT],
}

impl Default for FluidRates {
    fn default() -> Self {
        FluidRates {
            diffusion: [0; CHEM_COUNT],
            advection: [Q10_ONE; CHEM_COUNT],
        }
    }
}

/// Run one fluid step: diffusion then advection, every chemical.
pub fn step(substrate: &mut Substrate, rates: &FluidRates, scratch: &mut FluidScratch) {
    sweep(substrate, rates, scratch, true, true);
}

/// One diffusion sweep over every chemical.
pub fn diffuse(substrate: &mut Substrate, rates: &FluidRates, scratch: &mut FluidScratch) {
    sweep(substrate, rates, scratch, true, false);
}

/// One advection sweep over every chemical.
pub fn advect(substrate: &mut Substrate, rates: &FluidRates, scratch: &mut FluidScratch) {
    sweep(substrate, rates, scratch, false, true);
}

/// Chemicals do not interact in the fluid, so all of one chemical's passes run before the
/// next is touched at all. One plane is a megabyte and stays in cache across its own passes;
/// interleaving them would evict it sixteen times over. The results are identical either way,
/// which is exactly why the reordering is free.
fn sweep(
    substrate: &mut Substrate,
    rates: &FluidRates,
    scratch: &mut FluidScratch,
    do_diffuse: bool,
    do_advect: bool,
) {
    substrate.sync_edge_velocity();
    scratch.fit(substrate.len());
    let w = substrate.width() as usize;
    let h = substrate.height() as usize;
    let masked = substrate.has_barriers();
    let flowing = do_advect && substrate.has_flow();
    let present = substrate.present();
    let (planes, open_x, open_y, evx, evy) = substrate.planes_masks_and_edge_velocity();

    for (c, plane) in planes.iter_mut().enumerate() {
        if !present[c] {
            continue;
        }
        let rate = if do_diffuse {
            rates.diffusion[c].clamp(0, MAX_DIFFUSION)
        } else {
            0
        };
        if rate > 0 {
            flux_x(
                plane,
                &mut scratch.fx,
                open_x,
                w,
                masked,
                None,
                rate,
                Q10_ONE,
            );
            flux_y(
                plane,
                &mut scratch.fy,
                open_y,
                w,
                h,
                masked,
                None,
                rate,
                Q10_ONE,
            );
            apply(plane, &scratch.fx, &scratch.fy, w);
        }
        // A species that couples to the flow not at all is not advected at all, and skipping it
        // is the same answer for less work rather than a different one.
        let mobility = rates.advection[c].clamp(0, Q10_ONE);
        if flowing && mobility > 0 {
            flux_x(
                plane,
                &mut scratch.fx,
                open_x,
                w,
                masked,
                Some(evx),
                0,
                mobility,
            );
            flux_y(
                plane,
                &mut scratch.fy,
                open_y,
                w,
                h,
                masked,
                Some(evy),
                0,
                mobility,
            );
            apply(plane, &scratch.fx, &scratch.fy, w);
        }
    }
}

/// Flux across every x-edge. `edge_v` selects advection; without it this is diffusion.
///
/// The last column of each row has no square to its right, and neither does it have one in
/// the row below: the flat array puts them next to each other, and the closed box does not.
#[allow(clippy::too_many_arguments)]
fn flux_x(
    plane: &[i32],
    out: &mut [i32],
    open: &[bool],
    w: usize,
    masked: bool,
    edge_v: Option<&[i16]>,
    rate: i32,
    mobility: i32,
) {
    if w < 2 {
        out.fill(0);
        return;
    }
    bands(out, w, |base, chunk| {
        for (r, frow) in chunk.chunks_mut(w).enumerate() {
            let rb = base + r * w;
            let row = &plane[rb..rb + frow.len()];
            let last = row.len().saturating_sub(1);
            match edge_v {
                None => {
                    // No headroom cap on the diffusion path. The convex-combination bound
                    // already keeps the result inside the range of its own inputs, and
                    // `MAX_QUANTITY` leaves slack for the four truncations that bound cannot
                    // see. Two branches per edge is the difference between this loop
                    // vectorising and not.
                    for k in 0..last {
                        frow[k] = q10_scale(row[k] - row[k + 1], rate);
                    }
                }
                // Full coupling is every chemical the default table ships, so it keeps the
                // loop it always had: branching once per row rather than paying a multiply per
                // edge, which cost 15% of the flowing sweep when it was in the inner loop.
                Some(v) if mobility >= Q10_ONE => {
                    for k in 0..last {
                        let (a, b) = (row[k], row[k + 1]);
                        frow[k] = cap(upwind(a, b, v[rb + k]), a, b);
                    }
                }
                Some(v) => {
                    for k in 0..last {
                        let (a, b) = (row[k], row[k + 1]);
                        frow[k] = cap(upwind(a, b, damped(v[rb + k], mobility)), a, b);
                    }
                }
            }
            if masked {
                for k in 0..last {
                    if !open[rb + k] {
                        frow[k] = 0;
                    }
                }
            }
            if let Some(slot) = frow.get_mut(last) {
                *slot = 0;
            }
        }
    });
}

/// Flux across every y-edge. The last row has no square below it.
#[allow(clippy::too_many_arguments)]
fn flux_y(
    plane: &[i32],
    out: &mut [i32],
    open: &[bool],
    w: usize,
    h: usize,
    masked: bool,
    edge_v: Option<&[i16]>,
    rate: i32,
    mobility: i32,
) {
    if h < 2 {
        out.fill(0);
        return;
    }
    let interior = (h - 1) * w;
    bands(out, w, |base, chunk| {
        let n = chunk.len();
        let end = (base + n).min(interior);
        let live = end.saturating_sub(base);
        let here = &plane[base..base + live];
        let below = &plane[base + w..base + w + live];
        match edge_v {
            None => {
                for k in 0..live {
                    chunk[k] = q10_scale(here[k] - below[k], rate);
                }
            }
            // As in `flux_x`: the full-coupling path is the loop this always had.
            Some(v) if mobility >= Q10_ONE => {
                for k in 0..live {
                    let (a, b) = (here[k], below[k]);
                    chunk[k] = cap(upwind(a, b, v[base + k]), a, b);
                }
            }
            Some(v) => {
                for k in 0..live {
                    let (a, b) = (here[k], below[k]);
                    chunk[k] = cap(upwind(a, b, damped(v[base + k], mobility)), a, b);
                }
            }
        }
        if masked {
            for k in 0..live {
                if !open[base + k] {
                    chunk[k] = 0;
                }
            }
        }
        for slot in chunk.iter_mut().skip(live) {
            *slot = 0;
        }
    });
}

/// Apply both axes' fluxes at once: each square loses what leaves and gains what arrives.
///
/// This is where conservation happens. Every `fx[i]` was subtracted from square `i` and added
/// to square `i+1`; every `fy[i]` from `i` and to `i+w`. Sum over the plane and every term
/// appears twice with opposite signs.
///
/// One pass, five unit-stride reads and one write. The first square of each row has no
/// x-neighbour behind it and the first row has no y-neighbour above it — the grid is a closed
/// box for flux, whatever addressing does.
fn apply(plane: &mut [i32], fx: &[i32], fy: &[i32], w: usize) {
    bands(plane, w, |base, chunk| {
        for (r, row) in chunk.chunks_mut(w).enumerate() {
            let rb = base + r * w;
            let n = row.len();
            let fxr = &fx[rb..rb + n];
            let fyr = &fy[rb..rb + n];
            if rb >= w {
                let above = &fy[rb - w..rb - w + n];
                row[0] = settle(row[0], fxr[0], fyr[0], 0, above[0]);
                for k in 1..n {
                    row[k] = settle(row[k], fxr[k], fyr[k], fxr[k - 1], above[k]);
                }
            } else {
                row[0] = settle(row[0], fxr[0], fyr[0], 0, 0);
                for k in 1..n {
                    row[k] = settle(row[k], fxr[k], fyr[k], fxr[k - 1], 0);
                }
            }
        }
    });
}

/// Largest flux across one edge, given what the receiving square already holds.
#[inline(always)]
fn cap(f: i32, a: i32, b: i32) -> i32 {
    if f > 0 {
        f.min((i32::MAX - b) >> 2)
    } else if f < 0 {
        -((-f).min((i32::MAX - a) >> 2))
    } else {
        0
    }
}

/// Upwind flux across one edge: a fraction of whichever square the flow comes from.
///
/// A positive result moves matter from `a` to `b`, a negative one the other way.
#[inline(always)]
fn upwind(a: i32, b: i32, u: i16) -> i32 {
    let u = u as i32;
    if u > 0 {
        q10_scale(a, u)
    } else if u < 0 {
        -q10_scale(b, -u)
    } else {
        0
    }
}

/// The velocity a species of this coupling sees.
///
/// Applied to the *velocity* rather than to the flux. The same thing arithmetically and not the
/// same thing to read: what is being said is that this chemical is carried by a slower current,
/// which is what being heavy means in a slide with no downward to fall in. Conservation is
/// untouched either way, because the flux is still one number leaving one square and arriving
/// in its neighbour.
#[inline(always)]
fn damped(u: i16, mobility: i32) -> i16 {
    q10_scale(u as i32, mobility) as i16
}

/// A square's new value: lose what leaves on both axes, gain what arrives on both.
#[inline(always)]
fn settle(cell: i32, out_x: i32, out_y: i32, in_x: i32, in_y: i32) -> i32 {
    let v = cell as i64 - out_x as i64 - out_y as i64 + in_x as i64 + in_y as i64;
    debug_assert!(
        (0..=i32::MAX as i64).contains(&v),
        "flux application left the legal range: {v}"
    );
    v as i32
}

/// Split a buffer into row-aligned bands, in parallel when there is enough work.
#[inline]
fn bands<F>(buf: &mut [i32], w: usize, f: F)
where
    F: Fn(usize, &mut [i32]) + Sync + Send,
{
    let band = w.max(1) * ROWS_PER_TASK;
    if buf.len() < PARALLEL_THRESHOLD {
        for (b, chunk) in buf.chunks_mut(band).enumerate() {
            f(b * band, chunk);
        }
    } else {
        buf.par_chunks_mut(band)
            .enumerate()
            .for_each(|(b, chunk)| f(b * band, chunk));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chem::ChemTable;
    use crate::fixed::q10;
    use crate::rng::{Purpose, RandCtx};

    /// Every chemical at one diffusion rate, all fully carried by the flow.
    fn rates(rate: i32) -> FluidRates {
        FluidRates {
            diffusion: [rate; CHEM_COUNT],
            advection: [Q10_ONE; CHEM_COUNT],
        }
    }

    /// Every chemical at one diffusion rate and one coupling to the flow.
    fn rates_at(rate: i32, mobility: i32) -> FluidRates {
        FluidRates {
            diffusion: [rate; CHEM_COUNT],
            advection: [mobility; CHEM_COUNT],
        }
    }

    fn scratch(s: &Substrate) -> FluidScratch {
        FluidScratch::new(s.len())
    }

    /// Fill every square of every chemical with arbitrary non-negative amounts, and stir the
    /// velocity field arbitrarily too.
    fn scramble(s: &mut Substrate, seed: u64) {
        let ctx = RandCtx::new(seed, 0, 0);
        let mut i = 0u64;
        for y in 0..s.height() as i32 {
            for x in 0..s.width() as i32 {
                for c in 0..CHEM_COUNT {
                    i += 1;
                    let v = (ctx.draw(Purpose::Harness, i) % 4_000_000) as i32;
                    s.set_chem(c, x, y, v);
                }
                i += 1;
                let u = (ctx.draw(Purpose::Harness, i) % (2 * MAX_VELOCITY as u64)) as i32
                    - MAX_VELOCITY;
                i += 1;
                let v = (ctx.draw(Purpose::Harness, i) % (2 * MAX_VELOCITY as u64)) as i32
                    - MAX_VELOCITY;
                s.set_velocity(x, y, u, v);
            }
        }
    }

    #[test]
    fn upwind_takes_from_the_side_the_flow_comes_from() {
        assert_eq!(upwind(1024, 0, 512), 512, "rightward takes from the left");
        assert_eq!(upwind(0, 1024, -512), -512, "leftward takes from the right");
        assert_eq!(upwind(1024, 4096, 0), 0);
        // and never takes more than half, which is what keeps a divergent flow safe
        for a in [0i32, 1, 1023, i32::MAX] {
            assert!(upwind(a, 0, MAX_VELOCITY as i16).abs() <= a / 2 + 1);
        }
    }

    #[test]
    fn conservation_holds_over_a_scrambled_grid() {
        for (w, h) in [
            (1, 1),
            (2, 1),
            (1, 2),
            (3, 3),
            (5, 4),
            (4, 5),
            (17, 31),
            (64, 64),
        ] {
            let mut s = Substrate::new(w, h).unwrap();
            let mut sc = scratch(&s);
            scramble(&mut s, 3);
            let before = s.total_chem();
            for _ in 0..60 {
                step(&mut s, &rates(MAX_DIFFUSION), &mut sc);
            }
            assert_eq!(s.total_chem(), before, "{w}x{h} did not conserve");
            assert!(!s.any_negative(), "{w}x{h} went negative");
        }
    }

    #[test]
    fn a_grid_large_enough_to_be_split_conserves() {
        // Above PARALLEL_THRESHOLD, so this exercises the rayon path and its band
        // boundaries, where a mistake would show up as a leak.
        let mut s = Substrate::new(131, 97).unwrap();
        let mut sc = scratch(&s);
        assert!(s.len() > PARALLEL_THRESHOLD);
        scramble(&mut s, 5);
        let before = s.total_chem();
        for _ in 0..30 {
            step(&mut s, &rates(MAX_DIFFUSION), &mut sc);
        }
        assert_eq!(s.total_chem(), before);
        assert!(!s.any_negative());
    }

    #[test]
    fn saturated_squares_conserve() {
        // Every square at the top of the i32 range, so any overflow in the apply pass shows
        // up immediately rather than only under a rare gradient.
        let mut s = Substrate::new(40, 40).unwrap();
        let mut sc = scratch(&s);
        for y in 0..40 {
            for x in 0..40 {
                s.set_chem(0, x, y, if (x + y) % 2 == 0 { i32::MAX } else { 0 });
                s.set_velocity(x, y, MAX_VELOCITY, -MAX_VELOCITY);
            }
        }
        let before = s.total_chem();
        for _ in 0..200 {
            step(&mut s, &rates(MAX_DIFFUSION), &mut sc);
        }
        assert_eq!(s.total_chem(), before);
        assert!(!s.any_negative());
    }

    #[test]
    fn diffusion_evens_out_a_spike() {
        let mut s = Substrate::new(16, 16).unwrap();
        let mut sc = scratch(&s);
        s.set_chem(0, 8, 8, q10(100_000));
        let before = s.total_chem();
        for _ in 0..800 {
            diffuse(&mut s, &rates(MAX_DIFFUSION), &mut sc);
        }
        assert_eq!(s.total_chem(), before);
        let plane = s.chem_plane(0);
        let peak = plane.iter().copied().max().unwrap_or(0);
        let floor = plane.iter().copied().min().unwrap_or(0);
        assert!(
            peak < q10(100_000) / 10,
            "spike did not spread: peak {peak}"
        );
        assert!(floor > 0, "corners never received anything");
    }

    #[test]
    fn diffusion_is_isotropic() {
        // The reason for a simultaneous update rather than a phased one: a spike in the
        // middle of an empty grid must spread the same distance in all four directions.
        let mut s = Substrate::new(33, 33).unwrap();
        let mut sc = scratch(&s);
        s.set_chem(0, 16, 16, q10(1_000_000));
        for _ in 0..200 {
            diffuse(&mut s, &rates(MAX_DIFFUSION), &mut sc);
        }
        let left = s.chem_at(0, 16 - 5, 16);
        let right = s.chem_at(0, 16 + 5, 16);
        let up = s.chem_at(0, 16, 16 - 5);
        let down = s.chem_at(0, 16, 16 + 5);
        assert_eq!(left, right, "asymmetric along x");
        assert_eq!(up, down, "asymmetric along y");
        assert_eq!(left, up, "asymmetric between axes");
    }

    #[test]
    fn advection_carries_matter_downstream() {
        let mut s = Substrate::new(32, 4).unwrap();
        let mut sc = scratch(&s);
        s.set_chem(0, 2, 2, q10(10_000));
        for y in 0..4 {
            for x in 0..32 {
                s.set_velocity(x, y, MAX_VELOCITY / 2, 0);
            }
        }
        for _ in 0..40 {
            advect(&mut s, &rates(0), &mut sc);
        }
        let mut num = 0i64;
        let mut den = 0i64;
        for x in 0..32i32 {
            let v = s.chem_at(0, x, 2) as i64;
            num += v * x as i64;
            den += v;
        }
        assert!(num / den.max(1) > 4, "did not move downstream");
    }

    #[test]
    fn barriers_are_impermeable() {
        let mut s = Substrate::new(16, 16).unwrap();
        let mut sc = scratch(&s);
        for y in 0..16 {
            s.set_blocked(8, y, true);
        }
        for y in 0..16 {
            for x in 0..8 {
                s.set_chem(0, x, y, q10(50_000));
            }
            for x in 0..16 {
                s.set_velocity(x, y, MAX_VELOCITY, MAX_VELOCITY);
            }
        }
        let before = s.total_chem();
        for _ in 0..1500 {
            step(&mut s, &rates(MAX_DIFFUSION), &mut sc);
        }
        assert_eq!(s.total_chem(), before);
        for y in 0..16 {
            for x in 8..16 {
                assert_eq!(
                    s.chem_at(0, x, y),
                    0,
                    "matter crossed the wall at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn a_sealed_pocket_keeps_exactly_what_it_started_with() {
        let mut s = Substrate::new(16, 16).unwrap();
        let mut sc = scratch(&s);
        for i in 0..5i32 {
            s.set_blocked(4 + i, 4, true);
            s.set_blocked(4 + i, 8, true);
            s.set_blocked(4, 4 + i, true);
            s.set_blocked(8, 4 + i, true);
        }
        for y in 5..8 {
            for x in 5..8 {
                s.set_chem(1, x, y, q10(1000));
            }
        }
        let inside: i64 = (5..8)
            .flat_map(|y| (5..8).map(move |x| (x, y)))
            .map(|(x, y)| s.chem_at(1, x, y) as i64)
            .sum();
        for y in 0..16 {
            for x in 0..16 {
                s.set_velocity(x, y, MAX_VELOCITY, -MAX_VELOCITY);
            }
        }
        for _ in 0..1000 {
            step(&mut s, &rates(MAX_DIFFUSION), &mut sc);
        }
        assert_eq!(s.total_chem()[1], inside, "matter escaped the pocket");
    }

    #[test]
    fn the_grid_is_a_closed_box() {
        // Addressing wraps, but flux does not: a flow pressed against the right edge must
        // pile matter up there rather than reappear on the left.
        let mut s = Substrate::new(16, 4).unwrap();
        let mut sc = scratch(&s);
        for y in 0..4 {
            s.set_chem(0, 14, y, q10(1000));
            for x in 0..16 {
                s.set_velocity(x, y, MAX_VELOCITY, 0);
            }
        }
        let before = s.total_chem();
        for _ in 0..200 {
            advect(&mut s, &rates(0), &mut sc);
        }
        assert_eq!(s.total_chem(), before);
        for y in 0..4 {
            assert_eq!(s.chem_at(0, 0, y), 0, "matter wrapped around the edge");
            assert!(
                s.chem_at(0, 15, y) > 0,
                "matter did not pile up at the wall"
            );
        }
    }

    #[test]
    fn a_row_does_not_leak_into_the_next_one() {
        // The flat array puts the last column of a row next to the first column of the row
        // below, so a missing edge check there would look like a slow horizontal drift rather
        // than an obvious break. Matter seeded at the right-hand edge moves at most one square
        // per step, so after one step column 0 must still be empty — reaching it at all would
        // mean it had crossed the slide.
        let mut s = Substrate::new(8, 8).unwrap();
        let mut sc = scratch(&s);
        s.set_chem(0, 7, 0, q10(1000));
        diffuse(&mut s, &rates(MAX_DIFFUSION), &mut sc);

        assert!(s.chem_at(0, 6, 0) > 0, "did not spread left");
        assert!(s.chem_at(0, 7, 1) > 0, "did not spread down");
        for y in 0..8i32 {
            assert_eq!(
                s.chem_at(0, 0, y),
                0,
                "matter wrapped into column 0 at row {y}"
            );
        }
    }

    #[test]
    fn a_heavy_species_lags_the_flow_and_a_light_one_keeps_up() {
        // The whole point of the field: two identical spikes in the same square, one carried
        // fully by the water and one at a quarter, in one current for the same number of steps.
        let mut s = Substrate::new(48, 4).unwrap();
        let mut sc = scratch(&s);
        s.set_chem(0, 2, 2, q10(10_000));
        s.set_chem(1, 2, 2, q10(10_000));
        for y in 0..4 {
            for x in 0..48 {
                s.set_velocity(x, y, MAX_VELOCITY / 2, 0);
            }
        }
        let mut r = rates(0);
        r.advection[1] = Q10_ONE / 4;
        for _ in 0..40 {
            step(&mut s, &r, &mut sc);
        }
        // Centre of mass along the flow: where each plume has actually got to.
        let centre = |c: usize, s: &Substrate| {
            let (mut num, mut den) = (0i64, 0i64);
            for x in 0..48i32 {
                let v = s.chem_at(c, x, 2) as i64;
                num += v * i64::from(x);
                den += v;
            }
            num as f64 / den.max(1) as f64
        };
        let (light, heavy) = (centre(0, &s), centre(1, &s));
        assert!(
            light > heavy + 1.0,
            "the heavy species kept up with the light one: {heavy:.2} against {light:.2}"
        );
        assert!(
            heavy > 2.0,
            "the heavy species did not move at all: {heavy:.2}"
        );
    }

    #[test]
    fn coupling_to_the_flow_does_not_cost_a_single_unit_of_matter() {
        // I4 is the one thing this must not touch. Scaling the velocity scales the flux, and a
        // flux is still one number leaving one square and arriving in its neighbour — so this
        // asserts the property rather than trusting the argument, at every coupling including
        // the awkward ones that do not divide evenly.
        for mobility in [0, 1, Q10_ONE / 3, Q10_ONE / 2, Q10_ONE - 1, Q10_ONE] {
            let mut s = Substrate::new(24, 24).unwrap();
            let mut sc = scratch(&s);
            for y in 0..24 {
                for x in 0..24 {
                    s.set_velocity(x, y, MAX_VELOCITY / 2, -MAX_VELOCITY / 3);
                }
            }
            let mut before = [0i64; CHEM_COUNT];
            for c in 0..CHEM_COUNT {
                s.set_chem(c, 5 + c as i32 % 7, 3 + c as i32 % 11, q10(500 + c as i32));
                before[c] = i64::from(s.chem_at(c, 5 + c as i32 % 7, 3 + c as i32 % 11));
            }
            let r = rates_at(MAX_DIFFUSION, mobility);
            for _ in 0..200 {
                step(&mut s, &r, &mut sc);
            }
            for c in 0..CHEM_COUNT {
                let mut total = 0i64;
                for y in 0..24i32 {
                    for x in 0..24i32 {
                        total += i64::from(s.chem_at(c, x, y));
                    }
                }
                assert_eq!(
                    total, before[c],
                    "chemical {c} drifted at coupling {mobility}"
                );
            }
        }
    }

    #[test]
    fn a_species_with_no_coupling_is_one_the_current_cannot_move() {
        let mut s = Substrate::new(32, 4).unwrap();
        let mut sc = scratch(&s);
        s.set_chem(0, 4, 2, q10(1000));
        for y in 0..4 {
            for x in 0..32 {
                s.set_velocity(x, y, MAX_VELOCITY, 0);
            }
        }
        let r = rates_at(0, 0);
        for _ in 0..100 {
            step(&mut s, &r, &mut sc);
        }
        assert_eq!(
            s.chem_at(0, 4, 2),
            q10(1000),
            "a current moved a species with no coupling to it"
        );
    }

    #[test]
    fn a_zero_diffusion_chemical_does_not_move() {
        let mut s = Substrate::new(8, 8).unwrap();
        let mut sc = scratch(&s);
        s.set_chem(0, 4, 4, q10(1000));
        let mut r = rates(MAX_DIFFUSION);
        r.diffusion[0] = 0;
        for _ in 0..100 {
            diffuse(&mut s, &r, &mut sc);
        }
        assert_eq!(s.chem_at(0, 4, 4), q10(1000));
    }

    #[test]
    fn skipping_absent_chemicals_and_still_water_changes_nothing() {
        // Both are optimisations, so both must be invisible.
        let mut a = Substrate::new(24, 24).unwrap();
        let mut b = Substrate::new(24, 24).unwrap();
        let mut sa = scratch(&a);
        let mut sb = scratch(&b);
        a.set_chem(3, 12, 12, q10(5000));
        b.set_chem(3, 12, 12, q10(5000));
        b.set_chem(7, 1, 1, q10(10));
        assert!(!a.has_flow() && !b.has_flow());
        for _ in 0..200 {
            step(&mut a, &rates(MAX_DIFFUSION), &mut sa);
            step(&mut b, &rates(MAX_DIFFUSION), &mut sb);
        }
        assert_eq!(a.chem_plane(3), b.chem_plane(3));
    }

    #[test]
    fn the_default_chemical_table_conserves() {
        let table = ChemTable::spec_default();
        let mut s = Substrate::new(24, 24).unwrap();
        let mut sc = scratch(&s);
        scramble(&mut s, 4);
        let before = s.total_chem();
        for _ in 0..200 {
            step(
                &mut s,
                &FluidRates {
                    diffusion: table.diffusion_rates(),
                    advection: table.advection_rates(),
                },
                &mut sc,
            );
        }
        assert_eq!(s.total_chem(), before);
    }
}
