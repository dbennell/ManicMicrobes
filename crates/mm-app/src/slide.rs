//! The simulation, and the wall between it and the renderer.
//!
//! # Why this type exists
//!
//! M4's first acceptance test is that **rendering cannot affect the simulation**: the state
//! hash at 100,000 ticks must be identical whether the world was run through the front-end at
//! sixty frames a second or through `mm-cli` headless. The way to pass a test like that is not
//! to be careful; it is to make the other outcome unrepresentable.
//!
//! So [`Slide`] owns the world and hands the renderer a [`Frame`] — a plain snapshot of
//! positions and colours with no reference back. The render layer never sees `&mut World`, so
//! there is no code path by which a frame rate, a window resize, a dropped frame or a paused
//! camera could reach a tick. `Slide::advance` takes a number of ticks and nothing else: not
//! a delta time, not a frame duration, not a clock. A simulation that took wall-clock time as
//! input would produce different worlds on different machines, which is I1 gone.
//!
//! This is also why the type lives in `mm-app` and not in `mm-core`. `mm-core` does not know
//! what a frame is, and must not learn.

use rayon::prelude::*;

use mm_core::chem::CHEM_COUNT;
use mm_core::ecology::TrophicMix;
use mm_core::fixed::{pos_to_square, POS_ONE, Q10_ONE};
use mm_core::metrics::Sample;
use mm_core::{Scenario, World};

/// One cell, as the renderer needs it.
///
/// Floats, because this is rendering — SPEC's no-floats rule (I2) is about `mm-core`, and the
/// conversion happens here, on the way out, where it cannot affect anything.
#[derive(Clone, PartialEq, Debug)]
pub struct CellDot {
    /// Position in substrate squares.
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    /// Colour, from what the cell is made of rather than from what it is called: a cell full
    /// of chloroplasts is green because chloroplasts are green.
    pub rgb: [f32; 3],
    /// Depth below the focal plane, for depth-of-field. Derived from identity, so a cell does
    /// not swim in and out of focus as it moves. See [`crate::optics::depth_of`].
    pub depth: f32,
    /// Which cell this is, so the inspector can be pointed at it and so selection survives a
    /// frame boundary.
    pub id: mm_core::CellId,
    /// Organelles, present only at [`Lod::Organelles`] and above. Empty at far zoom, because
    /// at far zoom nobody can see them and building the list would be a hundred thousand
    /// allocations for nothing.
    pub organelles: Vec<OrganelleDot>,
    /// What this cell has grown that reaches outside its membrane — cilia, a flagellum, a drawn
    /// spike, a holdfast, an exoenzyme's cloud. Present on the same terms as `organelles`, and
    /// usually empty even then: on the mixed benchmark slide 3.8% of cells carry a cilium and
    /// 7.7% a holdfast.
    pub limbs: Vec<LimbDot>,
    /// How many cells are in this one's organism, over hard junctions (M7). One means a
    /// solitary cell.
    pub cluster_size: u32,
    /// How long this cell has existed, in ticks. Only the first few matter to the renderer,
    /// which uses them to swell a newborn into place rather than have it appear whole.
    pub age: u32,
    /// Where this cell is flattened by the neighbours it is pressed into.
    ///
    /// Empty below [`Lod::Packed`], which is a tier earlier than `organelles` — a crowd reads as
    /// overlapping discs long before there is anything inside a cell worth resolving. Below that
    /// a cell is a few pixels across, cannot show a flattened side at all, and building the list
    /// for fifty thousand of them would be work with nothing to show for it.
    pub squash: Vec<Squash>,
    /// How much larger than [`PACKING`] this cell is drawn so that its clipped outline still
    /// encloses the area it has. See [`area_swell`]. One for a cell nothing is pressing on.
    pub area_swell: f32,
}

/// One flat face where a cell is pressed into a neighbour.
///
/// A plane rather than a bite taken out: two cells that overlap should meet along one seam,
/// with no gap and no doubled edge, and both of them have to agree where it is without talking
/// to each other. The plane through the two points where their outlines cross is the one
/// choice that satisfies that from either side — it depends only on the two centres and the
/// two radii, so each cell computes the same seam independently.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Squash {
    /// Unit vector from this cell's centre towards the neighbour.
    pub nx: f32,
    pub ny: f32,
    /// How far along it the seam sits, as a fraction of this cell's own radius. Less than one
    /// means the cell is genuinely cut into; a big neighbour can push this below zero and take
    /// a bite past the centre, which is what being engulfed looks like.
    pub face: f32,
}

/// How much larger than its physical radius a cell is *drawn*.
///
/// Cells are drawn touching because otherwise they never are. Separation pushes every
/// overlapping pair apart on every tick and stops only when `d >= ri + rj`, so the state the
/// physics is always driving towards is cells resting exactly *at* contact with zero overlap —
/// and circles that touch at a point leave a triangular hole between every three of them. A
/// clump drawn honestly at the simulation's radii is mostly gaps, which is why it reads as
/// cells floating near each other rather than packed together.
///
/// So the drawing is bigger than the physics by a fifth, and the seams are computed at the
/// drawn size. Two cells resting at contact then overlap *on screen* and cut each other along
/// the plane between them: the hole closes, the two share a flat wall, and because the seam
/// partitions the overlap exactly there is still no pixel drawn twice.
///
/// This changes nothing but the picture. Collision, sensing, junction reach and everything else
/// use the radius `mm-core` reports; this is the last step before the mesh, and the amount is a
/// judgement about how a crowd should look rather than a measurement of anything.
pub const PACKING: f32 = 1.15;

/// How far out to look for neighbours, in permille of the two physical radii.
///
/// [`PACKING`] **times [`MAX_SWELL`]**, which is the part that was missing. A cell is drawn at
/// `PACKING * area_swell` times its physical radius, so two cells can be far enough apart to be
/// no contact at all by the `PACKING` measure and still have their *drawn* outlines overlap once
/// both have swollen. Such a pair gets no seam, so neither cuts the other, and they are drawn one
/// lying over the other with the lower one's outline running on behind — which is precisely the
/// overlapping that survived every other fix.
///
/// The rule is that the search radius has to cover the largest a cell can ever be drawn, not the
/// size it usually is — and with margin. At exactly `PACKING * MAX_SWELL` (1.219) the seam
/// appears at 1.220 and the drawn outlines meet at 1.219, a gap of one part in a thousand, which
/// is inside the quantisation of an integer position. A pair sitting on that distance is drawn
/// overlapping with no seam on one frame and sharing a wall on the next, which is exactly what
/// two cells flicking back and forth between overlapping and having a boundary looks like.
///
/// Wide enough that a seam is always in hand well before it is needed. A contact that is not yet
/// overlapping costs one half-plane test that clips nothing, which is cheap; a contact that
/// arrives late costs the picture.
const PACKING_PERMILLE: i32 = 1750;

/// The closest to its own centre a cell may be cut, as a fraction of its drawn radius.
///
/// A cell keeps a core, however hard it is squeezed. The plane through where two outlines
/// cross is the right seam for two circles that overlap a little, and it goes on being the
/// arithmetically right seam as they overlap a lot — the cut marches inward, past the centre
/// if you let it, and a cell with eight neighbours all pressing gets cut by eight planes until
/// there is nothing of it left. On the packing bench the outer ring tiles correctly and the
/// middle collapses into wedges and slivers, which is that.
///
/// Correct for circles and wrong for cells. Two pressurised bodies do not pass through one
/// another; they resist, and the contact facet stops advancing. This is that resistance, as
/// the one number it takes to express: eight seams at this distance still leave a polygon with
/// this inradius, so the core survives from every direction at once.
///
/// The cost is that a clamped pair no longer agrees exactly — the two faces stop summing to
/// the distance between the centres, so cells that deep in each other overlap slightly rather
/// than sharing a wall. That is the right way round. Past this depth something has to give,
/// and a little overlap between two cells that should not be that close is a far smaller lie
/// than shattering both of them.
///
/// The physics now keeps the same core, as `mm_core::neighbours::CORE_PERMILLE` — the same
/// fraction, as a floor on how close two centres may come rather than on where a cell may be
/// cut. **Change the two together.** They are not redundant: for equal radii they coincide
/// exactly, but a cell twice its neighbour's radius has the crossing plane past the smaller
/// cell's centre while the two cores are still apart, so this clamp still does real work. What
/// has changed is that it is no longer the *only* thing standing between a crowd and collapse,
/// which is why cells that deep in each other are now rare rather than the normal state of the
/// middle of a pack. See SPEC §6.4.
/// **Not** `mm_core::neighbours::CORE_PERMILLE`, and deliberately far below it. Tried tying the
/// two together and it was a clear mistake, worth recording so it is not tried again.
///
/// They sound like the same idea and they are not. The core is where the *physics* stops pressing
/// cells together, and a packed crowd sits exactly on it. This is a floor on where a cell may be
/// *cut*, and its job is to catch the pathological case — a big neighbour whose crossing plane
/// falls past a small cell's centre — which is rare. Setting this to the core made it bind on
/// every contact in the pack rather than on the rare bad one, so the seam stopped being the plane
/// through the crossing outlines and became the clamp, and cells came out as wedges.
///
/// Worse, the two-cores-do-not-fit branch below then sat exactly on the boundary the physics pins
/// crowds to, so it flickered in and out between frames — two different seam rules alternating on
/// the same pair, which reads as boundaries fighting over where they belong.
pub const MIN_FACE: f32 = 0.55;

/// A cell's radius for drawing, as a smooth function of its mass.
///
/// `mm_core::biology::radius` is a **staircase**, and rightly so: hard rule 2 forbids floats in
/// `mm-core`, and the physics wants a radius that is cheap and monotone. It truncates mass to
/// whole units, takes an integer square root, and returns `0.25 + isqrt(mass) * 0.125` squares —
/// so the tread is a fixed eighth of a square whatever the cell's size, which is 17% of a
/// three-quarter-square cell and 6% of a two-square one.
///
/// Drawn straight, that is a cell changing size by up to a fifth **in one tick**, and changing
/// back when its mass wanders across the threshold again. Measured on a settled pack: six cells
/// in a thousand step in any given tick, the worst by 22% — which across four thousand cells at
/// sixty ticks a second is on the order of a thousand pops a second, scattered over the sheet,
/// each one a cell suddenly overlapping its neighbours and then not. That is what a packed slide
/// looks like when it is flickering.
///
/// The same curve in floating point, which the front end is allowed. It agrees with `mm-core`'s
/// at every point the staircase touches and interpolates between them, so nothing moves on
/// average and no cell is drawn a size it could not be — it simply stops arriving there all at
/// once.
///
/// This is presentation only. The physics keeps the staircase: collision, contact and every
/// invariant still run on the integer radius, and must, or the picture would be of a different
/// world than the one being simulated.
#[must_use]
pub fn drawn_radius(mass: i32) -> f32 {
    let m = (mass as f32 / Q10_ONE as f32).max(0.0);
    0.25 + m.sqrt() * 0.125
}

/// The most a cell may be swollen to keep its area. See [`area_swell`].
const MAX_SWELL: f32 = 1.25;

/// How many directions the clipped area is measured along. See [`area_swell`].
const SWELL_RAYS: usize = 64;

/// How much of the area-preserving correction is applied. **One, and this is the record of why.**
///
/// The swell solve is a wildly sensitive function of its input: on a settled pack one tick apart,
/// 14% of cells change size by more than a percent and the worst by **eleven**, with their seam
/// sets unchanged — the whole outline rescaling because one neighbour moved a fraction of a
/// square. In a sheet whose gaps are far smaller than that, an eleven percent rescale is a lobe
/// appearing over a neighbour and going away again, which is what a packed slide looks like:
/// overlaps flickering on and off all over it.
///
/// Applying only part of the correction attenuates that, exactly and only in proportion:
///
/// | gain | cells resizing >1% | worst jump | how it looks |
/// |------|--------------------|------------|--------------|
/// | 1.0  | 141‰               | 0.109      | tiles like tissue |
/// | 0.8  | 118‰               | 0.087      | very slightly loose |
/// | 0.5  |  69‰               | 0.055      | open gaps everywhere, reads as pebbles |
///
/// **Which is why it is not the fix.** Sensitivity and fill are the same quantity scaled by the
/// same number, so the trade is one for one: the flicker is only tolerable at a gain where the
/// packing has visibly come apart, and the packing is the whole reason the swell exists. Left at
/// one, with the lever in place and measured, so nobody has to find this out twice.
///
/// The real fix is structural. The solve has **one** degree of freedom — a single scale on the
/// whole circle — for a constraint that is entirely local, so any local change has to be absorbed
/// by moving the entire outline, including the parts pointing at cells that had nothing to do
/// with it. Give each free arc its own correction and a local change stays local, at the same
/// fill and with no memory of the previous frame.
const SWELL_GAIN: f32 = 1.0;

/// How much larger a cell must be drawn for its *clipped* outline to enclose the area it has.
///
/// This is what separates a foam from a gravel pile, and it is the thing that was missing when
/// a packed crowd still read as a heap of pebbles with holes between them.
///
/// A cell is a bag of nearly incompressible fluid. Squeeze it and it does not lose volume; it
/// changes shape and bulges out wherever nothing is holding it in. Clipping alone models the
/// first half and not the second: every seam *removes* area, so a cell with six neighbours is
/// drawn as a hexagon distinctly smaller than the disc it started as, and the area it lost turns
/// into the gaps between cells. Real tissue has no gaps for exactly this reason.
///
/// So: hold the seams still and grow the circle until what survives the cutting is the area the
/// cell actually has. The seam planes do not move, which matters — they are the shared walls, and
/// both cells have to keep agreeing on them — so the growth goes entirely into the free arcs,
/// which is precisely where the gaps are.
///
/// Measured rather than derived. The clipped shape is a disc intersected with up to eight
/// half-planes, all of which contain the centre, so it is star-shaped about the centre and its
/// area is `½∫ρ(θ)²dθ` with `ρ(θ) = min(radius, min_k face_k / cos(θ - φ_k))`. Only the `radius`
/// term depends on the scale being solved for, so the per-seam part is computed once and the
/// solve is a bisection over a fixed array. Subtracting circular segments instead would have been
/// cheaper and wrong: segments overlap once a cell has more than about three neighbours, and
/// double-subtracting the overlaps says the cell has lost more area than it has.
///
/// Capped, because a cell enclosed on every side has no free arc to grow into and the solve would
/// otherwise run away.
///
/// Depends on nothing but this frame's seams — no feedback from the previous frame's swell — so
/// it cannot oscillate on its own account. It does *amplify* a seam appearing or disappearing,
/// because that resizes the whole cell rather than one edge of it.
/// Public because it is a named suspect and the shader bench runs it on cells no simulation
/// made — see [`crate::phantom`]. Pure arithmetic over plain data; it never sees a `World`.
pub fn area_swell(radius: f32, want_radius: f32, seams: &[Squash]) -> f32 {
    if seams.is_empty() || radius <= 0.0 {
        return 1.0;
    }
    // Distance to the nearest seam along each of `SWELL_RAYS` directions, ignoring the circle.
    //
    // The directions come from a table computed once rather than from `sin_cos` per ray per
    // cell. Same numbers — the table is built by the same call — but sixty-four transcendentals
    // per cell per frame was the largest single cost in building a frame, and a frame is built
    // on the simulation thread under the lock a tick takes.
    let mut reach = [f32::INFINITY; SWELL_RAYS];
    for (r, (sx, sy)) in reach.iter_mut().zip(ray_directions().iter()) {
        for s in seams {
            // `face` is a fraction of `radius`; the seam sits at `face * radius` along its own
            // normal. A ray pointing away from a seam is not limited by it.
            let along = sx * s.nx + sy * s.ny;
            if along > 1e-4 {
                *r = r.min((s.face * radius) / along);
            }
        }
    }
    let target = std::f32::consts::PI * want_radius * want_radius;
    // The scale that hits the target exactly, solved rather than searched for.
    //
    // The clipped area along the rays is `½ dθ Σ min(reach_j, r)²` with `r = radius · scale`,
    // which is a *piecewise quadratic* in `r`: as `r` grows, one ray at a time stops being capped
    // by the circle and starts being capped by its seam, and between two consecutive reach values
    // nothing changes shape. So sort the reaches, walk the brackets, and in whichever one
    // contains the answer invert the quadratic directly:
    //
    //     r² = (target / (½ dθ) − Σ_{reach ≤ r} reach²) / (however many rays are still uncapped)
    //
    // This replaces sixteen bisection steps, each of which summed all sixty-four rays. It is also
    // the *exact* root rather than one bracketed to a part in 65,536 — a bisection over a range of
    // 0.25 leaves about 4·10⁻⁶ of slack, and that slack was the only thing separating two cells'
    // idea of a shared wall from each other.
    let mut sorted = reach;
    sorted.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let half_dtheta = 0.5 * std::f32::consts::TAU / SWELL_RAYS as f32;
    let want = target / half_dtheta;

    let mut capped_area = 0.0f32; // Σ reach² over the rays already capped by their seam
    let mut r_star = f32::INFINITY;
    for k in 0..=SWELL_RAYS {
        let lower = if k == 0 { 0.0 } else { sorted[k - 1] };
        let upper = if k == SWELL_RAYS {
            f32::INFINITY
        } else {
            sorted[k]
        };
        let uncapped = (SWELL_RAYS - k) as f32;
        if uncapped > 0.0 {
            let needed = (want - capped_area) / uncapped;
            if needed >= 0.0 {
                let r = needed.sqrt();
                if r >= lower && r <= upper {
                    r_star = r;
                    break;
                }
            }
        } else if capped_area >= want {
            // Every ray is capped by a seam, so the area cannot grow any further. The cell is
            // enclosed and the answer is wherever it stopped.
            r_star = lower;
            break;
        }
        if k < SWELL_RAYS {
            // The next bracket up has this ray capped. An unreachable seam contributes an
            // infinite reach, which no finite `r` can pass, so the walk ends before it is added.
            if !sorted[k].is_finite() {
                break;
            }
            capped_area += sorted[k] * sorted[k];
        }
    }

    // The bottom of the range is *below* one, and deliberately. `radius` arrives already
    // inflated by `PACKING`, which exists only so that cells the physics leaves touching still
    // overlap on screen and have a seam to share. That inflation is a lie about the cell's size,
    // and an unclipped cell should not be told it: with the target set to the honest area, a cell
    // nothing is pressing on solves to `1 / PACKING` and is drawn at exactly the radius it has.
    //
    // Clamped to the same interval the search used to be confined to, which is where the cap at
    // `MAX_SWELL` and the floor at one both come from.
    let scale = (r_star / radius).clamp(1.0, MAX_SWELL);
    // Only part of the way. See `SWELL_GAIN`.
    1.0 + SWELL_GAIN * (scale - 1.0)
}

