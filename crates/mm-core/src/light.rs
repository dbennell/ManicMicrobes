//! Light regimes (SPEC §7.3).
//!
//! Light is the only thing that keeps the biosphere from equilibrating. Matter is exactly
//! conserved, so without an energy gradient the world runs down into an all-waste steady
//! state and dies; the chloroplast pathway of §7.2 is what closes the matter loop, and light
//! is what powers it. That makes the *shape of the gradient* the single most consequential
//! scenario knob there is, which is why it is a first-class authored object rather than a
//! constant.
//!
//! The user-facing question is never open-versus-closed. It is: where is the light, and what
//! is it doing over time? A directional gradient makes one edge of the slide worth living on
//! and creates a reason to move. A day/night cycle makes dormancy pay. A slow decline
//! guarantees a mass extinction and asks what survives it. These generate the events the
//! wiki timeline exists to report.
//!
//! Light is *not* conserved and does not participate in I4. It is a prescribed field,
//! recomputed from the regime each fluid step, and it enters the energy ledger only when
//! something absorbs it.

use serde::{Deserialize, Serialize};

use crate::fixed::{q10_scale, Q10_ONE};
use crate::fluid::MAX_VELOCITY;
use crate::state_hash::{StateHash, StateHasher};
use crate::substrate::Substrate;

