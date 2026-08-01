//! Sensors and motility (M3).
//!
//! # What a sensor is for
//!
//! A chemosensor does not tell a cell where food is. It tells it the concentration here and
//! which way it rises — and those are two separate readings on two separate `OGET` indices,
//! so a genome that wants to follow a gradient has to *connect* them to something. The
//! connection is the thing that has to evolve, and the acceptance test for this milestone is
//! whether it does.
//!
//! That is why the ancestor for the chemotaxis experiment has a chemosensor and cilia and no
//! code linking them. Everything needed is present and inert; what is absent is the idea.
//!
//! # Gradients are read from the fluid, and only from the fluid
//!
//! A sensor samples the substrate around the cell during the sense phase, before anything has
//! moved. Nothing here writes; sensor outputs are derived on read from a world nobody is
//! modifying, which is what keeps the execute phase parallelisable and free of ordering
//! (SPEC §12).
//!
//! # Cilia push water, and the water pushes back
//!
//! A cilium applies thrust to its own cell and injects the opposite impulse into the fluid.
//! That is what makes colony locomotion emergent at M7 — cilia on one cell of a cluster push
//! that cell, and the junction constraints drag the rest — and it is also what stops a cell
//! rowing against nothing. The impulse decays, so momentum is not conserved and is not
//! claimed to be; what is bounded is how much a cilium can inject per tick.

use crate::cell::CellArena;
use crate::chem::{chem_index, CHEM_COUNT};
use crate::fixed::{pos_to_square, q10_scale, sat_i16, POS_ONE, Q10_ONE};
use crate::organelle::{Organelle, OrganelleType};
use crate::rng::{Purpose, RandCtx};
use crate::substrate::Substrate;

/// How far a sensor reaches, in substrate squares.
///
/// One square. A sensor that could see across the slide would make the world legible without
/// the cell having to move through it, and moving through it is the behaviour this milestone
/// exists to make evolvable.
pub const SENSOR_RANGE: i32 = 1;

/// Thrust one unit of cilium `param` can produce, `Q10` of a square per tick, per unit power.
pub const THRUST_PER_PARAM: i32 = 4;

/// Fraction of a cell's velocity that survives each tick, `Q10`.
///
/// Water at this scale is syrup: a cell that stops beating stops moving almost at once, which
/// is what life at low Reynolds number is actually like. It also means velocity carries almost
/// no information between ticks, so a cell cannot coast and must keep paying to move.
pub const DRAG_RETAIN: i32 = Q10_ONE / 4;

/// Energy a cilium spends per unit of thrust, `Q10`.
///
/// Set against the upkeep of a modest body, so that swimming at full power costs about what
/// staying alive does. Much cheaper and motility is free, which would make the chemotaxis
/// experiment a test of whether a genome can find a mechanism rather than whether the
/// mechanism is worth its cost — and the second question is the interesting one.
pub const THRUST_ENERGY: i32 = Q10_ONE / 4;

/// What a chemosensor reports (SPEC §6.2): concentration, and the gradient's two components.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ChemReading {
    pub concentration: i32,
    pub gradient_x: i32,
    pub gradient_y: i32,
}

/// Sample one chemical around a square.
///
/// The gradient is a plain central difference: what is to the right minus what is to the left.
/// A cell that wants to climb it adds it to its heading; a cell that wants to flee subtracts.
/// Neither is privileged and neither is provided.
#[must_use]
pub fn sense_chemical(substrate: &Substrate, chemical: usize, x: i32, y: i32) -> ChemReading {
    let c = chemical % CHEM_COUNT;
    let at = |dx: i32, dy: i32| -> i32 {
        let sx = x + dx;
        let sy = y + dy;
        // Outside the slide reads as nothing rather than wrapping. A gradient that wrapped
        // would point across the closed boundary and tell a cell to swim into a wall.
        if sx < 0 || sy < 0 || sx >= substrate.width() as i32 || sy >= substrate.height() as i32 {
            return 0;
        }
        substrate.chem_at(c, sx, sy)
    };
    let here = at(0, 0);
    ChemReading {
        concentration: here,
        gradient_x: at(SENSOR_RANGE, 0).saturating_sub(at(-SENSOR_RANGE, 0)),
        gradient_y: at(0, SENSOR_RANGE).saturating_sub(at(0, -SENSOR_RANGE)),
    }
}

