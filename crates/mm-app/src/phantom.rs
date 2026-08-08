//! A slide of cells no simulation made.
//!
//! In imaging, a *phantom* is an object of known geometry you put under the instrument when you
//! want to measure the instrument rather than the specimen. That is exactly what this is: cells
//! whose positions, radii and seams are arithmetic on a frame number, so that when something is
//! drawn wrong there is no question of what it was given.
//!
//! # Why
//!
//! The flickering overlaps have been chased through the physics for three days, and every
//! measurement has had to run a world to get a picture. That means every experiment carries the
//! whole simulation with it: division, the contact set, the neighbour index, level of detail,
//! the camera. Ruling any one of those out took a run, and none of the runs could rule out the
//! two things actually named in the report — the fragment shader, and the data handed to it.
//!
//! Here there is no world. [`Bench::blobs`] places cells on a lattice and moves them by a closed
//! form; [`Bench::draw`] computes their seams **all-pairs**, with no contact set to churn, no
//! reach to fall short and no cap to truncate unless one is asked for. What reaches the shader is
//! then correct by construction, and:
//!
//! * if the artefact appears anyway, it is the shader or the packing of the attributes;
//! * if it does not, the shader is exonerated and the fault is upstream, in `slide.rs`.
//!
//! And because each of those upstream faults is a knob here — [`Bench::cap`], [`Bench::reach`],
//! [`Bench::churn`], [`Bench::staircase`] — a suspected cause can be *injected* and compared
//! against what the real slide looks like, rather than argued about.
//!
//! # What is shared with the real thing
//!
//! The seam plane is [`slide::seam_between`] and the swell is [`slide::area_swell`], both called
//! here exactly as `slide::squash_of` calls them. Nothing about the geometry is reimplemented —
//! only the *source of the cells* is different, which is the whole point.
//!
//! [`Drawn::outline`] is the one exception and is a deliberate copy: the fragment shader's own
//! outline, in Rust, so that it can be measured without a GPU. `cell.wgsl` and it must agree, and
//! `tests/shader_probe.rs` is where that is checked.

use crate::cellmesh::FIELD_FILL;
use crate::slide::{self, Squash, PACKING};

/// One cell of the phantom: where it is, how big, and who it is.
///
/// The radius is the *physical* one, as `mm-core` would report it. Everything drawn is `PACKING`
/// times this or more.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Blob {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    /// Fixes the silhouette and the colour. Small integers, as `cellmesh::seed_of` wants.
    pub id: u64,
}

/// How the cells are arranged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// Two cells of unequal size. The case that found the swell taper bug: a big cell's flat
    /// running on past the point where its small neighbour actually touches it.
    Pair,
    /// Three by three, which is what the artefact was reported on — six to eight neighbours
    /// apiece and nowhere near any cap.
    Nine,
    /// Five by three. Fifteen cells, which is the size the artefact was reported at — big enough
    /// that the middle row is enclosed on every side, small enough that no cell comes near a cap.
    Fifteen,
    /// Fifteen on a hexagonal lattice: three rows of five, every other row offset by half a
    /// pitch and the rows √3/2 of a pitch apart.
    ///
    /// The arrangement a monolayer actually settles into, and the one where every interior cell
    /// has six neighbours at the same distance and six seams meeting at sixty degrees. A square
    /// lattice never asks that: its four orthogonal neighbours are much nearer than its four
    /// diagonal ones, so the diagonals barely cut and the awkward case of *six* facets sharing a
    /// cell never comes up.
    Hex,
    /// Fifteen cells dropped on a sunflower spiral and jogged off it, so no two contacts are
    /// alike: neighbour counts from three to eight, contact distances from grazing to deep, and
    /// none of the symmetry a lattice has.
    ///
    /// A lattice is the one arrangement that never asks an awkward question — every contact is
    /// the same contact. This is where the awkward ones are, and where an overlap that only
    /// happens at some particular angle or depth will show up.
    Scatter,
    /// A hex raft of three rings, 37 cells. A cell in the middle has six neighbours at the same
    /// distance, which is the arrangement a monolayer settles into.
    Raft,
}

impl Layout {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Layout::Pair => "pair",
            Layout::Nine => "nine",
            Layout::Fifteen => "fifteen",
            Layout::Hex => "hex",
            Layout::Scatter => "scatter",
            Layout::Raft => "raft",
        }
    }

    #[must_use]
    pub fn next(self) -> Self {
        let all = Layout::ALL;
        let i = all.iter().position(|l| *l == self).map_or(0, |i| i + 1);
        all[i % all.len()]
    }

    /// Every arrangement, smallest first.
    pub const ALL: [Layout; 6] = [
        Layout::Pair,
        Layout::Nine,
        Layout::Fifteen,
        Layout::Hex,
        Layout::Scatter,
        Layout::Raft,
    ];
}