/// How light falls on the slide.
///
/// All intensities are `Q10`, where [`Q10_ONE`] is "full daylight" — the reference a
/// chloroplast's yield is calibrated against.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum LightRegime {
    /// The same everywhere, forever. The control condition.
    Uniform { intensity: i32 },

    /// Sinusoid-free day/night: a triangular cycle, because a triangle is exact in integers
    /// and a sine is not. Night is `intensity` at its darkest, day at its brightest.
    DayNight {
        period_ticks: u32,
        day: i32,
        night: i32,
    },

    /// Bright at one edge, dark at the other. Makes position worth something, and is the
    /// scenario that phototaxis has a reason to evolve in.
    Directional {
        /// Intensity at the bright edge.
        bright: i32,
        /// Intensity at the dark edge.
        dark: i32,
        /// Which edge is bright.
        from: Edge,
    },

    /// A hydrothermal vent: a point source falling off with distance, and nothing else. The
    /// chemosynthetic world.
    PointSource {
        x: u32,
        y: u32,
        intensity: i32,
        /// Distance in squares at which intensity has fallen to half.
        half_life_squares: u32,
    },

    /// Declining flux over millions of ticks. Adaptation or extinction, and the timeline has
    /// to say which.
    SlowDecline {
        start: i32,
        end: i32,
        over_ticks: u64,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Default for LightRegime {
    fn default() -> Self {
        LightRegime::Uniform { intensity: Q10_ONE }
    }
}

impl LightRegime {
    /// Write this regime's field into the substrate for a given tick.
    ///
    /// A blocked square receives no light: a barrier is opaque, which is what makes drawing
    /// one a way of casting a shadow as well as damming a flow.
    pub fn apply(&self, substrate: &mut Substrate, tick: u64) {
        let w = substrate.width();
        let h = substrate.height();
        let (field, blocked_snapshot) = substrate.light_and_blocked_mut();
        for y in 0..h {
            for x in 0..w {
                let i = (y as usize) * (w as usize) + x as usize;
                let v = if blocked_snapshot[i] {
                    0
                } else {
                    self.intensity_at(x, y, w, h, tick)
                };
                field[i] = v;
            }
        }
    }

    /// Intensity at one square, `Q10`. Never negative.
    #[must_use]
    pub fn intensity_at(&self, x: u32, y: u32, w: u32, h: u32, tick: u64) -> i32 {
        let v = match self {
            LightRegime::Uniform { intensity } => *intensity,

            LightRegime::DayNight {
                period_ticks,
                day,
                night,
            } => {
                let period = (*period_ticks).max(1) as u64;
                let phase = tick % period;
                let half = period / 2;
                // Triangular: up for the first half of the cycle, down for the second.
                let t = if phase < half.max(1) {
                    phase
                } else {
                    period.saturating_sub(phase)
                };
                let num = t.min(half.max(1));
                lerp(*night, *day, num as i64, half.max(1) as i64)
            }

            LightRegime::Directional { bright, dark, from } => {
                let (num, den) = match from {
                    Edge::Left => (x as i64, w.saturating_sub(1).max(1) as i64),
                    Edge::Right => (
                        w.saturating_sub(1).saturating_sub(x) as i64,
                        w.saturating_sub(1).max(1) as i64,
                    ),
                    Edge::Top => (y as i64, h.saturating_sub(1).max(1) as i64),
                    Edge::Bottom => (
                        h.saturating_sub(1).saturating_sub(y) as i64,
                        h.saturating_sub(1).max(1) as i64,
                    ),
                };
                // num counts distance *from* the bright edge, so interpolate bright -> dark.
                lerp(*bright, *dark, num, den)
            }

            LightRegime::PointSource {
                x: sx,
                y: sy,
                intensity,
                half_life_squares,
            } => {
                let dx = (x as i64 - *sx as i64).abs();
                let dy = (y as i64 - *sy as i64).abs();
                // Chebyshev-ish octagonal distance: exact in integers, and close enough to
                // Euclidean that a vent looks round rather than diamond-shaped.
                let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
                let dist = hi + lo / 2;
                let half_life = (*half_life_squares).max(1) as i64;
                // Halve once per half-life, then interpolate linearly within the last one.
                let halvings = (dist / half_life).min(30) as u32;
                let remainder = dist % half_life;
                let base = *intensity >> halvings;
                let next = base / 2;
                lerp(base, next, remainder, half_life)
            }

            LightRegime::SlowDecline {
                start,
                end,
                over_ticks,
            } => {
                let span = (*over_ticks).max(1);
                let t = tick.min(span);
                lerp(*start, *end, t as i64, span as i64)
            }
        };
        v.max(0)
    }

    /// Whether this regime's field depends on the tick. A static field only has to be
    /// written once, which matters when the grid is 512×512 and the step budget is 2ms.
    #[must_use]
    pub fn is_time_varying(&self) -> bool {
        matches!(
            self,
            LightRegime::DayNight { .. } | LightRegime::SlowDecline { .. }
        )
    }
}

/// Integer linear interpolation from `a` at `num == 0` to `b` at `num == den`.
///
/// Computed in `i64` and clamped, so no combination of scenario values can overflow it.
#[inline]
#[must_use]
fn lerp(a: i32, b: i32, num: i64, den: i64) -> i32 {
    let den = den.max(1);
    let num = num.clamp(0, den);
    let v = a as i64 + (b as i64 - a as i64) * num / den;
    crate::fixed::sat_i32(v)
}

impl StateHash for LightRegime {
    fn hash_state(&self, h: &mut StateHasher) {
        match self {
            LightRegime::Uniform { intensity } => {
                h.u8(0);
                h.i32(*intensity);
            }
            LightRegime::DayNight {
                period_ticks,
                day,
                night,
            } => {
                h.u8(1);
                h.u32(*period_ticks);
                h.i32(*day);
                h.i32(*night);
            }
            LightRegime::Directional { bright, dark, from } => {
                h.u8(2);
                h.i32(*bright);
                h.i32(*dark);
                h.u8(*from as u8);
            }
            LightRegime::PointSource {
                x,
                y,
                intensity,
                half_life_squares,
            } => {
                h.u8(3);
                h.u32(*x);
                h.u32(*y);
                h.i32(*intensity);
                h.u32(*half_life_squares);
            }
            LightRegime::SlowDecline {
                start,
                end,
                over_ticks,
            } => {
                h.u8(4);
                h.i32(*start);
                h.i32(*end);
                h.u64(*over_ticks);
            }
        }
    }
}

/// A prescribed background flow (SPEC §7.4).
///
/// Velocity comes from a slowly varying prescribed field plus local impulses injected by
/// cilia. This is the prescribed part; the impulse part lives in [`crate::world::World`],
/// because it decays and is therefore state rather than configuration.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum CurrentField {
    /// Still water.
    #[default]
    Still,
    /// The same drift everywhere, `Q10` squares per fluid step.
    Uniform { vx: i32, vy: i32 },
    /// Rotation about the centre of the slide — the stirred beaker. `strength` is the speed
    /// at the rim.
    Rotational { strength: i32 },
    /// Opposed horizontal flows, fast at the top and bottom edges and still in the middle.
    Shear { strength: i32 },
}



