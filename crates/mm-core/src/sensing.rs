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
//! rowing against nothing. Momentum is not conserved and is not claimed to be; what is bounded
//! is how much a cilium can inject per tick.
//!
//! **And a cell is not carried by its own wake.** The water it is beating is moving relative to
//! the cell by construction, so it reads it as `slip` — which is what makes a ciliate a pump as
//! well as a swimmer, since `ecology::captured` charges a filter on exactly that — but it is not
//! swept along by it. Without that exemption the two paths were wildly asymmetric, because
//! thrust is damped by [`DRAG_RETAIN`] on the way in and drift is not damped at all, and every
//! cilium in `genomes/` was below the threshold where forward beat backward. `ECONOMY.md` §14
//! is the measurement, and `step_physics` is where both halves live.

use rayon::prelude::*;

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

/// Thrust one unit of cilium `param` can produce, in [`THRUST_SCALE`]ths of `Q10` of a square
/// per tick, per unit power.
///
/// # Why this is denominated in sixteenths, and why it is now a quarter of what it was
///
/// It was `4` — whole `Q10` per unit of `param` — and four is not a number a rate can be tuned
/// with. Halving it once is the only reduction that unit can express; halving it twice reaches
/// `1`, and a third time a cilium does nothing at all.
///
/// The measurement that made that matter: two `param 80` cilia at full power settle at
/// `2 x 4 x 80 / (1 - DRAG_RETAIN)` = 853 `Q10`, which is 0.83 squares a tick — **fifty squares a
/// second at 1x, so a cell crossed a 64-square slide in a second and a third.** That is not a
/// swimmer, it is a projectile.
///
/// So the unit is a sixteenth and the number carries the sixteen. `64` would be exactly the old
/// `4`, because 16 divides 64 and `64 * param / 16 == 4 * param` for every `param` with no
/// rounding anywhere. `16` is a quarter of that: two `param 80` cilia now settle at 0.21 squares
/// a tick, about twelve squares a second, and cross the same slide in five. Half of the quarter
/// is the tempo change every other rate in this commit took; the other half is that swimming was
/// independently too fast to watch, which `docs/ECONOMY.md` §14 measured from the other side.
///
/// The dial has sixty-four settings between here and stopped, where it had five.
pub const THRUST_PER_PARAM: i32 = 16;

/// What [`THRUST_PER_PARAM`] is denominated in.
///
/// Sixteenths, matching `MetabolicRates::throughput_per_param`, which is the same shape of
/// quantity — a rate per unit of `param` — and already the finer unit. Making the two agree means
/// a person reading either one is reading the same convention.
const THRUST_SCALE: i32 = 16;

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

/// Holding force one unit of holdfast `param` provides, per unit of effort.
///
/// The same figure as [`THRUST_PER_PARAM`], deliberately, so that gripping and swimming are
/// denominated in the same currency and a genome trading one for the other is trading like for
/// like. Against `fluid::MAX_VELOCITY` that makes a holdfast at half `param` just enough to
/// pin a newly-seeded cell in the fastest water the fluid can produce, and a larger body
/// proportionally harder to hold — see the load term in `step_physics`.
pub const GRIP_PER_PARAM: i32 = 4;

/// Energy a holdfast spends per unit of force it resists, `Q10`.
///
/// A quarter of [`THRUST_ENERGY`], which is the number that decides whether being sessile is
/// worth anything at all. Holding station has to be *cheaper* than swimming against the same
/// current or there is no reason to prefer it, and much cheaper still is wrong for the opposite
/// reason: a free anchor is one every lineage grows and never lets go of. A quarter puts a
/// fully-loaded grip at about a sixteenth of a working body's upkeep, so an anchored cell is
/// meaningfully better off than a swimming one and still visibly paying for something.
pub const HOLDFAST_ENERGY: i32 = Q10_ONE / 8;

/// How far past its own body a holdfast can reach to grip, `POS`.
///
/// Not decoration, and the reason is worth writing down because it cost a failing test to see.
/// The barrier pass drives a cell out of a wall until it *exactly* stops overlapping and then
/// stops pushing, so a cell resting against a wall settles at precisely the distance where an
/// overlap test flips to false. With the grip gated on overlap alone, the resting position is
/// the one position in which nothing holds: a cell would grip while being pushed out, let go
/// the moment it arrived, and slide away — measured at 3,776 `POS` of slip against 3,841 for a
/// cell with no holdfast at all, which is to say the anchor did nothing.
///
/// Half a square of reach past the body puts the grip's range comfortably outside the band the
/// collision pass parks a cell in. It is also the more honest picture: an attachment is a
/// stalk, not a point of tangency.
pub const HOLDFAST_REACH: i32 = POS_ONE / 2;

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