/// What is moving, which is the whole question: the artefact is reported not to appear at all
/// when the slide is still, and to appear with the least movement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    /// Nothing moves. The control, and the condition the artefact is known *not* to appear in —
    /// which is why five earlier measurements taken here could never have shown it.
    Still,
    /// The whole clump slides along a slow circle, rigidly. Every cell keeps its neighbours and
    /// its distances exactly; all that changes is where the outlines land on the pixel grid.
    /// **This is the sharpest test in the bench.** If the artefact appears under it, no seam,
    /// swell or contact can be responsible, because none of them changed.
    Drift,
    /// Every cell takes an independent step each frame. The Brownian jitter, and the one motion
    /// that genuinely changes the distances the seams are computed from.
    Jitter,
    /// The clump turns about its centre. Rigid like `Drift`, but every cell's *direction* to
    /// every neighbour sweeps, so the seam normals move without the faces doing so.
    Orbit,
    /// The radii breathe in and out. Nothing moves; the cells change size, which is what the
    /// `mm-core` radius staircase does to a settled pack one tick at a time.
    Breathe,
}

impl Motion {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Motion::Still => "still",
            Motion::Drift => "drift",
            Motion::Jitter => "jitter",
            Motion::Orbit => "orbit",
            Motion::Breathe => "breathe",
        }
    }

    /// In the order a bisection wants them: still, then the two rigid motions that cannot change
    /// the data, then the two that can.
    pub const ALL: [Motion; 5] = [
        Motion::Still,
        Motion::Drift,
        Motion::Orbit,
        Motion::Jitter,
        Motion::Breathe,
    ];
}

/// How big a phantom cell is, in squares.
///
/// The size the scenes in `tests/` produce: `mm-core` radius `0.25 + sqrt(mass) * 0.125` at the
/// masses the packing bench seeds, which is a little under a square across.
const BASE_RADIUS: f32 = 0.9;

/// The scene, and every fault that can be injected into it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bench {
    pub layout: Layout,
    /// Centre-to-centre distance as a fraction of the two physical radii summed.
    ///
    /// One is the state the physics drives every pack towards — exactly touching, zero overlap —
    /// and therefore the state the picture has to look right in. Below one the cells genuinely
    /// interpenetrate; above about 1.3 nothing touches and every cell is drawn round.
    pub spacing: f32,
    /// How much the radii vary across the population. Zero is a lattice of identical cells, which
    /// is the one arrangement that never asks an awkward question.
    pub spread: f32,
    /// A fixed jog off the lattice, per cell, as a fraction of the pitch.
    ///
    /// Nothing in a real slide is on a lattice, and a lattice is where an off-by-one hides: every
    /// distance is one of two or three values, every seam normal is an exact axis or an exact
    /// sixty degrees, and every cell centre lands on the same sub-pixel phase. A sign error on a
    /// normal, a ray index rounded the wrong way, a wall measured from the wrong side — all of
    /// them can be *exactly* cancelled by symmetry and show nothing.
    ///
    /// This is fixed per cell rather than per frame, so it is part of the arrangement and not a
    /// motion: the rigid motions stay rigid, a frame is still a pure function of its number, and
    /// the scene is still the same scene every run.
    pub dither: f32,
    pub motion: Motion,
    /// How far things move, in squares. The default is a twentieth of a cell, which at any
    /// ordinary magnification is a fraction of a pixel.
    pub amplitude: f32,
    /// How fast, in radians per frame.
    pub speed: f32,
    /// How firmly a cell holds its own shape, `0..=1`. **The foam-to-marbles slider.**
    ///
    /// This is the one knob here that is not a fault to inject. Everything else in this struct
    /// exists to reproduce something going wrong; this one asks what the picture *should* be.
    ///
    /// Zero is a bag of fluid: the area-preserving swell is applied in full, so a cell squeezed
    /// on one side bulges on the other until what survives its neighbours' seams encloses the
    /// area it has. A crowd of them tiles into polygons with no gaps — a moss leaf.
    ///
    /// One is a walled body: no swell at all, so a cell is drawn at `PACKING` times its radius,
    /// cut by its seams and no more. A crowd of them stays round with gaps between — a smear of
    /// yeast. This was `swell: bool` and its two states are this knob's two ends.
    ///
    /// In between is in between, and the point of it being continuous is that nobody has yet
    /// established which end the interesting pictures are near, or whether the middle is a
    /// picture at all rather than a smear of the two.
    ///
    /// **What sets it on a real slide is deliberately not decided here.** `slide::squash_of`
    /// currently derives it from junctions and `biology::rigidity`, which is one answer; this
    /// bench exists so the *look* can be settled before the mechanism is argued about, the same
    /// way the rest of the module separates the shader from the simulation.
    pub firmness: f32,
    /// How many seams reach the shader, deepest first. Twelve is `cellmesh::SQUASH_PER_CELL`;
    /// anything less is the truncation that was found dropping neighbours in bucket order.
    pub cap: usize,
    /// How far a cell looks for a neighbour, in multiples of the two *drawn* radii summed.
    ///
    /// One is exactly the outlines touching. `slide::PACKING_PERMILLE` works out to about 1.52 of
    /// this measure; below one, a pair overlaps on screen having never been offered as a contact,
    /// which is one of the two faults found last week.
    pub reach: f32,
    /// Chance per cell per frame that one of its seams is dropped on the floor.
    ///
    /// The contact set was measured churning six tenths of a contact per cell per tick. This is
    /// that, injected: set it to 0.6 and the phantom should look like the slide does, if churn is
    /// what the slide is suffering from.
    pub churn: f32,
    /// Quantise the radius to `mm-core`'s staircase — `0.25 + isqrt(mass) * 0.125` — instead of
    /// the smooth curve the front end draws. The pre-`drawn_radius` behaviour, as a control.
    pub staircase: bool,
}