/// The `SWELL_RAYS` directions the clipped area is measured along, as `(cos, sin)`.
///
/// Built once. `area_swell` is called for every cell on camera at `Lod::Packed` and above, sixty
/// times a second, and it was calling `sin_cos` once per ray inside that — sixty-four
/// transcendentals per cell per frame, for a table of constants.
fn ray_directions() -> &'static [(f32, f32); SWELL_RAYS] {
    static RAYS: std::sync::LazyLock<[(f32, f32); SWELL_RAYS]> = std::sync::LazyLock::new(|| {
        let mut out = [(0.0, 0.0); SWELL_RAYS];
        for (j, slot) in out.iter_mut().enumerate() {
            let theta = std::f32::consts::TAU * j as f32 / SWELL_RAYS as f32;
            let (sy, sx) = theta.sin_cos();
            *slot = (sx, sy);
        }
        out
    });
    &RAYS
}

/// The one seam between a cell of drawn radius `radius` and a neighbour of drawn radius `other`,
/// whose centre is `(dx, dy)` away.
///
/// The whole scheme rests on this being computable from either side: swap the two radii, measure
/// from the far centre, and every step below gives `d` minus what it gives here. So two cells
/// arrive at one plane without talking to each other, and a shared wall has no gap and no doubled
/// edge. `None` only for centres too close together to have a direction at all.
///
/// Split out of [`squash_of`] so that the shader bench can compute seams for cells no simulation
/// made — see [`crate::phantom`]. Everything the plane depends on is in the arguments: there is
/// no `World`, no neighbour index and no contact set, which is what makes the bench's data
/// trustworthy where a frame's is exactly what is in doubt.
#[must_use]
pub fn seam_between(
    radius: f32,
    other: f32,
    dx: f32,
    dy: f32,
    rigidity: f32,
    other_rigidity: f32,
) -> Option<Squash> {
    let d = (dx * dx + dy * dy).sqrt();
    // Exactly coincident centres have no direction to be squashed along. Separation will pull
    // them apart next tick; until then they are simply drawn whole.
    if d <= f32::EPSILON {
        return None;
    }
    // The plane through the two points where the outlines cross:
    //   face = (d² + r² - other²) / 2d
    let face = (d * d + radius * radius - other * other) / (2.0 * d);
    // Then moved towards whichever of the two is softer.
    //
    // The plane on its own says two cells give way equally, and they do not: a cell that has paid
    // for a thick membrane holds its shape and one that has not gives in. So the seam slides along
    // the overlap in proportion to the difference, bounded by half of it — a firm cell pressed
    // against a soft one stays round and dents the other, and two cells of the same build still
    // meet in the middle.
    //
    // Both sides compute the same seam: the shift is antisymmetric in the two rigidities, so the
    // softer cell arrives at the same line from its own side and they still meet with no gap.
    let overlap = (radius + other - d).max(0.0);
    let firmness = (rigidity - other_rigidity) / (rigidity + other_rigidity).max(1.0);
    let face = face + 0.5 * overlap * firmness;
    // Then held off both cores — this one's and the neighbour's — as *one* interval rather than
    // one clamp per side.
    //
    // Clamping each cell's own face independently is what this replaced, and it broke the one
    // property the whole scheme rests on: that both cells arrive at the same plane. Whenever the
    // clamp bit, the two faces stopped summing to the distance between the centres and the pair
    // was drawn overlapping instead of sharing a wall. With cells of a size, that was rare. Now
    // that the physics presses crowds to their core it fires constantly on any mismatched pair —
    // the neighbour's face is the part that goes short, and nothing on this side could see it.
    //
    // Written as an interval, it is antisymmetric again: if this cell's face is pushed out to
    // `d - theirs`, the neighbour computing from its own side is pushed in to exactly `theirs`,
    // and the two still meet on one line.
    let my_core = MIN_FACE * radius;
    let their_core = MIN_FACE * other;
    let face = if my_core + their_core >= d {
        // No plane can respect both cores: the pair is closer than SPEC §6.4 should allow, which
        // happens for a tick after a division places a daughter inside its parent. Split the
        // distance between them in proportion instead — still one plane, still the same from both
        // sides.
        d * my_core / (my_core + their_core).max(f32::EPSILON)
    } else {
        face.clamp(my_core, d - their_core)
    };
    Some(Squash {
        nx: dx / d,
        ny: dy / d,
        face: face / radius,
    })
}

/// How much larger than its physical radius a cell of this firmness is drawn.
///
/// [`PACKING`] for a bag of fluid, one for a walled body, linear between. `phantom::Bench::packing`
/// is the same function and its documentation is the long version; the short one is that "a firm
/// cell should be cut less deeply" cannot be done by moving the seam, because the seam is the one
/// plane both cells of a pair arrive at independently and moving it for one of them draws the two
/// overlapping. What can move is the size each is drawn at: cross two circles barely and the plane
/// through the crossings barely cuts either.
///
/// Measured on a raft of phantom cells at the spacing the physics drives a pack to, this takes the
/// outline from 0.387 out of round — more angular than a square — to 0.167, between a hexagon and
/// a circle. The rest of the distance is *spacing*, which is the physics' to give.
///
/// **Clamped at one, and `phantom::Bench::packing` is deliberately not.** One is where the drawn
/// radius equals the radius `mm-core` says the cell has. Past it the cells do keep getting
/// rounder — measured, all the way to 0.043 out of round, which is a sphere — and they do it by
/// being drawn smaller than they are: at a firmness of two, **two thirds of the pairs the physics
/// has pressed together are drawn with clear air between them**. That is the mirror image of the
/// fault `docs/OVERLAPS.md` is about, and the worse kind for being flattering, because the picture
/// looks better and every crowding and packing result read off it is wrong.
/// `firmness_probe::the_renderer_never_draws_a_cell_smaller_than_it_is` holds the line.
pub fn packing_for(firmness: f32) -> f32 {
    PACKING + (1.0 - PACKING) * firmness.clamp(0.0, 1.0)
}

/// The seams a cell is flattened along, and how much it must swell to keep its area.
///
/// `radius` is the drawn radius, not the physical one — the seams have to be worked out at the
/// size the cell is actually going to appear, or they cut it somewhere other than where its
/// outline is.
///
/// The returned faces are fractions of the *swollen* radius, so that the seam planes stay exactly
/// where they were in absolute terms. Both cells of a pair must still agree on their shared wall,
/// and they compute it from the two unswollen radii — swelling is a thing each cell does to its
/// own free arcs, not to the walls it shares.
fn squash_of(world: &World, i: usize, radius: f32) -> (Vec<Squash>, f32) {
    if radius <= 0.0 {
        return (Vec::new(), 1.0);
    }
    let scale = 1.0 / POS_ONE as f32;
    let rates = &world.biology().metabolism.rates;
    // How much this cell gives way when another is pressed into it — slot zero's `param`, which
    // decides which of two cells the shared wall slides towards. Not the same thing as the
    // firmness below, which decides how *large* either is drawn.
    let give = world
        .cells()
        .slots(i)
        .first()
        .map_or(0.0, |m| m.param as f32);

    // The contact set once, not twice. It is walked for the seams and for how much of this cell
    // is glued, and it is the expensive thing on this path.
    let set = world
        .neighbours()
        .contacts(world.cells(), i, PACKING_PERMILLE, rates);
    let near = set.as_slice();

    // How firmly this cell holds its own shape: wall times turgor, both of which a genome pays
    // for. Raw, with no tissue term — see below for why the two firmnesses part company here.
    let own = mm_core::biology::rigidity(world.cells(), i, rates) as f32
        / mm_core::Q10_ONE as f32;

    // Size first, because the seams are cut at the size the cell is going to appear.
    //
    // A firm cell is drawn nearer its true radius and so crosses its neighbours less deeply, and
    // the plane through two barely-crossing circles barely cuts either. That is how "cut less
    // deeply" is done without moving the seam: moving the seam breaks the one property the whole
    // scheme rests on, since both cells of a pair must arrive at the same plane independently.
    //
    // **The tissue term is deliberately absent here and present in the swell.** Each cell has to
    // know how large its neighbour is drawn, and `Contact::firmness` carries exactly that — but a
    // neighbour's *glued* fraction would need its whole contact set walked, which is a second
    // neighbourhood scan per pair. Leaving it out is also the better answer: glue changes whether
    // a cell bulges into the one beside it, not how big it is.
    let mine = packing_for(own);
    let shrink = mine / PACKING;
    let bare = radius * shrink;

    let mut seams: Vec<Squash> = near
        .iter()
        .filter_map(|c| {
            let (dx, dy) = (c.dx as f32 * scale, c.dy as f32 * scale);
            // The neighbour's drawn radius, worked out the same way this cell's is — see
            // `drawn_radius`. Reading `c.radius` here instead is what a smoothed radius makes
            // wrong: this cell would use the smooth curve for itself and the staircase for its
            // neighbour, the neighbour would do the reverse, and the pair would compute two
            // different planes for one wall. The same now goes for its firmness.
            let theirs = drawn_radius(c.mass)
                * packing_for(c.firmness as f32 / mm_core::Q10_ONE as f32);
            seam_between(bare, theirs, dx, dy, give, c.rigidity as f32)
        })
        .collect();

    // Then grown until what survives the cutting is the area the cell has — but only as far as
    // this cell is the kind of thing that bulges.
    //
    // # Tissue or marbles
    //
    // A packed crowd of cells has two pictures and the engine should be able to draw both. A
    // moss leaf is a tessellation: cells flattened into polygons, sharing walls, no gaps
    // anywhere. A smear of yeast is a heap: pressed together just as hard, still obstinately
    // round, with gaps between them.
    //
    // **Is it stuck to its neighbours?** A tissue shares its walls because its cells are *glued*,
    // not because they are soft, and the engine has had the mechanism since M7. The fraction of a
    // cell's contacts that are joined is the fraction of it that is tissue, and tissue always
    // tessellates however rigid its cells are.
    //
    // **If it is free, is it firm?** A bag of fluid squeezed on one side bulges on the other; a
    // walled, turgid cell does not.
    //
    // The default falls out and is what matters most: the shipped ancestors join nothing and
    // build a membrane of 24 out of a possible 255, so they are about 9% firm, entirely free, and
    // drawn very nearly exactly as they always were.
    //
    // The per-*seam* version of this is not expressible — see `SWELL_GAIN`, which records the same
    // limitation from the other end: the solve has one degree of freedom, a single scale on the
    // whole circle. A cell half joined and half free is drawn half way between the two pictures
    // rather than being tissue on one side and a marble on the other.
    let contacts = near.len().max(1) as f32;
    let joined = near.iter().filter(|c| c.joined).count() as f32;
    let tissue = (joined / contacts).clamp(0.0, 1.0);
    let bulge = 1.0 - (own * (1.0 - tissue)).clamp(0.0, 1.0);
    let grown = 1.0 + bulge * (area_swell(bare, bare, &seams) - 1.0);

    // The faces are fractions of the *swollen* radius, so the planes stay where they were. Only
    // the growth divides out — the shrink is already in `bare`, which the faces were measured
    // against.
    for s in seams.iter_mut() {
        s.face /= grown;
    }

    // Both effects reach the shader through the one `swell` attribute it already has: it draws at
    // `radius * PACKING * swell`, so handing back the growth times the shrink is the same as
    // handing it a smaller `PACKING`. Nothing in `cellmesh` or `cell.wgsl` moves.
    (seams, grown * shrink)
}

/// One organelle, as it is drawn inside its cell.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct OrganelleDot {
    pub kind: mm_core::OrganelleType,
    /// Offset from the cell centre, in substrate squares.
    pub dx: f32,
    pub dy: f32,
    pub radius: f32,
    pub rgb: [f32; 3],
    /// `0..=1`. Scaffolding that is still being built is drawn faint.
    pub built: f32,
}

/// One thing a cell has grown that reaches **outside its own membrane**.
///
/// Six organelles in the catalogue do — cilium, flagellum, spike, holdfast, exoenzyme, and the
/// junction port through the links it anchors — and every one of them was drawn as a coloured dot
/// on the ring inside the cell, so nothing a cell built ever changed its silhouette. See
/// `docs/MORPHOLOGY.md`.
///
/// This is geometry only: where the limb starts, which way it points, how far it reaches and how
/// hard it is working. What each form actually looks like is `limb.wgsl`'s business, and how it
/// gets there is `limbmesh.rs`'s.
///
/// # What each field is allowed to say
///
/// Everything here comes from a named quantity in `mm-core` and nothing is invented, with two
/// exceptions that are labelled where they arise: the mount angle of a limb whose organelle has no
/// simulated direction (SPEC §6 — position within a cell is not state), and [`LimbDot::phase`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LimbDot {
    pub kind: mm_core::OrganelleType,
    /// Where it leaves the body: an offset from the cell centre, in substrate squares, on the
    /// cell's own *drawn* wall in the mount direction — cut back by the seams, so a limb on a
    /// squashed side starts where that side actually is.
    pub dx: f32,
    pub dy: f32,
    /// Outward unit direction.
    pub ux: f32,
    pub uy: f32,
    /// How far past the root it reaches, in squares.
    pub length: f32,
    /// Half the quad's span across the limb, in squares: **the widest the form ever reaches**,
    /// not the thickness of the thing drawn in it.
    ///
    /// The two differ for every form but the spike. A flagellum's whip is thin and its *wave* is
    /// wide; a cilium is a tuft of thin hairs spread across an arc. The shader owns those
    /// proportions — a hair is a fixed fraction of the quad, a wave's amplitude is another — and
    /// this owns the overall size, so a form can never reach outside the rectangle it is drawn in
    /// and be silently clipped square.
    pub width: f32,
    /// How far back *inside* the body the quad starts, in squares.
    ///
    /// The limb mesh is drawn under the cells, so the body covers this and there is no join to
    /// draw. Without it a limb meets the membrane along a hairline of background.
    pub inset: f32,
    /// How hard it is working, `-1..=1`.
    ///
    /// **Signed exactly where the control it comes from is signed.** A cilium beating backwards
    /// is a thing a genome can do and nothing in the picture could say so; a spike's extension
    /// clamps at zero, because SPEC §8 calls it "signed extension" and a retracted spike does
    /// nothing.
    pub extent: f32,
    /// Where in its beat it is, `0..1`. **A drawing convention** — nothing in the simulation has
    /// a beat phase. Derived from the tick and never from the clock, so a paused slide is still
    /// and a screenshot at tick N is reproducible.
    pub phase: f32,
    /// How many sub-elements the form draws: hairs in a tuft, rootlets on a foot, one otherwise.
    pub count: f32,
    /// The hollow fraction of a form that has one — the halo, and nothing else. `0` for a solid.
    pub inner: f32,
    /// Tip width as a fraction of the root's. `0` tapers to a point.
    pub taper: f32,
    /// Fixes whatever the form varies per limb. From the cell and the slot, so it is stable.
    pub seed: f32,
}