/// What a photosensor reports: intensity, and which way the light gets brighter.
#[must_use]
pub fn sense_light(substrate: &Substrate, x: i32, y: i32) -> ChemReading {
    let at = |dx: i32, dy: i32| -> i32 {
        let sx = x + dx;
        let sy = y + dy;
        if sx < 0 || sy < 0 || sx >= substrate.width() as i32 || sy >= substrate.height() as i32 {
            return 0;
        }
        substrate.light_at(sx, sy)
    };
    ChemReading {
        concentration: at(0, 0),
        gradient_x: at(SENSOR_RANGE, 0).saturating_sub(at(-SENSOR_RANGE, 0)),
        gradient_y: at(0, SENSOR_RANGE).saturating_sub(at(0, -SENSOR_RANGE)),
    }
}

/// What an oscillator reports: a triangular phase, `0..Q10_ONE`.
///
/// The one sensor that reads nothing at all. It exists because rhythm is hard to build out of
/// a stateless genome and easy to build out of a clock, and because peristalsis and beating
/// (M7) need one. Triangular rather than sinusoidal for the same reason as the day/night
/// cycle: a triangle is exact in integers.
#[must_use]
pub fn oscillator_phase(period: i16, tick: u64, cell_key: u64) -> i32 {
    let period = (period as u16 as u64).max(2);
    // Offset by the cell's own key so a clonal population does not beat in lockstep for
    // reasons that have nothing to do with coordination.
    let phase = (tick.wrapping_add(cell_key)) % period;
    let half = period / 2;
    let up = phase.min(half);
    let t = if phase < half {
        up
    } else {
        period.saturating_sub(phase).min(half)
    };
    ((t as i64 * Q10_ONE as i64) / half.max(1) as i64) as i32
}

/// What a touch sensor reports: how many cells are within reach, and how heavy they are.
///
/// Answering it properly needs the spatial index M9 builds; for now it is derived from the
/// cell's own neighbourhood by the caller, which is why it takes the answer rather than
/// computing it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TouchReading {
    pub contacts: i16,
    pub nearest: i16,
    pub contact_mass: i16,
}

/// Where and when a sensor is being read, and what is next to it.
///
/// Grouped rather than passed as loose arguments because every sensor needs some of it and
/// none of them need all of it, and a call site with eight positional parameters is a place
/// where two of them get swapped.
#[derive(Clone, Copy, Debug)]
pub struct SensorContext<'a> {
    pub substrate: &'a Substrate,
    /// The square the cell is standing on.
    pub x: i32,
    pub y: i32,
    pub tick: u64,
    /// The cell's own id, so clonal cells do not share a clock phase.
    pub cell_key: u64,
    pub touch: TouchReading,
}

/// Read an organelle's output, for the sensor types (M3).
///
/// Returns `None` for anything that is not a sensor, so the caller can fall through to the
/// types it handles.
#[must_use]
pub fn read_sensor(organelle: &Organelle, index: i16, ctx: SensorContext<'_>) -> Option<i16> {
    let SensorContext {
        substrate,
        x,
        y,
        tick,
        cell_key,
        touch,
    } = ctx;
    if !organelle.is_active() {
        return None;
    }
    let visible = |q: i32| sat_i16(q / Q10_ONE);
    match organelle.kind {
        OrganelleType::Chemosensor => {
            // Which chemical it is tuned to is a control input, so a mutation retunes a sensor
            // rather than replacing it.
            let chemical = chem_index(organelle.control[0]);
            let r = sense_chemical(substrate, chemical, x, y);
            Some(match (index as u16) % 3 {
                0 => visible(r.concentration),
                1 => visible_gradient(r.gradient_x),
                _ => visible_gradient(r.gradient_y),
            })
        }
        OrganelleType::Photosensor => {
            let r = sense_light(substrate, x, y);
            Some(match (index as u16) % 3 {
                0 => visible(r.concentration),
                1 => visible_gradient(r.gradient_x),
                _ => visible_gradient(r.gradient_y),
            })
        }
        OrganelleType::TouchSensor => Some(match (index as u16) % 3 {
            0 => touch.contacts,
            1 => touch.nearest,
            _ => touch.contact_mass,
        }),
        OrganelleType::Oscillator => {
            let phase = oscillator_phase(organelle.control[0], tick, cell_key);
            Some(match (index as u16) % 2 {
                0 => visible(phase),
                // The phase again, shifted a quarter turn, so a genome can get a second
                // rhythm out of one clock without arithmetic.
                _ => visible(Q10_ONE.saturating_sub(phase)),
            })
        }
        OrganelleType::Cilium => Some(match (index as u16) % 2 {
            // Achieved thrust, and the load it is pushing against.
            0 => sat_i16(cilium_thrust(organelle) / Q10_ONE),
            _ => sat_i16(organelle.param as i32),
        }),
        _ => None,
    }
}

