//! The microscope's optics, as data (SPEC §14).
//!
//! > The substrate is presented as a slide plate under a microscope: circular vignette,
//! > subtle depth-of-field falling off from the focal plane, faint chromatic aberration at
//! > the edge of the field, dust motes.
//!
//! # Why this is a module and not a shader
//!
//! Most of it *is* a shader in the end. But the parameters that drive it — how far off focus
//! a cell is, how strong the vignette is at a given radius, where a dust mote is this tick —
//! are decisions, and decisions want testing. Keeping them here means the look of the
//! microscope is checkable on a machine with no GPU, and the shader is left doing only what a
//! shader is good at.
//!
//! # Dust motes do not have a clock
//!
//! A mote's position is a pure function of its index and the tick, using the same hash the
//! simulation's randomness uses. So the dust drifts the same way every time a run is
//! replayed, a recording lines up with the world it was recorded from, and — most
//! importantly — nothing here needs a wall-clock, which is the one thing that could smuggle
//! frame timing into a scene.
//!
//! Motes are drawn *over* the slide and are not in it: they are on the objective lens, not in
//! the water, so they do not move with the fluid and cells do not collide with them. That is
//! also why they are allowed to be floats and to ignore the simulation entirely.

/// Everything the microscope's look is made of.
///
/// Defaults are the "as seen through a real objective" setting. Turning them all off gives a
/// flat orthographic view, which is what the M2 viewer was and what a screenshot for a bug
/// report should be.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Optics {
    /// Fraction of the half-diagonal at which the vignette starts darkening, `0..=1`.
    pub vignette_start: f32,
    /// How dark the very corner goes, `0..=1`. Zero is black.
    pub vignette_depth: f32,
    /// Focal plane, in the same depth units as [`depth_of`]. Zero is the slide surface.
    pub focus: f32,
    /// Depth either side of `focus` that stays sharp.
    pub depth_of_field: f32,
    /// Blur radius in pixels at maximum defocus.
    pub max_blur: f32,
    /// Channel separation in pixels at the edge of the field. Faint on purpose: this is a
    /// good objective, not a bad one.
    pub aberration: f32,
    /// How many dust motes are on the lens.
    pub motes: u32,
    /// Whether any of this is applied at all.
    pub enabled: bool,
}

impl Default for Optics {
    fn default() -> Optics {
        Optics {
            vignette_start: 0.55,
            vignette_depth: 0.72,
            focus: 0.0,
            depth_of_field: 0.35,
            max_blur: 3.5,
            aberration: 1.25,
            motes: 48,
            enabled: true,
        }
    }
}

impl Optics {
    /// The flat view: no vignette, everything in focus, no motes.
    #[must_use]
    pub fn flat() -> Optics {
        Optics {
            enabled: false,
            ..Optics::default()
        }
    }

    /// Brightness multiplier at a point, given its distance from the centre of the field as a
    /// fraction of the half-diagonal.
    ///
    /// Smoothstepped rather than linear so the edge of the field has no visible ring; a linear
    /// ramp shows its kink exactly where the eye is drawn.
    #[must_use]
    pub fn vignette(&self, radius: f32) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        let r = radius.clamp(0.0, 1.0);
        if r <= self.vignette_start {
            return 1.0;
        }
        let span = (1.0 - self.vignette_start).max(f32::EPSILON);
        let t = ((r - self.vignette_start) / span).clamp(0.0, 1.0);
        let smooth = t * t * (3.0 - 2.0 * t);
        1.0 - self.vignette_depth * smooth
    }

    /// Blur radius in pixels for something at `depth`.
    ///
    /// Flat inside the depth of field and then rising, which is how a real objective behaves
    /// and, more to the point, means nothing you are actually looking at is ever slightly
    /// soft.
    #[must_use]
    pub fn blur(&self, depth: f32) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        let off = (depth - self.focus).abs();
        if off <= self.depth_of_field {
            return 0.0;
        }
        let over = off - self.depth_of_field;
        (over / self.depth_of_field.max(f32::EPSILON)).min(1.0) * self.max_blur
    }

    /// Channel separation in pixels at a given field radius. Zero at the centre by
    /// construction — an objective that smeared colour in the middle would be a broken one.
    #[must_use]
    pub fn separation(&self, radius: f32) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        let r = radius.clamp(0.0, 1.0);
        self.aberration * r * r
    }
}

/// A cell's depth below the focal plane.
///
/// The simulation is two-dimensional, so depth has to be invented. It is derived from the
/// cell's identity rather than its position, which means a cell keeps its depth as it swims
/// instead of sliding in and out of focus as it crosses invisible lines — the illusion is
/// "cells at slightly different heights in a drop of water", and a cell whose depth changed
/// with x and y would read as a rippling floor instead.
#[must_use]
pub fn depth_of(cell_key: u64) -> f32 {
    let h = mix(cell_key ^ 0x5EED_D00D_F0CA_1111);
    // -1..=1, biased to nothing in particular.
    ((h & 0xFFFF) as f32 / 32_768.0) - 1.0
}

