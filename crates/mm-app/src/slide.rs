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
    /// Ticks per frame when running. Zero means paused.
    speed: u32,
    /// Ticks still owed from a `step` request, honoured whatever the frame rate is.
    pending_steps: u64,
    /// Detail tier the next frame will be built at.
    lod: Lod,
    /// The microscope's look.
    pub optics: crate::optics::Optics,
    /// Rolling history for the live plots.
    history: MetricHistory,
}

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
            overlays,
            speed: 1,
            pending_steps: 0,
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
        }
    }

    /// Advance by whatever the current speed setting owes, once per frame.
    ///
    /// Paused means paused: zero speed advances nothing, however long the frame took.
    pub fn advance_one_frame(&mut self) {
        let owed = self.pending_steps + self.speed as u64;
        self.pending_steps = 0;
        self.advance(owed);
    }

    /// Ticks per frame. Zero pauses.
    pub fn set_speed(&mut self, ticks_per_frame: u32) {
        self.speed = ticks_per_frame;
    }

    #[must_use]
    pub fn speed(&self) -> u32 {
        self.speed
    }

    /// Advance exactly one tick on the next frame, whatever the speed is. The debugger's
    /// step button (M6), and the thing that makes a paused world inspectable.
    pub fn request_step(&mut self) {
        self.pending_steps = self.pending_steps.saturating_add(1);
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
        let dots = cells
            .iter()
            .map(|i| {
                let id = cells.id_at(i);
                let radius = mm_core::biology::radius(cells, i) as f32 / Q10_ONE as f32;
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
                }
            })
            .collect();

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
    fn the_frame_rate_cannot_change_the_world() {
        // M4 acceptance 3: dropping the render to 5fps must not change tick output or
        // ordering. Here that is the claim that the same total number of ticks gives the same
        // world however they were grouped into frames.
        let mut smooth = Slide::new(scenario()).unwrap();
        smooth.set_speed(1);
        for _ in 0..600 {
            smooth.advance_one_frame();
        }

        let mut stuttering = Slide::new(scenario()).unwrap();
        stuttering.set_speed(12);
        for _ in 0..50 {
            stuttering.advance_one_frame();
        }

        assert_eq!(smooth.world().tick_count(), 600);
        assert_eq!(stuttering.world().tick_count(), 600);
        assert_eq!(
            smooth.world().state_hash(),
            stuttering.world().state_hash(),
            "how the ticks were grouped into frames changed the world"
        );
    }

    #[test]
    fn paused_means_paused() {
        let mut slide = Slide::new(scenario()).unwrap();
        slide.advance(10);
        let hash = slide.world().state_hash();
        slide.set_speed(0);
        for _ in 0..1000 {
            slide.advance_one_frame();
            let _ = slide.frame();
        }
        assert_eq!(slide.world().tick_count(), 10);
        assert_eq!(slide.world().state_hash(), hash);
    }

    #[test]
    fn stepping_a_paused_world_advances_exactly_one_tick() {
        let mut slide = Slide::new(scenario()).unwrap();
        slide.set_speed(0);
        slide.request_step();
        slide.advance_one_frame();
        assert_eq!(slide.world().tick_count(), 1);
        slide.advance_one_frame();
        assert_eq!(
            slide.world().tick_count(),
            1,
            "a step is one step, not a resume"
        );
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