/// How much finer a gradient reading is than a concentration reading.
///
/// # Why gradients need their own scale
///
/// A concentration is an amount; a gradient is the *difference* between two amounts one square
/// apart, and in a diffused field that difference is two or three orders of magnitude smaller.
/// Both used to go through the same `q / Q10_ONE`, and the consequence was not a rounding
/// detail — it was that the gradient outputs read **zero**.
///
/// Measured on M3's own patchy slide, food diffused for two thousand ticks:
///
/// ```text
///                       raw gradient   old reading   with this gain
///   near the centre              452             0              113
///   midway to a patch          1,916             1              479
///   at a patch edge            2,340             2              585
/// ```
///
/// The founders start between the patches. They were being handed a zero and asked to evolve
/// navigation from it, which is not a hard problem — it is an impossible one, and no length of
/// run would have fixed it. This is what starved M3's chemotaxis acceptance test.
///
/// 256 rather than 1024 so that a genuinely sharp edge — the boundary of a fresh patch, three
/// orders of magnitude across — still saturates rather than wrapping, which is hard rule 4.
const GRADIENT_GAIN: i32 = 256;

/// A gradient as a genome sees it: signed, saturating, and at [`GRADIENT_GAIN`]'s resolution.
#[inline]
#[must_use]
fn visible_gradient(q: i32) -> i16 {
    sat_i16(q / (Q10_ONE / GRADIENT_GAIN))
}

/// Thrust one cilium is producing, `Q10` of a square per tick.
///
/// Signed: a cilium beating backwards pushes the cell backwards. Power is the genome's control
/// input and saturates, so a mutation to it is a small change in speed rather than a reversal
/// (SPEC §3).
#[must_use]
pub fn cilium_thrust(o: &Organelle) -> i32 {
    if !o.is_active() || o.kind != OrganelleType::Cilium {
        return 0;
    }
    let power = (o.control[0] as i32).clamp(-Q10_ONE, Q10_ONE);
    let capacity = THRUST_PER_PARAM.saturating_mul(o.param as i32);
    q10_scale(capacity, power)
}

/// The direction a cilium is mounted, as a unit-ish vector in `Q10`.
///
/// Sixteen mount angles, from the second control input. Sixteen rather than a continuum
/// because it is a 4-bit quantity like everything else a genome addresses, so a mutation
/// turns a cilium a little rather than reversing it.
#[must_use]
pub fn cilium_direction(o: &Organelle) -> (i32, i32) {
    const COS: [i32; 16] = [
        1024, 946, 724, 392, 0, -392, -724, -946, -1024, -946, -724, -392, 0, 392, 724, 946,
    ];
    let angle = (o.control[1] as u16 as usize) % 16;
    let quarter = (angle + 4) % 16;
    (COS[angle], COS[quarter])
}