impl Default for Bench {
    fn default() -> Self {
        Bench {
            layout: Layout::Nine,
            spacing: 1.0,
            spread: 0.3,
            // Small enough that a lattice still reads as one, large enough that no two distances,
            // no two normals and no two sub-pixel phases are ever the same.
            dither: 0.1,
            motion: Motion::Drift,
            amplitude: 0.05,
            speed: 0.02,
            firmness: 0.0,
            cap: crate::cellmesh::SQUASH_PER_CELL,
            reach: 1.52,
            churn: 0.0,
            staircase: false,
        }
    }
}

/// Deterministic noise, so a frame number is the whole state of the bench.
///
/// Nothing here may consult a clock or a generator: the point of the phantom is that frame 4102
/// is the same picture every time it is reached, so a screenshot can be compared with one taken
/// yesterday and a step backwards is possible at all.
fn hash(a: u64, b: u64) -> u64 {
    let mut x = a
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(b.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// `0..1` from a hash.
fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / ((1u64 << 24) as f32)
}

/// `-1..1` from a hash.
fn signed(h: u64) -> f32 {
    unit(h) * 2.0 - 1.0
}

impl Bench {
    /// Where the cells are at `frame`, before anything is drawn.
    ///
    /// A closed form in the frame number and nothing else. There is no integration, so there is
    /// no state to accumulate and no way for one frame to be the cause of the next.
    #[must_use]
    pub fn blobs(&self, frame: u64) -> Vec<Blob> {
        let t = frame as f32 * self.speed;
        let mut out = Vec::new();
        for (id, (gx, gy, size)) in self.sites().into_iter().enumerate() {
            let id = id as u64;
            // Off the lattice, once and for all. See `dither`.
            let pitch = 2.0 * BASE_RADIUS * self.spacing;
            let gx = gx + self.dither * pitch * signed(hash(id, 0xD1));
            let gy = gy + self.dither * pitch * signed(hash(id, 0xD2));
            let mut r = size * BASE_RADIUS;
            // The physical radius `mm-core` would report, if it were reporting one: an integer
            // square root of an integer mass, so the radius lands on an eighth of a square and
            // arrives there all at once. See `slide::drawn_radius` for why the front end does not
            // draw this directly.
            if self.staircase {
                r = 0.25 + ((r - 0.25) / 0.125).floor() * 0.125;
            }
            let (mut x, mut y) = (gx, gy);
            match self.motion {
                Motion::Still => {}
                // Rigid: the same offset for everybody, so no distance between any two cells
                // changes by so much as a float.
                Motion::Drift => {
                    x += self.amplitude * t.cos();
                    y += self.amplitude * t.sin();
                }
                // Also rigid, and about the clump's own centre, which is the origin.
                Motion::Orbit => {
                    let a = self.amplitude * t;
                    let (s, c) = a.sin_cos();
                    let (px, py) = (x, y);
                    x = px * c - py * s;
                    y = px * s + py * c;
                }
                // The only motion that moves cells relative to each other.
                Motion::Jitter => {
                    x += self.amplitude * signed(hash(id, frame));
                    y += self.amplitude * signed(hash(id, frame ^ 0x5EED));
                }
                // Nothing moves; the sizes do.
                Motion::Breathe => {
                    let phase = unit(hash(id, 0)) * std::f32::consts::TAU;
                    r *= 1.0 + self.amplitude * (t + phase).sin();
                }
            }
            out.push(Blob { x, y, r, id });
        }
        out
    }

    /// The lattice, as `(x, y, size)` with the sizes about one.
    fn sites(&self) -> Vec<(f32, f32, f32)> {
        // Centre distance for two cells of the base size at the current spacing.
        let pitch = 2.0 * BASE_RADIUS * self.spacing;
        let size = |k: u64| 1.0 + self.spread * signed(hash(k, 0x5153));
        match self.layout {
            Layout::Pair => {
                // Placed from their own radii rather than from the lattice, so that `spacing` is
                // exactly what it says for the one case where the two cells are different sizes.
                let (a, b) = (1.0 + self.spread, 1.0 - self.spread);
                let d = self.spacing * (a + b) * BASE_RADIUS;
                vec![(-d * 0.5, 0.0, a), (d * 0.5, 0.0, b)]
            }
            Layout::Nine => (0..9u64)
                .map(|k| {
                    let (cx, cy) = ((k % 3) as f32 - 1.0, (k / 3) as f32 - 1.0);
                    (cx * pitch, cy * pitch, size(k))
                })
                .collect(),
            Layout::Fifteen => (0..15u64)
                .map(|k| {
                    let (cx, cy) = ((k % 5) as f32 - 2.0, (k / 5) as f32 - 1.0);
                    (cx * pitch, cy * pitch, size(k))
                })
                .collect(),
            Layout::Hex => (0..15u64)
                .map(|k| {
                    let (col, row) = ((k % 5) as f32, (k / 5) as f32);
                    // Alternate rows offset by half a pitch, rows √3/2 apart: the lattice whose
                    // every cell has six neighbours at exactly one pitch.
                    let stagger = if (k / 5) % 2 == 1 { 0.5 } else { 0.0 };
                    (
                        (col - 2.0 + stagger) * pitch,
                        (row - 1.0) * pitch * 0.866_025_4,
                        size(k),
                    )
                })
                .collect(),
            Layout::Scatter => {
                // A sunflower spiral — the golden angle, radius as the square root of the index —
                // which fills a disc evenly without ever repeating a local arrangement, and then
                // jogged by a third of a pitch so that even the spiral's own regularity goes.
                //
                // The radius is set so fifteen cells of the base size cover about four fifths of
                // the disc, which is crowded enough that most pairs touch and some are deep in
                // each other.
                let n = 15u64;
                let disc = pitch * 0.5 * (n as f32 / 0.8).sqrt();
                (0..n)
                    .map(|k| {
                        let r = disc * ((k as f32 + 0.5) / n as f32).sqrt();
                        let a = k as f32 * 2.399_963_2; // the golden angle, in radians
                        let jog = pitch * 0.33;
                        (
                            r * a.cos() + jog * signed(hash(k, 0xA1)),
                            r * a.sin() + jog * signed(hash(k, 0xB2)),
                            size(k),
                        )
                    })
                    .collect()
            }
            Layout::Raft => {
                let mut out = vec![(0.0, 0.0, size(0))];
                let mut k = 1u64;
                for ring in 1..=3i32 {
                    // A hex ring: six corners, `ring` cells along each edge between them.
                    for corner in 0..6 {
                        let a0 = std::f32::consts::FRAC_PI_3 * corner as f32;
                        let a1 = std::f32::consts::FRAC_PI_3 * (corner + 1) as f32;
                        let (x0, y0) = (a0.cos() * ring as f32, a0.sin() * ring as f32);
                        let (x1, y1) = (a1.cos() * ring as f32, a1.sin() * ring as f32);
                        for step in 0..ring {
                            let f = step as f32 / ring as f32;
                            out.push((
                                (x0 + (x1 - x0) * f) * pitch,
                                (y0 + (y1 - y0) * f) * pitch,
                                size(k),
                            ));
                            k += 1;
                        }
                    }
                }
                out
            }
        }
    }

    /// The cells as the shader will get them: seams and swell, all-pairs.
    ///
    /// Every step here is `slide::squash_of`'s, in the same order and calling the same functions.
    /// What is different is where the neighbours come from — every other cell, tested directly —
    /// so a seam is missing only if this bench was asked to lose it.
    #[must_use]
    pub fn draw(&self, blobs: &[Blob], frame: u64) -> Vec<Drawn> {
        let mut out = Vec::with_capacity(blobs.len());
        for (i, me) in blobs.iter().enumerate() {
            let bare = me.r * PACKING;
            let mut seams: Vec<Squash> = Vec::new();
            for (j, other) in blobs.iter().enumerate() {
                if i == j {
                    continue;
                }
                let (dx, dy) = (other.x - me.x, other.y - me.y);
                let theirs = other.r * PACKING;
                let d = (dx * dx + dy * dy).sqrt();
                // The reach. A pair further apart than this is never offered as a contact — and
                // whether the outlines overlap at that distance is exactly the question.
                if d > self.reach * (bare + theirs) {
                    continue;
                }
                // Rigidity equal, so the seam sits on the plane through the crossing outlines and
                // is not slid towards the softer of the two. One variable fewer.
                if let Some(s) = slide::seam_between(bare, theirs, dx, dy, 1.0, 1.0) {
                    seams.push(s);
                }
            }
            // Then the swell, from the *whole* seam list — which is what `squash_of` does, before
            // anything truncates it.
            // The same expression `slide::squash_of` applies, so the bench and the slide cannot
            // drift: full swell at firmness zero, none at one, linear between.
            let swell = 1.0
                + (1.0 - self.firmness.clamp(0.0, 1.0)) * (slide::area_swell(bare, bare, &seams) - 1.0);
            for s in seams.iter_mut() {
                s.face /= swell;
            }
            // Then the faults, in the order the real path applies them: the contact set drops
            // one, and the mesh keeps the deepest twelve.
            if self.churn > 0.0 && !seams.is_empty() {
                let h = hash(me.id, frame ^ 0xC0FFEE);
                if unit(h) < self.churn {
                    let k = (hash(me.id, frame) as usize) % seams.len();
                    seams.remove(k);
                }
            }
            if seams.len() > self.cap {
                // Deepest first, as `main::squash_of` sorts them. `face` is how far along its
                // normal the seam sits, so smaller is a deeper cut.
                seams.sort_by(|a, b| a.face.partial_cmp(&b.face).unwrap_or(std::cmp::Ordering::Equal));
                seams.truncate(self.cap);
            }
            out.push(Drawn {
                blob: *me,
                bare,
                seams,
                swell,
            });
        }
        out
    }

    /// Everything in one call: place the cells, then draw them.
    #[must_use]
    pub fn frame(&self, frame: u64) -> Vec<Drawn> {
        let blobs = self.blobs(frame);
        self.draw(&blobs, frame)
    }
}

/// A phantom cell with its seams worked out — exactly the per-cell data `cell.wgsl` receives.
#[derive(Clone, PartialEq, Debug)]
pub struct Drawn {
    pub blob: Blob,
    /// The drawn radius before swelling: `PACKING` times the physical one. `bare` in the shader,
    /// and the radius both cells of a pair computed their shared wall from.
    pub bare: f32,
    pub seams: Vec<Squash>,
    pub swell: f32,
}

/// How wide the swell tapers out around a facet, in cosine of angle. `cell.wgsl`'s `TAPER`.
const TAPER: f32 = 0.25;

impl Drawn {
    /// Half the quad, in squares: one field unit is `FIELD_FILL` of this.
    #[must_use]
    pub fn half(&self) -> f32 {
        self.bare * self.swell / FIELD_FILL
    }

    /// The radius the shader draws in direction `theta`, in squares.
    ///
    /// **A copy of the fragment shader**, and the only thing in this module that is. It is here so
    /// that the outline can be measured on a machine with no GPU, which is what turns "it looks
    /// wrong" into a number. `tests/shader_probe.rs` checks the properties both must have; if the
    /// two ever disagree, this one is wrong by definition.
    ///
    /// The shoulder from `smax` is not modelled: it erodes the outline slightly near a seam, by
    /// design and by `radius * 0.035`. So a wall measured here is the geometric one, and the
    /// drawn one is a hair inside it.
    #[must_use]
    pub fn outline(&self, theta: f32, wobble: bool) -> f32 {
        self.outline_field(theta, wobble) * self.half()
    }

    /// The same, in the field's own units, where `FIELD_FILL` is the swollen radius.
    #[must_use]
    pub fn outline_field(&self, theta: f32, wobble: bool) -> f32 {
        let (sy, sx) = theta.sin_cos();
        let bare = FIELD_FILL / self.swell;
        // How much of the swell this direction gets: one out in a free arc, nothing across a
        // facet, and the taper between. See `seam_room` in `cell.wgsl`.
        let mut room = 1.0f32;
        for s in &self.seams {
            let face = s.face * FIELD_FILL;
            let edge_cos = face / bare.max(0.0001);
            let along = sx * s.nx + sy * s.ny;
            room = room.min(1.0 - smoothstep(edge_cos - TAPER, edge_cos, along));
        }
        let wob = if wobble { self.wobble(theta) } else { 0.0 };
        let mut r = bare + (FIELD_FILL * (1.0 + wob) - bare) * room;
        // Then cut by each seam. `smax` in the shader; a plain intersection here.
        for s in &self.seams {
            let along = sx * s.nx + sy * s.ny;
            if along > 1e-4 {
                r = r.min(s.face * FIELD_FILL / along);
            }
        }
        r
    }

    /// The three harmonics of private irregularity, damped by how hard the cell is pressed.
    /// `cell.wgsl`'s `wobble`, including the `slack` that scales it.
    ///
    /// `theta` is a direction in the phantom's own frame, whose y runs **down** the screen as the
    /// slide's does. The shader's is the quad's corner, whose y runs **up** — so the angle it
    /// evaluates is the negative of this one and the outline is mirrored about the x axis between
    /// the two frames. Hence the turn-over here, which is the whole difference between an overlay
    /// that sits on the drawn edge and one that hovers a tenth of a radius off it in the free arcs
    /// and meets it only at the seams.
    ///
    /// The seams need no such correction: `main::squash_of` and the bench both negate a seam's
    /// `ny` on the way to the mesh, so the half-planes arrive in the shader's frame already. It is
    /// only the harmonics, which are computed from the angle itself, that have to be turned over
    /// here instead.
    ///
    /// # How closely this matches the shader, measured
    ///
    /// For a cell whose seed is small, exactly: `tools/check_outline.py` on an isolated cell reads
    /// an **rms of 0.008 px** against a radius of 148, which is agreement to a hundredth of a
    /// pixel. For a cell whose seed is large it does not, and the reason is worth recording
    /// because it is a defect in its own right.
    ///
    /// `hash11` begins `fract(p * 0.1031)`, and [`crate::cellmesh::seed_of`] hands it seeds up to
    /// 262144. At that size `p * 0.1031` is around 2·10⁴, where one `f32` ulp is about 0.002 — so
    /// the fraction that survives carries only a few bits, and the hash is *chaotic* in the last
    /// one of them:
    ///
    /// ```text
    ///   id 1   seed 162013.9   hash11 0.8202   one ulp along: 0.6385
    ///   id 2    seed 61883.8   hash11 0.6859   one ulp along: 0.1726
    /// ```
    ///
    /// So whether this and the GPU agree comes down to whether both rounded `seed * 0.1031` the
    /// same way, which nothing guarantees — a fused multiply-add on one side and not the other is
    /// enough. Measured on that cell: rms 2.1 px of outline, on an amplitude of 13% of the radius.
    ///
    /// It is not a flicker: a seed is fixed for a cell's life, so whatever silhouette a given GPU
    /// arrives at, it keeps. What it does mean is that a cell's outline is not reproducible across
    /// machines, that the silhouettes are drawn from far fewer distinct shapes than `seed_of`
    /// intends, and that nothing on the CPU can predict what the shader will draw. `seed_of`'s own
    /// documentation says the seed is "kept small because the shader hashes it as an `f32` and a
    /// large integer loses its low bits to the mantissa" — the intent is right and the shift does
    /// not achieve it.
    #[must_use]
    pub fn wobble(&self, theta: f32) -> f32 {
        let theta = -theta;
        let seed = crate::cellmesh::seed_of(self.blob.id);
        let slack = 1.0 - 0.96 * self.pressure();
        let a1 = (0.055 + 0.045 * hash11(seed)) * slack;
        let a2 = (0.030 + 0.030 * hash11(seed + 7.0)) * slack;
        let a3 = (0.015 + 0.020 * hash11(seed + 13.0)) * slack;
        a1 * (3.0 * theta + hash11(seed + 3.0) * std::f32::consts::TAU).sin()
            + a2 * (5.0 * theta + hash11(seed + 11.0) * std::f32::consts::TAU).sin()
            + a3 * (7.0 * theta + hash11(seed + 17.0) * std::f32::consts::TAU).sin()
    }

    /// How hard this cell is being squeezed, off the nearest seam. `cell.wgsl`'s `pressure`.
    ///
    /// The quantity the wobble is damped by, and the one measured swinging by twenty-five times
    /// on a clump that had barely moved.
    #[must_use]
    pub fn pressure(&self) -> f32 {
        let nearest = self
            .seams
            .iter()
            .map(|s| s.face)
            .fold(crate::cellmesh::NO_SQUASH, f32::min);
        ((1.0 - nearest) / 0.25).clamp(0.0, 1.0)
    }

    /// Where this cell's outline ends along the line towards a point, in squares.
    #[must_use]
    pub fn reach_towards(&self, x: f32, y: f32, wobble: bool) -> f32 {
        self.outline((y - self.blob.y).atan2(x - self.blob.x), wobble)
    }
}

/// `cell.wgsl`'s `hash11`, which is the same shape `art.rs` uses in the space WGSL has.
fn hash11(p: f32) -> f32 {
    let mut x = (p * 0.1031).fract();
    x *= x + 33.33;
    x *= x + x;
    x.fract()
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What one frame of the phantom looks like, as numbers.
///
/// The same detector `tests/nine_cells.rs` runs on the real slide, so the two are directly
/// comparable — plus the wall error, which needs the outline and therefore could not be measured
/// before this module existed.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Report {
    pub cells: usize,
    /// Pairs whose drawn outlines reach past each other.
    pub touching: usize,
    /// Of those, the pairs where at least one side has no seam pointing at the other — a cell
    /// drawn over its neighbour with no wall between them, which is the artefact.
    pub no_wall: usize,
    /// The deepest such overlap, as a fraction of the smaller cell's drawn radius.
    pub worst: f32,
    pub max_seams: usize,
    /// How far one cell's outline is drawn *past* where its neighbour's ends, in squares, at the
    /// worst pair.
    ///
    /// Zero is the whole design: the seam is one plane both cells computed from the same two
    /// centres and the same two radii, so along the line between them the two outlines must end at
    /// the same point and nothing may cross. Anything above the width of a pixel is one cell
    /// drawn over another, which is the artefact — whether or not either of them has a seam.
    pub wall_cross: f32,
    /// The worst daylight left between two cells that are genuinely pressed together, in squares.
    ///
    /// Measured only where the two *unswollen* drawn circles overlap, which is a pair that has a
    /// real chord between them. This is the other half of the complaint the swell exists to
    /// answer: a pack with gaps reads as a pile of pebbles rather than as tissue.
    pub wall_gap: f32,
    pub swell_lo: f32,
    pub swell_hi: f32,
}

/// Measure one frame.
#[must_use]
pub fn inspect(cells: &[Drawn]) -> Report {
    let mut r = Report {
        cells: cells.len(),
        swell_lo: f32::MAX,
        swell_hi: 0.0,
        worst: 0.0,
        ..Report::default()
    };
    for c in cells {
        r.max_seams = r.max_seams.max(c.seams.len());
        r.swell_lo = r.swell_lo.min(c.swell);
        r.swell_hi = r.swell_hi.max(c.swell);
    }
    if cells.is_empty() {
        r.swell_lo = 1.0;
        r.swell_hi = 1.0;
    }
    for (a, i) in cells.iter().enumerate() {
        for j in cells.iter().skip(a + 1) {
            let (dx, dy) = (j.blob.x - i.blob.x, j.blob.y - i.blob.y);
            let d = (dx * dx + dy * dy).sqrt();
            if d <= 1e-4 {
                continue;
            }
            let (ux, uy) = (dx / d, dy / d);
            // Where each one's outline actually ends along the line between them. Both should
            // stop on the shared wall, so the two should sum to `d`.
            let mine = i.reach_towards(j.blob.x, j.blob.y, true);
            let theirs = j.reach_towards(i.blob.x, i.blob.y, true);
            // A pair that needs a wall: uncut, the two drawn outlines would reach past each other.
            // Asked of the swollen radii rather than of where the outlines actually end, because
            // where they end is the answer and not the question — a pair that is correctly walled
            // has its two outlines meeting at exactly `d`, so testing that would report a packed
            // sheet as having no contacts at all. The same measure `tests/nine_cells.rs` takes of
            // the real slide, so the two are comparable.
            let reach = i.bare * i.swell + j.bare * j.swell;
            if d < reach {
                r.touching += 1;
                let sees =
                    |c: &Drawn, x: f32, y: f32| c.seams.iter().any(|s| s.nx * x + s.ny * y > 0.999);
                if !sees(i, ux, uy) || !sees(j, -ux, -uy) {
                    r.no_wall += 1;
                    r.worst = r.worst.max((reach - d) / i.bare.min(j.bare).max(1e-4));
                }
            }
            // Crossing is a fault wherever it happens and needs no qualification: the two
            // outlines have run past each other, so there are pixels both cells claim.
            r.wall_cross = r.wall_cross.max(mine + theirs - d);
            // A gap is only a fault between cells that are actually pressed together. Two cells
            // a radius apart are not leaving daylight, they are simply not touching — so this is
            // asked of pairs whose unswollen drawn circles overlap, which is a pair with a real
            // chord between them.
            if d < i.bare + j.bare {
                r.wall_gap = r.wall_gap.max(d - mine - theirs);
            }
        }
    }
    r
}

/// What changed between two frames.
///
/// The flicker metric. A cell whose swell jumps eleven percent between one frame and the next is
/// a lobe appearing over a neighbour and going away again, and that was measured on a real pack
/// with the seam set unchanged — so it is worth knowing whether the phantom does it too, and
/// under which motion.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Flicker {
    /// The largest change in any cell's swell, as a fraction.
    pub worst_swell: f32,
    /// How many cells changed size by more than one percent.
    pub resizing: usize,
    /// How many gained or lost a seam.
    pub churned: usize,
    /// The largest change in any cell's outline in a fixed direction, as a fraction of its own
    /// radius. Directions are sampled, so this is a floor rather than a maximum.
    pub worst_outline: f32,
}