impl CurrentField {
    /// Velocity at one square, `Q10` squares per step, clamped to the CFL limit of one
    /// square per step in each axis.
    #[must_use]
    pub fn velocity_at(&self, x: u32, y: u32, w: u32, h: u32) -> (i32, i32) {
        let (vx, vy) = match self {
            CurrentField::Still => (0, 0),
            CurrentField::Uniform { vx, vy } => (*vx, *vy),

            CurrentField::Rotational { strength } => {
                // Offset from centre, in half-squares to keep the integer arithmetic exact
                // for both even and odd dimensions.
                let cx2 = w.saturating_sub(1) as i64;
                let cy2 = h.saturating_sub(1) as i64;
                let dx2 = 2 * x as i64 - cx2;
                let dy2 = 2 * y as i64 - cy2;
                let radius2 = cx2.max(cy2).max(1);
                // v = strength * (-dy, dx) / radius: a rigid rotation.
                let vx = -(*strength as i64) * dy2 / radius2;
                let vy = (*strength as i64) * dx2 / radius2;
                (crate::fixed::sat_i32(vx), crate::fixed::sat_i32(vy))
            }

            CurrentField::Shear { strength } => {
                let cy2 = h.saturating_sub(1) as i64;
                let dy2 = 2 * y as i64 - cy2;
                let radius2 = cy2.max(1);
                let vx = (*strength as i64) * dy2 / radius2;
                (crate::fixed::sat_i32(vx), 0)
            }
        };
        (
            vx.clamp(-MAX_VELOCITY, MAX_VELOCITY),
            vy.clamp(-MAX_VELOCITY, MAX_VELOCITY),
        )
    }

    /// Write the prescribed field into the substrate, adding the decaying impulse layer.
    ///
    /// A blocked square has no velocity: nothing flows inside a barrier.
    pub fn apply(&self, substrate: &mut Substrate, impulse_x: &[i32], impulse_y: &[i32]) {
        let w = substrate.width();
        let h = substrate.height();
        let (vx, vy, blocked_snapshot) = substrate.velocity_and_blocked_mut();
        for y in 0..h {
            for x in 0..w {
                let i = (y as usize) * (w as usize) + x as usize;
                if blocked_snapshot[i] {
                    vx[i] = 0;
                    vy[i] = 0;
                    continue;
                }
                let (bx, by) = self.velocity_at(x, y, w, h);
                let ix = impulse_x.get(i).copied().unwrap_or(0);
                let iy = impulse_y.get(i).copied().unwrap_or(0);
                let lim = MAX_VELOCITY as i64;
                vx[i] = (bx as i64 + ix as i64).clamp(-lim, lim) as i32;
                vy[i] = (by as i64 + iy as i64).clamp(-lim, lim) as i32;
            }
        }
        // The field was written in bulk, so the "is anything flowing" flag has to be
        // recomputed rather than inferred one square at a time.
        substrate.refresh_flow();
    }
}

impl StateHash for CurrentField {
    fn hash_state(&self, h: &mut StateHasher) {
        match self {
            CurrentField::Still => h.u8(0),
            CurrentField::Uniform { vx, vy } => {
                h.u8(1);
                h.i32(*vx);
                h.i32(*vy);
            }
            CurrentField::Rotational { strength } => {
                h.u8(2);
                h.i32(*strength);
            }
            CurrentField::Shear { strength } => {
                h.u8(3);
                h.i32(*strength);
            }
        }
    }
}