/// How much detail a frame carries, chosen by zoom (SPEC §14).
///
/// > instanced dots at far zoom, organelle-resolved sprites near, full membrane and junction
/// > rendering at maximum zoom.
///
/// The tier is a property of the *frame*, not of the renderer, because it decides how much
/// work `Slide::frame` does. A hundred thousand cells at whole-slide zoom must not each build
/// an organelle list that will be drawn as one pixel.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum Lod {
    /// Instanced points. One draw call, no per-cell detail.
    #[default]
    Dots,
    /// Cells cut against their neighbours: shared walls, no contents.
    Packed,
    /// Organelle-resolved sprites.
    Organelles,
    /// Membranes, organelles and junctions.
    Full,
}

impl Lod {
    /// Which tier a zoom level calls for.
    ///
    /// The thresholds are in pixels per substrate square, which is the only unit that says
    /// anything about whether a thing is visible. Multiply by 12.5 for the magnification the
    /// status bar reports, which is what you are actually reading when you decide a tier
    /// arrives too early: 6 is 75x, 28 is 350x, 48 is 600x.
    ///
    /// # Why packing and contents are separate tiers
    ///
    /// They used to arrive together, and a single step took the picture from rounded cells
    /// floating unaligned to shared walls *and* a confetti of organelles in the same frame.
    /// Both halves were wrong at that zoom. The packing was wrong below it — cells that are
    /// touching are drawn overlapping, which is the thing the seams exist to fix and it is
    /// visible long before an organelle is. The organelles were wrong above it: at twelve
    /// pixels per square an organelle is two or three pixels, so a crowd reads as coloured
    /// speckle laid over the cells rather than as anything inside them, and it hides the
    /// packing structure that had just become legible.
    ///
    /// So the seams come in as soon as they read and the contents wait until there is a cell
    /// big enough to have an inside. There is a wide band between them where the slide is a
    /// sheet of plain tiled cells, and that is the most legible view of a large population
    /// there is.
    ///
    /// Packing starts at six rather than the twelve the organelles used to share, because
    /// overlap is visible far earlier than an organelle is. A cell is about two squares across,
    /// so six pixels per square still puts twelve pixels on a cell and three or four on a
    /// flattened side — and *un*packed at that size does not read as small, it reads as a heap
    /// of loose discs, which is a wrong picture rather than a coarse one. The floor is where a
    /// cell stops being wide enough to have a side at all.
    #[must_use]
    pub fn for_pixels_per_square(pixels: f32) -> Lod {
        if pixels >= 48.0 {
            Lod::Full
        } else if pixels >= 28.0 {
            Lod::Organelles
        } else if pixels >= 6.0 {
            Lod::Packed
        } else {
            Lod::Dots
        }
    }

    /// Whether this tier cuts cells against their neighbours.
    ///
    /// Cheaper than it looks and worth having early: it is the neighbour walk and a handful of
    /// half-planes per cell, and without it a packed crowd is drawn as overlapping discs.
    #[must_use]
    pub fn resolves_packing(self) -> bool {
        self >= Lod::Packed
    }

    /// Whether this tier draws individual organelles.
    #[must_use]
    pub fn resolves_organelles(self) -> bool {
        self >= Lod::Organelles
    }
}

/// One frame's worth of world, with no way back to it.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub tick: u64,
    pub width: u32,
    pub height: u32,
    pub cells: Vec<CellDot>,
    /// Every chemical overlay currently switched on, in chemical order. Each carries its own
    /// colour and its own peak, which is what the legend needs: an overlay normalised against
    /// its own maximum is legible but meaningless without the number it was divided by.
    pub overlays: Vec<OverlayLayer>,
    /// Incident light, normalised. Rendered as a warm luminance layer (SPEC §14).
    pub light: Vec<f32>,
    /// The prescribed flow plus whatever cilia have stirred into it, coarsened to one sample
    /// per [`FLOW_STRIDE`] squares each way, in squares per fluid step.
    ///
    /// Present when the arrows are switched on *or* when there is particulate to carry, since
    /// the specks are advected by it — so this being non-empty does not mean the overlay was
    /// asked for. [`Frame::flow_shown`] is that question; drawing arrows off this field alone
    /// is how the overlay came to be on whether or not it was switched on.
    ///
    /// Coarsened rather than carried whole because the full field is two `i32` planes — two
    /// megabytes a frame at 512×512 — to draw a few hundred arrows from. Averaged over each
    /// block rather than point-sampled, so an arrow is what the water in that block is doing
    /// and not what one square of it happens to be doing.
    pub flow: Vec<[f32; 2]>,
    /// Whether the flow overlay was switched on when this frame was built.
    ///
    /// The renderer's gate for the arrows. Carried on the frame rather than read from the
    /// engine at draw time so that the arrows and the field they are drawn from are the same
    /// tick's answer.
    pub flow_shown: bool,
    /// Samples across, so the renderer can index `flow` without recomputing the division.
    pub flow_cols: u32,
    /// Detritus on the same lattice as `flow`, as a fraction of the busiest block.
    ///
    /// The suspended particulate, for drawing it as the swarm of specks it looks like rather
    /// than as a colour wash. Normalised against the frame's own peak, like an overlay layer,
    /// because what the picture is saying is *where* the particulate is rather than how much of
    /// it there is — the budget pane is where the number lives.
    pub detritus: Vec<f32>,
    /// How much of the water's speed the particulate actually travels at, `0..1`.
    ///
    /// Detritus is coupled to the flow at a fraction of it — that lag is what makes it
    /// particulate rather than dissolved — so specks drawn at the water's speed would be a
    /// picture of the wrong thing, and would say the current carries food faster than it does.
    pub detritus_drift: f32,
    /// Carrion on the same lattice as `flow`, as a fraction of the busiest block.
    ///
    /// Drawn as slow brown flakes rather than as a stipple of specks, because it is not the same
    /// stuff: detritus is what a body has already broken down into and carrion is the body, and
    /// a picture that drew them alike would be saying the decay chain has one stage. See
    /// `art::fleck`.
    pub carrion: Vec<f32>,
    /// How much of the water's speed carrion travels at, `0..1`. Lower than the particulate's —
    /// a corpse is the least mobile thing in the chemical table.
    pub carrion_drift: f32,
    /// The sources and drains in force, so the view can show where they are.
    ///
    /// A source is an area of water that behaves differently and has nothing to see until it has
    /// filled up, so an unmarked one is a thing you placed and can never find again.
    pub flux: Vec<mm_core::Flux>,
    /// The barrier mask, one entry per square, or empty on a slide with no barriers.
    ///
    /// Until this existed the renderer was never told where the walls were, so a barrier was
    /// visible only as an *absence*: `set_blocked` evicts the square's chemistry to its
    /// neighbours and the light regime shadows behind it, which reads as a dark patch and is
    /// indistinguishable from a square that merely has nothing in it. On an unlit slide — the
    /// vent, or a night — it read as nothing at all. A wall is a thing, and drawing it as a
    /// hole is why drawing one did not feel like drawing one.
    ///
    /// Empty rather than all-false when there are no barriers, so a slide without them pays
    /// neither the copy nor the per-texel branch.
    pub barriers: Vec<bool>,
    /// What each square's solid mineral looks like, or empty on a slide with none.
    ///
    /// A colour rather than the amounts, because resolving it needs the chemical table and this
    /// is the side of the wall that has one. Zero means the square holds no solid — which for a
    /// blocked square means it is plain immutable rock, and for an open one means nothing at all.
    pub mineral: Vec<[f32; 3]>,
    pub population: usize,
    /// Detail tier this frame was built at.
    pub lod: Lod,
    /// Dust on the objective. Presentation only — see [`crate::optics`].
    pub motes: Vec<crate::optics::Mote>,
    /// Junctions, present only at [`Lod::Organelles`] and above — at whole-slide zoom a
    /// junction is shorter than a pixel and there may be fifty thousand of them.
    pub junctions: Vec<JunctionLine>,
    /// The largest organism on the slide, in cells.
    pub largest_cluster: u32,
}

/// One junction, as it is drawn.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct JunctionLine {
    /// Both ends, in substrate squares.
    pub from: (f32, f32),
    pub to: (f32, f32),
    /// Hard junctions are structure and are drawn solid; soft ones are channels and are drawn
    /// faint, because one is a body and the other is a conversation.
    pub hard: bool,
}

/// One chemical field, ready to draw.
#[derive(Clone, PartialEq, Debug)]
pub struct OverlayLayer {
    pub chemical: usize,
    pub name: String,
    pub rgb: [f32; 3],
    /// Per-square concentration normalised against `scale`, clamped to `0..=1`.
    ///
    /// Clamped rather than guaranteed in range: `scale` is a high quantile and not the maximum,
    /// so the squares above it saturate at the top of the ramp. That is the trade — see
    /// [`SCALE_QUANTILE`] for what it buys and how many squares it costs.
    pub field: Vec<f32>,
    /// What `field` was divided by, in `Q10` units. The top of the colour ramp, and the legend's
    /// number.
    ///
    /// **Not the maximum**, and eased between frames rather than recomputed: see
    /// [`SCALE_QUANTILE`] and [`SCALE_EASE`].
    pub scale: i32,
    /// Total of this chemical in the fluid, for the legend's readout.
    pub total: i64,
}

/// How many squares each flow sample covers, each way.
///
/// Four. The arrows are drawn on a lattice and want to be sparse enough to read as a field
/// rather than as a hedge; four squares is finer than any spacing the renderer actually draws
/// at, so the renderer is free to skip samples for the screen density it wants and never has
/// to interpolate between them.
pub const FLOW_STRIDE: u32 = 4;

/// The simulation, and the only thing the front-end is allowed to hold.
pub struct Slide {
    world: World,
    /// Which chemical overlays are switched on. Individually toggleable (M4), so this is a
    /// set and not a choice.
    overlays: [bool; CHEM_COUNT],
    /// Whether to gather the flow field into the frame. Off by default: it is an instrument
    /// reading rather than part of the picture, and gathering it costs a pass over the grid.
    pub show_flow: bool,
    /// Detail tier the next frame will be built at.
    lod: Lod,
    /// The microscope's look.
    pub optics: crate::optics::Optics,
    /// What the camera can see, in substrate squares: centre and half-extents.
    ///
    /// Infinite until someone says otherwise, so a headless run — which has no camera — builds
    /// every cell in full, and so does anything that forgets to set one. Detail that goes
    /// missing by default is a bug that hides.
    camera: (f32, f32, f32, f32),
    /// Rolling history for the live plots.
    history: MetricHistory,
    /// Trophic flows over the last complete window, and the one still filling (M8).
    flows: crate::foodweb::Flows,
    flows_filling: crate::foodweb::Flows,
    /// The top of each overlay's colour ramp, carried between frames so it can be eased into
    /// rather than recomputed from scratch. `0` means this layer has no exposure yet and the
    /// next frame should take the reading outright. See [`Slide::overlay_scale`].
    overlay_scale: [f32; CHEM_COUNT],
}

/// How many ticks the food web averages over.
///
/// Long enough that a single tick's births and deaths do not make the arrows twitch, short
/// enough that a shift in the ecosystem shows up while the user is still looking at it.
const FLOW_WINDOW_TICKS: u64 = 600;

/// Where the top of an overlay's colour ramp sits in its own distribution.
///
/// **Not the maximum, and that is the whole point.** An overlay is normalised against a
/// statistic of its plane, so the entire picture's brightness is that statistic's reciprocal:
/// when it moves, every square on the slide changes shade at once, and what is seen is the field
/// flickering when it is actually the ruler. The maximum is the worst possible choice for this,
/// because it is decided by *one square out of a quarter of a million* — a cell dying and
/// dumping its body into the square it occupied moves it, and the whole slide flashes.
///
/// Measured on a settled 128² slide with 3,586 cells, one reading per tick over 400 ticks
/// (`tests/overlay_scale.rs`):
///
/// | statistic | worst step between ticks | mean step |
/// | --- | ---: | ---: |
/// | maximum | 43.8% | 5.36% |
/// | 99.9th | 3.8% | 0.31% |
/// | 99th | 0.2% | 0.03% |
/// | 95th | 0.0% | 0.01% |
///
/// A 43.8% jump in the divisor is a 17% jump in the brightness of every texel on the slide,
/// after the square-root curve — once per tick, at whatever the tick rate happens to be.
///
/// 99.9th rather than 99th because the two are not competing on steadiness alone: everything
/// above the mark saturates, and the 99th clips ten times as many squares. At 512² the 99.9th
/// flattens 262 squares out of 262,144, and its residual 3.8% is handed to [`SCALE_EASE`].
const SCALE_QUANTILE: f32 = 0.999;

/// How much of the way the ramp moves towards a new reading each frame.
///
/// The second half of the answer, and the half that handles the honest movement rather than the
/// noise: a bloom eating its way through the carbon really does change what the scale should be,
/// and it should arrive as a fade rather than as a step.
///
/// An eighth, so a reading settles over roughly twenty frames — a third of a second at 60fps.
/// Per *published* frame rather than per tick, deliberately: this is an exposure control on
/// something a person is looking at, so its time constant belongs in the units of what they see.
/// A frame is built only when the renderer has taken the last one ([`crate::engine`]), which
/// makes one step here exactly one displayed image.
const SCALE_EASE: f32 = 0.125;

/// How many buckets the quantile is estimated over.
///
/// A quantile normally wants a sort, and [`Slide::frame`] runs on the simulation thread under
/// the same lock a tick takes — a frame that costs 20ms is 20ms the world is not being stepped
/// in (`tests/frame_cost.rs`). Sorting a quarter of a million `i32`s per overlay per frame is not
/// affordable and neither is the megabyte of scratch it would want.
///
/// A histogram is one extra sequential pass and no allocation, and the error it admits is
/// bounded by one bucket width — a five-hundredth of the maximum, against a statistic that is
/// then eased by [`SCALE_EASE`] anyway.
const SCALE_BUCKETS: usize = 512;

/// The value `share` of the way up a plane, to within a bucket, without sorting it.
///
/// Returns `0` for an empty or entirely-zero plane, which the caller reads as "no exposure".
fn quantile_of(plane: &[i32], share: f32) -> i32 {
    let max = plane.iter().copied().max().unwrap_or(0);
    if max <= 0 || plane.is_empty() {
        return 0;
    }
    let last = (SCALE_BUCKETS - 1) as i64;
    let mut hist = [0u32; SCALE_BUCKETS];
    for &v in plane {
        let bucket = (i64::from(v.max(0)) * last / i64::from(max)) as usize;
        hist[bucket.min(SCALE_BUCKETS - 1)] += 1;
    }
    // How many squares must be at or below the mark. `ceil`, so `share` of 1.0 asks for all of
    // them and lands on the maximum rather than one bucket short of it.
    let want = (plane.len() as f32 * share).ceil() as u32;
    let mut seen = 0u32;
    for (b, count) in hist.iter().enumerate() {
        seen += count;
        if seen >= want {
            // The top of the bucket the mark fell in, so the value returned is one a square
            // could actually hold rather than the bottom of the band it sits in.
            let edge = (b as i64 + 1) * i64::from(max) / last;
            return edge.min(i64::from(max)) as i32;
        }
    }
    max
}