/// Compare two frames of the same scene. They must be the same population in the same order.
#[must_use]
pub fn flicker(prev: &[Drawn], now: &[Drawn]) -> Flicker {
    let mut f = Flicker::default();
    for (a, b) in prev.iter().zip(now.iter()) {
        let ds = (b.swell - a.swell).abs() / a.swell.max(1e-4);
        f.worst_swell = f.worst_swell.max(ds);
        if ds > 0.01 {
            f.resizing += 1;
        }
        if a.seams.len() != b.seams.len() {
            f.churned += 1;
        }
        for k in 0..32 {
            let theta = std::f32::consts::TAU * k as f32 / 32.0;
            let d = (b.outline(theta, true) - a.outline(theta, true)).abs() / a.bare.max(1e-4);
            f.worst_outline = f.worst_outline.max(d);
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_a_function_of_its_number_and_nothing_else() {
        // The whole bench rests on this: a screenshot of frame 4102 taken today and one taken
        // next week are pictures of the same thing, so they can be diffed.
        let bench = Bench::default();
        assert_eq!(bench.frame(4102), bench.frame(4102));
        assert_ne!(bench.frame(4102), bench.frame(4103));
    }

    #[test]
    fn a_rigid_motion_changes_nothing_a_seam_depends_on() {
        // Drift moves every cell by the same offset, so no distance between any two of them
        // changes. If the picture changes under it, nothing in the data can be the cause — which
        // is the one measurement this bench exists to make.
        let bench = Bench {
            motion: Motion::Drift,
            amplitude: 0.05,
            ..Bench::default()
        };
        let a = bench.frame(0);
        let b = bench.frame(97);
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.seams.len(), y.seams.len(), "a seam appeared or vanished");
            assert!(
                (x.swell - y.swell).abs() < 1e-5,
                "the swell moved under a rigid translation: {} to {}",
                x.swell,
                y.swell
            );
            for (p, q) in x.seams.iter().zip(y.seams.iter()) {
                assert!((p.face - q.face).abs() < 1e-5, "a face moved");
            }
        }
    }

    #[test]
    fn two_cells_agree_on_the_wall_between_them() {
        // The property the whole seam scheme rests on, on data that cannot be blamed: both
        // outlines have to end on the same line, so the two reaches sum to the distance between
        // the centres.
        let bench = Bench {
            layout: Layout::Pair,
            motion: Motion::Still,
            ..Bench::default()
        };
        let cells = bench.frame(0);
        let r = inspect(&cells);
        assert_eq!(r.cells, 2);
        assert_eq!(r.touching, 1, "the pair is not in contact at all");
        assert!(
            r.wall_cross < 1e-3 && r.wall_gap < 1e-3,
            "two cells disagree about their shared wall: {:.6} squares of crossing, {:.6} of gap",
            r.wall_cross,
            r.wall_gap,
        );
    }

    #[test]
    fn every_cell_of_a_raft_finds_its_neighbours() {
        let bench = Bench {
            layout: Layout::Raft,
            motion: Motion::Still,
            ..Bench::default()
        };
        let cells = bench.frame(0);
        assert_eq!(cells.len(), 37);
        // The middle of a hex raft has six, and the bench must not be quietly starving it.
        assert!(
            cells[0].seams.len() >= 6,
            "the centre of the raft has {} seams",
            cells[0].seams.len()
        );
    }

    #[test]
    fn the_outline_stops_on_the_seam_and_not_on_the_swollen_radius() {
        // The taper's whole job. A cell with one neighbour swells into the whole of its free arc
        // and gives every bit of that back across the facet, so the outline down the seam normal
        // is the *unswollen* wall the pair agreed on — measured on a pair, where there is only
        // one seam and so nothing else the minimum could be coming from.
        let bench = Bench {
            layout: Layout::Pair,
            motion: Motion::Still,
            ..Bench::default()
        };
        for c in bench.frame(0) {
            let s = *c.seams.first().expect("a pair has one seam each");
            let theta = s.ny.atan2(s.nx);
            let want = s.face * c.swell * c.bare;
            let got = c.outline(theta, true);
            assert!(
                (got - want).abs() < 1e-3,
                "the outline ends at {got:.4} where the seam is at {want:.4}"
            );
            // And out the other side there is nothing in the way, so it is the full swollen
            // radius plus whatever the wobble is doing — which is where the swell was meant to go.
            let free = c.outline(theta + std::f32::consts::PI, false);
            assert!(
                free > c.bare * 0.99,
                "the free arc is drawn at {free:.4}, inside the unswollen {:.4}",
                c.bare
            );
        }
    }

    #[test]
    fn no_direction_is_drawn_past_a_seam_that_faces_it() {
        // The general form of the above, over a crowded cell: whatever the minimum turns out to
        // be, it is never *outside* any facing seam. A cell that crosses one of its own walls is
        // the artefact, and here there is no data to blame for it.
        let bench = Bench::default();
        for c in bench.frame(0) {
            for s in &c.seams {
                let theta = s.ny.atan2(s.nx);
                let wall = s.face * c.swell * c.bare;
                let got = c.outline(theta, true);
                assert!(
                    got <= wall + 1e-3,
                    "cell {} is drawn to {got:.4} across a wall at {wall:.4}",
                    c.blob.id
                );
            }
        }
    }
}