/// Decay the impulse layer toward zero by a `Q10` fraction per step.
///
/// Impulses are momentum a cilium put into the water; without decay a single flick would
/// stir the slide forever. Decay is not a matter loss — velocity is not matter.
pub fn decay_impulses(ix: &mut [i32], iy: &mut [i32], retain: i32) {
    let retain = retain.clamp(0, Q10_ONE);
    for v in ix.iter_mut().chain(iy.iter_mut()) {
        *v = q10_scale(*v, retain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_light_is_uniform() {
        let r = LightRegime::Uniform { intensity: 700 };
        for tick in [0u64, 1, 1_000_000] {
            for x in 0..8 {
                assert_eq!(r.intensity_at(x, 3, 8, 8, tick), 700);
            }
        }
        assert!(!r.is_time_varying());
    }

    #[test]
    fn day_night_reaches_both_ends_and_repeats() {
        let r = LightRegime::DayNight {
            period_ticks: 100,
            day: 1024,
            night: 0,
        };
        let series: Vec<i32> = (0..100).map(|t| r.intensity_at(0, 0, 8, 8, t)).collect();
        assert_eq!(series[0], 0, "starts at night");
        assert_eq!(series[50], 1024, "noon at the half-way point");
        assert!(series.iter().all(|v| (0..=1024).contains(v)));
        // and it is periodic
        for t in 0..100u64 {
            assert_eq!(
                r.intensity_at(0, 0, 8, 8, t),
                r.intensity_at(0, 0, 8, 8, t + 100)
            );
        }
        assert!(r.is_time_varying());
    }

    #[test]
    fn a_directional_gradient_runs_between_its_two_edges() {
        let r = LightRegime::Directional {
            bright: 1024,
            dark: 0,
            from: Edge::Left,
        };
        assert_eq!(r.intensity_at(0, 0, 9, 9, 0), 1024);
        assert_eq!(r.intensity_at(8, 0, 9, 9, 0), 0);
        assert_eq!(r.intensity_at(4, 0, 9, 9, 0), 512);
        // monotonic across the slide, which is what a gradient has to be for a cell to climb
        let row: Vec<i32> = (0..9).map(|x| r.intensity_at(x, 0, 9, 9, 0)).collect();
        assert!(row.windows(2).all(|p| p[0] >= p[1]), "{row:?}");

        let from_right = LightRegime::Directional {
            bright: 1024,
            dark: 0,
            from: Edge::Right,
        };
        assert_eq!(from_right.intensity_at(8, 0, 9, 9, 0), 1024);
        assert_eq!(from_right.intensity_at(0, 0, 9, 9, 0), 0);
    }

    #[test]
    fn a_point_source_falls_off_with_distance() {
        let r = LightRegime::PointSource {
            x: 16,
            y: 16,
            intensity: 1024,
            half_life_squares: 4,
        };
        assert_eq!(r.intensity_at(16, 16, 32, 32, 0), 1024);
        assert_eq!(r.intensity_at(20, 16, 32, 32, 0), 512, "one half-life out");
        assert_eq!(r.intensity_at(24, 16, 32, 32, 0), 256, "two half-lives out");
        // monotonic falloff, and never negative however far away
        let mut last = i32::MAX;
        for d in 0..32u32 {
            let v = r.intensity_at(16 + d, 16, 64, 64, 0);
            assert!(v <= last, "not monotonic at {d}");
            assert!(v >= 0);
            last = v;
        }
    }

    #[test]
    fn slow_decline_reaches_its_end_and_stays() {
        let r = LightRegime::SlowDecline {
            start: 1024,
            end: 64,
            over_ticks: 1_000_000,
        };
        assert_eq!(r.intensity_at(0, 0, 8, 8, 0), 1024);
        assert_eq!(r.intensity_at(0, 0, 8, 8, 500_000), 544);
        assert_eq!(r.intensity_at(0, 0, 8, 8, 1_000_000), 64);
        assert_eq!(
            r.intensity_at(0, 0, 8, 8, 9_000_000),
            64,
            "and does not run past its floor"
        );
    }

    #[test]
    fn barriers_cast_shadows() {
        let mut s = Substrate::new(8, 8).unwrap();
        s.set_blocked(3, 3, true);
        LightRegime::Uniform { intensity: 900 }.apply(&mut s, 0);
        assert_eq!(s.light_at(3, 3), 0);
        assert_eq!(s.light_at(3, 4), 900);
    }

    #[test]
    fn every_regime_is_non_negative_everywhere() {
        // Including with hostile scenario values: a negative intensity is not a thing.
        let regimes = [
            LightRegime::Uniform { intensity: -500 },
            LightRegime::DayNight {
                period_ticks: 0,
                day: -10,
                night: -20,
            },
            LightRegime::Directional {
                bright: -1,
                dark: i32::MIN,
                from: Edge::Bottom,
            },
            LightRegime::PointSource {
                x: 999,
                y: 999,
                intensity: i32::MAX,
                half_life_squares: 0,
            },
            LightRegime::SlowDecline {
                start: i32::MIN,
                end: i32::MAX,
                over_ticks: 0,
            },
        ];
        for r in regimes {
            for t in [0u64, 1, 12345, u64::MAX] {
                for x in 0..12u32 {
                    for y in 0..12u32 {
                        assert!(r.intensity_at(x, y, 12, 12, t) >= 0, "{r:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn rotation_circulates_and_respects_the_cfl_limit() {
        let f = CurrentField::Rotational { strength: 600 };
        let (vx, vy) = f.velocity_at(0, 0, 33, 33);
        // top-left corner of a counter-clockwise rotation: up and to the left
        assert!(vx > 0 && vy < 0, "({vx}, {vy})");
        let centre = f.velocity_at(16, 16, 33, 33);
        assert_eq!(centre, (0, 0), "the axis of rotation does not move");
        for x in 0..33u32 {
            for y in 0..33u32 {
                let (u, v) = f.velocity_at(x, y, 33, 33);
                assert!(u.abs() <= MAX_VELOCITY && v.abs() <= MAX_VELOCITY);
            }
        }
    }

    #[test]
    fn shear_opposes_across_the_slide() {
        let f = CurrentField::Shear { strength: 512 };
        let (top, _) = f.velocity_at(0, 0, 16, 17);
        let (bottom, _) = f.velocity_at(0, 16, 16, 17);
        let (middle, _) = f.velocity_at(0, 8, 16, 17);
        assert_eq!(top, -bottom);
        assert_eq!(middle, 0);
    }

    #[test]
    fn impulses_add_to_the_prescribed_field_and_decay_away() {
        let mut s = Substrate::new(4, 4).unwrap();
        let mut ix = vec![0i32; s.len()];
        let mut iy = vec![0i32; s.len()];
        ix[s.index(1, 1)] = 100;
        CurrentField::Uniform { vx: 100, vy: 0 }.apply(&mut s, &ix, &iy);
        assert_eq!(s.velocity_at(1, 1), (200, 0), "impulse adds to the current");
        assert_eq!(s.velocity_at(0, 0), (100, 0));
        // ...and the sum is still held to the CFL limit, however hard the cilium pushed.
        ix[s.index(2, 2)] = MAX_VELOCITY * 4;
        CurrentField::Uniform { vx: 100, vy: 0 }.apply(&mut s, &ix, &iy);
        assert_eq!(s.velocity_at(2, 2), (MAX_VELOCITY, 0));

        for _ in 0..64 {
            decay_impulses(&mut ix, &mut iy, Q10_ONE / 2);
        }
        assert_eq!(ix[s.index(1, 1)], 0, "impulses must not stir forever");
        assert_eq!(ix[s.index(2, 2)], 0);
    }

    #[test]
    fn velocity_never_exceeds_the_cfl_limit_even_with_huge_impulses() {
        let mut s = Substrate::new(4, 4).unwrap();
        let ix = vec![i32::MAX; s.len()];
        let iy = vec![i32::MIN; s.len()];
        CurrentField::Uniform {
            vx: i32::MAX,
            vy: i32::MIN,
        }
        .apply(&mut s, &ix, &iy);
        for i in 0..s.len() {
            let (vx, vy) = s.velocity();
            assert!(vx[i].abs() <= MAX_VELOCITY && vy[i].abs() <= MAX_VELOCITY);
        }
    }

    #[test]
    fn regimes_round_trip_through_ron() {
        let r = LightRegime::PointSource {
            x: 3,
            y: 4,
            intensity: 900,
            half_life_squares: 7,
        };
        let back: LightRegime = ron::from_str(&ron::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);

        let f = CurrentField::Rotational { strength: 42 };
        let back: CurrentField = ron::from_str(&ron::to_string(&f).unwrap()).unwrap();
        assert_eq!(back, f);
    }
}