/// One speck on the objective lens.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Mote {
    /// Position as a fraction of the viewport, `0..=1` in each axis.
    pub u: f32,
    pub v: f32,
    /// Radius in pixels.
    pub radius: f32,
    /// How visible it is, `0..=1`. Motes are meant to be noticed only once.
    pub alpha: f32,
}

/// Where the dust is at `tick`.
///
/// Each mote drifts slowly along its own heading and wraps at the edges of the field. Pure
/// function of `(index, tick)`: no accumulated position, so there is no state to diverge and
/// no wall-clock to make two machines disagree.
#[must_use]
pub fn motes(optics: &Optics, tick: u64) -> Vec<Mote> {
    if !optics.enabled {
        return Vec::new();
    }
    (0..optics.motes as u64)
        .map(|i| {
            let a = mix(i.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32);
            let b = mix(a ^ 0x0F1E_2D3C);
            // Speeds are small and irrational-ish relative to each other, so the field never
            // visibly repeats.
            let speed_u = ((a >> 16) & 0x3FF) as f32 / 4_000_000.0;
            let speed_v = ((b >> 16) & 0x3FF) as f32 / 4_000_000.0;
            let t = tick as f32;
            let u = fract((a & 0xFFFF) as f32 / 65_536.0 + speed_u * t);
            let v = fract((b & 0xFFFF) as f32 / 65_536.0 + speed_v * t);
            Mote {
                u,
                v,
                radius: 0.7 + ((a >> 40) & 7) as f32 * 0.45,
                alpha: 0.05 + ((b >> 40) & 15) as f32 * 0.006,
            }
        })
        .collect()
}

fn fract(v: f32) -> f32 {
    let f = v - v.floor();
    // `floor` on a large enough f32 can give back the input exactly, which would pin a mote
    // to an edge forever rather than wrapping it.
    if f.is_finite() {
        f.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The simulation's mixer, reused so the dust is as reproducible as everything else.
fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^ (x >> 33)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_centre_of_the_field_is_untouched() {
        let o = Optics::default();
        assert_eq!(o.vignette(0.0), 1.0);
        assert_eq!(o.separation(0.0), 0.0);
        assert_eq!(o.blur(0.0), 0.0);
    }

    #[test]
    fn the_vignette_only_ever_darkens() {
        let o = Optics::default();
        let mut last = 1.0;
        for step in 0..=100 {
            let v = o.vignette(step as f32 / 100.0);
            assert!(v <= last + 1e-6, "vignette brightened at r={step}");
            assert!((0.0..=1.0).contains(&v));
            last = v;
        }
        assert!(
            o.vignette(1.0) < 0.5,
            "the corner of the field is not noticeably dark"
        );
    }

    #[test]
    fn what_is_in_focus_is_sharp() {
        let o = Optics::default();
        assert_eq!(o.blur(o.focus + o.depth_of_field * 0.99), 0.0);
        assert!(o.blur(o.focus + o.depth_of_field * 2.0) > 0.0);
        assert!(o.blur(1000.0) <= o.max_blur, "blur is bounded");
        // Symmetric: above and below the focal plane look the same.
        assert_eq!(o.blur(0.8), o.blur(-0.8));
    }

    #[test]
    fn flat_optics_do_nothing() {
        let o = Optics::flat();
        assert_eq!(o.vignette(1.0), 1.0);
        assert_eq!(o.blur(50.0), 0.0);
        assert_eq!(o.separation(1.0), 0.0);
        assert!(motes(&o, 12_345).is_empty());
    }

    #[test]
    fn a_cell_keeps_its_depth() {
        // The point of deriving depth from identity: it does not change as the cell moves,
        // because it does not depend on where the cell is.
        let d = depth_of(42);
        assert_eq!(d, depth_of(42));
        assert!((-1.0..=1.0).contains(&d));
        let spread: Vec<f32> = (0..64).map(depth_of).collect();
        assert!(
            spread.iter().any(|v| *v < -0.3) && spread.iter().any(|v| *v > 0.3),
            "every cell landed on the same plane: {spread:?}"
        );
    }

    #[test]
    fn dust_drifts_but_stays_in_the_field() {
        let o = Optics::default();
        let a = motes(&o, 0);
        let b = motes(&o, 10_000);
        assert_eq!(a.len(), o.motes as usize);
        assert_ne!(a, b, "the dust never moved");
        for m in a.iter().chain(b.iter()) {
            assert!((0.0..=1.0).contains(&m.u), "mote left the field: {m:?}");
            assert!((0.0..=1.0).contains(&m.v), "mote left the field: {m:?}");
            assert!(m.alpha > 0.0 && m.alpha < 0.2, "dust should be faint");
        }
    }

    #[test]
    fn dust_is_reproducible() {
        // Same tick, same dust — on any machine, in any run. This is what lets a recording
        // line up with the world it was recorded from.
        let o = Optics::default();
        assert_eq!(motes(&o, 7_777), motes(&o, 7_777));
    }
}
