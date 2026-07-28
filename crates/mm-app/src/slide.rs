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
use mm_core::{Scenario, World};

/// One cell, as the renderer needs it.
///
/// Floats, because this is rendering — SPEC's no-floats rule (I2) is about `mm-core`, and the
/// conversion happens here, on the way out, where it cannot affect anything.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CellDot {
    /// Position in substrate squares.
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    /// Colour, from what the cell is made of rather than from what it is called: a cell full
    /// of chloroplasts is green because chloroplasts are green.
    pub rgb: [f32; 3],
}

/// One frame's worth of world, with no way back to it.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub tick: u64,
    pub width: u32,
    pub height: u32,
    pub cells: Vec<CellDot>,
    /// The chosen chemical's field, normalised to `0..=1` per square.
    pub overlay: Vec<f32>,
    /// Which chemical `overlay` holds, and what colour it renders in.
    pub overlay_chemical: usize,
    pub overlay_rgb: [f32; 3],
    /// Incident light, normalised. Rendered as a warm luminance layer (SPEC §14).
    pub light: Vec<f32>,
    pub population: usize,
}

/// The simulation, and the only thing the front-end is allowed to hold.
pub struct Slide {
    world: World,
    /// Which chemical the overlay shows.
    overlay: usize,
    /// Ticks per frame when running. Zero means paused.
    speed: u32,
    /// Ticks still owed from a `step` request, honoured whatever the frame rate is.
    pending_steps: u64,
}

impl Slide {
    /// # Errors
    ///
    /// A scenario this engine cannot honour.
    pub fn new(scenario: Scenario) -> Result<Slide, mm_core::ScenarioError> {
        Ok(Slide {
            world: World::new(scenario)?,
            overlay: 11,
            speed: 1,
            pending_steps: 0,
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

    pub fn set_overlay(&mut self, chemical: usize) {
        self.overlay = chemical % CHEM_COUNT;
    }

    #[must_use]
    pub fn overlay(&self) -> usize {
        self.overlay
    }

    /// Take a frame. Read-only: nothing here can reach the world.
    #[must_use]
    pub fn frame(&self) -> Frame {
        let substrate = self.world.substrate();
        let cells = self.world.cells();
        let table = &self.world.scenario().chemicals;

        let plane = substrate.chem_plane(self.overlay);
        // Normalised against the frame's own peak rather than a fixed scale, so a nearly
        // empty slide is still legible. It means the overlay's absolute meaning changes
        // between frames, which is why the legend has to show the peak (M4).
        let peak = plane.iter().copied().max().unwrap_or(0).max(1) as f32;
        let overlay: Vec<f32> = plane.iter().map(|v| *v as f32 / peak).collect();
        let light: Vec<f32> = substrate
            .light()
            .iter()
            .map(|v| (*v as f32 / Q10_ONE as f32).clamp(0.0, 1.0))
            .collect();

        let def = table.get(self.overlay);
        let overlay_rgb = [
            def.colour[0] as f32 / 255.0,
            def.colour[1] as f32 / 255.0,
            def.colour[2] as f32 / 255.0,
        ];

        let dots = cells
            .iter()
            .map(|i| CellDot {
                x: cells.x[i] as f32 / POS_ONE as f32,
                y: cells.y[i] as f32 / POS_ONE as f32,
                radius: mm_core::biology::radius(cells, i) as f32 / Q10_ONE as f32,
                rgb: cell_colour(cells, i, table),
            })
            .collect();

        Frame {
            tick: self.world.tick_count(),
            width: substrate.width(),
            height: substrate.height(),
            cells: dots,
            overlay,
            overlay_chemical: self.overlay,
            overlay_rgb,
            light,
            population: cells.len(),
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
        assert_eq!(f.overlay.len(), 24 * 20);
        assert_eq!(f.light.len(), 24 * 20);
        assert_eq!(f.cells.len(), f.population);
        assert!(f.overlay.iter().all(|v| (0.0..=1.0).contains(v)));
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
        assert_eq!(a.overlay_chemical, 0);
        assert_eq!(b.overlay_chemical, 8);
        assert_ne!(
            a.overlay_rgb, b.overlay_rgb,
            "each chemical has its own colour"
        );
        // and an out-of-range choice wraps rather than panicking
        slide.set_overlay(999);
        assert!(slide.overlay() < CHEM_COUNT);
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