impl Slide {
    /// # Errors
    ///
    /// A scenario this engine cannot honour.
    pub fn new(scenario: Scenario) -> Result<Slide, mm_core::ScenarioError> {
        let mut overlays = [false; CHEM_COUNT];
        // Carbon dioxide by default: it is what the ancestor breathes out, so it is the layer
        // that first shows there is something alive on the slide.
        // `MM_NO_OVERLAY` turns it off, which is a debugging flag and earned its place the day
        // it was added: the packing appeared to be leaving mauve wedges between cells, and mauve
        // is both what a lightly-coloured cell looks like and what the carbon-dioxide overlay
        // paints the background. With the overlay off the wedges came out black, which settled
        // in one run whether they were thin cells or holes. They are holes.
        if let Some(on) = overlays.get_mut(11) {
            *on = std::env::var("MM_NO_OVERLAY").is_err();
        }
        Ok(Slide {
            world: World::new(scenario)?,
            flows: crate::foodweb::Flows::default(),
            flows_filling: crate::foodweb::Flows::default(),
            overlay_scale: [0.0; CHEM_COUNT],
            overlays,
            show_flow: false,
            lod: Lod::Dots,
            // Off to begin with, and on from the View menu.
            //
            // `Optics::default()` is still the full look and still what the type means by a
            // microscope; this is only what the slide *opens* on. The optics are a photograph of
            // the thing rather than the thing — vignette, depth of field, dust — and they read
            // over the top of the one signal the picture is actually carrying, which is what each
            // cell is made of. Two cells of the same colour at different depths do not look like
            // two cells at different depths until you already know that is what you are seeing;
            // they look like two kinds of cell. Better to open on the honest view and let someone
            // reach for the microscope once they know what they are looking at.
            optics: crate::optics::Optics::flat(),
            camera: (0.0, 0.0, f32::INFINITY, f32::INFINITY),
            history: MetricHistory::new(600),
        })
    }

    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Advance by exactly this many ticks.
    ///
    /// Takes a tick count and nothing else. Not a delta time: a simulation whose step depended
    /// on how long the last frame took would produce a different world on a fast machine than
    /// on a slow one, and every guarantee in the spec rests on it not doing that.
    pub fn advance(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.world.step();
            self.history.maybe_sample(&self.world);
            self.accumulate_flows();
        }
    }

    /// Fold this tick's flows into the food web's window, rolling it over when it is full.
    ///
    /// The window is what stops the web reading as a run-long average that stops moving after
    /// the first ten thousand ticks. It is a display average and nothing reads it back, so a
    /// run that never opens the panel produces exactly the same world as one that does.
    fn accumulate_flows(&mut self) {
        let report = self.world.report();
        self.flows_filling.accumulate(&report);
        if self.flows_filling.ticks >= FLOW_WINDOW_TICKS {
            self.flows = self.flows_filling;
            self.flows_filling.reset();
        }
    }

    /// The food web as of the last complete window.
    ///
    /// Falls back to the window still filling, so the panel says something on a young run
    /// rather than sitting empty for its first six hundred ticks.
    #[must_use]
    pub fn food_web(&self) -> crate::foodweb::FoodWeb {
        let flows = if self.flows.ticks > 0 {
            &self.flows
        } else {
            &self.flows_filling
        };
        crate::foodweb::web(TrophicMix::of(self.world.cells()), flows)
    }

    /// Move one layer's colour ramp towards what this plane wants, and return where it now is.
    ///
    /// Snaps rather than eases when there is no exposure yet — a layer being switched on, or a
    /// world just loaded. A ramp that faded up from nothing over its first twenty frames would
    /// make every overlay open on a flash of black, which is a worse artefact than the one this
    /// is here to remove.
    fn ease_scale(held: &mut f32, plane: &[i32]) -> i32 {
        let want = quantile_of(plane, SCALE_QUANTILE).max(1) as f32;
        *held = if *held <= 0.0 {
            want
        } else {
            *held + (want - *held) * SCALE_EASE
        };
        // At least one, because it is about to be divided by.
        (held.round() as i64).clamp(1, i64::from(i32::MAX)) as i32
    }

    /// Forget every layer's exposure, so the next frame takes its reading outright.
    ///
    /// Anything that replaces what is on the slide has to call this. Easing is only meaningful
    /// between two frames of the *same* world; carried across a load it would fade the new slide
    /// in from the old one's brightness, which looks like the file taking a moment to settle.
    fn forget_overlay_scale(&mut self) {
        self.overlay_scale = [0.0; CHEM_COUNT];
    }

    /// Show this chemical's overlay and no other. The number keys.
    pub fn set_overlay(&mut self, chemical: usize) {
        self.overlays = [false; CHEM_COUNT];
        if let Some(on) = self.overlays.get_mut(chemical % CHEM_COUNT) {
            *on = true;
        }
        self.forget_overlay_scale();
    }

    /// Switch one chemical's overlay on or off without disturbing the others.
    pub fn toggle_overlay(&mut self, chemical: usize) {
        if let Some(on) = self.overlays.get_mut(chemical % CHEM_COUNT) {
            *on = !*on;
            // This one only. The others are still showing and their exposure is still good;
            // resetting them would flash every open layer whenever one was toggled.
            self.overlay_scale[chemical % CHEM_COUNT] = 0.0;
        }
    }

    /// Every overlay's state as one bit per chemical.
    ///
    /// So the simulation thread can be handed the renderer's overlay choices in a single atomic
    /// rather than through the world's lock — see [`crate::engine`] for why a menu click must
    /// not wait for a tick.
    #[must_use]
    pub fn overlay_mask(&self) -> u32 {
        let mut mask = 0u32;
        for (i, on) in self.overlays.iter().enumerate() {
            if *on {
                mask |= 1u32 << i;
            }
        }
        mask
    }

    pub fn set_overlay_mask(&mut self, mask: u32) {
        for (i, on) in self.overlays.iter_mut().enumerate() {
            let now = mask & (1u32 << i) != 0;
            // Only what changed, and only on the way *on*. This is called every frame with the
            // renderer's current choices, so resetting unconditionally would hold every layer at
            // "no exposure" forever and there would be no easing at all.
            if now != *on {
                self.overlay_scale[i] = 0.0;
            }
            *on = now;
        }
    }

    #[must_use]
    pub fn overlay_enabled(&self, chemical: usize) -> bool {
        self.overlays
            .get(chemical % CHEM_COUNT)
            .copied()
            .unwrap_or(false)
    }

    /// The lowest-numbered overlay currently on, if any.
    #[must_use]
    pub fn overlay(&self) -> Option<usize> {
        self.overlays.iter().position(|on| *on)
    }

    /// Choose the detail tier from how many pixels a substrate square occupies.
    /// Where the camera is looking, in substrate squares, and how much it can see.
    ///
    /// Only the *expensive* per-cell work is skipped outside it — seams and the organelle list.
    /// Every cell is still in the frame, wherever it is, with its position, size and colour.
    ///
    /// That distinction is the whole design. Dropping the cell entirely would be cheaper and
    /// would show as a hole: the frame in flight was built for wherever the camera was when it
    /// was published, so panning would reveal a band of empty water at the leading edge until
    /// the next frame caught up — and at four frames a second that is a quarter of a second of
    /// missing world, arriving exactly when the user is looking hardest. Skipping only the
    /// detail degrades to a cell drawn as a plain blob for one frame instead, which is a look
    /// the renderer already has: it is what the `Dots` tier draws, and this is that tier's
    /// decision made per cell rather than per frame.
    pub fn set_camera(&mut self, x: f32, y: f32, half_w: f32, half_h: f32) {
        self.camera = (x, y, half_w.max(0.0), half_h.max(0.0));
    }

    pub fn set_zoom(&mut self, pixels_per_square: f32) {
        self.lod = Lod::for_pixels_per_square(pixels_per_square);
    }

    #[must_use]
    pub fn lod(&self) -> Lod {
        self.lod
    }

    /// The rolling metric history, for the live plots.
    #[must_use]
    pub fn history(&self) -> &MetricHistory {
        &self.history
    }

    /// Put a different world on the slide, and throw away everything derived from the old one.
    ///
    /// The derived state is the point. `history`, `flows` and `flows_filling` are all summaries
    /// of a world that no longer exists, and assigning through `world_mut` leaves every one of
    /// them in place — so opening a scenario left the metrics rail plotting the population of
    /// the slide you had just replaced, complete with its curve, and the food web describing
    /// flows between species that were gone. Reseeding and the packing bench did the same, and
    /// nobody noticed because those keep the slide's *size*, so a stale population curve looks
    /// like a continuous one.
    ///
    /// Not `Default`-ing the whole `Slide`: the overlays, the optics, the camera and the flow
    /// toggle are the *viewer's* settings and belong to the person, not to the world. Opening a
    /// scenario must not switch off the overlay you were watching.
    pub fn set_world(&mut self, world: World) {
        self.world = world;
        self.history = MetricHistory::new(self.history.capacity());
        self.flows = crate::foodweb::Flows::default();
        self.flows_filling = crate::foodweb::Flows::default();
        self.forget_overlay_scale();
    }

    /// A reading of one cell, for the inspector panel.
    #[must_use]
    pub fn inspect(&self, id: mm_core::CellId) -> Option<crate::inspector::Inspection> {
        crate::inspector::Inspection::of(&self.world, id)
    }

    /// Chemical names from the scenario's table, in index order.
    ///
    /// So a panel can say "carbon_dioxide 6.00" instead of "11: 6.00". The table is authored
    /// per scenario (SPEC §7.1), so the names have to come from the world rather than from a
    /// list in the front-end that would be wrong for any scenario that posed its own
    /// chemistry.
    #[must_use]
    pub fn chemical_names(&self) -> Vec<String> {
        (0..CHEM_COUNT)
            .map(|c| self.world.scenario().chemicals.get(c).name.clone())
            .collect()
    }

    /// Each chemical's overlay colour, in index order.
    ///
    /// From the scenario for the same reason the names are: the table is authored per scenario
    /// (SPEC §7.1), so a colour written down in the front end would be the wrong colour for any
    /// world that posed its own chemistry.
    #[must_use]
    pub fn chemical_colours(&self) -> Vec<[u8; 3]> {
        (0..CHEM_COUNT)
            .map(|c| self.world.scenario().chemicals.get(c).colour)
            .collect()
    }

    /// What the wiki calls this species, or a placeholder if the archive has not named it yet.
    #[must_use]
    pub fn species_name(&self, species: u32) -> String {
        self.world
            .archive()
            .get(species)
            .map(|s| s.name.to_string())
            .unwrap_or_else(|| format!("species {species}"))
    }

    /// Take a frame. Read-only *of the world*: nothing here can reach the simulation.
    ///
    /// `&mut self` for the overlay exposure alone ([`Slide::overlay_scale`]), which is carried
    /// between frames so the colour ramp can be eased rather than recomputed from nothing each
    /// time. It is presentation state, like the camera and the detail tier beside it; the world
    /// is still only read, which is what M4's guarantee is about and what
    /// `a_watched_world_matches_a_headless_one` checks.
    #[must_use]
    pub fn frame(&mut self) -> Frame {
        // Destructured so the borrow checker can see that the exposure being written and the
        // world being read are different fields. The obvious spelling clones the chemical
        // table to get out of the way, which is sixteen `String` allocations a frame for a
        // borrow that was always disjoint.
        //
        // Scoped, so the split borrow ends here and the rest of the frame can read the world
        // the ordinary way.
        let overlays: Vec<OverlayLayer> = {
            let Slide {
                world,
                overlays: shown,
                overlay_scale,
                ..
            } = self;
            let table = &world.scenario().chemicals;
            (0..CHEM_COUNT)
                .filter(|c| shown[*c])
                .map(|c| {
                    let plane = world.substrate().chem_plane(c);
                    // Normalised against a statistic of the frame rather than a fixed scale, so
                    // a nearly empty slide is still legible. That means the overlay's absolute
                    // meaning changes between frames, which is why `scale` travels with the
                    // layer and the legend shows it.
                    let scale = Self::ease_scale(&mut overlay_scale[c], plane);
                    let def = table.get(c);
                    OverlayLayer {
                        chemical: c,
                        name: def.name.to_string(),
                        rgb: [
                            def.colour[0] as f32 / 255.0,
                            def.colour[1] as f32 / 255.0,
                            def.colour[2] as f32 / 255.0,
                        ],
                        field: plane
                            .iter()
                            .map(|v| (*v as f32 / scale as f32).clamp(0.0, 1.0))
                            .collect(),
                        scale,
                        total: plane.iter().map(|v| i64::from(*v)).sum(),
                    }
                })
                .collect()
        };
        let substrate = self.world.substrate();
        let cells = self.world.cells();
        let table = &self.world.scenario().chemicals;

        let light: Vec<f32> = substrate
            .light()
            .iter()
            .map(|v| (*v as f32 / Q10_ONE as f32).clamp(0.0, 1.0))
            .collect();

        // Detritus on the flow lattice, and the flow with it.
        //
        // The particulate is gathered first and decides whether the flow is: the specks need
        // both — the concentration says where to draw them and the velocity says which way they
        // go — so a slide with particulate on it pays for the velocity samples whether or not
        // the arrows are switched on, and a slide with none pays for neither. One pass over one
        // plane also answers "is there any", which saves scanning for it separately.
        let (w, h) = (substrate.width(), substrate.height());
        let cols = w.div_ceil(FLOW_STRIDE);
        let rows = h.div_ceil(FLOW_STRIDE);
        // `present` is the fluid solver's own per-plane emptiness flag, and it is what keeps
        // this free on every slide that has no particulate on it: without it the gather is a
        // full pass over a quarter of a million squares each published frame, paid by everyone,
        // to discover that the answer is nothing.
        // One chemical gathered onto the flow lattice, normalised against its own busiest
        // block. Written once and called twice: the particulate and the dead are drawn the same
        // way and differ only in what they are drawn *as*, and a second copy of this loop would
        // be a second place for the `present` short-circuit to be forgotten.
        let gather = |chemical: usize| -> Vec<f32> {
            // `present` is the fluid solver's own per-plane emptiness flag, and it is what keeps
            // this free on every slide that has none of the chemical: without it the gather is a
            // full pass over a quarter of a million squares each published frame, paid by
            // everyone, to discover that the answer is nothing.
            if !substrate.present()[chemical] {
                return Vec::new();
            }
            let plane = substrate.chem_plane(chemical);
            let mut out = Vec::with_capacity((cols * rows) as usize);
            let mut peak = 0i64;
            for by in 0..rows {
                for bx in 0..cols {
                    let mut sum = 0i64;
                    for y in by * FLOW_STRIDE..((by + 1) * FLOW_STRIDE).min(h) {
                        for x in bx * FLOW_STRIDE..((bx + 1) * FLOW_STRIDE).min(w) {
                            sum += i64::from(plane.get((y * w + x) as usize).copied().unwrap_or(0));
                        }
                    }
                    peak = peak.max(sum);
                    out.push(sum as f32);
                }
            }
            if peak == 0 {
                return Vec::new();
            }
            for v in &mut out {
                *v /= peak as f32;
            }
            out
        };
        let detritus: Vec<f32> = gather(mm_core::ecology::DETRITUS);
        let carrion: Vec<f32> = gather(mm_core::ecology::CARRION);

        let (flow, flow_cols) = if self.show_flow || !detritus.is_empty() || !carrion.is_empty()
        {
            let (svx, svy) = substrate.velocity();
            let mut out = Vec::with_capacity((cols * rows) as usize);
            for by in 0..rows {
                for bx in 0..cols {
                    let (mut sx, mut sy, mut n) = (0i64, 0i64, 0i64);
                    for y in by * FLOW_STRIDE..((by + 1) * FLOW_STRIDE).min(h) {
                        for x in bx * FLOW_STRIDE..((bx + 1) * FLOW_STRIDE).min(w) {
                            let i = (y * w + x) as usize;
                            sx += i64::from(svx.get(i).copied().unwrap_or(0));
                            sy += i64::from(svy.get(i).copied().unwrap_or(0));
                            n += 1;
                        }
                    }
                    let d = n.max(1) as f32 * Q10_ONE as f32;
                    out.push([sx as f32 / d, sy as f32 / d]);
                }
            }
            (out, cols)
        } else {
            (Vec::new(), 0)
        };

        let barriers: Vec<bool> = if substrate.has_barriers() {
            substrate.blocked().to_vec()
        } else {
            Vec::new()
        };

        // What each square of rock is *made of*, resolved to a colour here rather than in the
        // renderer, because this is where the chemical table is and the table is where a
        // chemical's colour lives. A silica reef comes out pale blue-grey and a phosphate outcrop
        // yellow-brown, and the picture says what the rock is without a legend.
        //
        // Empty on a slide with no solid anywhere, so the ordinary world pays neither the walk
        // nor the copy — the same bargain `barriers` itself makes.
        let mineral: Vec<[f32; 3]> = {
            let any = (0..mm_core::chem::SOLID_COUNT)
                .any(|k| substrate.solid_plane(k).iter().any(|v| *v > 0));
            if !any {
                Vec::new()
            } else {
                let table = &self.world().scenario().chemicals;
                let mut out = vec![[0.0f32; 3]; substrate.len()];
                for (k, c) in mm_core::chem::SOLID_CHEMICALS.iter().enumerate() {
                    let rgb = table.get(*c).colour;
                    for (i, held) in substrate.solid_plane(k).iter().enumerate() {
                        if *held <= 0 {
                            continue;
                        }
                        // Weighted by how much is there, so a square holding both reads as the
                        // mixture rather than as whichever chemical was looked at last.
                        let w = *held as f32;
                        for j in 0..3 {
                            out[i][j] += w * (rgb[j] as f32 / 255.0);
                        }
                    }
                }
                // Normalised per square by its own total, so the colour says *composition* and
                // the brightness does not double as a reading of how much rock is there — a thin
                // crust and a deep bed of the same mineral are the same stuff.
                let mut totals = vec![0.0f32; substrate.len()];
                for k in 0..mm_core::chem::SOLID_COUNT {
                    for (i, held) in substrate.solid_plane(k).iter().enumerate() {
                        totals[i] += (*held).max(0) as f32;
                    }
                }
                for (i, t) in totals.iter().enumerate() {
                    if *t > 0.0 {
                        for j in 0..3 {
                            out[i][j] /= *t;
                        }
                    }
                }
                out
            }
        };

        // Two questions, not one. Cutting cells against their neighbours starts a long way
        // before there is a cell big enough to have a visible inside — see [`Lod`].
        let packed = self.lod.resolves_packing();
        let detailed = self.lod.resolves_organelles();
        // The only clock a limb is allowed to have. See `limb_phase`.
        let tick = self.world.tick_count();

        // Components, for colouring a cluster as one thing. Rebuilt here rather than kept,
        // because they are derived from the junctions and a stale copy would draw an organism
        // that came apart three ticks ago.
        let mut components = mm_core::junction::Components::new();
        components.rebuild(cells);
        let largest_cluster = components.largest();

        let mut junctions: Vec<JunctionLine> = Vec::new();
        if detailed {
            for i in cells.iter() {
                for j in cells.junctions(i) {
                    let Some(other) = cells.index(j.other) else {
                        continue;
                    };
                    // Drawn once per pair, by the lower slot. Both ends hold the junction, so
                    // drawing from both would lay every line on top of itself.
                    if other <= i {
                        continue;
                    }
                    junctions.push(JunctionLine {
                        from: (
                            cells.x[i] as f32 / POS_ONE as f32,
                            cells.y[i] as f32 / POS_ONE as f32,
                        ),
                        to: (
                            cells.x[other] as f32 / POS_ONE as f32,
                            cells.y[other] as f32 / POS_ONE as f32,
                        ),
                        hard: j.kind == mm_core::junction::JunctionKind::Hard,
                    });
                }
            }
        }

        // In parallel, because this is where a frame's time goes and it is embarrassingly
        // parallel: every cell reads the world and writes only its own dot.
        //
        // Measured before doing it, at 40,206 cells: 135ms to take a frame at the packed tier
        // against 25ms for a whole *tick*. The renderer was five times the simulation, and it
        // runs on the engine thread holding the slide lock — so it was not only dropping frames,
        // it was stealing ticks. The cost is `squash_of`, which walks a cell's neighbourhood and
        // then solves a sixty-four-ray area integral by bisection to work out how far it has to
        // swell; that is a thousand operations a cell and there were forty thousand cells.
        //
        // `collect` into a `Vec` preserves order, so the frame is identical however the work was
        // split. That matters less here than it does in `mm-core` — a frame is presentation and
        // cannot reach the simulation (see this module's own note) — but a screenshot that
        // differed run to run would still be a bug.
        // Resolved before the parallel loop, because `size_of` compresses paths as it answers
        // and so wants `&mut`. Cheap to do here: the whole components rebuild measured 0.1ms
        // against the 135ms this loop was taking.
        let mut cluster_size = vec![0u32; cells.capacity()];
        for i in cells.iter() {
            cluster_size[i] = components.size_of(i);
        }

        // The camera, with a screen's worth of margin either side, in `POS`.
        //
        // The margin is what makes the degradation graceful rather than visible: ordinary
        // panning stays inside it, so the detail is already built by the time the cell reaches
        // the screen, and only a fling or a fast zoom-out outruns it — for one frame.
        let (cam_x, cam_y, cam_w, cam_h) = self.camera;
        let visible = |i: usize| -> bool {
            if !cam_w.is_finite() || !cam_h.is_finite() {
                return true;
            }
            let x = cells.x[i] as f32 / POS_ONE as f32;
            let y = cells.y[i] as f32 / POS_ONE as f32;
            (x - cam_x).abs() <= cam_w * 2.0 && (y - cam_y).abs() <= cam_h * 2.0
        };

        let indices: Vec<usize> = cells.iter().collect();
        let dots: Vec<CellDot> = indices
            .par_iter()
            .map(|&i| {
                let id = cells.id_at(i);
                let radius = drawn_radius(cells.mass[i]);
                // The two expensive things, and the only two that are skipped off camera. A
                // cell out of view still gets everything below — where it is, how big, what
                // colour — so it is never missing, only plain. `Vec::new` does not allocate,
                // so skipping these skips the heap traffic as well as the arithmetic.
                let near = visible(i);
                let (squash, area_swell) = if packed && near {
                    squash_of(&self.world, i, radius * PACKING)
                } else {
                    (Vec::new(), 1.0)
                };
                let (organelles, limbs) = if detailed && near {
                    cell_parts(
                        cells,
                        i,
                        radius,
                        &squash,
                        area_swell,
                        tick,
                        id.ordering_key(),
                    )
                } else {
                    (Vec::new(), Vec::new())
                };
                CellDot {
                    x: cells.x[i] as f32 / POS_ONE as f32,
                    y: cells.y[i] as f32 / POS_ONE as f32,
                    radius,
                    rgb: cell_colour(cells, i, table),
                    depth: crate::optics::depth_of(id.ordering_key()),
                    id,
                    organelles,
                    limbs,
                    cluster_size: cluster_size[i],
                    age: cells.age[i],
                    squash,
                    area_swell,
                }
            })
            .collect();

        // `MM_SEAM_STATS=1`: seam-slot usage, swell, and how fast the crowd is actually moving.
        // Earned its keep twice — it found that 68% of cells were saturating their eight seam
        // slots, and that a "settled" pack had every cell moving a fifteenth of a square a tick.
        if std::env::var("MM_SEAM_STATS").is_ok() {
            let cap: usize = mm_core::neighbours::CONTACTS_PER_CELL;
            let mut hist: Vec<usize> = vec![0; cap + 1];
            let mut swell = (f32::MAX, 0.0f32, 0.0f32);
            for d in dots.iter() {
                let d: &CellDot = d;
                hist[d.squash.len().min(cap)] += 1;
                swell.0 = swell.0.min(d.area_swell);
                swell.1 = swell.1.max(d.area_swell);
                swell.2 += d.area_swell;
            }
            // How fast the crowd is actually moving — and it takes *two* numbers, which is the
            // trap this fell into.
            //
            // `speed` is `cells.vx/vy`, and reading it alone says a packed slide is perfectly
            // still. It is not. The fluid drift is added straight to the position step in
            // `sensing`, never stored in velocity, so a cell can be shoved a measurable distance
            // every single tick with its velocity reading exactly zero. `drift` is that motion:
            // the amount the current moves a cell each tick before the contact solver pushes it
            // back, which is the amplitude of the in-and-out the centre of a jammed pack does.
            // The only metric that cannot lie: how far each cell actually moved since the last
            // tick. `speed` reads `cells.vx`, and the contact solver writes positions directly —
            // so a cell can shuttle back and forth every tick with its velocity reading zero.
            // Both earlier diagnoses were wrong for exactly this reason.
            type Snapshot = (u64, Vec<(i32, i32)>);
            static LAST: std::sync::Mutex<Option<Snapshot>> = std::sync::Mutex::new(None);
            let now: Vec<(i32, i32)> = (0..cells.capacity())
                .map(|i| (cells.x[i], cells.y[i]))
                .collect();
            let mut moved = (0.0f32, 0.0f32);
            {
                let mut last = LAST.lock().unwrap();
                if let Some((t, prev)) = last.as_ref() {
                    if *t != self.world.tick_count() && prev.len() == now.len() {
                        let mut c = 0.0f32;
                        for i in 0..now.len() {
                            if !cells.occupied(i) {
                                continue;
                            }
                            let dx = (now[i].0 - prev[i].0) as f32 / POS_ONE as f32;
                            let dy = (now[i].1 - prev[i].1) as f32 / POS_ONE as f32;
                            let m = (dx * dx + dy * dy).sqrt();
                            moved.0 += m;
                            moved.1 = moved.1.max(m);
                            c += 1.0;
                        }
                        moved.0 /= c.max(1.0);
                    }
                }
                if last
                    .as_ref()
                    .is_none_or(|(t, _)| *t != self.world.tick_count())
                {
                    *last = Some((self.world.tick_count(), now));
                }
            }
            let (svx, svy) = substrate.velocity();
            // Mean distance from the middle of the slide: the one number that says whether a
            // crowd under an inward force is actually coming in.
            let (cx, cy) = (
                substrate.width() as f32 / 2.0,
                substrate.height() as f32 / 2.0,
            );
            let mut radius_sum = 0.0f32;
            // The whole velocity field, not just where cells happen to be: is the fluid moving
            // at all when the current is `Still`?
            let mut field_max = 0.0f32;
            for k in 0..svx.len().min(svy.len()) {
                let (a, b) = (svx[k] as f32, svy[k] as f32);
                let m = (a * a + b * b).sqrt() / Q10_ONE as f32;
                field_max = field_max.max(m);
            }
            let mut speed = (0.0f32, 0.0f32);
            let mut drift = (0.0f32, 0.0f32);
            let mut n = 0.0f32;
            for i in 0..cells.capacity() {
                if !cells.occupied(i) {
                    continue;
                }
                let (vx, vy) = (cells.vx[i] as f32, cells.vy[i] as f32);
                let s = (vx * vx + vy * vy).sqrt() / Q10_ONE as f32;
                speed.0 += s;
                speed.1 = speed.1.max(s);
                let sq = pos_to_square(cells.x[i]) as usize
                    + pos_to_square(cells.y[i]) as usize * substrate.width() as usize;
                let dx = svx.get(sq).copied().unwrap_or(0) as f32;
                let dy = svy.get(sq).copied().unwrap_or(0) as f32;
                let d = (dx * dx + dy * dy).sqrt() / Q10_ONE as f32;
                drift.0 += d;
                drift.1 = drift.1.max(d);
                let px = cells.x[i] as f32 / POS_ONE as f32 - cx;
                let py = cells.y[i] as f32 / POS_ONE as f32 - cy;
                radius_sum += (px * px + py * py).sqrt();
                n += 1.0;
            }
            eprintln!(
                "SEAMS tick={} hist={:?} full={} swell {:.2}/{:.2}/{:.2} speed mean {:.4} max {:.4} drift mean {:.4} max {:.4} squares/tick radius {:.2} FIELD {:.4} MOVED mean {:.4} max {:.4}",
                self.world.tick_count(),
                hist,
                hist[cap],
                swell.0,
                swell.2 / dots.len().max(1) as f32,
                swell.1,
                speed.0 / n.max(1.0),
                speed.1,
                drift.0 / n.max(1.0),
                drift.1,
                radius_sum / n.max(1.0),
                field_max,
                moved.0,
                moved.1,
            );
        }

        Frame {
            tick: self.world.tick_count(),
            width: substrate.width(),
            height: substrate.height(),
            cells: dots,
            overlays,
            light,
            flow,
            flow_cols,
            flow_shown: self.show_flow,
            detritus,
            carrion,
            carrion_drift: table.advection_rates()[mm_core::ecology::CARRION] as f32
                / mm_core::Q10_ONE as f32,
            detritus_drift: table.advection_rates()[mm_core::ecology::DETRITUS] as f32
                / Q10_ONE as f32,
            flux: self.world.scenario().flux.clone(),
            barriers,
            mineral,
            population: cells.len(),
            lod: self.lod,
            motes: crate::optics::motes(&self.optics, self.world.tick_count()),
            junctions,
            largest_cluster,
        }
    }

    /// The square under a point, for the inspector and the tweezers (M6).
    #[must_use]
    pub fn square_at(&self, x: f32, y: f32) -> Option<(u32, u32)> {
        let s = self.world.substrate();
        let (sx, sy) = (x.floor() as i32, y.floor() as i32);
        if sx < 0 || sy < 0 || sx >= s.width() as i32 || sy >= s.height() as i32 {
            return None;
        }
        Some((sx as u32, sy as u32))
    }

    /// The cell nearest a point, within a radius, for selection.
    #[must_use]
    pub fn cell_at(&self, x: f32, y: f32, within: f32) -> Option<mm_core::CellId> {
        let cells = self.world.cells();
        let mut best: Option<(f32, mm_core::CellId)> = None;
        for i in cells.iter() {
            let cx = cells.x[i] as f32 / POS_ONE as f32;
            let cy = cells.y[i] as f32 / POS_ONE as f32;
            let d = ((cx - x).powi(2) + (cy - y).powi(2)).sqrt();
            if d <= within && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, cells.id_at(i)));
            }
        }
        best.map(|(_, id)| id)
    }
}

