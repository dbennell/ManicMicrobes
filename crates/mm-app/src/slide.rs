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
    /// How many cells are in this one's organism, over hard junctions (M7). One means a
    /// solitary cell.
    pub cluster_size: u32,
    /// How long this cell has existed, in ticks. Only the first few matter to the renderer,
    /// which uses them to swell a newborn into place rather than have it appear whole.
    pub age: u32,
    /// Where this cell is flattened by the neighbours it is pressed into.
    ///
    /// Empty below [`Lod::Organelles`], like `organelles`: a cell a few pixels across cannot
    /// show a flattened side, and building the list for fifty thousand of them would be work
    /// with nothing to show for it.
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
const PACKING_PERMILLE: i32 = 1500;

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

/// The most a cell may be swollen to keep its area. See [`area_swell`].
const MAX_SWELL: f32 = 1.25;

/// How many directions the clipped area is measured along. See [`area_swell`].
const SWELL_RAYS: usize = 64;

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
fn area_swell(radius: f32, want_radius: f32, seams: &[Squash]) -> f32 {
    if seams.is_empty() || radius <= 0.0 {
        return 1.0;
    }
    // Distance to the nearest seam along each of `SWELL_RAYS` directions, ignoring the circle.
    let mut reach = [f32::INFINITY; SWELL_RAYS];
    for (j, r) in reach.iter_mut().enumerate() {
        let theta = std::f32::consts::TAU * j as f32 / SWELL_RAYS as f32;
        let (sy, sx) = theta.sin_cos();
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
    let area_at = |scale: f32| -> f32 {
        let r = radius * scale;
        let mut sum = 0.0;
        for reach in reach.iter() {
            let rho = reach.min(r);
            sum += rho * rho;
        }
        // ½ ρ² dθ, with dθ the same for every ray.
        0.5 * sum * std::f32::consts::TAU / SWELL_RAYS as f32
    };
    if area_at(MAX_SWELL) < target {
        return MAX_SWELL;
    }
    // The bottom of the range is *below* one, and deliberately. `radius` arrives already
    // inflated by `PACKING`, which exists only so that cells the physics leaves touching still
    // overlap on screen and have a seam to share. That inflation is a lie about the cell's size,
    // and an unclipped cell should not be told it: with the target set to the honest area, a cell
    // nothing is pressing on solves to `1 / PACKING` and is drawn at exactly the radius it has.
    let (mut lo, mut hi) = (1.0f32, MAX_SWELL);
    for _ in 0..16 {
        let mid = 0.5 * (lo + hi);
        if area_at(mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
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
    // How firmly this cell holds its own shape. See `Contact::rigidity`.
    let rigidity = world
        .cells()
        .slots(i)
        .first()
        .map_or(0.0, |m| m.param as f32);
    let mut seams: Vec<Squash> = world
        .neighbours()
        .contacts(world.cells(), i, PACKING_PERMILLE)
        .as_slice()
        .iter()
        .filter_map(|c| {
            let (dx, dy) = (c.dx as f32 * scale, c.dy as f32 * scale);
            let d = (dx * dx + dy * dy).sqrt();
            // Exactly coincident centres have no direction to be squashed along. Separation
            // will pull them apart next tick; until then they are simply drawn whole.
            if d <= f32::EPSILON {
                return None;
            }
            let other = c.radius as f32 * scale * PACKING;
            // The plane through the two points where the outlines cross:
            //   face = (d² + r² - other²) / 2d
            // Both cells get the same seam from their own side, because swapping r and other
            // and measuring from the far centre gives d minus this.
            let face = (d * d + radius * radius - other * other) / (2.0 * d);
            // Then moved towards whichever of the two is softer.
            //
            // The plane on its own says two cells give way equally, and they do not: a cell
            // that has paid for a thick membrane holds its shape and one that has not gives
            // in. So the seam slides along the overlap in proportion to the difference,
            // bounded by half of it — a firm cell pressed against a soft one stays round and
            // dents the other, and two cells of the same build still meet in the middle.
            //
            // Both sides compute the same seam: the shift is antisymmetric in the two
            // rigidities, so the softer cell arrives at the same line from its own side and
            // they still meet with no gap.
            let overlap = (radius + other - d).max(0.0);
            let (mine, theirs) = (rigidity, c.rigidity as f32);
            let firmness = (mine - theirs) / (mine + theirs).max(1.0);
            let face = face + 0.5 * overlap * firmness;
            // Then held off both cores — this one's and the neighbour's — as *one* interval
            // rather than one clamp per side.
            //
            // Clamping each cell's own face independently is what this replaced, and it broke
            // the one property the whole scheme rests on: that both cells arrive at the same
            // plane. Whenever the clamp bit, the two faces stopped summing to the distance
            // between the centres and the pair was drawn overlapping instead of sharing a wall.
            // With cells of a size, that was rare. Now that the physics presses crowds to their
            // core it fires constantly on any mismatched pair — the neighbour's face is the part
            // that goes short, and nothing on this side could see it.
            //
            // Written as an interval, it is antisymmetric again: if this cell's face is pushed
            // out to `d - theirs`, the neighbour computing from its own side is pushed in to
            // exactly `theirs`, and the two still meet on one line.
            let my_core = MIN_FACE * radius;
            let their_core = MIN_FACE * other;
            let face = if my_core + their_core >= d {
                // No plane can respect both cores: the pair is closer than SPEC §6.4 should
                // allow, which happens for a tick after a division places a daughter inside its
                // parent. Split the distance between them in proportion instead — still one
                // plane, still the same from both sides.
                d * my_core / (my_core + their_core).max(f32::EPSILON)
            } else {
                face.clamp(my_core, d - their_core)
            };
            Some(Squash {
                nx: dx / d,
                ny: dy / d,
                face: face / radius,
            })
        })
        .collect();

    // Then grown until what survives the cutting is the area the cell has, and the faces
    // re-expressed against the bigger radius so the planes themselves have not moved.
    let swell = area_swell(radius, radius, &seams);
    for s in seams.iter_mut() {
        s.face /= swell;
    }
    (seams, swell)
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
    /// Organelle-resolved sprites.
    Organelles,
    /// Membranes, organelles and junctions.
    Full,
}

impl Lod {
    /// Which tier a zoom level calls for.
    ///
    /// The thresholds are in pixels per substrate square, which is the only unit that says
    /// anything about whether a thing is visible. A cell is about half a square across, so
    /// below roughly twelve pixels per square its organelles are sub-pixel and there is
    /// nothing to resolve.
    #[must_use]
    pub fn for_pixels_per_square(pixels: f32) -> Lod {
        if pixels >= 48.0 {
            Lod::Full
        } else if pixels >= 12.0 {
            Lod::Organelles
        } else {
            Lod::Dots
        }
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
    /// Per-square concentration normalised against `peak`, `0..=1`.
    pub field: Vec<f32>,
    /// What `field` was divided by, in `Q10` units. The legend's scale.
    pub peak: i32,
    /// Total of this chemical in the fluid, for the legend's readout.
    pub total: i64,
}

/// The simulation, and the only thing the front-end is allowed to hold.
pub struct Slide {
    world: World,
    /// Which chemical overlays are switched on. Individually toggleable (M4), so this is a
    /// set and not a choice.
    overlays: [bool; CHEM_COUNT],
    /// Detail tier the next frame will be built at.
    lod: Lod,
    /// The microscope's look.
    pub optics: crate::optics::Optics,
    /// Rolling history for the live plots.
    history: MetricHistory,
    /// Trophic flows over the last complete window, and the one still filling (M8).
    flows: crate::foodweb::Flows,
    flows_filling: crate::foodweb::Flows,
}

/// How many ticks the food web averages over.
///
/// Long enough that a single tick's births and deaths do not make the arrows twitch, short
/// enough that a shift in the ecosystem shows up while the user is still looking at it.
const FLOW_WINDOW_TICKS: u64 = 600;

impl Slide {
    /// # Errors
    ///
    /// A scenario this engine cannot honour.
    pub fn new(scenario: Scenario) -> Result<Slide, mm_core::ScenarioError> {
        let mut overlays = [false; CHEM_COUNT];
        // Carbon dioxide by default: it is what the ancestor breathes out, so it is the layer
        // that first shows there is something alive on the slide.
        if let Some(on) = overlays.get_mut(11) {
            *on = true;
        }
        Ok(Slide {
            world: World::new(scenario)?,
            flows: crate::foodweb::Flows::default(),
            flows_filling: crate::foodweb::Flows::default(),
            overlays,
            lod: Lod::Dots,
            optics: crate::optics::Optics::default(),
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

    /// Show this chemical's overlay and no other. The number keys.
    pub fn set_overlay(&mut self, chemical: usize) {
        self.overlays = [false; CHEM_COUNT];
        if let Some(on) = self.overlays.get_mut(chemical % CHEM_COUNT) {
            *on = true;
        }
    }

    /// Switch one chemical's overlay on or off without disturbing the others.
    pub fn toggle_overlay(&mut self, chemical: usize) {
        if let Some(on) = self.overlays.get_mut(chemical % CHEM_COUNT) {
            *on = !*on;
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
            *on = mask & (1u32 << i) != 0;
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

    /// What the wiki calls this species, or a placeholder if the archive has not named it yet.
    #[must_use]
    pub fn species_name(&self, species: u32) -> String {
        self.world
            .archive()
            .get(species)
            .map(|s| s.name.to_string())
            .unwrap_or_else(|| format!("species {species}"))
    }

    /// Take a frame. Read-only: nothing here can reach the world.
    #[must_use]
    pub fn frame(&self) -> Frame {
        let substrate = self.world.substrate();
        let cells = self.world.cells();
        let table = &self.world.scenario().chemicals;

        let overlays: Vec<OverlayLayer> = (0..CHEM_COUNT)
            .filter(|c| self.overlays[*c])
            .map(|c| {
                let plane = substrate.chem_plane(c);
                // Normalised against the frame's own peak rather than a fixed scale, so a
                // nearly empty slide is still legible. It means the overlay's absolute
                // meaning changes between frames, which is exactly why `peak` travels with
                // the layer and the legend shows it.
                let peak = plane.iter().copied().max().unwrap_or(0).max(1);
                let def = table.get(c);
                OverlayLayer {
                    chemical: c,
                    name: def.name.to_string(),
                    rgb: [
                        def.colour[0] as f32 / 255.0,
                        def.colour[1] as f32 / 255.0,
                        def.colour[2] as f32 / 255.0,
                    ],
                    field: plane.iter().map(|v| *v as f32 / peak as f32).collect(),
                    peak,
                    total: plane.iter().map(|v| i64::from(*v)).sum(),
                }
            })
            .collect();

        let light: Vec<f32> = substrate
            .light()
            .iter()
            .map(|v| (*v as f32 / Q10_ONE as f32).clamp(0.0, 1.0))
            .collect();

        let detailed = self.lod.resolves_organelles();

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

        let dots: Vec<CellDot> = cells
            .iter()
            .map(|i| {
                let id = cells.id_at(i);
                let radius = mm_core::biology::radius(cells, i) as f32 / Q10_ONE as f32;
                let (squash, area_swell) = if detailed {
                    squash_of(&self.world, i, radius * PACKING)
                } else {
                    (Vec::new(), 1.0)
                };
                CellDot {
                    x: cells.x[i] as f32 / POS_ONE as f32,
                    y: cells.y[i] as f32 / POS_ONE as f32,
                    radius,
                    rgb: cell_colour(cells, i, table),
                    depth: crate::optics::depth_of(id.ordering_key()),
                    id,
                    organelles: if detailed {
                        organelle_dots(cells, i, radius)
                    } else {
                        Vec::new()
                    },
                    cluster_size: components.size_of(i),
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
                if last.as_ref().is_none_or(|(t, _)| *t != self.world.tick_count()) {
                    *last = Some((self.world.tick_count(), now));
                }
            }
            let (svx, svy) = substrate.velocity();
            // Mean distance from the middle of the slide: the one number that says whether a
            // crowd under an inward force is actually coming in.
            let (cx, cy) = (substrate.width() as f32 / 2.0, substrate.height() as f32 / 2.0);
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
fn organelle_dots(cells: &mm_core::CellArena, i: usize, radius: f32) -> Vec<OrganelleDot> {
    use mm_core::organelle::MEMBRANE_SLOT;
    let slots = cells.slots(i);
    let mut out = Vec::new();
    // How many non-membrane slots are occupied, so the ring is evenly spaced for what is
    // actually there rather than leaving gaps where empty slots would have been.
    let occupied: Vec<usize> = (0..slots.len())
        .filter(|n| *n != MEMBRANE_SLOT && slots[*n].is_present())
        .collect();
    let count = occupied.len().max(1) as f32;
    for (nth, n) in occupied.iter().enumerate() {
        let o = &slots[*n];
        let angle = std::f32::consts::TAU * nth as f32 / count;
        let ring = radius * 0.45;
        out.push(OrganelleDot {
            kind: o.kind,
            dx: ring * angle.cos(),
            dy: ring * angle.sin(),
            radius: (radius * 0.28).max(0.02),
            rgb: organelle_colour(o.kind),
            // Scaffolding is drawn faint so that a cell mid-build looks mid-build.
            built: if o.remaining_build == 0 { 1.0 } else { 0.35 },
        });
    }
    out
}

/// The colour an organelle is drawn in, so the inspector's schematic and the cell on the
/// slide agree about which blob is which.
#[must_use]
pub fn organelle_rgb(kind: mm_core::OrganelleType) -> [f32; 3] {
    organelle_colour(kind)
}

fn organelle_colour(kind: mm_core::OrganelleType) -> [f32; 3] {
    use mm_core::OrganelleType as T;
    match kind {
        T::Chloroplast => [0.35, 0.78, 0.38],
        T::Mitochondrion => [0.90, 0.55, 0.28],
        T::Nucleus => [0.58, 0.52, 0.80],
        T::Vacuole => [0.55, 0.72, 0.88],
        T::Pump => [0.75, 0.75, 0.78],
        T::Cilium => [0.86, 0.84, 0.55],
        T::Chemosensor => [0.88, 0.42, 0.62],
        T::Photosensor => [0.95, 0.80, 0.35],
        T::TouchSensor => [0.70, 0.45, 0.35],
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
fn cell_colour(cells: &mm_core::CellArena, i: usize, table: &mm_core::ChemTable) -> [f32; 3] {
    use mm_core::OrganelleType;
    let mut rgb = [0.30f32, 0.32, 0.36];
    let mut weight = 1.0f32;
    for o in cells.slots(i) {
        if !o.is_present() {
            continue;
        }
        let tint = match o.kind {
            OrganelleType::Chloroplast => [0.35, 0.75, 0.35],
            OrganelleType::Mitochondrion => [0.85, 0.55, 0.30],
            OrganelleType::Nucleus => [0.55, 0.50, 0.75],
            OrganelleType::Vacuole => [0.55, 0.70, 0.85],
            OrganelleType::Cilium => [0.80, 0.80, 0.55],
            OrganelleType::Chemosensor | OrganelleType::Photosensor => [0.85, 0.45, 0.65],
            _ => continue,
        };
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
            assert!(layer.peak > 0);
            assert_eq!(layer.field.len(), 24 * 20);
            assert!(layer.field.iter().all(|v| (0.0..=1.0).contains(v)));
        }
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

        slide.set_zoom(24.0);
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
}