/// One tick of physics for the population: thrust, jitter, drag, and integration.
///
/// Sequential and in slot order, like resolve — this is where cells push on a shared fluid,
/// so it is not a place for scheduling to be observable.
pub fn step_physics(
    cells: &mut CellArena,
    substrate: &Substrate,
    impulse_x: &mut [i32],
    impulse_y: &mut [i32],
    jitter: i32,
    tick: u64,
    seed: u64,
) -> PhysicsReport {
    let mut report = PhysicsReport::default();
    let w = substrate.width() as i32;
    let h = substrate.height() as i32;

    for i in 0..cells.capacity() {
        if !cells.occupied(i) {
            continue;
        }
        let id = cells.id_at(i);
        let ctx = RandCtx::new(seed, tick, id.ordering_key());

        // --- thrust from every cilium, and the reaction into the water ---
        let (mut fx, mut fy) = (0i32, 0i32);
        let mut spent = 0i32;
        for o in cells.slots(i) {
            let thrust = cilium_thrust(o);
            if thrust == 0 {
                continue;
            }
            let (dx, dy) = cilium_direction(o);
            fx = fx.saturating_add(q10_scale(thrust, dx));
            fy = fy.saturating_add(q10_scale(thrust, dy));
            spent = spent.saturating_add(q10_scale(thrust.abs(), THRUST_ENERGY));
        }
        if spent > 0 {
            // Beating costs energy whether or not it achieves anything, which is what makes
            // swimming a trade rather than a free action.
            let paid = cells.energy[i].min(spent);
            cells.energy[i] = cells.energy[i].saturating_sub(paid);
            if paid < spent && spent > 0 {
                // Could not afford full power; scale the thrust back to what was paid for.
                fx = ((fx as i64 * paid as i64) / spent as i64) as i32;
                fy = ((fy as i64 * paid as i64) / spent as i64) as i32;
            }
            report.energy_spent += paid as i64;
        }

        // --- Brownian jitter ---
        if jitter > 0 {
            let jx = (ctx.draw_below(Purpose::Jitter, 1, (2 * jitter + 1) as u64) as i32) - jitter;
            let jy = (ctx.draw_below(Purpose::Jitter, 2, (2 * jitter + 1) as u64) as i32) - jitter;
            fx = fx.saturating_add(jx);
            fy = fy.saturating_add(jy);
        }

        // --- drag, then integrate ---
        cells.vx[i] = q10_scale(cells.vx[i], DRAG_RETAIN).saturating_add(fx);
        cells.vy[i] = q10_scale(cells.vy[i], DRAG_RETAIN).saturating_add(fy);

        // The fluid carries the cell along with it.
        let sq = substrate.index(pos_to_square(cells.x[i]), pos_to_square(cells.y[i]));
        let (svx, svy) = substrate.velocity();
        let drift_x = svx.get(sq).copied().unwrap_or(0);
        let drift_y = svy.get(sq).copied().unwrap_or(0);

        // Velocity is `Q10` squares per tick; position is `POS` within a square.
        let step_x =
            ((cells.vx[i].saturating_add(drift_x)) as i64 * POS_ONE as i64) / Q10_ONE as i64;
        let step_y =
            ((cells.vy[i].saturating_add(drift_y)) as i64 * POS_ONE as i64) / Q10_ONE as i64;
        let nx = (cells.x[i] as i64).saturating_add(step_x);
        let ny = (cells.y[i] as i64).saturating_add(step_y);

        // The slide is a closed box: a cell that swims into the edge stops there rather than
        // reappearing on the far side, because flux does not wrap and neither should bodies.
        let max_x = (w as i64 * POS_ONE as i64) - 1;
        let max_y = (h as i64 * POS_ONE as i64) - 1;
        cells.x[i] = nx.clamp(0, max_x) as i32;
        cells.y[i] = ny.clamp(0, max_y) as i32;
        if nx != cells.x[i] as i64 {
            cells.vx[i] = 0;
        }
        if ny != cells.y[i] as i64 {
            cells.vy[i] = 0;
        }
        report.moved += step_x.abs() + step_y.abs();

        // --- the reaction: what the cilia pushed against ---
        //
        // Equal and opposite into the water, so a cell rowing is stirring. The impulse decays,
        // so this is not a claim that momentum is conserved; it is a claim that a cilium
        // cannot push on nothing.
        if fx != 0 || fy != 0 {
            if let Some(slot) = impulse_x.get_mut(sq) {
                *slot = slot.saturating_sub(fx).clamp(-Q10_ONE, Q10_ONE);
            }
            if let Some(slot) = impulse_y.get_mut(sq) {
                *slot = slot.saturating_sub(fy).clamp(-Q10_ONE, Q10_ONE);
            }
        }
    }
    report
}