/// Where a cell's organelles sit inside it, and what colour they are.
///
/// The arrangement is fixed rather than simulated: slots are laid out on a ring in slot
/// order, with the membrane as the rim. The simulation has no opinion about where inside a
/// cell an organelle is — position within a cell is not state (SPEC §6) — so any arrangement
/// here is a drawing convention. A fixed one is the right convention because it makes slot 3
/// the same place in every cell on screen, which is what turns the picture into something you
/// can read at a glance instead of a bag of coloured dots.
/// The organelles a cell is drawn with, kept inside the cell that owns them.
///
/// They sit on a ring at `0.45` of the radius and are `0.28` across, so the outermost reaches
/// `0.73` of it — and a seam may cut a cell back to `MIN_FACE * PACKING`, which is `0.63`. An
/// organelle on the ring where a neighbour has bitten in is therefore drawn *outside its own
/// cell*, over whatever is on the other side of that wall: a bright blob crossing a boundary,
/// appearing and going away as the clump shifts, on a cell with any number of neighbours at all.
///
/// Measured on nine immortal cells jostling in a dish: 2.8% of organelles were outside their own
/// outline, on 68% of ticks, by up to a third of their own radius.
///
/// So the ring is pulled in to whatever room there is in each organelle's own direction. Per
/// organelle rather than per cell, because a cell flattened on one side has plenty of room on
/// the other and shrinking the whole ring would waste it.
/// The dots inside a cell and the limbs outside it, from one walk of the slots.
///
/// One walk rather than two because the two have to agree: a spike's blade must grow out of the
/// spike's own dot, and the dot's angle is the ring convention above. Two walks would be two
/// copies of "which slot is `nth`".
fn cell_parts(
    cells: &mm_core::CellArena,
    i: usize,
    radius: f32,
    seams: &[Squash],
    swell: f32,
    tick: u64,
    key: u64,
) -> (Vec<OrganelleDot>, Vec<LimbDot>) {
    use mm_core::organelle::MEMBRANE_SLOT;
    let slots = cells.slots(i);
    let mut dots = Vec::new();
    let mut limbs = Vec::new();
    // How many non-membrane slots are occupied, so the ring is evenly spaced for what is
    // actually there rather than leaving gaps where empty slots would have been.
    let occupied: Vec<usize> = (0..slots.len())
        .filter(|n| *n != MEMBRANE_SLOT && slots[*n].is_present())
        .collect();
    let count = occupied.len().max(1) as f32;
    // How far the cell's own outline is in a direction: the swollen radius, cut back by whichever
    // seam bites first. The same expression the shader draws to.
    let drawn = radius * PACKING * swell;
    let wall_towards = |ux: f32, uy: f32| -> f32 {
        let mut wall = drawn;
        for s in seams {
            let along = s.nx * ux + s.ny * uy;
            if along > 1e-4 {
                wall = wall.min(s.face * drawn / along);
            }
        }
        wall.max(0.0)
    };
    for (nth, n) in occupied.iter().enumerate() {
        let o = &slots[*n];
        let angle = std::f32::consts::TAU * nth as f32 / count;
        let (ux, uy) = (angle.cos(), angle.sin());
        let size = (radius * 0.28).max(0.02);
        let wall = wall_towards(ux, uy);
        // Room to sit in, and never negative: a cell cut past its own middle has nowhere to put
        // anything, and an organelle at the centre is better than one outside the wall.
        let ring = (radius * 0.45).min((wall - size).max(0.0));
        dots.push(OrganelleDot {
            kind: o.kind,
            dx: ring * ux,
            dy: ring * uy,
            radius: size,
            rgb: organelle_colour(o.kind),
            // Scaffolding is drawn faint so that a cell mid-build looks mid-build.
            built: if o.remaining_build == 0 { 1.0 } else { 0.35 },
        });
        if let Some(limb) = limb_of(o, *n, drawn, (ux, uy), &wall_towards, tick, key) {
            limbs.push(limb);
        }
    }
    (dots, limbs)
}