/// What a pH sensor reports: the acidity here, and which way the water sours.
///
/// pH is derived rather than stored (`chem::ph_of`), so this reads the two planes it is a
/// function of at each sample point rather than a plane of its own — the same walk
/// [`sense_chemical`] makes, done twice.
///
/// **Off the slide reads neutral, not nothing.** A chemical gradient at the edge reads the
/// outside as zero because there is genuinely none there; pH has no zero to read, and treating
/// the world's edge as maximally acidic would put a permanent gradient around the rim that every
/// cell on it would follow.
#[must_use]
pub fn sense_ph(substrate: &Substrate, x: i32, y: i32) -> ChemReading {
    let at = |dx: i32, dy: i32| -> i32 {
        let sx = x + dx;
        let sy = y + dy;
        if sx < 0 || sy < 0 || sx >= substrate.width() as i32 || sy >= substrate.height() as i32 {
            return crate::chem::PH_NEUTRAL;
        }
        substrate.ph_at(sx, sy)
    };
    ChemReading {
        concentration: at(0, 0),
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
    /// What the nearest neighbour is wearing. See `CellArena::badge`.
    pub badge: i16,
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
    /// What is glowing nearby, by band, with the direction it is coming from.
    ///
    /// Supplied only for a photosensor and only when it is asked for, the same way `touch` is:
    /// the scan is a square of side `2 * range + 1`, and a chemosensor has no business paying
    /// for one.
    pub glow: [ChemReading; crate::organelle::OrganelleType::EM_BANDS],
    /// How much of the cell is behind a shell, `Q10`.
    ///
    /// A whole-cell sum, so it cannot be worked out from the one organelle being read — several
    /// shells add up and `organelle::shell_cover` caps the total. Supplied the way `touch` and
    /// `glow` are, and for the same reason: the reading needs the cell and `read_sensor` is only
    /// given an organelle.
    pub shell_cover: i32,
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
        glow,
        shell_cover,
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
            // A photosensor detects electromagnetic radiation, and ambient light and a cell's
            // own glow are the same thing arriving from different places. So the glow readings
            // live here rather than on an organelle of their own — of which there is none free
            // anyway, `ReservedB` being `drifter_blind.mm`'s deliberately blind control.
            //
            // Appended, so 0, 1 and 2 still mean what they have always meant and only the
            // indices that used to wrap have changed.
            Some(match (index as u16) % 9 {
                0 => visible(sense_light(substrate, x, y).concentration),
                1 => visible_gradient(sense_light(substrate, x, y).gradient_x),
                2 => visible_gradient(sense_light(substrate, x, y).gradient_y),
                // Reported in `Q10` energy a tick rather than divided down to whole units like
                // the light readings are. A whole cell's upkeep is well under one unit — the
                // ancestor's is 0.41 — so `visible` would round every signature on the slide to
                // nothing. Saturating instead means a very bright crowd reads as 32767 and the
                // genome learns "too bright to count", which is a better failure than "dark".
                3 => sat_i16(glow[0].concentration),
                4 => sat_i16(glow[0].gradient_x),
                5 => sat_i16(glow[0].gradient_y),
                6 => sat_i16(glow[1].concentration),
                7 => sat_i16(glow[1].gradient_x),
                _ => sat_i16(glow[1].gradient_y),
            })
        }
        OrganelleType::PhSensor => {
            // The water itself rather than something in it. `index % 3` gives the value and its
            // two gradients, exactly as the chemosensor's does, so a genome that already knows
            // how to follow a chemical knows how to follow acidity.
            //
            // Reported as raw `Q10` rather than through `visible`, following the photosensor's
            // glow readings and for the reason their note gives: `visible` divides by `Q10_ONE`
            // because it is for *amounts*, and pH is a scale from nought to fourteen. Divided
            // down, the whole interesting range of a slide would be the integer 7 and every
            // gradient would be nothing.
            let r = sense_ph(substrate, x, y);
            Some(match (index as u16) % 3 {
                0 => sat_i16(r.concentration),
                1 => sat_i16(r.gradient_x),
                _ => sat_i16(r.gradient_y),
            })
        }
        OrganelleType::Shell | OrganelleType::CalciteShell => {
            // A shell reports the trade it is making, from both ends of it.
            //
            // The coverage is a readback: a genome that closed its shell can find out how much
            // of itself is actually behind mineral, which is not the same as what it asked for
            // once several shells are summed and the cap in `shell_cover` has bitten.
            //
            // The light is the other half, and it is the reading that cannot be computed from
            // anything else the cell can see: the incident light *after* its own shade. A
            // photosensor reports what is falling on the square; this reports what is getting
            // through to the chloroplasts. The difference between the two is what the armour
            // costs, in the only currency that matters to an autotroph.
            let cover = shell_cover;
            // Reported as raw `Q10`, not through `visible`.
            //
            // `visible` divides by `Q10_ONE` because it is for *amounts* — a concentration in
            // `Q10` becoming whole units. Both readings here are *fractions* of one, and coverage
            // is capped at seven eighths, so every value this organelle can ever report would
            // divide to zero. A sensor whose whole range rounds to nothing is not a sensor, and
            // it is the same integer truncation that made `detritus` never decay.
            Some(match (index as u16) % 2 {
                0 => sat_i16(cover),
                _ => {
                    let incident = sense_light(substrate, x, y).concentration;
                    sat_i16(q10_scale(
                        incident,
                        crate::organelle::shell_admits(cover),
                    ))
                }
            })
        }
        OrganelleType::TouchSensor => Some(match (index as u16) % 4 {
            0 => touch.contacts,
            1 => touch.nearest,
            2 => touch.contact_mass,
            // Appended rather than inserted, so 0, 1 and 2 mean what they have always meant.
            _ => touch.badge,
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
        OrganelleType::Holdfast => {
            let sq = substrate.index(x, y);
            let (svx, svy) = substrate.velocity();
            let flow = svx
                .get(sq)
                .copied()
                .unwrap_or(0)
                .saturating_abs()
                .saturating_add(svy.get(sq).copied().unwrap_or(0).saturating_abs());
            Some(match (index as u16) % 3 {
                // The grip it is exerting, so a genome can read back what it asked for.
                0 => sat_i16(holdfast_grip_of(organelle) / Q10_ONE),
                // How fast the water is going past. The reason to anchor at all, and the
                // quantity the load in `step_physics` is computed from.
                1 => sat_i16(flow / Q10_ONE),
                // Whether there is anything here worth gripping.
                //
                // Deliberately coarser than the physics: this asks whether any of the nine
                // squares around the cell is blocked, where `neighbours::touches_barrier` asks
                // whether the body actually overlaps one. So a cell can *feel* a wall it is
                // not yet holding, which is what makes a wall something to swim towards. The
                // two are allowed to differ because one is a sensor and the other is a
                // constraint; they would not be allowed to differ if this gated the grip.
                _ => {
                    let near = (-SENSOR_RANGE..=SENSOR_RANGE).any(|dy| {
                        (-SENSOR_RANGE..=SENSOR_RANGE)
                            .any(|dx| substrate.is_blocked(x + dx, y + dy))
                    });
                    i16::from(near)
                }
            })
        }
        _ => None,
    }
}

/// How hard one holdfast is holding on, `Q10` of its own capacity — zero if it has let go, is
/// still building, or is not a holdfast.
///
/// Unsigned, unlike [`cilium_power`]: there is no such thing as gripping backwards, and a negative
/// control input clamps to zero, which is "let go".
///
/// The sibling of [`cilium_power`] and [`crate::ecology::spike_reach`], and separate from
/// [`holdfast_grip_of`] for the reason they are: effort and size are independent, and the renderer
/// needs them apart. A large holdfast that has let go is drawn thick and limp.
#[must_use]
pub fn holdfast_effort(o: &Organelle) -> i32 {
    if !o.is_active() || o.kind != OrganelleType::Holdfast {
        return 0;
    }
    (o.control[0] as i32).clamp(0, Q10_ONE)
}

/// The grip one holdfast is exerting, `Q10`. See [`holdfast_grip`] for the whole cell.
#[must_use]
pub fn holdfast_grip_of(o: &Organelle) -> i32 {
    let effort = holdfast_effort(o);
    if effort == 0 {
        return 0;
    }
    q10_scale(GRIP_PER_PARAM.saturating_mul(o.param as i32), effort)
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

/// How hard one propulsor is beating, `Q10` of its own capacity — zero if it is idle, still
/// building, or not a propulsor.
///
/// Signed, and that is the whole of it: a cilium beating backwards pushes the cell backwards.
/// Power saturates, so a mutation to it is a small change in speed rather than a reversal
/// (SPEC §3).
///
/// Separate from [`cilium_thrust`], which multiplies this by the capacity `param` bought, because
/// they answer different questions. Effort and size are the two independent things a propulsor
/// has, and the renderer needs them apart: a beat's *amplitude* is the effort and a beat's
/// *length* is the size, so a large cilium idling is drawn long and still rather than short.
#[must_use]
pub fn cilium_power(o: &Organelle) -> i32 {
    if !o.is_active() || !matches!(o.kind, OrganelleType::Cilium | OrganelleType::Flagellum) {
        return 0;
    }
    (o.control[0] as i32).clamp(-Q10_ONE, Q10_ONE)
}

/// Thrust one cilium is producing, `Q10` of a square per tick.
///
/// [`cilium_power`] against the capacity the organelle's `param` bought.
#[must_use]
pub fn cilium_thrust(o: &Organelle) -> i32 {
    let power = cilium_power(o);
    if power == 0 {
        return 0;
    }
    if o.kind == OrganelleType::Flagellum {
        let capacity = FLAGELLUM_THRUST_PER_PARAM.saturating_mul(o.param as i32) / THRUST_SCALE;
        return q10_scale(capacity, power);
    }
    // Multiply before dividing, so the sixteenths are spent on resolution rather than lost to
    // truncation: `param 3` at the old unit was 12 and is 3 here, not 0.
    let capacity = THRUST_PER_PARAM.saturating_mul(o.param as i32) / THRUST_SCALE;
    q10_scale(capacity, power)
}

/// Thrust one unit of flagellum `param` produces, in [`THRUST_SCALE`]ths of `Q10`.
///
/// Half again a cilium's. A flagellum is one large organ where cilia are many small ones, and it
/// costs more to build and more to carry — see the catalogue.
pub const FLAGELLUM_THRUST_PER_PARAM: i32 = THRUST_PER_PARAM * 3 / 2;

/// How much of a flagellum's thrust the water feels, `Q10`.
///
/// **This is the whole difference between the two, and it is a number rather than a mechanism.**
/// `docs/FEEDING.md` §7: "a cilium stirs and a flagellum propels" — a rotifer beats cilia to make
/// a vortex that brings food to a body going nowhere; a flagellate swims. In engine terms that is
/// the split between how much of a thrust goes into the fluid as impulse and how much goes into
/// the body as motion, and a cilium puts all of it into the water.
///
/// A quarter, so a flagellum is a poor pump and a good engine. The consequence that matters is
/// that an anchored flagellate cannot filter-feed on its own current the way an anchored ciliate
/// can — `tests/ciliary_probe.rs` measures the ciliate at 1.03× a real current — so the choice
/// between the pair is a choice between two livings and not an upgrade.
pub const FLAGELLUM_WAKE: i32 = Q10_ONE / 4;

/// Total holding force this cell's holdfasts are exerting, `Q10`.
///
/// Unsigned, unlike [`cilium_thrust`]: there is no such thing as gripping backwards. A negative
/// control input clamps to zero, which is "let go" — the one instruction a holdfast needs
/// besides "hold on", and it costs nothing to give.
#[must_use]
pub fn holdfast_grip(cells: &CellArena, i: usize) -> i32 {
    let mut grip = 0i32;
    for o in cells.slots(i) {
        grip = grip.saturating_add(holdfast_grip_of(o));
    }
    grip
}

/// What one cell's organelles are doing to move it, worked out before the sequential loop.
///
/// The same hoist `metabolism::Capacities` is, for the same reason and with the same argument
/// for why it is exact: every field here is a function of the cell's organelle slots alone, and
/// [`step_physics`] never builds, tears down or retypes an organelle. So computing them a pass
/// earlier cannot change what they are, and the pass that computes them runs on every core while
/// the loop that consumes them runs on one.
///
/// It was two full walks of all sixteen slots per cell per tick — one summing cilium thrust, one
/// summing holdfast grip — for organelles most cells never grow. On the mixed benchmark slide
/// 3.8% of cells carry a cilium and 7.7% a holdfast; on the autotroph slide, none do, and the
/// loop walked every slot of every cell to discover that.
///
/// Scratch, in the sense `World::slip` and `World::crowding` are: derived fresh every tick from
/// the loadout, so it is excluded from equality, hashing and snapshots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BodyScan {
    /// Summed cilium thrust, `Q10` of a square per tick, before any energy shortfall is applied.
    pub thrust_x: i32,
    pub thrust_y: i32,
    /// The share of that thrust the water feels. Equal to `thrust_*` for a cell whose propulsors
    /// are all cilia, and a quarter of it for one driven by flagella — see [`FLAGELLUM_WAKE`].
    pub wake_x: i32,
    pub wake_y: i32,
    /// What beating that hard costs, whether or not the cell can afford it.
    pub spent: i32,
    /// Summed holdfast grip.
    pub grip: i32,
}

/// Fill `into` with one [`BodyScan`] per arena slot.
pub fn scan_bodies_into(cells: &CellArena, into: &mut Vec<BodyScan>) {
    into.clear();
    into.resize(cells.capacity(), BodyScan::default());
    into.par_iter_mut().enumerate().for_each(|(i, scan)| {
        if !cells.occupied(i) {
            return;
        }
        // Slots in order, and accumulated with the same saturating adds the loop used, because
        // both are order-sensitive in general and the point of this is that nothing changes.
        let (mut fx, mut fy, mut spent) = (0i32, 0i32, 0i32);
        // What the *water* feels, which is not the same sum: a cilium gives the water all of its
        // thrust and a flagellum a quarter of it. See `FLAGELLUM_WAKE`.
        let (mut wx, mut wy) = (0i32, 0i32);
        for o in cells.slots(i) {
            let thrust = cilium_thrust(o);
            if thrust == 0 {
                continue;
            }
            let (dx, dy) = cilium_direction(o);
            let (tx, ty) = (q10_scale(thrust, dx), q10_scale(thrust, dy));
            fx = fx.saturating_add(tx);
            fy = fy.saturating_add(ty);
            let share = if o.kind == OrganelleType::Flagellum {
                FLAGELLUM_WAKE
            } else {
                Q10_ONE
            };
            wx = wx.saturating_add(q10_scale(tx, share));
            wy = wy.saturating_add(q10_scale(ty, share));
            spent = spent.saturating_add(q10_scale(thrust.abs(), THRUST_ENERGY));
        }
        scan.thrust_x = fx;
        scan.thrust_y = fy;
        scan.wake_x = wx;
        scan.wake_y = wy;
        scan.spent = spent;
        scan.grip = holdfast_grip(cells, i);
    });
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

/// What acts on every cell whatever it is doing, as opposed to what a cell does to itself.
///
/// Grouped because they arrive together from the scenario and because neither of them is a
/// thing a cell chooses — which is also the distinction that decides whether the water feels a
/// reaction. Cilium thrust is the cell pushing on the world and the world pushes back; these are
/// the world pushing on the cell, and it does not.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BodyForces {
    /// Brownian jitter, `Q10` of a square per tick.
    pub jitter: i32,
    /// Pull towards the middle of the slide, `Q10` of a square per tick per tick.
    pub gravity: i32,
}

/// One tick of physics for the population: thrust, jitter, gravity, drag, and integration.
///
/// Sequential and in slot order, like resolve — this is where cells push on a shared fluid,
/// so it is not a place for scheduling to be observable.
pub fn step_physics(
    cells: &mut CellArena,
    substrate: &Substrate,
    // The cilia's own contribution to the water, rebuilt from scratch every tick.
    //
    // Separate from `World::impulse_x/y`, which accumulates at `impulse_retain` and which only
    // `World::inject_impulse` writes now — this phase no longer touches it at all. A cilium's reaction must *not* accumulate, and the reason
    // is `ECONOMY.md` §14: at 15/16 a steady beat builds to sixteen times one tick's injection
    // and saturates at `fluid::MAX_VELOCITY` however small the cilium, so every cell's own wake
    // was the same size and the exemption below could not be sized to the cell that earned it.
    // Rebuilt each tick it is exactly `-thrust`, which is a quantity this loop already has.
    //
    // It is also the better answer for a colony: one accumulating cell saturates its square on
    // its own, so a hundred ciliated cells stir no harder than one. Summed fresh, a hundred of
    // them stir a hundred times as hard.
    stir_x: &mut [i32],
    stir_y: &mut [i32],
    forces: BodyForces,
    // Written, not read: the speed of the water *past* each cell, `Q10`.
    //
    // It has to be computed here because nowhere else can. A cell's velocity through the water
    // is not `vx` and it is not the fluid's velocity either — the drift is added to the
    // position step without ever touching `vx`, so a cell being carried along has a velocity of
    // zero and sees a full current, and a cell holding station against one has a velocity of
    // zero and sees the same. The two are opposite situations with identical fields, and the
    // only place the difference exists is here, where the holdfast decides how much of the
    // drift the cell actually takes.
    //
    // Scratch in the same sense as `crowding` and `pressure`: derived fresh every tick from
    // positions and organelles, so it is excluded from equality, hashing and snapshots.
    slip: &mut Vec<i32>,
    // What each cell's cilia and holdfasts add up to, from `scan_bodies_into`.
    scan: &[BodyScan],
    tick: u64,
    seed: u64,
) -> PhysicsReport {
    let BodyForces { jitter, gravity } = forces;
    let mut report = PhysicsReport::default();
    slip.clear();
    slip.resize(cells.capacity(), 0);
    // Last tick's stir has already been published into the velocity field and read back as
    // drift; what the cilia are doing *now* replaces it rather than adding to it.
    stir_x.fill(0);
    stir_y.fill(0);
    let w = substrate.width() as i32;
    let h = substrate.height() as i32;
    // Empty on a slide with no barriers, which makes `touches_barrier` a single branch and a
    // holdfast on such a slide correctly useless: there is nothing to hold.
    let blocked: &[bool] = if substrate.has_barriers() {
        substrate.blocked()
    } else {
        &[]
    };

    for i in 0..cells.capacity() {
        if !cells.occupied(i) {
            continue;
        }
        let id = cells.id_at(i);
        let ctx = RandCtx::new(seed, tick, id.ordering_key());

        // --- thrust from every cilium, and the reaction into the water ---
        //
        // Summed in `scan_bodies_into`, which is a parallel pass. See `BodyScan`.
        let body = scan.get(i).copied().unwrap_or_default();
        let (mut fx, mut fy) = (body.thrust_x, body.thrust_y);
        let (mut wake_x, mut wake_y) = (body.wake_x, body.wake_y);
        let spent = body.spent;
        if spent > 0 {
            // Beating costs energy whether or not it achieves anything, which is what makes
            // swimming a trade rather than a free action.
            let paid = cells.energy[i].min(spent);
            cells.energy[i] = cells.energy[i].saturating_sub(paid);
            if paid < spent && spent > 0 {
                // Could not afford full power; scale the thrust back to what was paid for.
                fx = ((fx as i64 * paid as i64) / spent as i64) as i32;
                fy = ((fy as i64 * paid as i64) / spent as i64) as i32;
                wake_x = ((wake_x as i64 * paid as i64) / spent as i64) as i32;
                wake_y = ((wake_y as i64 * paid as i64) / spent as i64) as i32;
            }
            report.energy_spent += paid as i64;
        }

        // What the water is entitled to feel, kept before anything else is added to `fx`. Not
        // the body's whole motion and, since the flagellum arrived, not the whole of its thrust
        // either — a flagellum puts most of its push into going somewhere. See the reaction
        // below and `FLAGELLUM_WAKE`.
        let (thrust_x, thrust_y) = (wake_x, wake_y);

        // --- gravity, towards the middle of the slide ---
        //
        // A force, so it is damped by drag and undone by contact reconciliation. See
        // `Scenario::gravity` for why this is not a current.
        if gravity > 0 {
            let cx = (w as i64 * POS_ONE as i64) / 2;
            let cy = (h as i64 * POS_ONE as i64) / 2;
            let dx = cx - cells.x[i] as i64;
            let dy = cy - cells.y[i] as i64;
            let d = (dx * dx + dy * dy).isqrt();
            if d > 0 {
                fx = fx.saturating_add((gravity as i64 * dx / d) as i32);
                fy = fy.saturating_add((gravity as i64 * dy / d) as i32);
            }
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

        // The fluid carries the cell along with it. Kept as coordinates as well as an index,
        // because the reaction at the bottom of the loop needs the square *behind* this one and
        // by then the position has already been integrated.
        let (sqx, sqy) = (pos_to_square(cells.x[i]), pos_to_square(cells.y[i]));
        let sq = substrate.index(sqx, sqy);
        let (svx, svy) = substrate.velocity();
        let mut drift_x = svx.get(sq).copied().unwrap_or(0);
        let mut drift_y = svy.get(sq).copied().unwrap_or(0);

        // --- a cell is carried by the water, but not by the part of it that is its own wake ---
        //
        // `ECONOMY.md` §14. The two paths into a cell are not symmetric: thrust arrives through
        // `vx` and is damped by `DRAG_RETAIN`, so `f` settles the cell at `4f/3`; the reaction
        // arrives here, as drift, and is added straight to the position step undamped. A cell
        // reading its own wake back therefore had a threshold at 192 `Q10` of thrust below which
        // it travelled *backwards*, and every cilium in `genomes/` was under it.
        //
        // The fix is not to hide the wake — the wake is the point, it stirs the chemistry, it
        // pushes the neighbours, and `slip` below reads it as water going past, which is what
        // makes a ciliate a pump as well as a swimmer. The fix is that a swimmer does not ride
        // its own current. A cell in a river is carried by the river; a cell beating water past
        // itself is not carried by the water it is beating, because that water's motion is
        // *relative to the cell* by construction. Both halves of that are what a cilium is for
        // in the first place, and until now the second half was drowning the first.
        //
        // Only ever toward zero, and never by more than the cell's own contribution. That is
        // what makes this an exemption rather than a free anchor: a cell facing into a current
        // can cancel exactly as much of it as it is itself generating and no more, so holding
        // station against a river costs the full thrust of swimming up it. The cap is exact
        // because `stir` does not accumulate — one tick's injection is the whole of this cell's
        // share of it.
        let exempt = |drift: &mut i32, thrust: i32| {
            if thrust == 0 {
                return;
            }
            // The wake is antiparallel to the thrust; anything parallel is somebody else's.
            if drift.signum() == thrust.signum() {
                return;
            }
            let own = thrust.saturating_abs().min(drift.saturating_abs());
            *drift -= own * drift.signum();
        };
        // `thrust_x`, not `body.thrust_x`: what the cell could *afford* is what it injected.
        exempt(&mut drift_x, thrust_x);
        exempt(&mut drift_y, thrust_y);

        // --- the holdfast: how much of that carrying a cell can refuse ---
        //
        // Everything above is a body in free fall with the water. This is the one thing that
        // can decline, and it is what makes staying put a strategy rather than an impossibility
        // (SPEC §17.6). It needs a barrier to grip: a cell holding nothing but water holds
        // nothing, which is why this and cell-barrier contact had to arrive together.
        //
        // Load rises with the drift it is resisting *and with the cell's own radius*, because a
        // bigger body presents more of itself to the current. That is the term that makes size
        // a trade here — a large sessile cell intercepts more water and must grip harder for
        // it — and it is the same frontal-area reasoning particulate capture will want.
        //
        // Slipping is proportional rather than all-or-nothing. A cliff at `grip == load` would
        // be exactly the discontinuity SPEC §3 works to keep out of the landscape: one point of
        // `param` would flip a cell from anchored to adrift, and evolution cannot climb that.
        // Under-gripping instead means being carried more slowly, which is a gradient.
        // It resists what moves the cell, and that is not only the water.
        //
        // `drift` was reduced here and `cells.vx` was not, so a holdfast cancelled the current's
        // pull and left the cell's own push untouched: a ciliate could beat its way off its own
        // anchor for nothing. Measured by `tests/ciliary_probe.rs` before this changed — a
        // gripping cell with two cilia at full power travelled twenty-four squares in four
        // hundred ticks while an identical cell holding station against a quarter-speed current
        // moved half a square. The asymmetry was not a decision; `step_physics` advances a body
        // by `velocity + drift` and only one of the two was ever offered to the anchor.
        //
        // One surface holds one body, and it does not know whether the pull it is resisting came
        // from the water or from the cell's own cilia. Resisting the *net* of the two is what
        // makes that true — a cell swimming upstream at exactly the current's speed is not going
        // anywhere and has nothing for its holdfast to do.
        //
        // What it buys is the trade FEEDING.md §7 is about. Thrust that no longer moves the body
        // still goes into the water as impulse, and the water comes back as `slip` a few lines
        // below, so gripping hard turns a beating cell into a *pump* — the sessile ciliary
        // suspension feeder — while letting go turns the same cell into a swimmer. One organelle,
        // one control word, two livings, and the dial between them is continuous.
        let grip = body.grip;
        let net_x = drift_x.saturating_add(cells.vx[i]);
        let net_y = drift_y.saturating_add(cells.vy[i]);
        if grip > 0 && (net_x != 0 || net_y != 0) {
            // Only now is the barrier scan worth doing. A cell with no holdfast, or one going
            // nowhere, never pays for it — which matters because this runs over the whole
            // population every tick and most cells will never grow one.
            let radius = crate::biology::radius(cells, i);
            let ri = crate::fixed::q10_to_pos(radius).saturating_add(HOLDFAST_REACH);
            if crate::neighbours::touches_barrier(cells, blocked, w, h, i, ri) {
                let speed = net_x
                    .saturating_abs()
                    .saturating_add(net_y.saturating_abs());
                let load = q10_scale(speed, radius).max(1);
                let want =
                    ((grip as i64 * Q10_ONE as i64) / load as i64).min(Q10_ONE as i64) as i32;
                // Charged on the force actually resisted, so an anchored cell in still water
                // pays nothing beyond the organelle's upkeep and one in a torrent pays for the
                // torrent. Routed through `energy_spent`, which `World` dissipates through the
                // ledger — holding on is work, and work leaves the world as heat (I5).
                //
                // A cell that cannot afford the whole grip buys the part it can, rather than
                // being charged for a hold it does not get. Same shape as the cilium above.
                let cost = q10_scale(q10_scale(load, want), HOLDFAST_ENERGY);
                let held = if cost <= 0 {
                    want
                } else {
                    let paid = cells.energy[i].min(cost);
                    cells.energy[i] = cells.energy[i].saturating_sub(paid);
                    report.energy_spent += paid as i64;
                    ((want as i64 * paid as i64) / cost as i64) as i32
                };
                drift_x = drift_x.saturating_sub(q10_scale(drift_x, held));
                drift_y = drift_y.saturating_sub(q10_scale(drift_y, held));
                // The same fraction off the cell's own motion, component by component as the
                // drift is. Written back to the velocity rather than taken off the position
                // step, because the anchor absorbs the momentum: a cell that has been held does
                // not arrive next tick still carrying the speed it was held against.
                //
                // Before `slip` is read, deliberately. An anchored cell's velocity is now near
                // zero, so what it reads as water going past is the stir its own cilia put into
                // the square — which is the whole of how a pump feeds.
                cells.vx[i] = cells.vx[i].saturating_sub(q10_scale(cells.vx[i], held));
                cells.vy[i] = cells.vy[i].saturating_sub(q10_scale(cells.vy[i], held));
            }
        }

        // What the water is doing past this cell, which is the whole of whether it can filter.
        //
        // The water moves at the field's velocity; the cell moves at its own velocity plus
        // whatever share of the drift it did not refuse. The difference is the two subtracted,
        // and it comes out right in all three cases without any of them being special-cased: a
        // cell carried along reads zero, a cell gripping reads the full current, and a cell
        // swimming through still water reads its own speed.
        let water_x = svx.get(sq).copied().unwrap_or(0);
        let water_y = svy.get(sq).copied().unwrap_or(0);
        let past_x = water_x.saturating_sub(cells.vx[i]).saturating_sub(drift_x);
        let past_y = water_y.saturating_sub(cells.vy[i]).saturating_sub(drift_y);
        if let Some(slot) = slip.get_mut(i) {
            *slot = past_x
                .saturating_abs()
                .saturating_add(past_y.saturating_abs());
        }

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
        //
        // **Thrust only.** This used to react to the whole of `fx`, which was harmless while
        // thrust was the only thing in it and became badly wrong the moment it was not. A
        // cilium is self-propulsion and owes the water a reaction; gravity and Brownian jitter
        // are external, and a cell falling does not shove the water the other way any more than
        // a stone does.
        //
        // Found by adding `Scenario::gravity` and watching a crowd under an inward force
        // evacuate the middle and pile against the walls. Every cell pulled inward was pushing
        // water outward, the impulse retains fifteen sixteenths of itself per fluid step so it
        // accumulated, and the outward current it built measured about 0.05 squares per tick
        // against gravity's terminal 0.008 — so the water won by roughly eight to one and
        // carried the whole population out with it. Gravity was never backwards; it was
        // fighting a current it had itself created.
        //
        // **Into `stir`, under the cell, and not accumulated.** See `ECONOMY.md` §14 and the
        // exemption above, which is the other half of this and cannot be written without it.
        //
        // Under the cell because that is where a cilium's water is: `slip` is read at the cell's
        // own square, `ecology::captured` charges a filter on `slip`, and a ciliate's feeding
        // current is the same beating that moves it. Putting the wake anywhere else buys a
        // clean swimmer at the price of the pump, and a cilium is both.
        //
        // Into `stir` rather than `impulse` because `impulse` accumulates at 15/16 and a steady
        // beat therefore reaches sixteen times one tick's injection and saturates, which makes
        // every wake the same size whatever made it — and a wake that cannot be attributed
        // cannot be exempted. Rebuilt fresh, this square holds exactly the sum of what the cells
        // standing in it are doing right now.
        if thrust_x != 0 || thrust_y != 0 {
            report.stirred = report.stirred.saturating_add(1);
            if let Some(slot) = stir_x.get_mut(sq) {
                *slot = slot.saturating_sub(thrust_x).clamp(-Q10_ONE, Q10_ONE);
            }
            if let Some(slot) = stir_y.get_mut(sq) {
                *slot = slot.saturating_sub(thrust_y).clamp(-Q10_ONE, Q10_ONE);
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
    /// Cells that put something into the water this tick, so the caller knows the velocity
    /// field is out of date without having to scan the slide to find out.
    pub stirred: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{CellId, CellSeed};
    use crate::fixed::{pos, q10};
    use crate::genome::GenomePool;

    /// The physics phase as `World::step` runs it: body scan first, then the loop.
    ///
    /// Shadows [`super::step_physics`] deliberately, so that every test below exercises the pair
    /// together. Gathering the scan is not optional for a caller — a stale or absent one is a
    /// slide where no cilium pushes and no holdfast holds — and a test that skipped it would be
    /// testing a phase nothing runs.
    fn step_physics(
        cells: &mut CellArena,
        substrate: &Substrate,
        stir_x: &mut [i32],
        stir_y: &mut [i32],
        forces: BodyForces,
        slip: &mut Vec<i32>,
        tick: u64,
        seed: u64,
    ) -> PhysicsReport {
        let mut scan = Vec::new();
        super::scan_bodies_into(cells, &mut scan);
        super::step_physics(
            cells, substrate, stir_x, stir_y, forces, slip, &scan, tick, seed,
        )
    }

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
            badge: 0,
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
        let report = step_physics(
            &mut cells,
            &substrate,
            &mut ix,
            &mut iy,
            BodyForces {
                jitter: 0,
                gravity: 0,
            },
            &mut Vec::new(),
            0,
            1,
        );

        assert!(cells.x[0] > before_x, "the cell did not move");
        assert!(cells.energy[0] < before_energy, "swimming was free");
        assert!(report.energy_spent > 0);

        // Under the cell, because that is where a cilium's water is and `slip` is read there:
        // a ciliate is a pump as well as a swimmer, and the wake is the pump. What stops the
        // wake from also driving the cell backwards is the exemption in `step_physics`, not
        // moving the wake somewhere the cell cannot feel it — see `ECONOMY.md` §14 and
        // `a_swimmer_does_not_have_to_outrun_its_own_wake` below, which is the other half.
        let under = substrate.index(8, 8);
        assert!(
            ix[under] < 0,
            "the water under the cell was not pushed the other way: {}",
            ix[under]
        );
        assert_eq!(
            ix[under], -cilium_thrust(&cilium),
            "the stir layer must hold exactly one tick of thrust, not an accumulation"
        );
    }

    #[test]
    fn a_swimmer_does_not_have_to_outrun_its_own_wake() {
        // `ECONOMY.md` §14, as two assertions rather than a place. A cell beating steadily must
        // end up ahead of where it started at *any* power — the weakest cilium in the catalogue
        // is the case that used to end up at the far wall behind it — and it must get exactly as
        // far as it would in water it had not stirred, because it does not ride its own current.
        //
        // Both are needed and they guard different things. The first fails if a cilium's
        // reaction goes back to accumulating in `impulse`; the second fails if the exemption in
        // `step_physics` is dropped, which costs a swimmer three quarters of its speed without
        // ever reversing it.
        //
        // The control is the same run with the stir layer never published: same thrust, same
        // drag, water that stays still.
        let pool = GenomePool::new();
        for param in [8u8, 20, 40, 80, 200] {
            let mut swim = |publish: bool| -> i32 {
                let mut cilium = Organelle::finished(OrganelleType::Cilium, param);
                cilium.control[0] = Q10_ONE as i16;
                cilium.control[1] = 0; // due +x
                let mut cells = one_cell(&pool, &[(6, cilium)]);
                let mut substrate = Substrate::new(64, 16).unwrap();
                let mut sx = vec![0i32; substrate.len()];
                let mut sy = vec![0i32; substrate.len()];
                let nil = vec![0i32; substrate.len()];
                let before = cells.x[0];
                for tick in 0..200u64 {
                    step_physics(
                        &mut cells,
                        &substrate,
                        &mut sx,
                        &mut sy,
                        BodyForces {
                            jitter: 0,
                            gravity: 0,
                        },
                        &mut Vec::new(),
                        tick,
                        1,
                    );
                    // What `World::step` does between phases: publish the stir layer into the
                    // velocity field. It is rebuilt from the cilia every tick rather than
                    // decayed, which is what lets the exemption be sized to the cell exactly.
                    if publish {
                        crate::light::CurrentField::Still.apply(&mut substrate, &nil, &nil, &sx, &sy, 0);
                    }
                }
                cells.x[0] - before
            };
            let stirred = swim(true);
            let still = swim(false);
            assert!(
                stirred > 0,
                "a param {param} cilium drove its cell backwards: {stirred}"
            );
            assert_eq!(
                stirred, still,
                "a param {param} cilium was taxed by water it stirred itself"
            );
        }
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
            step_physics(
                &mut cells,
                &substrate,
                &mut ix,
                &mut iy,
                BodyForces {
                    jitter: 0,
                    gravity: 0,
                },
                &mut Vec::new(),
                tick,
                1,
            );
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
        step_physics(
            &mut rich,
            &substrate,
            &mut ix,
            &mut iy,
            BodyForces {
                jitter: 0,
                gravity: 0,
            },
            &mut Vec::new(),
            0,
            1,
        );

        let mut poor = one_cell(&pool, &[(6, cilium)]);
        poor.energy[0] = 1;
        let mut ix2 = vec![0i32; substrate.len()];
        let mut iy2 = vec![0i32; substrate.len()];
        step_physics(
            &mut poor,
            &substrate,
            &mut ix2,
            &mut iy2,
            BodyForces {
                jitter: 0,
                gravity: 0,
            },
            &mut Vec::new(),
            0,
            1,
        );

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
            step_physics(
                &mut cells,
                &substrate,
                &mut ix,
                &mut iy,
                BodyForces {
                    jitter: 0,
                    gravity: 0,
                },
                &mut Vec::new(),
                tick,
                1,
            );
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
            step_physics(
                &mut cells,
                &substrate,
                &mut ix,
                &mut iy,
                BodyForces {
                    jitter: 0,
                    gravity: 0,
                },
                &mut Vec::new(),
                tick,
                1,
            );
            assert!(cells.x[0] >= 0, "left the slide at tick {tick}");
            assert!(cells.x[0] < 16 * POS_ONE);
        }
        assert_eq!(cells.x[0], 0, "it should be pressed against the wall");
    }

    #[test]
    fn gravity_pulls_a_cell_towards_the_middle_of_the_slide() {
        // Direction, asserted, because the packing bench could not tell me: a crowd that drifts
        // apart under an inward force and a crowd that drifts apart under an outward one look
        // identical for the first few hundred ticks unless you know where the middle was.
        let pool = GenomePool::new();
        let substrate = Substrate::new(48, 48).unwrap();
        let mut cells = one_cell(&pool, &[]);
        let (mut ix, mut iy) = (vec![0i32; substrate.len()], vec![0i32; substrate.len()]);
        let before = (cells.x[0], cells.y[0]);
        for tick in 0..64u64 {
            step_physics(
                &mut cells,
                &substrate,
                &mut ix,
                &mut iy,
                BodyForces {
                    jitter: 0,
                    gravity: 64,
                },
                &mut Vec::new(),
                tick,
                1,
            );
        }
        assert!(
            cells.x[0] > before.0 && cells.y[0] > before.1,
            "a cell at the top left should fall towards the middle, not away from it: \
             {before:?} -> {:?}",
            (cells.x[0], cells.y[0])
        );
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
                step_physics(
                    &mut cells,
                    &substrate,
                    &mut ix,
                    &mut iy,
                    BodyForces {
                        jitter: 64,
                        gravity: 0,
                    },
                    &mut Vec::new(),
                    tick,
                    seed,
                );
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
                step_physics(
                    &mut cells,
                    &substrate,
                    &mut ix,
                    &mut iy,
                    BodyForces {
                        jitter: 32,
                        gravity: 0,
                    },
                    &mut Vec::new(),
                    tick,
                    7,
                );
            }
            (cells.x[0], cells.y[0])
        };
        assert_eq!(run(), run());
    }
}