/// What one tick of physics did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PhysicsReport {
    /// Total distance moved, `POS` units summed over the population.
    pub moved: i64,
    /// Energy spent on thrust.
    pub energy_spent: i64,
    /// Overlapping pairs pushed apart.
    pub separated: u32,
    /// Hard-junction distance constraints solved this tick (SPEC §8.4).
    pub constraints: u32,
    /// Junctions broken because an end died or drifted out of range.
    pub junctions_broken: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{CellId, CellSeed};
    use crate::fixed::{pos, q10};
    use crate::genome::GenomePool;

    fn substrate_with_gradient() -> Substrate {
        let mut s = Substrate::new(16, 16).unwrap();
        for y in 0..16i32 {
            for x in 0..16i32 {
                s.set_chem(5, x, y, q10(x * 10));
            }
        }
        s
    }

    #[test]
    fn a_chemosensor_reports_concentration_and_which_way_it_rises() {
        let s = substrate_with_gradient();
        let r = sense_chemical(&s, 5, 8, 8);
        assert_eq!(r.concentration, q10(80));
        assert!(r.gradient_x > 0, "the gradient should point up-slope");
        assert_eq!(r.gradient_y, 0, "there is no gradient on y");
        // and it is symmetric: reading down-slope gives the mirror image
        let left = sense_chemical(&s, 5, 2, 8);
        assert_eq!(left.gradient_x, r.gradient_x, "a linear ramp is uniform");
    }

    #[test]
    fn a_gradient_at_the_edge_does_not_point_across_the_boundary() {
        // The slide is a closed box. A gradient that wrapped would tell a cell to swim into
        // a wall and keep swimming.
        let s = substrate_with_gradient();
        let right_edge = sense_chemical(&s, 5, 15, 8);
        assert!(
            right_edge.gradient_x < 0,
            "at the right edge the only way is back: {right_edge:?}"
        );
    }

    #[test]
    fn a_photosensor_finds_the_bright_edge() {
        let mut s = Substrate::new(16, 16).unwrap();
        crate::light::LightRegime::Directional {
            bright: Q10_ONE,
            dark: 0,
            from: crate::light::Edge::Left,
        }
        .apply(&mut s, 0);
        let r = sense_light(&s, 8, 8);
        assert!(r.concentration > 0);
        assert!(r.gradient_x < 0, "brightness falls to the right");
    }

    #[test]
    fn an_oscillator_is_periodic_and_stays_in_range() {
        // Every control value is legal, including the ones that decode to enormous periods —
        // a genome can write anything into a control input and none of it may misbehave.
        for period in [i16::MIN, -1, 0, 1, 2, 3, 17, 256, i16::MAX] {
            for tick in [0u64, 1, 7, 1_000, u64::MAX] {
                let p = oscillator_phase(period, tick, 3);
                assert!(
                    (0..=Q10_ONE).contains(&p),
                    "period {period} tick {tick}: {p}"
                );
            }
        }
        // A period short enough to observe must actually swing across its whole range.
        for period in [2i16, 3, 17, 256] {
            let mut seen_low = false;
            let mut seen_high = false;
            for tick in 0..600u64 {
                let p = oscillator_phase(period, tick, 0);
                if p < Q10_ONE / 8 {
                    seen_low = true;
                }
                if p > Q10_ONE * 7 / 8 {
                    seen_high = true;
                }
            }
            assert!(seen_low && seen_high, "period {period} never swung");
        }
    }

    #[test]
    fn clones_do_not_beat_in_lockstep() {
        // Otherwise a clonal bloom would pulse as one organism for no reason connected to
        // coordination, which is a thing M7 has to be able to claim it achieved.
        let a = oscillator_phase(64, 10, 1);
        let b = oscillator_phase(64, 10, 33);
        assert_ne!(a, b);
    }

    #[test]
    fn cilium_thrust_is_signed_and_saturates() {
        let mut o = Organelle::finished(OrganelleType::Cilium, 100);
        o.control[0] = Q10_ONE as i16;
        let forward = cilium_thrust(&o);
        assert!(forward > 0);
        o.control[0] = -(Q10_ONE as i16);
        assert_eq!(
            cilium_thrust(&o),
            -forward,
            "reverse is the mirror of forward"
        );
        o.control[0] = i16::MAX;
        assert_eq!(
            cilium_thrust(&o),
            forward,
            "power saturates rather than wrapping"
        );
        o.control[0] = 0;
        assert_eq!(cilium_thrust(&o), 0);
    }

    #[test]
    fn an_unfinished_cilium_produces_nothing() {
        let mut o = Organelle::finished(OrganelleType::Cilium, 100);
        o.control[0] = Q10_ONE as i16;
        o.remaining_build = 3;
        assert_eq!(cilium_thrust(&o), 0);
    }

    #[test]
    fn cilium_directions_cover_the_circle() {
        let mut seen = std::collections::BTreeSet::new();
        for angle in 0..16i16 {
            let mut o = Organelle::finished(OrganelleType::Cilium, 10);
            o.control[1] = angle;
            let (dx, dy) = cilium_direction(&o);
            assert!(dx.abs() <= Q10_ONE && dy.abs() <= Q10_ONE);
            seen.insert((dx, dy));
        }
        assert_eq!(seen.len(), 16, "every mount angle should be distinct");
        // and the angles are a rotation: opposite mounts point opposite ways
        let mut a = Organelle::finished(OrganelleType::Cilium, 10);
        a.control[1] = 0;
        let mut b = Organelle::finished(OrganelleType::Cilium, 10);
        b.control[1] = 8;
        assert_eq!(cilium_direction(&a).0, -cilium_direction(&b).0);
    }

    fn one_cell(pool: &GenomePool, slots: &[(usize, Organelle)]) -> CellArena {
        let mut cells = CellArena::new();
        let id = cells.spawn(CellSeed {
            x: pos(8),
            y: pos(8),
            mass: q10(20),
            energy: q10(10_000),
            membrane: 16,
            key: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome: pool.intern(vec![0x2E]).unwrap(),
        });
        let i = cells.index(id).unwrap();
        for (slot, o) in slots {
            cells.slots_mut(i)[*slot] = *o;
        }
        cells
    }

    #[test]
    fn a_beating_cilium_moves_its_cell_and_stirs_the_water() {
        let pool = GenomePool::new();
        let mut cilium = Organelle::finished(OrganelleType::Cilium, 200);
        cilium.control[0] = Q10_ONE as i16;
        cilium.control[1] = 0; // due +x
        let mut cells = one_cell(&pool, &[(6, cilium)]);
        let substrate = Substrate::new(16, 16).unwrap();
        let mut ix = vec![0i32; substrate.len()];
        let mut iy = vec![0i32; substrate.len()];

        let before_x = cells.x[0];
        let before_energy = cells.energy[0];
        let report = step_physics(&mut cells, &substrate, &mut ix, &mut iy, 0, 0, 1);

        assert!(cells.x[0] > before_x, "the cell did not move");
        assert!(cells.energy[0] < before_energy, "swimming was free");
        assert!(report.energy_spent > 0);
        let sq = substrate.index(8, 8);
        assert!(
            ix[sq] < 0,
            "the water was not pushed the other way: {}",
            ix[sq]
        );
    }

    #[test]
    fn a_cell_with_no_cilia_stays_put() {
        let pool = GenomePool::new();
        let mut cells = one_cell(&pool, &[]);
        let substrate = Substrate::new(16, 16).unwrap();
        let mut ix = vec![0i32; substrate.len()];
        let mut iy = vec![0i32; substrate.len()];
        let (x, y) = (cells.x[0], cells.y[0]);
        for tick in 0..100 {
            step_physics(&mut cells, &substrate, &mut ix, &mut iy, 0, tick, 1);
        }
        assert_eq!((cells.x[0], cells.y[0]), (x, y));
    }

    #[test]
    fn a_cell_that_cannot_pay_swims_more_slowly() {
        // Thrust scales back to what was actually afforded, rather than a broke cell getting
        // a free ride.
        let pool = GenomePool::new();
        let mut cilium = Organelle::finished(OrganelleType::Cilium, 255);
        cilium.control[0] = Q10_ONE as i16;
        let substrate = Substrate::new(16, 16).unwrap();
        let mut ix = vec![0i32; substrate.len()];
        let mut iy = vec![0i32; substrate.len()];

        let mut rich = one_cell(&pool, &[(6, cilium)]);
        step_physics(&mut rich, &substrate, &mut ix, &mut iy, 0, 0, 1);

        let mut poor = one_cell(&pool, &[(6, cilium)]);
        poor.energy[0] = 1;
        let mut ix2 = vec![0i32; substrate.len()];
        let mut iy2 = vec![0i32; substrate.len()];
        step_physics(&mut poor, &substrate, &mut ix2, &mut iy2, 0, 0, 1);

        assert!(
            poor.x[0] < rich.x[0],
            "a cell with no energy swam as fast as one with plenty"
        );
    }

    #[test]
    fn drag_stops_a_cell_that_stops_beating() {
        // Life at low Reynolds number: no coasting, so a cell has to keep paying to move.
        let pool = GenomePool::new();
        let mut cells = one_cell(&pool, &[]);
        cells.vx[0] = Q10_ONE;
        let substrate = Substrate::new(16, 16).unwrap();
        let mut ix = vec![0i32; substrate.len()];
        let mut iy = vec![0i32; substrate.len()];
        for tick in 0..20 {
            step_physics(&mut cells, &substrate, &mut ix, &mut iy, 0, tick, 1);
        }
        assert_eq!(cells.vx[0], 0, "the cell coasted");
    }

    #[test]
    fn a_cell_cannot_swim_off_the_slide() {
        let pool = GenomePool::new();
        let mut cilium = Organelle::finished(OrganelleType::Cilium, 255);
        cilium.control[0] = Q10_ONE as i16;
        cilium.control[1] = 8; // due -x
        let mut cells = one_cell(&pool, &[(6, cilium)]);
        let substrate = Substrate::new(16, 16).unwrap();
        let mut ix = vec![0i32; substrate.len()];
        let mut iy = vec![0i32; substrate.len()];
        for tick in 0..500 {
            step_physics(&mut cells, &substrate, &mut ix, &mut iy, 0, tick, 1);
            assert!(cells.x[0] >= 0, "left the slide at tick {tick}");
            assert!(cells.x[0] < 16 * POS_ONE);
        }
        assert_eq!(cells.x[0], 0, "it should be pressed against the wall");
    }

    #[test]
    fn jitter_moves_a_cell_without_a_preferred_direction() {
        let pool = GenomePool::new();
        let substrate = Substrate::new(64, 64).unwrap();
        let mut net_x = 0i64;
        for seed in 0..64u64 {
            let mut cells = one_cell(&pool, &[]);
            cells.x[0] = pos(32);
            cells.y[0] = pos(32);
            let mut ix = vec![0i32; substrate.len()];
            let mut iy = vec![0i32; substrate.len()];
            for tick in 0..200 {
                step_physics(&mut cells, &substrate, &mut ix, &mut iy, 64, tick, seed);
            }
            net_x += (cells.x[0] - pos(32)) as i64;
        }
        // Over sixty-four independent walks the drift should be small next to the wander.
        assert!(
            net_x.abs() < 64 * POS_ONE as i64,
            "jitter has a bias: {net_x}"
        );
    }

    #[test]
    fn physics_is_deterministic() {
        let pool = GenomePool::new();
        let substrate = Substrate::new(16, 16).unwrap();
        let run = || {
            let mut cells = one_cell(&pool, &[]);
            let mut ix = vec![0i32; substrate.len()];
            let mut iy = vec![0i32; substrate.len()];
            for tick in 0..200 {
                step_physics(&mut cells, &substrate, &mut ix, &mut iy, 32, tick, 7);
            }
            (cells.x[0], cells.y[0])
        };
        assert_eq!(run(), run());
    }
}