/// How many ticks a beat takes.
///
/// Twenty, which at 1× is about three cycles a second — fast enough to read as a beat and slow
/// enough not to alias into a shimmer at the frame rates the microscope actually runs at.
const BEAT_TICKS: u64 = 20;

/// How far past the body a holdfast reaches, in squares. `mm_core::sensing::HOLDFAST_REACH` in
/// the renderer's units.
///
/// Absolute, and note that it does not scale with the cell: the reach is a property of the world,
/// so a large cell's foot is proportionally shorter and that is what the physics says.
const HOLDFAST_SQUARES: f32 =
    mm_core::sensing::HOLDFAST_REACH as f32 / mm_core::fixed::POS_ONE as f32;

/// How far past the body an exoenzyme's cloud spreads, in squares.
///
/// Half a square, matching the holdfast's reach, because both are "just outside the body" and the
/// picture should not imply that one carries further than the other without a mechanism saying so.
/// The dissolving itself happens to whatever is *touching*, and puts its result in the square.
const HALO_SQUARES: f32 = 0.5;

/// Where in its beat a limb is. See [`LimbDot::phase`].
///
/// **The modulo is in `u64` and that is not fussiness.** `tick as f32 / 20.0` loses the fraction
/// entirely past about sixteen million ticks — an hour at 1× — and every cilium on the slide would
/// quietly stop moving. Reducing first keeps it exact for as long as the run lasts.
fn limb_phase(tick: u64, key: u64, slot: usize) -> f32 {
    let cycle = (tick % BEAT_TICKS) as f32 / BEAT_TICKS as f32;
    // Spread, or every cilium on the slide beats in lockstep, which reads as a machine.
    let offset = crate::cellmesh::seed_of(key.wrapping_mul(31).wrapping_add(slot as u64 + 1));
    (cycle + offset).fract()
}

/// The limb one organelle grows, or `None` for the fifteen types that grow none.
///
/// `drawn` is the cell's drawn radius — `PACKING` and the area swell included — because a limb
/// belongs to the cell as it is *drawn*, not as the physics stores it. `ring` is the direction of
/// this slot's dot on the ring, used by the forms whose organelle has no simulated direction.
fn limb_of(
    o: &mm_core::Organelle,
    slot: usize,
    drawn: f32,
    ring: (f32, f32),
    wall_towards: &impl Fn(f32, f32) -> f32,
    tick: u64,
    key: u64,
) -> Option<LimbDot> {
    use mm_core::OrganelleType as T;
    let big = o.param as f32 / 255.0;
    let seed = crate::cellmesh::seed_of(key.wrapping_mul(97).wrapping_add(slot as u64 + 1));

    // Direction, length, half-width, inset, effort, sub-elements, hollow fraction, tip taper.
    let (dir, length, width, inset, extent, count, inner, taper) = match o.kind {
        T::Cilium | T::Flagellum if o.is_active() => {
            // **The one direction on this list that is real.** Sixteen mount angles from
            // `control[1]`, and the thrust genuinely goes that way — so a flagellate drawn with
            // its whip on the wrong side would be the picture contradicting the physics.
            let (qx, qy) = mm_core::sensing::cilium_direction(o);
            let (qx, qy) = (qx as f32, qy as f32);
            let len = (qx * qx + qy * qy).sqrt().max(1.0);
            let dir = (qx / len, qy / len);
            // Signed: the sign is which way the wave travels, and a cilium beating backwards
            // pushes its cell backwards.
            let power =
                mm_core::sensing::cilium_power(o) as f32 / mm_core::fixed::Q10_ONE as f32;
            if o.kind == T::Flagellum {
                // One large organ, longer than the body it drives. The quad is wide enough for
                // the wave, not for the whip: the whip is a fifth of it and the rest is where the
                // beat swings.
                (
                    dir,
                    (1.2 + 1.3 * big) * drawn,
                    (0.22 + 0.14 * big) * drawn,
                    0.12 * drawn,
                    power,
                    1.0,
                    0.0,
                    0.22,
                )
            } else {
                // Many small ones. The half-width is the tuft's span, not one hair's.
                (
                    dir,
                    (0.22 + 0.22 * big) * drawn,
                    (0.20 + 0.16 * big) * drawn,
                    0.06 * drawn,
                    power,
                    (2.0 + (o.param as f32 / 64.0).floor()).min(5.0),
                    0.0,
                    0.0,
                )
            }
        }
        T::Spike => {
            // Out when it is out and gone when it is sheathed, which is the whole point:
            // `OrganelleType::EM_MECHANICAL` already argues that a predator at rest must be
            // indistinguishable and unmistakable the instant it extends. Drawing the spike a cell
            // *has* rather than the spike it has *drawn* would contradict that.
            let reach = mm_core::ecology::spike_reach(o) as f32 / mm_core::fixed::Q10_ONE as f32;
            if reach <= 0.0 {
                return None;
            }
            (
                ring,
                0.9 * drawn * reach,
                0.10 * drawn * (0.35 + big),
                0.20 * drawn,
                reach,
                1.0,
                0.0,
                0.0,
            )
        }
        T::Holdfast if o.is_active() => {
            // Drawn whether or not it is gripping — cement built is cement carried — but limp
            // when it has let go and taut when it has not.
            let effort =
                mm_core::sensing::holdfast_effort(o) as f32 / mm_core::fixed::Q10_ONE as f32;
            // Wide enough for the rootlets to splay into, which is most of the quad; the stalk
            // itself is a third of it.
            (
                ring,
                HOLDFAST_SQUARES,
                (0.15 + 0.11 * big) * drawn,
                0.12 * drawn,
                effort,
                3.0,
                0.0,
                0.45,
            )
        }
        T::Exoenzyme => {
            let throttle =
                mm_core::ecology::exoenzyme_throttle(o) as f32 / mm_core::fixed::Q10_ONE as f32;
            if throttle <= 0.0 {
                return None;
            }
            // A cloud, not a limb: centred on the cell and pointing nowhere, because what an
            // exoenzyme dissolves it dissolves into the square. The quad is the square that holds
            // it, and `inner` is the body it is drawn around.
            let outer = drawn + HALO_SQUARES;
            return Some(LimbDot {
                kind: o.kind,
                dx: 0.0,
                dy: 0.0,
                ux: 1.0,
                uy: 0.0,
                length: outer,
                width: outer,
                inset: outer,
                extent: throttle,
                phase: limb_phase(tick, key, slot),
                count: 1.0,
                inner: (drawn / outer).clamp(0.0, 0.98),
                taper: 0.0,
                seed,
            });
        }
        _ => return None,
    };

    if length <= 0.0 || width <= 0.0 {
        return None;
    }
    // Rooted on the wall the cell is actually drawn to, so a limb on a squashed side leaves from
    // that side and not from where the cell would have been unpressed.
    let wall = wall_towards(dir.0, dir.1);
    Some(LimbDot {
        kind: o.kind,
        dx: dir.0 * wall,
        dy: dir.1 * wall,
        ux: dir.0,
        uy: dir.1,
        length,
        width,
        inset,
        extent,
        phase: limb_phase(tick, key, slot),
        count,
        inner,
        taper,
        seed,
    })
}

/// The colour an organelle is drawn in, so the inspector's schematic and the cell on the
/// slide agree about which blob is which.
#[must_use]
pub fn organelle_rgb(kind: mm_core::OrganelleType) -> [f32; 3] {
    organelle_colour(kind)
}

/// The one colour table, read by the dots inside a cell, by the cell's own colour and by the
/// inspector's schematic.
///
/// **Eleven of the twenty drawable types used to fall through the `_` arm**, so a spike, a shell,
/// a holdfast, a flagellum, a lysosome and a lipid droplet were all the same grey — which made the
/// inside of a cell say "six organelles" and nothing else. `cell_colour` had the same hole from
/// the other end and ignored them entirely, so an armoured cell and a bare one came out the same
/// colour.
///
/// # It is a family per job, not twenty hues
///
/// Twenty arbitrary colours is twenty things to memorise and about four the eye can actually tell
/// apart in a two-pixel dot. Grouped by what the organelle is *for*, a glance at a cell reads as
/// "mostly producer, some store, one weapon" before any individual blob is identified:
///
/// | family | | |
/// | --- | --- | --- |
/// | producers | chloroplast, chemosynth | green |
/// | burners | mitochondrion, diazosome | orange |
/// | stores | vacuole, lipid droplet | blue, and the droplet bright |
/// | information | nucleus, oscillator | violet |
/// | motility | cilium, flagellum | pale gold |
/// | sensing | chemo-, photo-, touch | pink and warm |
/// | attack | spike, exoenzyme, lysosome | red and chemical |
/// | structure | membrane, pump, shell, holdfast, junction port | mineral and neutral |
///
/// The families fall out along the catalogue's own `n + 16` pairing, which is not a coincidence:
/// bit 4 of a type operand means "the same job done a different way", so a pair one mutation apart
/// should read as a pair. Where the real thing genuinely looks different the picture follows the
/// real thing rather than the rule — a lipid droplet is bright and refractive and does not look
/// like a vacuole, whatever the pairing says.
fn organelle_colour(kind: mm_core::OrganelleType) -> [f32; 3] {
    use mm_core::OrganelleType as T;
    match kind {
        // Producers.
        T::Chloroplast => [0.35, 0.78, 0.38],
        // The lightless producer: the same reaction, run in the dark, so a colder green.
        T::Chemosynth => [0.36, 0.68, 0.62],
        // Burners.
        T::Mitochondrion => [0.90, 0.55, 0.28],
        // Fixing is expensive machinery — a deeper, more metallic amber than the engine it pairs
        // with, and far enough from it that an anoxic corner is legible at a glance.
        T::Diazosome => [0.80, 0.64, 0.22],
        // Stores.
        T::Vacuole => [0.55, 0.72, 0.88],
        // Fat, and fat is the one thing in a cell that is genuinely brighter than the cytoplasm.
        T::LipidDroplet => [0.95, 0.91, 0.74],
        // Information.
        T::Nucleus => [0.58, 0.52, 0.80],
        T::Oscillator => [0.72, 0.64, 0.88],
        // Motility.
        T::Cilium => [0.86, 0.84, 0.55],
        // One large organ where a cilium is many small ones, and darker so a tuft and a whip are
        // not the same mark at two pixels.
        T::Flagellum => [0.76, 0.72, 0.40],
        // Sensing.
        T::Chemosensor => [0.88, 0.42, 0.62],
        T::Photosensor => [0.95, 0.80, 0.35],
        T::TouchSensor => [0.70, 0.45, 0.35],
        // Attack. Three ways of making a living off somebody else, and they should not be one
        // colour: a spike is mechanical, an exoenzyme is chemical and outside the cell, and a
        // lysosome is chemical and inside it.
        T::Spike => [0.88, 0.36, 0.32],
        T::Exoenzyme => [0.74, 0.82, 0.34],
        T::Lysosome => [0.68, 0.34, 0.64],
        // Structure. Mineral and neutral, because none of it is metabolism.
        T::Pump => [0.75, 0.75, 0.78],
        // Silica: pale, cool and plainly not tissue.
        T::Shell => [0.82, 0.86, 0.90],
        // Cement, and the darkest thing in the catalogue. A holdfast is a commitment to a place.
        T::Holdfast => [0.50, 0.46, 0.40],
        T::JunctionPort => [0.58, 0.76, 0.78],
        T::Membrane => [0.62, 0.60, 0.58],
        // The reservations, which nothing builds on purpose and a mutation reaches constantly.
        // Deliberately drab: an organelle that does nothing should not be the brightest thing in
        // the cell.
        _ => [0.55, 0.55, 0.58],
    }
}

/// A rolling window of samples, for the live plots (M4).
///
/// Bounded on purpose: a run of ten million ticks must not accumulate ten million samples in
/// the front-end, and a plot wider than the window would be unreadable anyway. Sampling is
/// periodic rather than every tick for the same reason — [`mm_core::metrics::Sample::take`]
/// walks every cell, and doing that sixty times a second at a hundred thousand cells would
/// cost more than the simulation it is measuring.
#[derive(Clone, Debug)]
pub struct MetricHistory {
    samples: std::collections::VecDeque<Sample>,
    capacity: usize,
    /// Ticks between samples.
    every: u64,
    next_at: u64,
}

impl MetricHistory {
    /// How many samples it will keep.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    #[must_use]
    pub fn new(capacity: usize) -> MetricHistory {
        MetricHistory {
            samples: std::collections::VecDeque::new(),
            capacity: capacity.max(1),
            every: 25,
            next_at: 0,
        }
    }

    /// Take a sample if one is due. Called once per tick.
    fn maybe_sample(&mut self, world: &World) {
        let tick = world.tick_count();
        if tick < self.next_at {
            return;
        }
        self.next_at = tick.saturating_add(self.every);
        let sample = Sample::take(world, self.samples.back());
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// How often samples are taken, in ticks.
    pub fn set_interval(&mut self, ticks: u64) {
        self.every = ticks.max(1);
    }

    pub fn samples(&self) -> impl Iterator<Item = &Sample> {
        self.samples.iter()
    }

    #[must_use]
    pub fn latest(&self) -> Option<&Sample> {
        self.samples.back()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// One series, ready to plot: the value at each sample, and the range it spans.
    ///
    /// The range comes back with the data because a plot needs both and computing them in two
    /// passes invites them to disagree about which samples they saw.
    #[must_use]
    pub fn series(&self, pick: impl Fn(&Sample) -> i64) -> Series {
        let values: Vec<i64> = self.samples.iter().map(&pick).collect();
        let lo = values.iter().copied().min().unwrap_or(0);
        let hi = values.iter().copied().max().unwrap_or(0);
        Series {
            ticks: self.samples.iter().map(|s| s.tick).collect(),
            values,
            lo,
            hi,
        }
    }
}

/// One plottable series.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Series {
    pub ticks: Vec<u64>,
    pub values: Vec<i64>,
    pub lo: i64,
    pub hi: i64,
}

impl Series {
    /// Values mapped into `0..=1` against the series' own range.
    ///
    /// A flat series maps to the middle rather than to zero or to a division by nothing: a
    /// population that has been steady at forty thousand for a million ticks should draw as a
    /// horizontal line through the plot, not as a line along the floor.
    #[must_use]
    pub fn normalised(&self) -> Vec<f32> {
        let span = self.hi - self.lo;
        if span == 0 {
            return vec![0.5; self.values.len()];
        }
        self.values
            .iter()
            .map(|v| ((v - self.lo) as f64 / span as f64) as f32)
            .collect()
    }
}

/// A cell's colour, derived from what it is made of.
///
/// Not from a species palette and not from a type field — there is no cell-type enum
/// (SPEC §6.3) — but from its organelle loadout, mixed in the colours the scenario gave its
/// chemistry. A cell that has invested in chloroplasts looks like chloroplasts. That means the
/// picture shows what a cell *is* rather than what the analysis layer has decided to call it,
/// which is the whole reason the microscope is worth looking at.
///
/// # It is the same table the organelles are drawn from
///
/// This held its own copy of six tints, near-duplicates of [`organelle_colour`]'s that had drifted
/// a few hundredths apart, and a `_ => continue` that dropped the other fourteen types on the
/// floor. So a cell that had spent thirteen units of matter and a sixth of its slots on armour was
/// drawn in exactly the colour of a cell that had spent none, and the *inside* of that cell was
/// drawn silica-pale while the body around it was not.
///
/// One table now, and the consequence is the point rather than the tidiness: **a cell looks like
/// what it invested in, for everything it can invest in.** A shelled lineage reads mineral, a
/// predator reads red, a scavenger reads violet.
///
/// The membrane is excluded, as it is from the ring. Every cell has one, so it can only wash the
/// whole population towards one colour — it is the rim, and the rim is drawn by the wall.
///
/// **A cell carrying none of the newly-tinted types is unchanged to within a few hundredths**,
/// which is what makes this safe to land on its own: only cells that actually carry a shell, a
/// spike, a holdfast, a lysosome, a droplet or a variant move at all.
fn cell_colour(cells: &mm_core::CellArena, i: usize, table: &mm_core::ChemTable) -> [f32; 3] {
    use mm_core::organelle::MEMBRANE_SLOT;
    // The cytoplasm, at unit weight: what is left when a cell has built nothing.
    let mut rgb = [0.30f32, 0.32, 0.36];
    let mut weight = 1.0f32;
    for (n, o) in cells.slots(i).iter().enumerate() {
        if !o.is_present() || n == MEMBRANE_SLOT {
            continue;
        }
        let tint = organelle_colour(o.kind);
        // Size, with a floor: a `param 0` organelle is still a thing the cell built and paid for,
        // and a loadout of small ones should still show.
        let w = (o.param as f32 / 255.0).max(0.05);
        for k in 0..3 {
            rgb[k] += tint[k] * w;
        }
        weight += w;
    }
    let _ = table;
    for c in rgb.iter_mut() {
        *c = (*c / weight).clamp(0.0, 1.0);
    }
    rgb
}

/// Where a cell sits, in squares. Handy for tests and for the inspector.
#[must_use]
pub fn cell_square(cells: &mm_core::CellArena, i: usize) -> (i32, i32) {
    (pos_to_square(cells.x[i]), pos_to_square(cells.y[i]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario() -> Scenario {
        Scenario::stress(24, 20)
    }

    #[test]
    fn taking_frames_cannot_change_the_world() {
        // M4 acceptance 1, in the form that can be checked without a GPU: a world run while
        // being rendered must be bit-identical to one that was not. Taking a frame is the only
        // thing the renderer does to the simulation, so this is the whole surface.
        let mut watched = Slide::new(scenario()).unwrap();
        let mut headless = Slide::new(scenario()).unwrap();
        for _ in 0..500 {
            watched.advance(1);
            // Every frame the renderer could possibly ask for, and then some.
            let _ = watched.frame();
            let _ = watched.frame();
            let _ = watched.square_at(3.5, 4.5);
            let _ = watched.cell_at(3.5, 4.5, 2.0);
            headless.advance(1);
        }
        assert_eq!(
            watched.world().state_hash(),
            headless.world().state_hash(),
            "rendering reached the simulation"
        );
    }

    #[test]
    fn how_the_ticks_were_grouped_cannot_change_the_world() {
        // M4 acceptance 3: dropping the render to 5fps must not change tick output or
        // ordering. Here that is the claim that the same total number of ticks gives the same
        // world however they were grouped.
        //
        // This used to be written in terms of `set_speed` and `advance_one_frame`, which
        // belonged to `Slide` when the world was advanced once per rendered frame. Pacing moved
        // to `engine.rs` in M10.1 and the claim came with it, in a stronger form — a real
        // thread at a real rate. What is left here is the part that needs no thread: the
        // grouping itself.
        let mut smooth = Slide::new(scenario()).unwrap();
        for _ in 0..600 {
            smooth.advance(1);
            let _ = smooth.frame();
        }

        let mut stuttering = Slide::new(scenario()).unwrap();
        for _ in 0..50 {
            stuttering.advance(12);
            let _ = stuttering.frame();
        }

        assert_eq!(smooth.world().tick_count(), 600);
        assert_eq!(stuttering.world().tick_count(), 600);
        assert_eq!(
            smooth.world().state_hash(),
            stuttering.world().state_hash(),
            "how the ticks were grouped changed the world"
        );
    }

    #[test]
    fn advancing_by_nothing_advances_nothing() {
        // Zero is a legal tick count and has to mean zero, because that is what a paused world
        // asks for. `engine::tests::paused_means_paused` asserts the same thing about a running
        // thread that has been told to stop.
        let mut slide = Slide::new(scenario()).unwrap();
        slide.advance(10);
        let hash = slide.world().state_hash();
        for _ in 0..1000 {
            slide.advance(0);
            let _ = slide.frame();
        }
        assert_eq!(slide.world().tick_count(), 10);
        assert_eq!(slide.world().state_hash(), hash);
    }

    #[test]
    fn a_frame_describes_the_world_it_was_taken_from() {
        let mut slide = Slide::new(scenario()).unwrap();
        slide.advance(50);
        let f = slide.frame();
        assert_eq!(f.tick, 50);
        assert_eq!(f.width, 24);
        assert_eq!(f.height, 20);
        assert_eq!(f.overlays.len(), 1);
        assert_eq!(f.overlays[0].field.len(), 24 * 20);
        assert_eq!(f.light.len(), 24 * 20);
        assert_eq!(f.cells.len(), f.population);
        assert!(f.overlays[0].field.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(f.light.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn the_overlay_follows_the_chosen_chemical() {
        let mut slide = Slide::new(scenario()).unwrap();
        slide.advance(20);
        slide.set_overlay(0);
        let a = slide.frame();
        slide.set_overlay(8);
        let b = slide.frame();
        assert_eq!(a.overlays.len(), 1);
        assert_eq!(b.overlays.len(), 1);
        assert_eq!(a.overlays[0].chemical, 0);
        assert_eq!(b.overlays[0].chemical, 8);
        assert_ne!(
            a.overlays[0].rgb, b.overlays[0].rgb,
            "each chemical has its own colour"
        );
        // and an out-of-range choice wraps rather than panicking
        slide.set_overlay(999);
        assert!(slide.overlay().is_some_and(|c| c < CHEM_COUNT));
    }

    #[test]
    fn overlays_are_individually_toggleable() {
        // M4 asks for layers, not a radio button: watching carbon dioxide appear exactly
        // where oxidant disappears is the point of having overlays at all.
        let mut slide = Slide::new(scenario()).unwrap();
        slide.advance(20);
        slide.set_overlay(11);
        slide.toggle_overlay(4);
        slide.toggle_overlay(8);
        let f = slide.frame();
        let on: Vec<usize> = f.overlays.iter().map(|l| l.chemical).collect();
        assert_eq!(on, vec![4, 8, 11], "layers come back in chemical order");
        assert!(slide.overlay_enabled(4) && slide.overlay_enabled(11));

        slide.toggle_overlay(4);
        assert!(!slide.overlay_enabled(4));
        assert_eq!(slide.frame().overlays.len(), 2);

        // Every layer carries what the legend needs.
        for layer in &f.overlays {
            assert!(
                !layer.name.is_empty(),
                "a layer with no name is unlabellable"
            );
            assert!(layer.scale > 0);
            assert_eq!(layer.field.len(), 24 * 20);
            assert!(layer.field.iter().all(|v| (0.0..=1.0).contains(v)));
        }
    }

    #[test]
    fn the_flow_field_is_carried_for_the_specks_but_the_arrows_are_asked_for() {
        // The overlay was permanently on and its menu item inert, because the renderer drew
        // arrows from `flow` being non-empty and `flow` is also gathered for the particulate
        // to drift along. Two separate questions: whether the velocity is *available*, and
        // whether the arrows were *switched on*. Only the second belongs to the menu.
        let mut slide = Slide::new(scenario()).unwrap();
        slide.advance(20);
        slide
            .world
            .substrate_mut()
            .add_chem(mm_core::ecology::DETRITUS, 10, 10, 4096);

        assert!(!slide.show_flow, "the instrument starts off");
        let off = slide.frame();
        assert!(
            !off.flow.is_empty(),
            "particulate on the slide means the velocity is gathered regardless"
        );
        assert!(
            !off.flow_shown,
            "but a frame carrying the field is not a frame asking for arrows"
        );

        slide.show_flow = true;
        let on = slide.frame();
        assert!(on.flow_shown);
        assert_eq!(
            on.flow_cols, off.flow_cols,
            "the same field either way — the toggle is about drawing it"
        );
    }

    #[test]
    fn none_of_the_presentation_controls_reach_the_world() {
        // Everything the user can touch that is *not* the speed control: overlays, zoom,
        // optics, inspection. M4 acceptance 1 covers taking frames; this covers the knobs.
        let mut watched = Slide::new(scenario()).unwrap();
        let mut headless = Slide::new(scenario()).unwrap();
        for tick in 0..300 {
            watched.advance(1);
            watched.set_zoom((tick % 120) as f32);
            watched.toggle_overlay(tick as usize % CHEM_COUNT);
            // The mask form too: it is how `engine.rs` hands the renderer's overlay choices
            // across, so it is a path from the front end into this struct like any other.
            watched.set_overlay_mask(watched.overlay_mask() ^ (1 << (tick % 16)));
            watched.optics.focus = (tick % 7) as f32 * 0.1;
            watched.optics.enabled = tick % 3 == 0;
            let f = watched.frame();
            if let Some(dot) = f.cells.first() {
                let _ = watched.inspect(dot.id);
            }
            let _ = watched.history().series(|s| s.population as i64);
            headless.advance(1);
        }
        assert_eq!(
            watched.world().state_hash(),
            headless.world().state_hash(),
            "a presentation control reached the simulation"
        );
    }

    #[test]
    fn detail_arrives_as_you_zoom_in() {
        let mut slide = Slide::new(scenario()).unwrap();
        slide.advance(30);

        slide.set_zoom(4.0);
        let far = slide.frame();
        assert_eq!(far.lod, Lod::Dots);
        assert!(
            far.cells.iter().all(|c| c.organelles.is_empty()),
            "far zoom built organelle lists nobody can see"
        );
        assert!(
            far.cells.iter().all(|c| c.squash.is_empty()),
            "far zoom cut cells against neighbours nobody can see"
        );

        slide.set_zoom(16.0);
        assert_eq!(slide.frame().lod, Lod::Packed);

        slide.set_zoom(32.0);
        let near = slide.frame();
        assert_eq!(near.lod, Lod::Organelles);

        slide.set_zoom(96.0);
        assert_eq!(slide.frame().lod, Lod::Full);

        // The tier changes what is in the frame and nothing about where things are.
        assert_eq!(far.cells.len(), near.cells.len());
        for (a, b) in far.cells.iter().zip(near.cells.iter()) {
            assert_eq!((a.x, a.y, a.id), (b.x, b.y, b.id));
        }
    }

    #[test]
    fn packing_arrives_before_the_contents_do() {
        use mm_core::fixed::{pos, q10};
        use mm_core::{CellId, CellSeed, Organelle, OrganelleType};

        // Two cells placed close enough to press on each other, because the question is what
        // each tier *builds* and a slide with nothing touching cannot answer it.
        let mut slide = Slide::new(scenario()).unwrap();
        let genome = slide.world_mut().genomes().intern(vec![0u8; 16]).unwrap();
        let mut ids = Vec::new();
        for (n, x) in [6i32, 7].into_iter().enumerate() {
            let id = slide.world_mut().spawn_cell(CellSeed {
                x: pos(x),
                y: pos(6),
                mass: q10(40),
                energy: q10(100),
                membrane: 20,
                key: n as u8 + 1,
                badge: 0,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome: genome.clone(),
            });
            if let Some(i) = slide.world_mut().cells_mut().index(id) {
                slide.world_mut().cells_mut().slots_mut(i)[1] =
                    Organelle::finished(OrganelleType::Nucleus, 40);
            }
            ids.push(id);
        }
        // Pressed together rather than merely placed a square apart, and measured rather than
        // guessed: a contact is what this test is about, so the gap is set from the radius the
        // cells actually have instead of from a distance that looks close enough.
        {
            let (a, b) = (ids[0], ids[1]);
            let cells = slide.world_mut().cells_mut();
            let (a, b) = (cells.index(a).unwrap(), cells.index(b).unwrap());
            let r = mm_core::biology::radius(cells, a) as i64 * POS_ONE as i64
                / mm_core::Q10_ONE as i64;
            cells.x[b] = (cells.x[a] as i64 + r) as i32;
            cells.y[b] = cells.y[a];
        }
        slide.world_mut().adopt_current_contents_as_baseline();
        // One tick, because the neighbour index is built by stepping and `squash_of` reads it.
        // A pair this deep inside each other is still a contact after one round of separation.
        slide.advance(1);

        // The two used to share one threshold, so a single step of the wheel took the slide
        // from loose overlapping discs straight to tiled cells covered in two-pixel organelle
        // speckle. Both halves were wrong at that zoom.
        slide.set_zoom(16.0);
        let packed = slide.frame();
        assert_eq!(packed.lod, Lod::Packed);
        assert!(
            packed.cells.iter().any(|c| !c.squash.is_empty()),
            "the packing tier did not cut cells against their neighbours"
        );
        assert!(
            packed.cells.iter().all(|c| c.organelles.is_empty()),
            "organelles turned up at the packing tier"
        );

        // And the tier above adds the contents without losing the packing.
        slide.set_zoom(32.0);
        let detailed = slide.frame();
        assert_eq!(detailed.lod, Lod::Organelles);
        assert!(detailed.cells.iter().any(|c| !c.squash.is_empty()));
        assert!(detailed.cells.iter().any(|c| !c.organelles.is_empty()));
    }

    #[test]
    fn organelles_are_drawn_inside_the_cell_that_owns_them() {
        use mm_core::fixed::{pos, q10};
        use mm_core::{CellId, CellSeed, Organelle, OrganelleType};

        let mut slide = Slide::new(scenario()).unwrap();
        let genome = slide.world_mut().genomes().intern(vec![0u8; 16]).unwrap();
        let id = slide.world_mut().spawn_cell(CellSeed {
            x: pos(6),
            y: pos(6),
            mass: q10(40),
            energy: q10(100),
            membrane: 20,
            key: 1,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        if let Some(i) = slide.world_mut().cells_mut().index(id) {
            let cells = slide.world_mut().cells_mut();
            cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
            cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
            cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
        }
        slide.world_mut().adopt_current_contents_as_baseline();

        slide.set_zoom(64.0);
        let f = slide.frame();
        let dot = f
            .cells
            .iter()
            .find(|c| c.id == id)
            .expect("the cell is drawn");
        assert_eq!(dot.organelles.len(), 3, "membrane is the rim, not a blob");
        for o in &dot.organelles {
            let d = (o.dx * o.dx + o.dy * o.dy).sqrt();
            assert!(
                d + o.radius <= dot.radius + 1e-3,
                "organelle spills outside its cell: offset {d}, radius {}, cell {}",
                o.radius,
                dot.radius
            );
            assert_eq!(o.built, 1.0, "all three were built finished");
        }
    }

    #[test]
    fn the_history_plots_what_happened() {
        let mut slide = Slide::new(scenario()).unwrap();
        slide.advance(500);
        let h = slide.history();
        assert!(!h.is_empty());
        let pop = h.series(|s| s.population as i64);
        assert_eq!(pop.values.len(), h.len());
        assert_eq!(pop.ticks.len(), h.len());
        assert!(
            pop.ticks.windows(2).all(|w| w[0] < w[1]),
            "samples out of order"
        );
        let n = pop.normalised();
        assert_eq!(n.len(), pop.values.len());
        assert!(n.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn junctions_are_drawn_once_per_pair_not_twice() {
        // Both cells hold a junction, so a naive loop lays every line on top of itself and the
        // soft ones look as solid as the hard ones.
        use mm_core::cell::CellSeed;
        use mm_core::fixed::{pos, q10};
        use mm_core::junction::{Junction, JunctionKind};

        let mut slide = Slide::new(scenario()).unwrap();
        let g = slide.world_mut().genomes().intern(vec![0x2E; 4]).unwrap();
        let mut ids = Vec::new();
        for k in 0..4 {
            let id = slide.world_mut().spawn_cell(CellSeed {
                x: pos(4 + k * 2),
                y: pos(6),
                mass: q10(30),
                energy: q10(500),
                membrane: 24,
                key: 11,
                badge: 0,
                species: 0,
                parent: mm_core::CellId::NONE,
                birth_tick: 0,
                genome: g.clone(),
            });
            ids.push(id);
        }
        slide.world_mut().adopt_current_contents_as_baseline();
        for pair in ids.windows(2) {
            let (ia, ib) = (
                slide.world().cells().index(pair[0]).unwrap(),
                slide.world().cells().index(pair[1]).unwrap(),
            );
            let cells = slide.world_mut().cells_mut();
            let sa = mm_core::junction::free_slot(cells, ia).unwrap();
            cells.junctions_mut(ia)[sa] = Junction {
                kind: JunctionKind::Hard,
                other: pair[1],
                rest: 256,
            };
            let sb = mm_core::junction::free_slot(cells, ib).unwrap();
            cells.junctions_mut(ib)[sb] = Junction {
                kind: JunctionKind::Hard,
                other: pair[0],
                rest: 256,
            };
        }

        slide.set_zoom(64.0);
        let f = slide.frame();
        assert_eq!(
            f.junctions.len(),
            3,
            "three links drawn as {:?}",
            f.junctions.len()
        );
        assert!(f.junctions.iter().all(|j| j.hard));
        assert_eq!(f.largest_cluster, 4, "the chain is not one organism");
        assert!(f.cells.iter().any(|c| c.cluster_size == 4));

        // And at far zoom the lines are not built at all: a junction is sub-pixel and there
        // may be fifty thousand of them.
        slide.set_zoom(1.0);
        assert!(slide.frame().junctions.is_empty());
    }

    #[test]
    fn a_flat_series_draws_through_the_middle() {
        // Not at the bottom, and not a division by zero.
        let flat = Series {
            ticks: vec![0, 1, 2],
            values: vec![7, 7, 7],
            lo: 7,
            hi: 7,
        };
        assert_eq!(flat.normalised(), vec![0.5, 0.5, 0.5]);
        let empty = Series {
            ticks: vec![],
            values: vec![],
            lo: 0,
            hi: 0,
        };
        assert!(empty.normalised().is_empty());
    }

    #[test]
    fn the_history_does_not_grow_without_bound() {
        // A ten-million-tick run must not accumulate ten million samples in the front-end.
        let mut slide = Slide::new(scenario()).unwrap();
        let cap = 8;
        slide.history = MetricHistory::new(cap);
        slide.history.set_interval(1);
        slide.advance(200);
        assert_eq!(slide.history().len(), cap);
        let ticks: Vec<u64> = slide.history().samples().map(|s| s.tick).collect();
        assert_eq!(*ticks.last().unwrap(), 200, "the newest sample was dropped");
    }

    #[test]
    fn picking_outside_the_slide_finds_nothing() {
        let slide = Slide::new(scenario()).unwrap();
        assert_eq!(slide.square_at(-1.0, 4.0), None);
        assert_eq!(slide.square_at(4.0, -1.0), None);
        assert_eq!(slide.square_at(999.0, 4.0), None);
        assert_eq!(slide.square_at(0.0, 0.0), Some((0, 0)));
        assert_eq!(slide.cell_at(4.0, 4.0, 1.0), None, "nothing is alive yet");
    }

    /// A slide with one cell at (6, 6), zoomed in far enough to resolve its contents.
    fn one_cell() -> (Slide, mm_core::CellId) {
        use mm_core::fixed::{pos, q10};
        use mm_core::{CellId, CellSeed};
        let mut slide = Slide::new(scenario()).unwrap();
        let genome = slide.world_mut().genomes().intern(vec![0u8; 16]).unwrap();
        let id = slide.world_mut().spawn_cell(CellSeed {
            x: pos(6),
            y: pos(6),
            mass: q10(40),
            energy: q10(100),
            membrane: 20,
            key: 1,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        slide.world_mut().adopt_current_contents_as_baseline();
        slide.set_zoom(60.0);
        (slide, id)
    }

    fn limbs_of(slide: &mut Slide, id: mm_core::CellId) -> Vec<LimbDot> {
        let frame = slide.frame();
        frame
            .cells
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.limbs.clone())
            .unwrap_or_default()
    }

    #[test]
    fn a_sheathed_spike_is_nothing_and_a_drawn_one_is_as_long_as_it_is_out() {
        // The claim `OrganelleType::EM_MECHANICAL` already makes about the energy signature, said
        // in the picture: a predator at rest is indistinguishable from anything else its size and
        // becomes unmistakable the instant it extends. Drawing the spike a cell *has* rather than
        // the spike it has *drawn* would contradict it, and would make ambush unavailable.
        use mm_core::{Organelle, OrganelleType, Q10_ONE};
        let (mut slide, id) = one_cell();
        let i = slide.world_mut().cells_mut().index(id).unwrap();

        let mut spike = Organelle::finished(OrganelleType::Spike, 200);
        spike.control[0] = 0;
        slide.world_mut().cells_mut().slots_mut(i)[4] = spike;
        assert!(
            limbs_of(&mut slide, id).is_empty(),
            "a sheathed spike was drawn"
        );

        // Still being built: inert, and therefore invisible, however wide open the control is.
        let mut building = Organelle::building(OrganelleType::Spike, 200, 5);
        building.control[0] = Q10_ONE as i16;
        slide.world_mut().cells_mut().slots_mut(i)[4] = building;
        assert!(
            limbs_of(&mut slide, id).is_empty(),
            "an unfinished spike was drawn"
        );

        let mut half = Organelle::finished(OrganelleType::Spike, 200);
        half.control[0] = (Q10_ONE / 2) as i16;
        slide.world_mut().cells_mut().slots_mut(i)[4] = half;
        let short = limbs_of(&mut slide, id);
        assert_eq!(short.len(), 1);
        assert_eq!(short[0].kind, OrganelleType::Spike);

        let mut full = Organelle::finished(OrganelleType::Spike, 200);
        full.control[0] = Q10_ONE as i16;
        slide.world_mut().cells_mut().slots_mut(i)[4] = full;
        let long = limbs_of(&mut slide, id);
        assert_eq!(long.len(), 1);
        assert!(
            (long[0].length - 2.0 * short[0].length).abs() < 1e-4,
            "length is not the extension: {} against {}",
            long[0].length,
            short[0].length
        );
        // Thickness is what the cell built, and does not move with the extension.
        assert!((long[0].width - short[0].width).abs() < 1e-6);
    }

    #[test]
    fn a_propulsor_points_where_its_thrust_goes() {
        // The one direction on the list that is real. A flagellate drawn with its whip on the
        // wrong side is the picture contradicting the physics, and the mount angle is four bits
        // of `control[1]` that nothing on screen could previously show.
        use mm_core::{Organelle, OrganelleType, Q10_ONE};
        let (mut slide, id) = one_cell();
        let i = slide.world_mut().cells_mut().index(id).unwrap();
        for angle in 0..16i16 {
            let mut flagellum = Organelle::finished(OrganelleType::Flagellum, 128);
            flagellum.control[0] = Q10_ONE as i16;
            flagellum.control[1] = angle;
            slide.world_mut().cells_mut().slots_mut(i)[4] = flagellum;
            let limbs = limbs_of(&mut slide, id);
            assert_eq!(limbs.len(), 1);
            let (qx, qy) = mm_core::sensing::cilium_direction(&flagellum);
            let len = ((qx * qx + qy * qy) as f32).sqrt();
            assert!(
                (limbs[0].ux - qx as f32 / len).abs() < 1e-3
                    && (limbs[0].uy - qy as f32 / len).abs() < 1e-3,
                "mount {angle} drawn at ({}, {}) and thrusting at ({qx}, {qy})",
                limbs[0].ux,
                limbs[0].uy
            );
            // And the root is on the wall in that direction, not at the centre.
            let out = limbs[0].dx * limbs[0].ux + limbs[0].dy * limbs[0].uy;
            assert!(out > 0.0, "the flagellum leaves from inside the cell");
        }
    }

    #[test]
    fn a_cilium_beating_backwards_is_drawn_beating_backwards() {
        use mm_core::{Organelle, OrganelleType, Q10_ONE};
        let (mut slide, id) = one_cell();
        let i = slide.world_mut().cells_mut().index(id).unwrap();
        let mut cilium = Organelle::finished(OrganelleType::Cilium, 128);
        cilium.control[0] = -(Q10_ONE as i16);
        slide.world_mut().cells_mut().slots_mut(i)[4] = cilium;
        let back = limbs_of(&mut slide, id);
        assert_eq!(back.len(), 1);
        assert!(back[0].extent < -0.9, "extent {}", back[0].extent);

        // Idle, and still drawn: a cilium is not a weapon, and one a cell has built is one it is
        // paying for whether or not it is beating.
        cilium.control[0] = 0;
        slide.world_mut().cells_mut().slots_mut(i)[4] = cilium;
        let idle = limbs_of(&mut slide, id);
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0].extent, 0.0);
        assert!(
            (idle[0].length - back[0].length).abs() < 1e-6,
            "an idle cilium was drawn shorter; length is what the cell built"
        );
    }

    #[test]
    fn nothing_grows_a_limb_below_the_tier_that_can_show_one() {
        use mm_core::{Organelle, OrganelleType, Q10_ONE};
        let (mut slide, id) = one_cell();
        let i = slide.world_mut().cells_mut().index(id).unwrap();
        let mut spike = Organelle::finished(OrganelleType::Spike, 200);
        spike.control[0] = Q10_ONE as i16;
        slide.world_mut().cells_mut().slots_mut(i)[4] = spike;
        assert_eq!(limbs_of(&mut slide, id).len(), 1);

        slide.set_zoom(3.0);
        assert_eq!(slide.lod(), Lod::Dots);
        assert!(
            limbs_of(&mut slide, id).is_empty(),
            "whole-slide zoom built a limb list nobody can see"
        );
    }

    #[test]
    fn a_beat_does_not_stop_after_an_hour() {
        // `tick as f32 / 20.0` loses the fraction entirely past about sixteen million ticks, and
        // every cilium on the slide would quietly stop moving. The modulo is in `u64` for exactly
        // this, and the failure is invisible in any test that does not run the clock out.
        let mut seen = std::collections::BTreeSet::new();
        for tick in 40_000_000u64..40_000_000 + BEAT_TICKS {
            let p = limb_phase(tick, 12_345, 4);
            seen.insert((p * 1000.0) as i32);
        }
        assert_eq!(
            seen.len(),
            BEAT_TICKS as usize,
            "the beat has {} distinct phases forty million ticks in",
            seen.len()
        );
        // And it is the tick's, not the clock's: the same tick is always the same phase.
        assert_eq!(limb_phase(77, 12_345, 4), limb_phase(77, 12_345, 4));
        assert_ne!(limb_phase(77, 12_345, 4), limb_phase(77, 12_345, 5));
    }

    #[test]
    fn every_implemented_organelle_has_a_colour_of_its_own() {
        // Eleven of the twenty drawable types used to share the `_` arm's grey, so a spike, a
        // shell, a holdfast and a flagellum were the same mark. The catalogue is append-only and
        // the reservations are being filled one milestone at a time, so this is the test that
        // fires the *next* time an organ arrives without one — which is how the eleven got there.
        let fallback = organelle_colour(mm_core::OrganelleType::Reserved31);
        let mut colourless = Vec::new();
        for kind in *mm_core::OrganelleType::all() {
            if !kind.is_implemented() {
                continue;
            }
            if organelle_colour(kind) == fallback {
                colourless.push(kind.name());
            }
        }
        assert!(
            colourless.is_empty(),
            "{colourless:?} are drawn in the reservation grey and cannot be told apart"
        );
    }

    #[test]
    fn no_two_organelles_are_drawn_the_same_colour() {
        // A table of twenty is easy to add a duplicate to, and a duplicate is invisible until
        // somebody is trying to read a cell.
        let all: Vec<_> = mm_core::OrganelleType::all()
            .iter()
            .filter(|k| k.is_implemented())
            .collect();
        for (n, a) in all.iter().enumerate() {
            for b in &all[n + 1..] {
                let (ca, cb) = (organelle_colour(**a), organelle_colour(**b));
                let apart: f32 = (0..3).map(|k| (ca[k] - cb[k]).abs()).sum();
                assert!(
                    apart > 0.12,
                    "{} and {} are the same colour: {ca:?} against {cb:?}",
                    a.name(),
                    b.name()
                );
            }
        }
    }

    #[test]
    fn a_cell_looks_like_what_it_invested_in() {
        // The hole this closes: `cell_colour` tinted from six types and dropped the other
        // fourteen, so a cell that had spent a sixth of its slots and thirteen units of matter on
        // armour was drawn in exactly the colour of a cell that had spent none.
        use mm_core::fixed::{pos, q10};
        use mm_core::{CellId, CellSeed, Organelle, OrganelleType};
        let scenario = scenario();
        let table = scenario.chemicals.clone();
        let mut slide = Slide::new(scenario).unwrap();
        let genome = slide.world_mut().genomes().intern(vec![0u8; 16]).unwrap();
        let id = slide.world_mut().spawn_cell(CellSeed {
            x: pos(6),
            y: pos(6),
            mass: q10(40),
            energy: q10(100),
            membrane: 20,
            key: 1,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        let i = slide.world_mut().cells_mut().index(id).unwrap();
        let cells = slide.world_mut().cells_mut();
        cells.slots_mut(i)[0] = Organelle::finished(OrganelleType::Membrane, 40);
        for slot in 1..mm_core::organelle::SLOT_COUNT {
            cells.slots_mut(i)[slot] = Organelle::empty();
        }
        let bare = cell_colour(slide.world().cells(), i, &table);

        let cells = slide.world_mut().cells_mut();
        cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Shell, 255);
        cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Shell, 255);
        let armoured = cell_colour(slide.world().cells(), i, &table);
        let moved: f32 = (0..3).map(|k| (armoured[k] - bare[k]).abs()).sum();
        assert!(
            moved > 0.2,
            "a doubly-shelled cell is drawn as {armoured:?} against a bare {bare:?}"
        );

        // And the membrane is not in the mix. Every cell has one, so it could only wash the whole
        // population towards a single colour.
        let cells = slide.world_mut().cells_mut();
        cells.slots_mut(i)[1] = Organelle::empty();
        cells.slots_mut(i)[2] = Organelle::empty();
        cells.slots_mut(i)[0] = Organelle::finished(OrganelleType::Membrane, 255);
        assert_eq!(cell_colour(slide.world().cells(), i, &table), bare);
    }
}
