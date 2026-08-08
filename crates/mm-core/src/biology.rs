//! Execute and resolve: how a running genome reaches the world (SPEC §12).
//!
//! # The two halves
//!
//! **Execute** gives each cell its instruction budget. The VM's world-facing opcodes go
//! through [`CellHost`], which reads a world nobody is writing and records what the cell
//! asked for. No cell writes shared state, so no cell can observe another's turn.
//!
//! **Resolve** replays those requests in slot order — which is cell-id order — and actually
//! moves matter. Two cells that ate from the same square get served in that order, forever,
//! on every machine and at every thread count. That is the whole of I1 and I6 on the cell
//! side; a cell that could eat directly would make the outcome depend on which thread got
//! there first, and the failure would reproduce only sometimes.
//!
//! # Matter never enters or leaves
//!
//! Every operation here *moves* matter between the fluid, a cell's interior and a corpse.
//! All three are inside the conserved total (I4), so eating, excreting, dividing and dying
//! change where matter is and never how much there is. The one place a species total may
//! change is a balanced reaction, and those go through the ledger — see
//! [`crate::metabolism`].
//!
//! # Death returns everything
//!
//! A corpse is not a special object. When a cell dies its interior chemistry and its
//! structural mass are returned to the fluid it was standing on. Nothing is destroyed, and
//! nothing needs a decay timer to avoid leaking, because there was never anywhere for it to
//! leak to.

use std::sync::Arc;

use rayon::prelude::*;

use crate::cell::{CellArena, CellSeed};
use crate::chem::{ChemTable, CHEM_COUNT};
use crate::config::VmConfig;
use crate::fixed::{cell_to_q10, pos_to_square, q10, q10_scale, sat_i16, POS_ONE, Q10_ONE};
use crate::genome::{Genome, GenomePool};
use crate::host::Host;
use crate::intent::{Intent, IntentBuffer, Pending, PendingBirth, SlotIntents};
use crate::ledger::Ledger;
use crate::metabolism::Metabolism;
use crate::mutation::{copy_error, copy_error_rate, mutate_structural, MutationRates};
use crate::organelle::{
    slot_index, MembraneReading, Organelle, OrganelleType, MEMBRANE_SLOT, SLOT_COUNT,
};
use crate::rng::{Purpose, RandCtx};
use crate::substrate::Substrate;
use crate::vm::Vm;

/// How much of one chemical a cell can hold before it stops being able to eat, `Q10`.
///
/// A vacuole raises it, which is the whole point of building one. Without a bound a cell
/// could hoover up the entire slide into one square's worth of cytoplasm.
pub const BASE_INTERIOR_CAPACITY: i32 = 64 * Q10_ONE;

/// Bytes of genome one unit of nucleus `param` can hold.
///
/// Nucleus capacity bounds genome length (SPEC §4.1), and nucleus upkeep is charged per unit
/// of `param`, so genome bloat costs energy every tick rather than being forbidden by a rule.
pub const GENOME_BYTES_PER_NUCLEUS_PARAM: usize = 8;

/// What one tick of biology did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BiologyReport {
    pub births: u32,
    pub deaths: u32,
    pub eaten: i64,
    pub emitted: i64,
    pub built: u32,
    pub torn: u32,
    /// Divisions refused because the parent could not pay.
    pub failed_splits: u32,
    /// Junctions formed this tick, split by whether the key matched (SPEC §8.2).
    pub junctions_consensual: u32,
    pub junctions_forced: u32,
    /// `JOIN`s that failed: no free slot, out of reach, or could not pay the penalty.
    pub junctions_refused: u32,
    pub junctions_broken: u32,
    /// Matter and energy moved across soft junctions, `Q10`.
    pub transferred: i64,
    /// Genome bytes written into another cell's nucleus. Parasitism, counted.
    pub foreign_injections: u32,
    /// Structural mass the tick's dead left as carrion, `Q10`.
    pub to_carrion: i64,
}

/// The world as one cell sees it during its own execution.
///
/// Holds only shared references to the world, so "no cell writes shared state" is a property
/// of the type rather than a convention somebody has to remember.
#[derive(Debug)]
pub struct CellHost<'a> {
    slot: usize,
    cells: &'a CellArena,
    substrate: &'a Substrate,
    neighbours: &'a crate::neighbours::NeighbourIndex,
    intents: SlotIntents<'a>,
    /// The square the cell is standing on.
    square: usize,
    /// How much of each chemical the cell has already promised itself this tick, so that a
    /// genome calling `EAT` twice is not told the same food twice.
    claimed: [i32; CHEM_COUNT],
    /// The tick, for the one sensor that reads a clock rather than the world.
    tick: u64,
    /// What a unit of spike extension does, so the spike's reading can say what it is about to
    /// deal rather than how many cells are in front of it. Copied in rather than reached for
    /// through a config reference, because a host holds only what one cell may read.
    spike_damage: i32,
    /// How far a photosensor looks for other cells' glow, in squares. Copied in for the same
    /// reason `spike_damage` is.
    em_range: i32,
    /// Which reactions this world offers, so a mitochondrion's reading can be about the
    /// substrate *it* burns rather than about chemical 8 (M10.3).
    chemistry: crate::organelle::MetabolicChemistry,
}

/// Read an organelle's output the way `OGET` does, from outside the VM.
///
/// The readings of SPEC §6.2 are computed inside [`CellHost`], which only exists during the
/// execute phase. This lets a test — or the inspector — ask what a genome would see without
/// running one, so "the spike reports contact" is checkable rather than inferable from
/// behaviour three phases later.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn read_organelle(
    cells: &CellArena,
    substrate: &Substrate,
    neighbours: &crate::neighbours::NeighbourIndex,
    cell: usize,
    slot: usize,
    idx: i16,
    spike_damage: i32,
    em_range: i32,
    chemistry: crate::organelle::MetabolicChemistry,
) -> i16 {
    // A throwaway intent buffer: reading an organelle pushes nothing, but the host holds a
    // slot's worth either way.
    let mut buffer = IntentBuffer::new();
    buffer.begin_tick(cells.capacity());
    let Some(intents) = buffer.slots_mut().nth(cell) else {
        return 0;
    };
    let mut host = CellHost::new(
        cell,
        cells,
        substrate,
        neighbours,
        intents,
        0,
        spike_damage,
        em_range,
        chemistry,
    );
    Host::oget(&mut host, idx, slot as i16)
}

impl<'a> CellHost<'a> {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        slot: usize,
        cells: &'a CellArena,
        substrate: &'a Substrate,
        neighbours: &'a crate::neighbours::NeighbourIndex,
        intents: SlotIntents<'a>,
        tick: u64,
        spike_damage: i32,
        em_range: i32,
        chemistry: crate::organelle::MetabolicChemistry,
    ) -> CellHost<'a> {
        let square = substrate.index(pos_to_square(cells.x[slot]), pos_to_square(cells.y[slot]));
        CellHost {
            slot,
            cells,
            substrate,
            neighbours,
            intents,
            square,
            claimed: [0; CHEM_COUNT],
            tick,
            spike_damage,
            em_range,
            chemistry,
        }
    }

    /// How much more of one chemical this cell could hold.
    fn headroom(&self, c: usize) -> i32 {
        let capacity = interior_capacity(self.cells, self.slot);
        let held = self.cells.interior(self.slot)[c];
        capacity.saturating_sub(held).max(0)
    }

    /// What the membrane reports about the cell itself (SPEC §5.1).
    fn membrane_reading(&self, idx: i16) -> i16 {
        let i = self.slot;
        match MembraneReading::decode(idx) {
            MembraneReading::Mass => q10_to_visible(self.cells.mass[i]),
            MembraneReading::Energy => q10_to_visible(self.cells.energy[i]),
            MembraneReading::Age => sat_i16(self.cells.age[i].min(i32::MAX as u32) as i32),
            MembraneReading::Radius => q10_to_visible(radius(self.cells, i)),
            MembraneReading::Damage => q10_to_visible(self.cells.damage[i]),
            MembraneReading::Badge => self.cells.badge[i] as i16,
            MembraneReading::Chemical => {
                let c = MembraneReading::chemical_of(idx);
                q10_to_visible(self.cells.interior(i)[c])
            }
        }
    }
}

#[inline]
fn q10_to_visible(v: i32) -> i16 {
    sat_i16(v / Q10_ONE)
}

/// A cell's radius, `Q10` in substrate squares. Grows with mass, slowly.
#[must_use]
pub fn radius(cells: &CellArena, i: usize) -> i32 {
    // Integer square-root: radius rises with mass but not linearly, so a cell twice as heavy
    // is not twice as wide. Monotonic, which is all anything needs of it.
    //
    // Found by bit rather than by counting up. The counting version was correct and read
    // better, but this is called once per cell per neighbour on the collision and touch-sensor
    // paths, and its cost there was proportional to how heavy the cell was — so the simulation
    // got slower as its cells grew, which is not a thing anyone would think to look for.
    let m = (cells.mass[i] / Q10_ONE).max(0) as u32;
    let mut r = 0u32;
    let mut bit = 1u32 << 15;
    while bit != 0 {
        let try_r = r | bit;
        if try_r.saturating_mul(try_r) <= m {
            r = try_r;
        }
        bit >>= 1;
    }
    (Q10_ONE / 4).saturating_add((r as i32).saturating_mul(Q10_ONE / 8))
}

/// How much of any one chemical a cell can hold.
#[must_use]
pub fn interior_capacity(cells: &CellArena, i: usize) -> i32 {
    let mut capacity = BASE_INTERIOR_CAPACITY;
    for o in cells.slots(i) {
        if o.kind == OrganelleType::Vacuole && o.is_active() {
            capacity = capacity.saturating_add(q10(o.param as i32));
        }
    }
    capacity
}

/// How firmly a cell holds its own shape, `Q10`. Zero is a bag; one is a walled sphere.
///
/// A **product** of a wall and a pressure, not a sum, because that is what the two are physically:
/// a thick wall with no turgor is plasmolysed and floppy, and turgor with no wall bursts. Yeast
/// circle-pack because they have both; animal tissue deforms into a continuous sheet because it
/// has the second and not the first.
///
/// Both terms are things the genome already pays for and already chooses:
///
/// * the **wall** is `membrane.param`, set by `BUILD`, and it costs structural matter to raise —
///   a membrane at 200 costs four times the matter of one at 24, and more upkeep to carry;
/// * the **turgor** is [`osmotic_load`] against the threshold `osmotic_upkeep` already charges
///   on, so being firm means holding solute and paying the quadratic bill for it.
///
/// So "be a marble rather than a drop of foam" is a real investment in both currencies rather
/// than a free switch, which is the property that makes it worth having as a choice at all.
///
/// Read by two things that are otherwise unrelated, and the split matters. `neighbours::core_permille`
/// scales it by `MetabolicRates::rigidity_gain` and lets it change *what the simulation does*,
/// which is why that half is gated behind a scenario knob and off by default. `mm_app::slide`
/// reads it raw, to decide how much a cell bulges into the space its neighbours leave — which
/// changes only what is drawn, has no counterpart anywhere in the physics, and is therefore free
/// to vary per cell in every world.
#[must_use]
pub fn rigidity(cells: &CellArena, i: usize, rates: &crate::metabolism::MetabolicRates) -> i32 {
    let wall = (cells.slots(i)[MEMBRANE_SLOT].param as i32 * Q10_ONE) / 255;
    let load = osmotic_load(cells, i);
    let threshold = rates.osmotic_threshold.max(1) as i64;
    let turgor = ((load * Q10_ONE as i64) / threshold).clamp(0, Q10_ONE as i64) as i32;
    crate::fixed::q10_scale(wall, turgor).clamp(0, Q10_ONE)
}

/// Free solute in a cell's cytoplasm, `Q10`: everything it holds, less what is out of solution.
///
/// The quantity turgor is charged on (`MetabolicRates::osmotic_upkeep`), and the reason it is a
/// *sum* rather than a maximum. [`interior_capacity`] bounds one chemical at a time, and
/// [`CellHost::headroom`] checks one at a time against it, so a cell holding a legal amount of
/// all sixteen is holding sixteen capacities and nothing anywhere says otherwise. Osmotic
/// pressure does not care which species the particles are, only how many there are.
///
/// **A vacuole takes matter out of solution rather than making room for more of it.** This is
/// the real difference between glucose and glycogen: a thousand units of sugar dissolved is a
/// thousand particles pulling water in, and the same thousand polymerised into one chain is
/// one. Storage is not dangerous because it takes up space, it is dangerous because it is
/// *free*, and a polymer is the trade a cell makes to hold matter that is not.
///
/// So a vacuole earns its upkeep here rather than through a raised cap, which is what it had
/// before and what no cell in any run has ever thought worth building.
#[must_use]
pub fn osmotic_load(cells: &CellArena, i: usize) -> i64 {
    let solute: i64 = cells.interior(i).iter().map(|&v| v as i64).sum();
    let silent: i64 = cells
        .slots(i)
        .iter()
        .filter(|o| o.kind == OrganelleType::Vacuole && o.is_active())
        .map(|o| q10(o.param as i32) as i64)
        .sum();
    (solute - silent).max(0)
}

/// How many genome bytes a cell's nuclei can hold.
#[must_use]
pub fn nucleus_capacity(cells: &CellArena, i: usize) -> usize {
    let mut total = 0usize;
    for o in cells.slots(i) {
        if o.kind == OrganelleType::Nucleus && o.is_active() {
            total = total
                .saturating_add((o.param as usize).saturating_mul(GENOME_BYTES_PER_NUCLEUS_PARAM));
        }
    }
    total
}

impl Host for CellHost<'_> {
    fn build(&mut self, param: i16, ty: i16, slot: i16) {
        self.intents.push(Intent::Build {
            slot: slot_index(slot) as u8,
            kind: (ty as u16 % SLOT_COUNT as u16) as u8,
            param: (param as u16 & 0xFF) as u8,
        });
    }

    fn tear(&mut self, slot: i16) {
        self.intents.push(Intent::Tear {
            slot: slot_index(slot) as u8,
        });
    }

    fn oset(&mut self, v: i16, idx: i16, slot: i16) {
        self.intents.push(Intent::Control {
            slot: slot_index(slot) as u8,
            index: (idx as u16 % 2) as u8,
            value: v,
        });
    }

    fn oget(&mut self, idx: i16, slot: i16) -> i16 {
        let s = slot_index(slot);
        if s == MEMBRANE_SLOT {
            // The membrane is the self-sensor (SPEC §5.1).
            return self.membrane_reading(idx);
        }
        let o = self.cells.slots(self.slot)[s];
        if !o.is_present() {
            return 0;
        }
        match o.kind {
            OrganelleType::Nucleus => match (idx as u16) % 2 {
                0 => sat_i16(nucleus_capacity(self.cells, self.slot).min(i16::MAX as usize) as i32),
                _ => sat_i16(self.cells.genome[self.slot].len().min(i16::MAX as usize) as i32),
            },
            OrganelleType::Chloroplast => match (idx as u16) % 2 {
                // Rate and the light actually falling on it: the two things a genome needs to
                // decide whether photosynthesis is worth its upkeep.
                0 => sat_i16(o.param as i32),
                _ => q10_to_visible(
                    self.substrate
                        .light()
                        .get(self.square)
                        .copied()
                        .unwrap_or(0)
                        * 64,
                ),
            },
            OrganelleType::Mitochondrion => match (idx as u16) % 2 {
                0 => sat_i16(o.param as i32),
                // Substrate available, so a cell can tell starvation from idleness — and
                // specifically *its* substrate, the one this mitochondrion is set to burn.
                //
                // This read chemical 8 outright until M10.3, which was already a lie in any
                // scenario posing a different metabolic loop and became a much louder one when
                // a mitochondrion could be set to burn lipid and still be told about sugar.
                _ => {
                    let c = self.chemistry.pathway(o.control[1]).substrate % CHEM_COUNT;
                    q10_to_visible(self.cells.interior(self.slot)[c])
                }
            },
            OrganelleType::Vacuole => match (idx as u16) % 2 {
                0 => q10_to_visible(interior_capacity(self.cells, self.slot)),
                _ => q10_to_visible(self.cells.interior(self.slot).iter().copied().sum::<i32>()),
            },

            // The two M8 organelles. SPEC §6.2 gives both a reading and without them they are
            // write-only: a genome could extend a spike but not tell whether it was hitting
            // anything, and could open a lysosome but not tell whether there was anything to
            // digest. A predator with no feedback cannot retract when the prey run out, which
            // is half of what a predator-prey oscillation is made of.
            //
            // Derived on the spot rather than stored from last tick. That keeps them out of
            // world state — nothing to serialise, nothing to round-trip (hard rule 7) — and
            // reads better anyway: "what my spike is about to do to what is in front of me
            // now" is more use to a genome than a report on the tick before.
            OrganelleType::Spike => match (idx as u16) % 2 {
                0 => sat_i16(o.param as i32),
                _ => {
                    // What is within reach: damage per tick times the number of cells this
                    // spike is touching. Zero means nothing is in front of it.
                    let extension = crate::ecology::spike_extension(self.cells, self.slot);
                    if extension <= 0 {
                        return 0;
                    }
                    let reach = crate::junction::reach(self.cells, self.slot);
                    let sx = pos_to_square(self.cells.x[self.slot]);
                    let sy = pos_to_square(self.cells.y[self.slot]);
                    let touching = self
                        .neighbours
                        .around(sx, sy)
                        .filter(|j| *j != self.slot)
                        .filter(|j| self.cells.occupied(*j))
                        .filter(|j| crate::junction::distance(self.cells, self.slot, *j) <= reach)
                        .count();
                    let per = q10_scale(self.spike_damage, extension);
                    q10_to_visible(per.saturating_mul(touching.min(i32::MAX as usize) as i32))
                }
            },

            OrganelleType::Lysosome => match (idx as u16) % 2 {
                0 => sat_i16(o.param as i32),
                // Carrion under the cell, capped by what this cell could actually digest —
                // which is the rate it is about to achieve, not the size of the pile.
                _ => {
                    let capacity = crate::ecology::digestive_capacity(self.cells, self.slot);
                    let sx = pos_to_square(self.cells.x[self.slot]);
                    let sy = pos_to_square(self.cells.y[self.slot]);
                    let available = self.substrate.chem_at(crate::ecology::CARRION, sx, sy);
                    q10_to_visible(capacity.min(available).max(0))
                }
            },
            _ => {
                // Sensors and cilia (M3). Read from the world around the cell, which nobody
                // is writing during execute.
                let sx = pos_to_square(self.cells.x[self.slot]);
                let sy = pos_to_square(self.cells.y[self.slot]);
                // Only a touch sensor reads this, and it used to be built for all of them —
                // so a chemosensor walked its cell's whole neighbourhood and then went and
                // read a chemical. Asked for where it is used instead, and answered from the
                // table the sense phase gathered.
                let touch = if o.kind == OrganelleType::TouchSensor {
                    self.neighbours.touch(self.cells, self.slot)
                } else {
                    crate::sensing::TouchReading::default()
                };
                // Same argument as `touch`: only a photosensor reads it, so only a photosensor
                // pays for the scan. And only when it is asked for something past the ambient
                // light readings, which is what the index says.
                let glow = if o.kind == OrganelleType::Photosensor && (idx as u16) % 9 >= 3 {
                    let range = self.em_range;
                    [
                        crate::neighbours::glow_reading(
                            self.cells,
                            self.neighbours,
                            self.slot,
                            range,
                            0,
                        ),
                        crate::neighbours::glow_reading(
                            self.cells,
                            self.neighbours,
                            self.slot,
                            range,
                            1,
                        ),
                    ]
                } else {
                    Default::default()
                };
                crate::sensing::read_sensor(
                    &o,
                    idx,
                    crate::sensing::SensorContext {
                        substrate: self.substrate,
                        x: sx,
                        y: sy,
                        tick: self.tick,
                        cell_key: self.cells.id_at(self.slot).ordering_key(),
                        touch,
                        glow,
                    },
                )
                // Built, paid for, and not yet implemented. A `RESERVED` organelle reads as
                // nothing rather than as an error, because there is no error state.
                .unwrap_or(0)
            }
        }
    }

    fn otype(&mut self, slot: i16) -> i16 {
        self.cells.slots(self.slot)[slot_index(slot)].kind.number()
    }

    fn eat(&mut self, amount: i16, chem: i16) -> i16 {
        let c = crate::chem::chem_index(chem);
        if amount <= 0 {
            return 0;
        }
        let want = cell_to_q10(amount);
        // What the square held at the start of the tick, less what this cell has already
        // promised itself. Another cell with a lower id may still get there first, in which
        // case resolve delivers less than this — see `intent`'s module docs.
        let in_square = self
            .substrate
            .chem_plane(c)
            .get(self.square)
            .copied()
            .unwrap_or(0);
        let available = in_square.saturating_sub(self.claimed[c]).max(0);
        let promised = want.min(available).min(self.headroom(c));
        if promised <= 0 {
            return 0;
        }
        self.claimed[c] = self.claimed[c].saturating_add(promised);
        self.intents.push(Intent::Eat {
            chem: c as u8,
            promised,
        });
        q10_to_visible(promised)
    }

    fn emit(&mut self, amount: i16, chem: i16) -> i16 {
        let c = crate::chem::chem_index(chem);
        if amount <= 0 {
            return 0;
        }
        let held = self.cells.interior(self.slot)[c];
        let sending = cell_to_q10(amount).min(held);
        if sending <= 0 {
            return 0;
        }
        self.intents.push(Intent::Emit {
            chem: c as u8,
            amount: sending,
        });
        q10_to_visible(sending)
    }

    fn bud(&mut self, size: i16) -> i16 {
        if size <= 0 {
            return 0;
        }
        let capacity = nucleus_capacity(self.cells, self.slot);
        let want = size as usize;
        if capacity == 0 || want > capacity {
            // No nucleus, or more than it can hold. A cell whose genome outgrows its nucleus
            // is truncated at division (SPEC §4.1) rather than being allowed to carry it.
            return 0;
        }
        self.intents.push(Intent::Bud { size: want as u16 });
        1
    }

    fn copy_byte(&mut self, dst: u16, src: u8) {
        self.intents.push(Intent::CopyByte { dst, src });
    }

    fn split(&mut self) {
        self.intents.push(Intent::Split);
    }

    fn set_key(&mut self, key: u8) {
        self.intents.push(Intent::SetKey { key });
    }

    fn set_badge(&mut self, badge: u16) {
        self.intents.push(Intent::SetBadge { badge });
    }

    // --- junctions (SPEC §8) ---
    //
    // Every one of these records an intent and returns immediately. A genome learns whether a
    // `JOIN` worked on the *next* tick, by reading its junction slots — not from the return
    // value, which is a promise the execute phase is in no position to make. Execute reads a
    // world nobody is writing (SPEC §12), and a `JOIN` that returned real success would have
    // to have already taken the target's slot, which is exactly the shared write the phase
    // separation exists to prevent.

    fn join(&mut self, key: i16, kind: i16, handle: i16) -> i16 {
        self.intents.push(Intent::Join {
            key: (key as u16 & 0x7F) as u8,
            kind: (kind as u16 & 1) as u8,
            handle,
        });
        // Optimistic. The genome finds out by looking, which is also the only way it could
        // find out about a junction somebody else formed with *it*.
        1
    }

    fn leave(&mut self, jidx: i16) {
        self.intents.push(Intent::Leave {
            jidx: crate::junction::junction_index(jidx) as u8,
        });
    }

    fn jxfer(&mut self, amount: i16, what: i16, jidx: i16) -> i16 {
        if amount <= 0 {
            return 0;
        }
        let promised = cell_to_q10(amount);
        self.intents.push(Intent::Transfer {
            jidx: crate::junction::junction_index(jidx) as u8,
            what: (what as u16 & 0xFF) as u8,
            amount: promised,
        });
        q10_to_visible(promised)
    }

    fn jlen(&mut self, v: i16, jidx: i16) {
        self.intents.push(Intent::SetRest {
            jidx: crate::junction::junction_index(jidx) as u8,
            value: v,
        });
    }

    fn inject(&mut self, jidx: i16, dst: u16, src: u8) -> i16 {
        self.intents.push(Intent::Inject {
            // `INJECT_SELF` is the reserved index for this cell's own nucleus, and it must
            // survive the wrap that every other junction operand goes through — otherwise
            // "write to myself" would alias onto a real junction slot.
            jidx: if jidx == crate::host::INJECT_SELF {
                u8::MAX
            } else {
                crate::junction::junction_index(jidx) as u8
            },
            dst,
            src,
        });
        1
    }
}

/// Everything resolve needs that is not the world.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BiologyConfig {
    pub metabolism: Metabolism,
    pub mutation: MutationRates,
    /// Structural matter a daughter needs beyond half the parent's, `Q10`.
    pub division_matter: i32,
    /// Energy a division costs outright, `Q10`.
    pub division_energy: i32,
    /// The largest a cell's body may get, `Q10`. Zero switches the ceiling off.
    ///
    /// Not a tidiness rule. `radius` goes as the square root of mass, and the neighbour index
    /// sizes its search from the largest cell on the slide and applies it to every cell — so a
    /// single giant multiplies the cost of the collision phase for the whole population, and
    /// that phase is the tick. At `q10(400)` a cell can reach a radius of 2.75 squares, which is
    /// comfortably above the 1.50 that a populated slide put at the ninety-ninth percentile and
    /// well under the 7.00 one cell had reached.
    pub max_mass: i32,
    /// How wedged a cell may be and still bud, `Q10`, against the per-cell pressure
    /// [`crate::neighbours::resolve_collisions`] records: one unit per neighbour that has
    /// bottomed out on its core, nothing for a neighbour merely resting against it.
    ///
    /// A daughter has to go somewhere. Division used to ask only whether the parent could
    /// afford it — energy and mass — and never whether there was anywhere to put the result, so
    /// a slide that had run out of room went on accepting cells anyway: matter was the only
    /// ceiling, and a fixed budget of it accommodates any number of cells if they each get
    /// smaller. That is what a field of shards is.
    ///
    /// Deliberately *not* enclosure. A cell ringed by six neighbours that are all resting
    /// lightly is surrounded and perfectly able to divide — its whole neighbourhood has
    /// somewhere to spread, and the daughter's arrival pushes it there. What has to be refused
    /// is a cell pressed into a space too small, which is a statement about how hard its
    /// neighbours are pushing back rather than about how many of them there are.
    ///
    /// Zero switches the check off, which is what a scenario studying unbounded growth wants.
    pub split_pressure: i32,
    /// Which chemical structural mass is made of. Must match the metabolism's.
    pub structural_chemical: usize,
    /// Energy per genome byte copied at full fidelity, `Q10`. Accuracy is not free.
    pub copy_energy_per_byte: i32,
    /// What junctions cost and how they behave (SPEC §8).
    pub junctions: crate::junction::JunctionConfig,
    /// What predation and digestion cost and yield (M8).
    pub ecology: crate::ecology::EcologyConfig,
}

/// Every parameter, folded into the world's rolling state hash.
///
/// Two worlds that differ only in what a division costs are different worlds, and a hash that
/// could not tell them apart would let a determinism test pass across a parameter change. This
/// is written out field by field rather than derived so that adding a parameter and forgetting
/// to hash it is a visible omission here rather than an invisible one everywhere.
impl crate::state_hash::StateHash for BiologyConfig {
    fn hash_state(&self, h: &mut crate::state_hash::StateHasher) {
        let m = &self.mutation;
        h.u32(m.point);
        h.u32(m.insertion);
        h.u32(m.deletion);
        h.u32(m.duplication);
        h.u32(m.inversion);
        h.u32(m.translocation);
        h.u16(m.max_segment);
        h.u32(m.copy_error_max);

        h.i32(self.division_matter);
        h.i32(self.division_energy);
        h.i32(self.split_pressure);
        h.i32(self.max_mass);
        h.u64(self.structural_chemical as u64);
        h.i32(self.copy_energy_per_byte);

        let j = &self.junctions;
        h.i32(j.join_base_cost);
        h.i32(j.join_forced_penalty);
        h.i32(j.soft_max_range);
        h.i32(j.breaking_strain);
        h.i32(j.stiffness);
        h.u8(j.iterations);
        h.i32(j.muscle_range);
        h.u8(u8::from(j.probe_leaks_distance));
        h.i32(j.transfer_cost);

        let e = &self.ecology;
        h.i32(e.spike_damage);
        h.i32(e.spike_upkeep);
        h.i32(e.carrion_fraction);
        h.i32(e.digestion_rate);
        h.i32(e.digestion_efficiency);
        h.i32(e.crowding_damage);
        h.i32(e.crowding_reference_radius);
        h.u32(e.crowding_grace);

        let r = &self.metabolism.rates;
        h.i32(r.photosynthesis_efficiency);
        h.i32(r.respiration_efficiency);
        h.i32(r.reactive_fraction);
        h.i32(r.throughput_per_param);
        h.i32(r.latent_per_substrate);
        h.i32(r.toxicity_threshold);
        h.i32(r.growth_rate);
        h.i32(r.growth_pressure);
        h.i32(r.repair_per_tick);
        h.i32(r.metabolic_floor);
        h.i32(r.repair_energy_per_unit);
        h.i32(r.background_damage);
        h.i32(r.osmotic_threshold);
        h.i32(r.osmotic_upkeep);
        h.i32(r.energy_reserve);
        h.i32(r.energy_leak);

        let c = &self.metabolism.catalogue;
        let chem = c.metabolism;
        h.u64(chem.structural as u64);
        // Every pathway, in order. A world offering a different set of ways to make a living
        // is a different world, and one that offered them in a different order would assign
        // different reactions to the same control word.
        for p in &chem.pathways {
            h.u64(p.substrate as u64);
            h.u64(p.oxidant as u64);
            h.u64(p.waste as u64);
            h.u64(p.reactive as u64);
        }
        for spec in c.specs() {
            h.i32(spec.build_matter);
            h.i32(spec.build_matter_per_param);
            h.i32(spec.build_energy);
            h.u16(spec.build_ticks);
            h.i32(spec.upkeep);
            h.i32(spec.upkeep_per_param);
            h.i32(spec.teardown_recovery);
        }
    }
}

/// A parameter change made to a world that was already running (M10.2).
///
/// The whole configuration rather than a description of what changed. A `set field X to V`
/// encoding would need one variant per parameter — sixty of them, each a chance to forget one —
/// and replaying it would have to reproduce the mutation exactly. Storing the configuration
/// makes replay a copy, which cannot be wrong, and leaves the "what changed" question to the
/// display layer, which can answer it by comparing two of these.
///
/// A few hundred bytes each. A world with a thousand interventions in its history is a world
/// somebody has been fiddling with for hours, and it costs a quarter of a megabyte.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Intervention {
    /// The tick it took effect on.
    pub tick: u64,
    /// The configuration in force from that tick until the next intervention.
    pub biology: BiologyConfig,
}

impl Default for BiologyConfig {
    fn default() -> Self {
        BiologyConfig {
            metabolism: Metabolism::default(),
            mutation: MutationRates::default(),
            division_matter: q10(4),
            division_energy: q10(20),
            max_mass: q10(400),
            // One neighbour's worth of bottomed-out contact.
            //
            // Three was reasoned from the six contacts a settled monolayer has, and it is far
            // too permissive, because the gate is per cell and the overshoot is collective. A
            // cell at the edge of a colony reads low pressure and is right to — but its daughter
            // goes inward, and the colony as a whole sails past what the slide can hold. Grown
            // from one founder on a sixteen-square slide, the cells' combined area reached 243%
            // of the slide, which is not a packing problem the solver can fix: no arrangement of
            // those cells has them 95% apart.
            //
            // Swept, with the packing bench as the control:
            //
            // ```text
            //   threshold   pop   area   deep overlaps   worst pair
            //         3.0   145   243%          23.4%         6.1%
            //         2.0   104   162%          14.1%         7.8%
            //         1.5    79   128%           0.6%        75.2%
            //         1.0    79   125%           0.0%        83.1%
            //         0.6    70   121%           0.0%        87.1%
            // ```
            //
            // Deep overlaps collapse between 2.0 and 1.5 and are gone by 1.0, at no cost in
            // population against 1.5. It costs about 45% of the cells against 3.0, which is the
            // honest price of them not being inside each other.
            split_pressure: q10(1),
            structural_chemical: 4,
            copy_energy_per_byte: Q10_ONE / 64,
            junctions: crate::junction::JunctionConfig::default(),
            ecology: crate::ecology::EcologyConfig::default(),
        }
    }
}

/// Apply one tick of intents, in slot order.
///
/// Sequential by design: this is the phase where contested resources are settled, and the
/// order in which they are settled is part of the simulation's definition rather than an
/// implementation detail.
#[allow(clippy::too_many_arguments)]
pub fn resolve(
    cells: &mut CellArena,
    substrate: &mut Substrate,
    pool: &GenomePool,
    intents: &IntentBuffer,
    config: &BiologyConfig,
    chem: &ChemTable,
    ledger: &mut Ledger,
    pending: &mut Pending,
    pressure: &[i32],
    tick: u64,
    seed: u64,
) -> BiologyReport {
    let mut report = BiologyReport::default();

    for i in 0..cells.capacity() {
        if !cells.occupied(i) {
            continue;
        }
        let id = cells.id_at(i);
        let ctx = RandCtx::new(seed, tick, id.ordering_key());
        let sx = pos_to_square(cells.x[i]);
        let sy = pos_to_square(cells.y[i]);

        for intent in intents.for_slot(i) {
            match *intent {
                Intent::Eat { chem: c, promised } => {
                    let c = c as usize % CHEM_COUNT;
                    let headroom = interior_capacity(cells, i)
                        .saturating_sub(cells.interior(i)[c])
                        .max(0);
                    let want = promised.min(headroom);
                    if want <= 0 {
                        continue;
                    }
                    // Take what is actually there now, which may be less than the cell was
                    // told if somebody with a lower id has already eaten.
                    let moved = -substrate.add_chem(c, sx, sy, -want);
                    if moved > 0 {
                        cells.interior_mut(i)[c] = cells.interior(i)[c].saturating_add(moved);
                        report.eaten += moved as i64;
                    }
                }

                Intent::Emit { chem: c, amount } => {
                    let c = c as usize % CHEM_COUNT;
                    let held = cells.interior(i)[c];
                    let sending = amount.min(held);
                    if sending <= 0 {
                        continue;
                    }
                    let moved = substrate.add_chem(c, sx, sy, sending);
                    if moved > 0 {
                        cells.interior_mut(i)[c] = held.saturating_sub(moved);
                        report.emitted += moved as i64;
                    }
                }

                Intent::Control { slot, index, value } => {
                    let s = slot as usize % SLOT_COUNT;
                    let o = &mut cells.slots_mut(i)[s];
                    if o.is_present() {
                        o.control[index as usize % 2] = value;
                    }
                }

                Intent::Build { slot, kind, param } => {
                    let s = slot as usize % SLOT_COUNT;
                    // Slot 0 is always the membrane and cannot be *retyped* — a cell without a
                    // boundary is not a cell — so the type operand is ignored there and the slot
                    // stays a membrane whatever a genome asks for.
                    //
                    // It used to refuse the whole instruction, and that was over-broad against
                    // its own rule. Changing a membrane's `param` is neither tearing it down nor
                    // retyping it, and refusing it had a consequence nobody had noticed: a
                    // daughter is born with `cells.slots(parent)[0].param` and **nothing anywhere
                    // could ever change that number**. Mutation does not reach it, because it is
                    // not a genome byte. So the size of the one organelle every cell has was
                    // fixed by whatever `CellSeed` founded the lineage and was inherited
                    // unchanged forever — the single trait in the whole design that evolution
                    // could not act on, in a project whose premise is that what a cell can do is
                    // bounded by what it has built and paid for.
                    //
                    // Charged exactly like any other `BUILD`: `matter_cost(param)` out of the
                    // cytoplasm, `build_energy` off the top. The membrane's `build_ticks` is
                    // zero, which is what makes this safe — a rebuilt membrane is never inert,
                    // so there is no tick on which the cell is a cell without a boundary.
                    //
                    // `TEAR` on slot 0 is still refused, a few arms below. That is the half of
                    // the rule that was always about the rule.
                    let kind = if s == MEMBRANE_SLOT {
                        OrganelleType::Membrane
                    } else {
                        OrganelleType::from_operand(kind as i16)
                    };
                    // Building what is already there is a no-op, not a demolition.
                    //
                    // A genome that re-asserts its body every tick is the obvious way to
                    // write one, and under the other reading it would knock its own
                    // organelles back into scaffolding every tick and never finish any of
                    // them — while paying for each attempt. That is a trap with no upside:
                    // the cell asked for a chloroplast in slot 3 and there is a chloroplast
                    // in slot 3.
                    let existing = cells.slots(i)[s];
                    if existing.kind == kind
                        && existing.param == param
                        && existing.remaining_build == 0
                    {
                        continue;
                    }
                    let spec = *config.metabolism.catalogue.spec(kind);
                    let matter = spec.matter_cost(param);
                    let sc = config.structural_chemical % CHEM_COUNT;
                    if cells.interior(i)[sc] < matter || cells.energy[i] < spec.build_energy {
                        continue;
                    }
                    // And a ceiling on how big a body can get. Growth from the cytoplasm is
                    // already bounded — `metabolism` grows towards a target set by the membrane
                    // parameter — but building organelles adds structural mass with nothing
                    // stopping it, and that was the path a cell took to becoming a giant.
                    //
                    // The reason to care is not that giants look odd. The neighbour search is
                    // sized from the largest cell on the slide, so one of them widens the walk
                    // for the entire population, and the walk is the phase that *is* the tick.
                    // Measured on a populated slide: median radius 1.25 squares, p99 1.50, and
                    // one cell at 7.00 — which put the search at 961 grid squares per cell where
                    // 81 would have done.
                    //
                    // The matter simply stays in the interior, where it is still the cell's and
                    // still counted. Refusing to build is not destroying anything (I4).
                    if config.max_mass > 0 && cells.mass[i].saturating_add(matter) > config.max_mass
                    {
                        continue;
                    }
                    // Structural matter moves from the interior into the cell's mass. It has
                    // not left the world, only the pool the fluid can reach.
                    cells.interior_mut(i)[sc] = cells.interior(i)[sc].saturating_sub(matter);
                    cells.mass[i] = cells.mass[i].saturating_add(matter);
                    cells.energy[i] = cells.energy[i].saturating_sub(spec.build_energy);
                    report.dissipate_build(ledger, spec.build_energy);
                    cells.slots_mut(i)[s] = Organelle::building(kind, param, spec.build_ticks);
                    report.built = report.built.saturating_add(1);
                }

                Intent::Tear { slot } => {
                    let s = slot as usize % SLOT_COUNT;
                    if s == MEMBRANE_SLOT {
                        continue;
                    }
                    let o = cells.slots(i)[s];
                    if !o.is_present() {
                        continue;
                    }
                    let spec = *config.metabolism.catalogue.spec(o.kind);
                    // What it nominally cost, bounded by what the body actually has.
                    //
                    // Division halves a cell's mass but leaves its organelles where they are,
                    // so a cell that has divided since it built something has less body than
                    // that thing nominally cost. Giving back the nominal figure would create
                    // matter — a slow leak that only shows up once a population is large
                    // enough for teardowns to be common, which is exactly the kind of I4
                    // violation that is invisible in a small test.
                    let matter = spec.matter_cost(o.param).min(cells.mass[i]).max(0);
                    let recovered = q10_scale(matter, spec.teardown_recovery).min(matter);
                    let sc = config.structural_chemical % CHEM_COUNT;
                    // Everything comes off the mass; what is not recovered into the interior
                    // goes back to the fluid as waste. Nothing evaporates.
                    cells.mass[i] = cells.mass[i].saturating_sub(matter);
                    cells.interior_mut(i)[sc] = cells.interior(i)[sc].saturating_add(recovered);
                    let lost = matter.saturating_sub(recovered);
                    if lost > 0 {
                        let placed = substrate.add_chem(sc, sx, sy, lost);
                        // If the square is full or blocked, the remainder stays in the cell
                        // rather than vanishing.
                        let stuck = lost.saturating_sub(placed);
                        if stuck > 0 {
                            cells.interior_mut(i)[sc] = cells.interior(i)[sc].saturating_add(stuck);
                        }
                    }
                    cells.slots_mut(i)[s] = Organelle::empty();
                    report.torn = report.torn.saturating_add(1);
                }

                Intent::Bud { size } => {
                    cells.daughter[i] = Some(vec![0u8; size as usize]);
                    cells.vm[i].pb = 0;
                }

                Intent::CopyByte { dst, src } => {
                    // The copy error rate is set by the nucleus's fidelity control, and
                    // fidelity costs energy per byte. That is what makes the mutation rate an
                    // evolvable, physically costly trait rather than a constant.
                    //
                    // No nucleus, no copy. SPEC §4.1: a genome is "physically resident in the
                    // cell's nucleus organelles", so there is nowhere for a copy to go.
                    let Some(fidelity) = nucleus_fidelity(cells, i) else {
                        continue;
                    };
                    let rate = copy_error_rate(&config.mutation, fidelity);
                    let cost = q10_scale(config.copy_energy_per_byte, fidelity);
                    if cells.energy[i] < cost {
                        continue;
                    }
                    cells.energy[i] = cells.energy[i].saturating_sub(cost);
                    report.dissipate_build(ledger, cost);
                    let byte = copy_error(&ctx, rate, dst, src).unwrap_or(src);
                    if let Some(buffer) = cells.daughter[i].as_mut() {
                        if let Some(slot) = buffer.get_mut(dst as usize) {
                            *slot = byte;
                        }
                    }
                }

                Intent::Split => {
                    let squeezed = pressure.get(i).copied().unwrap_or(0);
                    if try_split(
                        cells,
                        config,
                        ledger,
                        pending,
                        &ctx,
                        i,
                        squeezed,
                        &mut report,
                    ) {
                        report.births = report.births.saturating_add(1);
                    } else {
                        report.failed_splits = report.failed_splits.saturating_add(1);
                    }
                }

                Intent::SetKey { key } => {
                    cells.key[i] = key & 0x7F;
                }

                Intent::SetBadge { badge } => {
                    cells.badge[i] = badge & 0x7FFF;
                }

                // --- junctions (SPEC §8) ---
                Intent::Join { key, kind, handle } => {
                    let cost = resolve_join(cells, config, i, key, kind, handle, &mut report);
                    if cost > 0 {
                        report.dissipate_build(ledger, cost);
                    }
                }

                Intent::Leave { jidx } => {
                    if dissolve(cells, i, jidx as usize) {
                        report.junctions_broken = report.junctions_broken.saturating_add(1);
                    }
                }

                Intent::Transfer { jidx, what, amount } => {
                    let moved = resolve_transfer(cells, config, i, jidx as usize, what, amount);
                    if moved > 0 {
                        report.transferred = report.transferred.saturating_add(moved as i64);
                        // Moving something across a membrane is work, whichever direction it
                        // goes. Charged to the sender, who asked for it.
                        let cost = crate::fixed::q10_scale(config.junctions.transfer_cost, moved);
                        let paid = cells.energy[i].min(cost);
                        cells.energy[i] = cells.energy[i].saturating_sub(paid);
                        report.dissipate_build(ledger, paid);
                    }
                }

                Intent::SetRest { jidx, value } => {
                    // Muscle. `JLEN` offsets the rest length within `±muscle_range` of the
                    // natural length, so a genome can contract and extend but cannot simply
                    // declare that two cells are forty squares apart.
                    let slot = jidx as usize % crate::junction::JUNCTIONS_PER_CELL;
                    let junction = cells.junctions(i)[slot];
                    if junction.kind != crate::junction::JunctionKind::Hard {
                        continue;
                    }
                    let Some(j) = cells.index(junction.other) else {
                        continue;
                    };
                    let natural = natural_rest(cells, i, j);
                    let range = config.junctions.muscle_range;
                    let offset = (value as i32)
                        .saturating_mul(range)
                        .saturating_div(i16::MAX as i32 / 2 + 1)
                        .clamp(-range, range);
                    let rest = natural.saturating_add(offset).max(POS_ONE / 4);
                    cells.junctions_mut(i)[slot].rest = rest;
                    // Both ends carry the same rest length, or the solver would be trying to
                    // satisfy two different constraints between the same pair.
                    if let Some(other_slot) = crate::junction::existing(cells, j, id) {
                        cells.junctions_mut(j)[other_slot].rest = rest;
                    }
                }

                Intent::Inject { jidx, dst, src } => {
                    resolve_inject(cells, pool, config, ledger, i, jidx, dst, src, &mut report);
                }
            }
        }
    }

    let _ = chem;
    report
}

impl BiologyReport {
    /// Energy spent doing work becomes heat.
    fn dissipate_build(&mut self, ledger: &mut Ledger, amount: i32) {
        let _ = self;
        ledger.dissipate(amount as i64);
    }
}

/// The nucleus copy-fidelity control input, `Q10`, or zero without a nucleus.
#[must_use]
/// `None` for a cell with no working nucleus — which is **not** the same as a nucleus turned
/// down to zero, and conflating the two was worth measuring.
///
/// It used to return `0` for both. Downstream, `q10_scale(copy_energy_per_byte, 0)` is zero,
/// so a cell with no nucleus copied its genome for nothing; and `try_split` skipped truncation
/// when capacity was zero, so it passed on a full-length genome as well. Between them, the
/// organelle whose entire job is to make genome bloat cost something (SPEC §4.1) was optional,
/// and dropping it was cheaper in energy, exempt from the length cap and free of its own
/// upkeep. Between 55% and 80% of every population had found that out.
pub fn nucleus_fidelity(cells: &CellArena, i: usize) -> Option<i32> {
    for o in cells.slots(i) {
        if o.kind == OrganelleType::Nucleus && o.is_active() {
            return Some((o.control[0] as i32).clamp(0, Q10_ONE));
        }
    }
    None
}

/// The rest length a junction naturally takes: the two cells just touching.
fn natural_rest(cells: &CellArena, i: usize, j: usize) -> i32 {
    // `radius` is `Q10` and a rest length is `POS`, so this has to be converted rather than
    // added straight. See `fixed::q10_to_pos`.
    crate::fixed::q10_to_pos(radius(cells, i).saturating_add(radius(cells, j))).max(POS_ONE / 2)
}

/// Form a junction, if it can be formed and paid for. Returns the energy spent.
///
/// The whole of SPEC §8.2's binding-key mechanic lives here: a matching key is nearly free, a
/// mismatched one costs `join_forced_penalty` scaled by what the target invested in its
/// membrane, and the junction forms anyway if the aggressor can pay. Consent is economic.
fn resolve_join(
    cells: &mut CellArena,
    config: &BiologyConfig,
    i: usize,
    key: u8,
    kind: u8,
    handle: i16,
    report: &mut BiologyReport,
) -> i32 {
    use crate::junction::{self, Junction, JunctionKind};

    let reach = junction::reach(cells, i);
    let Some(j) = junction::resolve_handle(cells, i, handle, reach) else {
        report.junctions_refused = report.junctions_refused.saturating_add(1);
        return 0;
    };
    let id_i = cells.id_at(i);
    let id_j = cells.id_at(j);
    // Already joined: nothing to do, and nothing to charge. A genome that calls `JOIN` every
    // tick should not be paying for a junction it already has.
    if junction::existing(cells, i, id_j).is_some() {
        return 0;
    }
    // Both ends need a slot, because a junction is symmetric. A cell that is already holding
    // four junctions cannot be joined, which is what stops one cell becoming a hub the whole
    // slide hangs off.
    let (Some(slot_i), Some(slot_j)) =
        (junction::free_slot(cells, i), junction::free_slot(cells, j))
    else {
        report.junctions_refused = report.junctions_refused.saturating_add(1);
        return 0;
    };

    let matched = key == cells.key[j];
    let target_membrane = cells.slots(j)[MEMBRANE_SLOT].param;
    let cost = junction::join_cost(&config.junctions, matched, target_membrane);
    if cells.energy[i] < cost {
        // Could not afford it. This is the deterrent working, not an error.
        report.junctions_refused = report.junctions_refused.saturating_add(1);
        return 0;
    }
    cells.energy[i] = cells.energy[i].saturating_sub(cost);

    let junction_kind = if kind == 0 {
        JunctionKind::Soft
    } else {
        JunctionKind::Hard
    };
    let rest = natural_rest(cells, i, j);
    cells.junctions_mut(i)[slot_i] = Junction {
        kind: junction_kind,
        other: id_j,
        rest,
    };
    cells.junctions_mut(j)[slot_j] = Junction {
        kind: junction_kind,
        other: id_i,
        rest,
    };
    if matched {
        report.junctions_consensual = report.junctions_consensual.saturating_add(1);
    } else {
        report.junctions_forced = report.junctions_forced.saturating_add(1);
    }
    cost
}

/// Dissolve a junction from both ends. Returns whether there was one.
///
/// From both ends because a junction is one relationship. Leaving from one side only would
/// leave the other holding a slot pointing at a cell that no longer agrees, and the solver
/// would keep pulling on a constraint nobody is party to.
fn dissolve(cells: &mut CellArena, i: usize, slot: usize) -> bool {
    let slot = slot % crate::junction::JUNCTIONS_PER_CELL;
    let junction = cells.junctions(i)[slot];
    if !junction.is_some() {
        return false;
    }
    let id_i = cells.id_at(i);
    cells.junctions_mut(i)[slot] = crate::junction::Junction::empty();
    if let Some(j) = cells.index(junction.other) {
        if let Some(other_slot) = crate::junction::existing(cells, j, id_i) {
            cells.junctions_mut(j)[other_slot] = crate::junction::Junction::empty();
        }
    }
    true
}

/// Move a chemical or energy across a soft junction.
///
/// What crosses is decided by `what`: `0` is energy, anything else selects a chemical. Both
/// are conserved — this moves, it does not create — so neither needs a ledger entry, only for
/// both compartments to be inside the total (I4, I5).
fn resolve_transfer(
    cells: &mut CellArena,
    _config: &BiologyConfig,
    i: usize,
    slot: usize,
    what: u8,
    amount: i32,
) -> i32 {
    let slot = slot % crate::junction::JUNCTIONS_PER_CELL;
    let junction = cells.junctions(i)[slot];
    if !junction.is_some() {
        return 0;
    }
    let Some(j) = cells.index(junction.other) else {
        return 0;
    };
    if what == 0 {
        // Energy. Bounded by what the sender has; energy has no capacity limit.
        let moved = amount.min(cells.energy[i]).max(0);
        if moved <= 0 {
            return 0;
        }
        cells.energy[i] = cells.energy[i].saturating_sub(moved);
        cells.energy[j] = cells.energy[j].saturating_add(moved);
        moved
    } else {
        // A chemical. Bounded by what the sender holds *and* by what the receiver can take,
        // or the difference would vanish.
        let c = crate::chem::chem_index(what as i16);
        let held = cells.interior(i)[c];
        let room = interior_capacity(cells, j)
            .saturating_sub(cells.interior(j)[c])
            .max(0);
        let moved = amount.min(held).min(room).max(0);
        if moved <= 0 {
            return 0;
        }
        cells.interior_mut(i)[c] = cells.interior(i)[c].saturating_sub(moved);
        cells.interior_mut(j)[c] = cells.interior(j)[c].saturating_add(moved);
        moved
    }
}

/// Write one genome byte, into this cell's own nucleus or a neighbour's.
///
/// SPEC §8.3: reading and writing genome bytes is the same interface whether the target is
/// self or a neighbour, which is why **viruses are emergent rather than implemented**. Nothing
/// here knows the word. A cell that forms a soft junction and writes into what comes out of it
/// is a parasite, and the only thing the engine notices is that a byte moved.
///
/// The target keeps executing. Its instruction pointer wraps modulo genome length, so there is
/// no invalid state to reach — which is hard rule 3 doing the work that would otherwise need a
/// validity check nobody could write.
#[allow(clippy::too_many_arguments)]
fn resolve_inject(
    cells: &mut CellArena,
    pool: &GenomePool,
    config: &BiologyConfig,
    ledger: &mut Ledger,
    i: usize,
    jidx: u8,
    dst: u16,
    src: u8,
    report: &mut BiologyReport,
) {
    // Writing a genome costs the same per byte as copying one: accuracy is not free, and a
    // parasite that could rewrite a host for nothing would be strictly better than one that
    // reproduced.
    let cost = config.copy_energy_per_byte;
    if cells.energy[i] < cost {
        return;
    }

    let target = if jidx == u8::MAX {
        // The reserved index: this cell's own nucleus. Self-modifying code.
        Some(i)
    } else {
        let junction = cells.junctions(i)[jidx as usize % crate::junction::JUNCTIONS_PER_CELL];
        // A soft junction is required for a non-self target (SPEC §8.3). A hard one is
        // structure, not a channel.
        if junction.kind == crate::junction::JunctionKind::Soft {
            cells.index(junction.other)
        } else {
            None
        }
    };
    let Some(target) = target else {
        return;
    };

    cells.energy[i] = cells.energy[i].saturating_sub(cost);
    report.dissipate_build(ledger, cost);

    // The genome is interned and shared, so writing to it means making a new one. That is the
    // same thing `SPLIT` does with a daughter buffer, and it is why a cell under attack does
    // not silently rewrite every clone that happens to share its genome.
    let mut bytes = cells.genome[target].bytes().to_vec();
    if bytes.is_empty() {
        return;
    }
    let at = (dst as usize) % bytes.len();
    bytes[at] = src;
    if let Ok(genome) = pool.intern(bytes) {
        cells.genome[target] = genome;
    }
    if target != i {
        report.foreign_injections = report.foreign_injections.saturating_add(1);
    }
}

/// Attempt a division. Returns whether one happened.
///
/// The daughter takes half of everything the parent has: half its mass, half its energy, half
/// its interior chemistry. Division splits a cell, it does not duplicate one — anything else
/// would create matter (I4) and make reproduction free.
fn try_split(
    cells: &mut CellArena,
    config: &BiologyConfig,
    ledger: &mut Ledger,
    pending: &mut Pending,
    ctx: &RandCtx,
    i: usize,
    pressure: i32,
    report: &mut BiologyReport,
) -> bool {
    let Some(buffer) = cells.daughter[i].take() else {
        return false;
    };
    let _ = report;
    if buffer.is_empty() {
        return false;
    }
    if cells.energy[i] < config.division_energy {
        // Not enough to pay for it. The buffer is spent either way: a failed division has
        // still cost the copying.
        return false;
    }
    if cells.mass[i] < config.division_matter.saturating_mul(2) {
        return false;
    }
    // Nowhere to put it. See `BiologyConfig::split_pressure` — the buffer is spent either way,
    // like the other refusals here, so a cell that keeps trying to divide in a jam keeps paying
    // for the copying and gets nothing, which is the cost of not reading the room.
    if config.split_pressure > 0 && pressure > config.split_pressure {
        return false;
    }

    // Everything that can refuse the division happens *before* anything is taken from the
    // parent. Preparing the daughter's genome is one of those things, so it goes here rather
    // than after the halving.
    //
    // It used to be the other way round, and the last refusal — an empty genome — sat below
    // the point where the parent had already given up half its mass and half its cytoplasm.
    // Anything that reached it destroyed that matter. Nothing ever did, because a genome could
    // only come out empty by being truncated to a zero-capacity nucleus and the truncation
    // skipped zero capacity. Fixing that hole made this one reachable and I4 caught it inside
    // one run: `chemical 4: ledger claims 141946363, world holds 141846333`. A latent leak
    // behind an unreachable branch is still a leak.

    // Structural mutation happens here, once, on the daughter's copy.
    let mut genome = buffer;
    let _ = mutate_structural(&mut genome, &config.mutation, ctx);

    // A cell whose genome outgrew its nucleus is truncated at division (SPEC §4.1).
    //
    // Zero capacity is a capacity, not an exemption. The guard here used to read
    // `capacity > 0 &&`, which reads as caution about truncating to nothing and worked out as
    // the exact opposite: a cell with no nucleus at all was the only one that got to pass on a
    // full-length genome. Truncating to empty makes the division fail, which is what "the
    // genome lives in the nucleus" has to mean for a cell that has not got one.
    let capacity = nucleus_capacity(cells, i);
    if genome.len() > capacity {
        genome.truncate(capacity);
    }
    if genome.is_empty() {
        return false;
    }

    // Committed. From here nothing may refuse, because from here the parent is being taken
    // apart.
    cells.energy[i] = cells.energy[i].saturating_sub(config.division_energy);
    ledger.dissipate(config.division_energy as i64);

    let mass = cells.mass[i] / 2;
    cells.mass[i] = cells.mass[i].saturating_sub(mass);
    let energy = cells.energy[i] / 2;
    cells.energy[i] = cells.energy[i].saturating_sub(energy);

    // A fixed array rather than a `Vec`: this runs once per division, and at fifty thousand
    // cells divisions are frequent enough that a heap allocation for sixteen integers is
    // worth not doing.
    let mut interior = [0i32; CHEM_COUNT];
    for (c, share) in interior.iter_mut().enumerate() {
        let half = cells.interior(i)[c] / 2;
        *share = half;
        cells.interior_mut(i)[c] = cells.interior(i)[c].saturating_sub(half);
    }

    let membrane = cells.slots(i)[MEMBRANE_SLOT].param;
    pending.births.push(PendingBirth {
        parent: cells.id_at(i),
        genome,
        mass,
        energy,
        interior,
        x: cells.x[i],
        y: cells.y[i],
        membrane,
        key: cells.key[i],
        // Born wearing her mother's colours. She has to be: the window in which a newborn is
        // vulnerable is the window before her own first expression cycle has run.
        badge: cells.badge[i],
        species: cells.species[i],
    });
    true
}

/// Add the tick's daughters to the arena.
///
/// Separate from resolve so the arena is not grown while it is being iterated, and so a cell
/// that divided and then died in the same tick is handled once.
pub fn apply_births(
    cells: &mut CellArena,
    pool: &GenomePool,
    pending: &mut Pending,
    archive: &mut crate::phylogeny::Phylogeny,
    tick: u64,
    seed: u64,
) -> u32 {
    let mut born = 0;
    for birth in pending.births.drain(..) {
        let Ok(genome) = pool.intern(birth.genome) else {
            continue;
        };
        // Daughters are placed a fraction of a square from the parent, deterministically, so
        // a clonal bloom spreads instead of stacking in one square forever.
        let ctx = RandCtx::new(seed, tick, birth.parent.ordering_key());
        let jitter_x = (ctx.draw_below(Purpose::Jitter, 1, 257) as i32) - 128;
        let jitter_y = (ctx.draw_below(Purpose::Jitter, 2, 257) as i32) - 128;

        // Speciation (SPEC §10.3). A daughter stays in its parent's species unless its
        // fingerprint has drifted past the threshold from that species' founder, in which case
        // it founds a new one parented to the old. An unmutated daughter interns to the same
        // `Arc<Genome>` as its parent, so the common case is a comparison of two equal
        // fingerprints and costs nothing.
        // Read off the *parent*, not the newborn. A daughter is born with a membrane and
        // nothing else — it builds its organelles over the following ticks from the same
        // genome — so asking the newborn what it is made of would name every species after an
        // empty cell. The parent is expressing the genome the daughter inherited, which is
        // exactly the thing being named.
        let traits = cells
            .index(birth.parent)
            .map(|p| crate::names::Traits::of(cells.slots(p), genome.len()))
            .unwrap_or_else(|| crate::names::Traits {
                counts: [0; crate::organelle::SLOT_COUNT],
                genome_len: genome.len().min(u16::MAX as usize) as u16,
            });
        let species = archive.on_birth(birth.species, &genome, traits, tick);
        archive.record_birth(species);

        let id = cells.spawn(CellSeed {
            x: birth.x.saturating_add(jitter_x),
            y: birth.y.saturating_add(jitter_y),
            mass: birth.mass,
            energy: birth.energy,
            membrane: birth.membrane,
            key: birth.key,
            badge: birth.badge,
            species,
            parent: birth.parent,
            birth_tick: tick,
            genome,
        });
        if let Some(j) = cells.index(id) {
            cells.interior_mut(j).copy_from_slice(&birth.interior);
        }
        born += 1;
    }
    born
}

/// What the tick's dead left behind.
///
/// `to_carrion` is what makes the food web's bottom edge a measurement rather than a guess:
/// it is the matter that actually reached a square as carrion, not what the dead were worth
/// in principle, so a death in a walled-in corner contributes what it really contributed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DeathReport {
    pub deaths: u32,
    /// Structural mass deposited as carrion, `Q10`.
    pub to_carrion: i64,
}

/// Remove the tick's dead, returning everything they held to the fluid.
///
/// A corpse is not a special object with a decay timer. The cell's interior chemistry and its
/// structural mass go back into the square it died on, and if that square is full or blocked
/// they go to the nearest one that is not. Nothing is destroyed, so nothing can leak.
pub fn apply_deaths(
    cells: &mut CellArena,
    substrate: &mut Substrate,
    config: &BiologyConfig,
    ledger: &mut Ledger,
    pending: &mut Pending,
) -> DeathReport {
    let mut died = DeathReport::default();
    // Sorted so that two cells dying on the same square deposit in a fixed order, whatever
    // order they were detected in.
    pending.deaths.sort_unstable_by_key(|id| id.ordering_key());
    pending.deaths.dedup();

    for id in pending.deaths.drain(..) {
        let Some(i) = cells.index(id) else {
            continue;
        };
        let sx = pos_to_square(cells.x[i]);
        let sy = pos_to_square(cells.y[i]);

        // Structural mass becomes a corpse, mostly.
        //
        // SPEC §7.2: a dead cell leaves "a localised deposit that lysosomes can digest and
        // that decays into the fluid over time". Carrion is a chemical rather than an object,
        // so it diffuses slowly, decays on its own and is conserved exactly, all through
        // machinery that already existed — see `ecology`.
        //
        // Not all of the body: a cell that starved has already spent itself, and a corpse that
        // swallowed the whole body would leave nothing to tell starvation from predation.
        // Turning body into carrion is a balanced reaction like any other, so it goes through
        // the ledger (I4).
        let mut unplaced = [0i32; CHEM_COUNT];
        let sc = config.structural_chemical % CHEM_COUNT;
        let mass = cells.mass[i];
        cells.mass[i] = 0;
        let as_carrion = q10_scale(mass, config.ecology.carrion_fraction);
        let as_chemical = mass.saturating_sub(as_carrion);
        if as_carrion > 0 {
            let placed = deposit(substrate, crate::ecology::CARRION, sx, sy, as_carrion);
            ledger.convert(sc, crate::ecology::CARRION, placed as i64);
            unplaced[crate::ecology::CARRION] = as_carrion.saturating_sub(placed);
            died.to_carrion = died.to_carrion.saturating_add(placed as i64);
        }
        unplaced[sc] = as_chemical.saturating_sub(deposit(substrate, sc, sx, sy, as_chemical));

        for (c, slot) in unplaced.iter_mut().enumerate() {
            let held = cells.interior(i)[c];
            if held > 0 {
                cells.interior_mut(i)[c] = 0;
                let placed = deposit(substrate, c, sx, sy, held);
                *slot = slot.saturating_add(held.saturating_sub(placed));
            }
        }
        // If the whole neighbourhood is full or walled in, the remainder has nowhere to go.
        // Recording it as evicted is the honest outcome: it has left the world, and I4 says
        // somebody has to say so rather than it quietly not adding up.
        if unplaced.iter().any(|v| *v > 0) {
            ledger.record_evicted(&unplaced);
        }

        // Whatever energy it still held is gone as heat: a corpse is not a battery.
        let energy = cells.energy[i];
        cells.energy[i] = 0;
        ledger.dissipate(energy as i64);

        cells.despawn(id);
        died.deaths = died.deaths.saturating_add(1);
    }
    died
}

/// Put matter into a square, spilling outward if it will not fit.
///
/// The spiral is bounded and deterministic. If nowhere within it will take the matter it stays
/// where it is rather than being discarded — I4 does not have an exception for "the square was
/// full".
fn deposit(substrate: &mut Substrate, c: usize, x: i32, y: i32, amount: i32) -> i32 {
    if amount <= 0 {
        return 0;
    }
    let mut remaining = amount;
    remaining -= substrate.add_chem(c, x, y, remaining);
    if remaining <= 0 {
        return amount;
    }
    for ring in 1..4i32 {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs() != ring && dy.abs() != ring {
                    continue;
                }
                remaining -= substrate.add_chem(c, x + dx, y + dy, remaining);
                if remaining <= 0 {
                    return amount;
                }
            }
        }
    }
    amount - remaining
}

/// Below this many slots the fan-out costs more than the work.
const PARALLEL_THRESHOLD: usize = 512;

/// Give every cell its instruction budget.
///
/// # Why this can be parallel at all
///
/// A cell's tick reads the world and writes nothing to it. Everything it wants to happen goes
/// into its own slice of the intent buffer, to be applied later in slot order by `resolve`;
/// the only thing it mutates is its own VM. So the three things that would normally make
/// this order-dependent are all absent:
///
/// * **No cell can see another cell's changes**, because there are none to see until the
///   resolve phase. `cells` is a shared reference for the whole phase.
/// * **No cell can write where another writes.** The VMs are handed out one per slot and the
///   intent lists are disjoint slices, both enforced by the borrow checker rather than by
///   convention.
/// * **No draw depends on when the cell ran.** Randomness is `hash(seed, tick, cell, purpose)`
///   (SPEC §11), so a cell gets the same numbers whichever thread picks it up and whenever it
///   does.
///
/// The result is that the state hash after a tick is the same on one thread as on sixteen,
/// which M2's determinism acceptance test checks at 500,000 ticks. The VMs come out of the
/// arena for the duration because the host needs a shared borrow of everything else.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    cells: &mut CellArena,
    substrate: &Substrate,
    neighbours: &crate::neighbours::NeighbourIndex,
    intents: &mut IntentBuffer,
    cfg: &VmConfig,
    tick: u64,
    seed: u64,
    // What a unit of spike extension deals, so a spike can report what it is about to do.
    // Passed in rather than taking the whole `BiologyConfig`, because these are the only things
    // a cell reads from it during execution.
    spike_damage: i32,
    em_range: i32,
    chemistry: crate::organelle::MetabolicChemistry,
) {
    let mut vms = std::mem::take(&mut cells.vm);
    let arena: &CellArena = cells;
    let run = |i: usize, vm: &mut Vm, slot: SlotIntents<'_>| -> u64 {
        if !arena.occupied(i) {
            return 0;
        }
        let genome: Arc<Genome> = Arc::clone(&arena.genome[i]);
        let ctx = RandCtx::new(seed, tick, arena.id_at(i).ordering_key());
        let mut host = CellHost::new(
            i,
            arena,
            substrate,
            neighbours,
            slot,
            tick,
            spike_damage,
            em_range,
            chemistry,
        );
        vm.tick(&genome, cfg, &ctx, &mut host);
        host.intents.dropped()
    };

    let dropped: u64 = if vms.len() < PARALLEL_THRESHOLD {
        vms.iter_mut()
            .zip(intents.slots_mut())
            .enumerate()
            .map(|(i, (vm, slot))| run(i, vm, slot))
            .sum()
    } else {
        // Collected first because `slots_mut` is a sequential iterator over disjoint chunks;
        // the collection is what makes it indexable, not what makes it safe.
        let slots: Vec<SlotIntents<'_>> = intents.slots_mut().collect();
        vms.par_iter_mut()
            .zip(slots.into_par_iter())
            .enumerate()
            .map(|(i, (vm, slot))| run(i, vm, slot))
            .sum()
    };

    cells.vm = vms;
    intents.add_dropped(dropped);
}

#[cfg(test)]
mod tests {
    /// The bit-by-bit square root in `radius` must agree with counting up, for every mass a
    /// cell can have. Written because the fast version is the kind of change that is right
    /// for every value anyone tries by hand and wrong at one boundary.
    #[test]
    fn the_fast_square_root_agrees_with_the_obvious_one() {
        fn counting(m: u32) -> u32 {
            let mut r = 0u32;
            while (r + 1).saturating_mul(r + 1) <= m {
                r += 1;
            }
            r
        }
        fn by_bit(m: u32) -> u32 {
            let mut r = 0u32;
            let mut bit = 1u32 << 15;
            while bit != 0 {
                let try_r = r | bit;
                if try_r.saturating_mul(try_r) <= m {
                    r = try_r;
                }
                bit >>= 1;
            }
            r
        }
        for m in 0..70_000u32 {
            assert_eq!(by_bit(m), counting(m), "disagreed at mass {m}");
        }
        // And around every perfect square up to the representable maximum, where an
        // off-by-one would hide.
        for k in 0..=0xFFFFu32 {
            let sq = k.saturating_mul(k);
            for m in [sq.saturating_sub(1), sq, sq.saturating_add(1)] {
                assert_eq!(by_bit(m), counting(m), "disagreed at mass {m} near {k}^2");
            }
        }
    }

    use super::*;
    use crate::cell::CellId;
    use crate::fixed::pos;

    struct Fixture {
        cells: CellArena,
        substrate: Substrate,
        chem: ChemTable,
        ledger: Ledger,
        intents: IntentBuffer,
        pending: Pending,
        config: BiologyConfig,
        pool: GenomePool,
    }

    impl Fixture {
        fn new() -> Fixture {
            Fixture {
                cells: CellArena::new(),
                substrate: Substrate::new(16, 16).unwrap(),
                chem: ChemTable::spec_default(),
                ledger: Ledger::new(),
                intents: IntentBuffer::new(),
                pending: Pending::default(),
                config: BiologyConfig::default(),
                pool: GenomePool::new(),
            }
        }

        fn spawn(&mut self, bytes: Vec<u8>) -> usize {
            let id = self.cells.spawn(CellSeed {
                x: pos(8),
                y: pos(8),
                mass: q10(20),
                energy: q10(500),
                membrane: 16,
                key: 3,
                badge: 0,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome: self.pool.intern(bytes).unwrap(),
            });
            self.cells.index(id).unwrap()
        }

        fn total(&self) -> [i64; CHEM_COUNT] {
            let a = self.cells.total_interior();
            let b = self.substrate.total_chem();
            let mut out: [i64; CHEM_COUNT] = std::array::from_fn(|c| a[c] + b[c]);
            // Structural mass is matter too: it is the structural chemical, held as body.
            let sc = self.config.structural_chemical % CHEM_COUNT;
            for i in self.cells.iter() {
                out[sc] += self.cells.mass[i] as i64;
            }
            out
        }

        fn resolve(&mut self) -> BiologyReport {
            resolve(
                &mut self.cells,
                &mut self.substrate,
                &self.pool,
                &self.intents,
                &self.config,
                &self.chem,
                &mut self.ledger,
                &mut self.pending,
                // No pressure: these tests are about what an intent does, not about whether
                // there is room for the result. `pressure_refuses_a_division_with_nowhere_to_go`
                // is the one that supplies some.
                &[],
                0,
                1,
            )
        }
    }

    #[test]
    fn eating_moves_matter_and_creates_none() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.substrate.set_chem(5, 8, 8, q10(40));
        let before = f.total();

        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(
            i,
            Intent::Eat {
                chem: 5,
                promised: q10(30),
            },
        );
        let report = f.resolve();

        assert_eq!(report.eaten, q10(30) as i64);
        assert_eq!(f.cells.interior(i)[5], q10(30));
        assert_eq!(f.substrate.chem_at(5, 8, 8), q10(10));
        assert_eq!(f.total(), before, "eating changed how much matter exists");
    }

    #[test]
    fn a_cell_cannot_eat_what_is_not_there() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.substrate.set_chem(5, 8, 8, q10(5));
        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(
            i,
            Intent::Eat {
                chem: 5,
                promised: q10(100),
            },
        );
        f.resolve();
        assert_eq!(f.cells.interior(i)[5], q10(5));
        assert_eq!(f.substrate.chem_at(5, 8, 8), 0);
    }

    #[test]
    fn the_lower_id_eats_first_and_the_other_goes_hungry() {
        // The contested-resource rule of SPEC §12, which is what makes a run reproducible.
        let mut f = Fixture::new();
        let first = f.spawn(vec![0x2E]);
        let second = f.spawn(vec![0x2E]);
        assert!(first < second);
        f.substrate.set_chem(5, 8, 8, q10(10));

        f.intents.begin_tick(f.cells.capacity());
        for slot in [second, first] {
            // Pushed in the wrong order on purpose: resolve must not care.
            f.intents.push(
                slot,
                Intent::Eat {
                    chem: 5,
                    promised: q10(10),
                },
            );
        }
        f.resolve();
        assert_eq!(
            f.cells.interior(first)[5],
            q10(10),
            "the lower id eats first"
        );
        assert_eq!(
            f.cells.interior(second)[5],
            0,
            "the higher id finds nothing"
        );
    }

    #[test]
    fn interior_capacity_bounds_what_a_cell_can_hold() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.substrate.set_chem(5, 8, 8, i32::MAX / 4);
        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(
            i,
            Intent::Eat {
                chem: 5,
                promised: i32::MAX / 4,
            },
        );
        f.resolve();
        assert_eq!(f.cells.interior(i)[5], BASE_INTERIOR_CAPACITY);

        // A vacuole raises the ceiling, which is the point of building one.
        f.cells.slots_mut(i)[4] = Organelle::finished(OrganelleType::Vacuole, 200);
        assert!(interior_capacity(&f.cells, i) > BASE_INTERIOR_CAPACITY);
    }

    #[test]
    fn emitting_returns_matter_to_the_fluid() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.cells.interior_mut(i)[7] = q10(20);
        let before = f.total();
        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(
            i,
            Intent::Emit {
                chem: 7,
                amount: q10(15),
            },
        );
        f.resolve();
        assert_eq!(f.cells.interior(i)[7], q10(5));
        assert_eq!(f.substrate.chem_at(7, 8, 8), q10(15));
        assert_eq!(f.total(), before);
    }

    #[test]
    fn a_cell_with_no_nucleus_cannot_copy_its_genome_or_divide() {
        // SPEC §4.1: the genome is "physically resident in the cell's nucleus organelles". A
        // cell without one has nowhere to put a copy, so it cannot reproduce.
        //
        // This was the most consequential bug in the project so far and it was invisible from
        // every direction. `nucleus_fidelity` returned 0 both for "no nucleus" and for "a
        // nucleus turned all the way down"; `q10_scale(copy_energy_per_byte, 0)` is 0, so
        // copying cost nothing; and `try_split` skipped truncation when capacity was 0, so a
        // full-length genome went to the daughter. Dropping the nucleus was therefore cheaper
        // in energy, exempt from the length cap, and free of the nucleus's own upkeep — and
        // between 55% and 80% of every population had found that out. What the metrics showed
        // was a falling mean fidelity, which reads as mutator alleles evolving and was in fact
        // a majority with no nucleus at all.
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E; 32]);
        // A body, but the nucleus slot left empty.
        f.cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 40);
        f.cells.mass[i] = q10(200);
        f.cells.energy[i] = q10(4_000);
        assert_eq!(nucleus_fidelity(&f.cells, i), None);
        assert_eq!(nucleus_capacity(&f.cells, i), 0);

        // A copy attempt does nothing, and costs nothing, because there is nowhere to copy to.
        let energy_before = f.cells.energy[i];
        f.cells.daughter[i] = Some(vec![0u8; 32]);
        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::CopyByte { dst: 0, src: 0x11 });
        f.resolve();
        assert_eq!(
            f.cells.daughter[i].as_ref().map(|d| d[0]),
            Some(0),
            "a cell with no nucleus copied a byte into a daughter"
        );
        assert_eq!(
            f.cells.energy[i], energy_before,
            "it was charged for a copy it could not make"
        );

        // And a division fails rather than producing a daughter with a full-length genome.
        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::Split);
        let report = f.resolve();
        assert_eq!(report.births, 0, "a cell with no nucleus divided");
        assert_eq!(report.failed_splits, 1);
    }

    #[test]
    fn a_refused_division_costs_the_parent_no_matter() {
        // I4 is not conditional on the division succeeding. A refusal must leave the parent
        // exactly as it found it, or every failed split quietly destroys half a cell.
        //
        // This leaked for six milestones behind an unreachable branch: the last refusal — an
        // empty genome — sat below the halving, and a genome could only come out empty by
        // being truncated to a zero-capacity nucleus, which the truncation skipped. Closing
        // that hole made this one reachable, and the M2 conservation guard found it in one
        // run at seed 2.
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E; 32]);
        // A body with no nucleus, so the daughter's genome truncates to nothing and the
        // division is refused at the last possible moment.
        f.cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 40);
        f.cells.mass[i] = q10(200);
        f.cells.energy[i] = q10(4_000);
        f.cells.interior_mut(i)[8] = q10(60);
        f.cells.daughter[i] = Some(vec![0x2E; 32]);

        let before = f.total();
        let (mass, energy, interior) = (
            f.cells.mass[i],
            f.cells.energy[i],
            f.cells.interior(i).to_vec(),
        );

        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::Split);
        let report = f.resolve();

        assert_eq!(report.births, 0, "it should have been refused");
        assert_eq!(f.total(), before, "a refused division destroyed matter");
        assert_eq!(
            f.cells.mass[i], mass,
            "the parent was halved and got nothing back"
        );
        assert_eq!(
            f.cells.energy[i], energy,
            "the parent paid for a division it did not get"
        );
        assert_eq!(f.cells.interior(i), interior.as_slice());
    }

    #[test]
    fn a_nucleus_dialled_to_zero_is_not_the_same_as_no_nucleus() {
        // The other half. A nucleus at zero fidelity is a real nucleus: it copies, cheaply and
        // badly, and it caps the genome length. That is the evolvable mutator allele SPEC §9
        // is about, and it must stay possible — the fix above must not have taken it away.
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E; 32]);
        let mut nucleus = Organelle::finished(OrganelleType::Nucleus, 40);
        nucleus.control[0] = 0;
        f.cells.slots_mut(i)[1] = nucleus;
        f.cells.mass[i] = q10(200);
        f.cells.energy[i] = q10(4_000);

        assert_eq!(
            nucleus_fidelity(&f.cells, i),
            Some(0),
            "it has a fidelity, and it is zero"
        );
        assert!(nucleus_capacity(&f.cells, i) > 0);

        f.cells.daughter[i] = Some(vec![0u8; 32]);
        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::CopyByte { dst: 0, src: 0x11 });
        f.resolve();
        assert!(
            f.cells.daughter[i].as_ref().is_some_and(|d| d[0] != 0),
            "a sloppy nucleus is still a nucleus and must be able to copy"
        );

        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::Split);
        assert_eq!(f.resolve().births, 1, "a sloppy cell must still divide");
    }

    #[test]
    fn pressure_refuses_a_division_with_nowhere_to_go() {
        // A daughter has to go somewhere. Division used to ask only whether the parent could
        // afford one, so a slide with no room left went on accepting cells and answered a fixed
        // matter budget by cutting it into more, smaller pieces.
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E; 32]);
        f.cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
        f.cells.mass[i] = q10(200);
        f.cells.energy[i] = q10(4_000);

        let threshold = f.config.split_pressure;
        let split = |f: &mut Fixture, pressure: i32| -> u32 {
            f.cells.daughter[i] = Some(vec![0u8; 32]);
            f.intents.begin_tick(f.cells.capacity());
            f.intents.push(i, Intent::Split);
            let squeeze = vec![pressure; f.cells.capacity()];
            resolve(
                &mut f.cells,
                &mut f.substrate,
                &f.pool,
                &f.intents,
                &f.config,
                &f.chem,
                &mut f.ledger,
                &mut f.pending,
                &squeeze,
                0,
                1,
            )
            .births
        };

        // Room to spare: a cell with neighbours resting against it divides normally. This is
        // the case enclosure alone would have refused, and it must not be refused — a cell
        // ringed by six neighbours that all have somewhere to spread has somewhere to bud.
        assert_eq!(split(&mut f, 0), 1, "an unpressed cell was refused");
        f.pending.births.clear();

        // Wedged: every neighbour bottomed out on its core and nothing left to give.
        assert_eq!(
            split(&mut f, threshold + 1),
            0,
            "a cell with nowhere to put a daughter divided anyway"
        );

        // Exactly at the threshold is still allowed, so the boundary is not a coin toss.
        f.pending.births.clear();
        assert_eq!(split(&mut f, threshold), 1);
    }

    #[test]
    fn a_refused_division_for_want_of_room_costs_the_parent_no_matter() {
        // The same guarantee as `a_refused_division_costs_the_parent_no_matter`, for the
        // refusal this change adds. I4 does not care why a division was declined.
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E; 32]);
        f.cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
        f.cells.mass[i] = q10(200);
        f.cells.energy[i] = q10(4_000);
        f.cells.daughter[i] = Some(vec![0u8; 32]);
        let before = f.total();
        let mass_before = f.cells.mass[i];

        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::Split);
        let squeeze = vec![f.config.split_pressure + 1; f.cells.capacity()];
        let report = resolve(
            &mut f.cells,
            &mut f.substrate,
            &f.pool,
            &f.intents,
            &f.config,
            &f.chem,
            &mut f.ledger,
            &mut f.pending,
            &squeeze,
            0,
            1,
        );
        assert_eq!(report.births, 0);
        assert_eq!(f.cells.mass[i], mass_before, "the parent was halved anyway");
        assert_eq!(f.total(), before, "a refused division moved matter");
    }

    #[test]
    fn building_moves_matter_from_the_interior_into_the_body() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        let sc = f.config.structural_chemical;
        f.cells.interior_mut(i)[sc] = q10(200);
        let before = f.total();

        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(
            i,
            Intent::Build {
                slot: 3,
                kind: OrganelleType::Chloroplast as u8,
                param: 40,
            },
        );
        let report = f.resolve();
        assert_eq!(report.built, 1);
        assert_eq!(f.cells.slots(i)[3].kind, OrganelleType::Chloroplast);
        assert!(f.cells.slots(i)[3].remaining_build > 0, "it must take time");
        assert!(f.cells.interior(i)[sc] < q10(200), "it must cost matter");
        assert_eq!(f.total(), before, "building created or destroyed matter");
    }

    #[test]
    fn a_cell_that_cannot_afford_an_organelle_does_not_get_one() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.cells.interior_mut(i)[f.config.structural_chemical] = 0;
        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(
            i,
            Intent::Build {
                slot: 3,
                kind: OrganelleType::Chloroplast as u8,
                param: 255,
            },
        );
        f.resolve();
        assert!(!f.cells.slots(i)[3].is_present());
    }

    #[test]
    fn the_membrane_cannot_be_retyped_or_torn_off() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.cells.interior_mut(i)[f.config.structural_chemical] = q10(500);
        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(
            i,
            Intent::Build {
                slot: 0,
                kind: OrganelleType::Spike as u8,
                param: 10,
            },
        );
        f.intents.push(i, Intent::Tear { slot: 0 });
        f.resolve();
        assert_eq!(
            f.cells.slots(i)[0].kind,
            OrganelleType::Membrane,
            "a cell without a boundary is not a cell"
        );
    }

    #[test]
    fn tearing_recovers_some_matter_and_loses_none() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
        let spec = *f
            .config
            .metabolism
            .catalogue
            .spec(OrganelleType::Mitochondrion);
        f.cells.mass[i] = f.cells.mass[i].saturating_add(spec.matter_cost(50));
        let before = f.total();

        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::Tear { slot: 2 });
        let report = f.resolve();
        assert_eq!(report.torn, 1);
        assert!(!f.cells.slots(i)[2].is_present());
        assert_eq!(f.total(), before, "dismantling lost matter");
    }

    #[test]
    fn tearing_after_a_division_cannot_give_back_more_body_than_there_is() {
        // Division halves a cell's mass but leaves its organelles in place, so a cell that
        // has divided since it built something has less body than that thing nominally cost.
        // Recovering the nominal figure creates matter — a leak invisible until a population
        // is large enough for teardowns to be common.
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 200);
        // Deliberately less body than the organelle nominally cost.
        let spec = *f
            .config
            .metabolism
            .catalogue
            .spec(OrganelleType::Mitochondrion);
        assert!(spec.matter_cost(200) > q10(1));
        f.cells.mass[i] = q10(1);
        let before = f.total();

        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::Tear { slot: 2 });
        f.resolve();

        assert_eq!(f.total(), before, "tearing invented matter out of nothing");
        assert!(f.cells.mass[i] >= 0);
    }

    #[test]
    fn division_splits_a_cell_rather_than_duplicating_it() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E, 0x2E, 0x2E, 0x2E]);
        f.cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
        f.cells.interior_mut(i)[9] = q10(60);
        f.cells.daughter[i] = Some(vec![0x2E, 0x04, 0x10, 0x2E]);
        f.config.mutation = MutationRates::none();

        let before = f.total();
        let parent_mass = f.cells.mass[i];
        let parent_energy = f.cells.energy[i];

        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::Split);
        let report = f.resolve();
        assert_eq!(report.births, 1);

        let born = apply_births(
            &mut f.cells,
            &f.pool,
            &mut f.pending,
            &mut crate::phylogeny::Phylogeny::new(),
            1,
            1,
        );
        assert_eq!(born, 1);
        assert_eq!(f.cells.len(), 2);

        assert!(f.cells.mass[i] < parent_mass, "the parent gave up mass");
        assert!(f.cells.energy[i] < parent_energy);
        assert_eq!(f.total(), before, "division created matter");

        let daughter = f.cells.iter().find(|j| *j != i).unwrap();
        assert_eq!(f.cells.parent[daughter], f.cells.id_at(i));
        assert_eq!(f.cells.interior(daughter)[9], q10(30), "half the chemistry");
        assert_eq!(f.cells.genome[daughter].bytes(), &[0x2E, 0x04, 0x10, 0x2E]);
    }

    #[test]
    fn a_cell_that_cannot_pay_does_not_divide() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.cells.energy[i] = 1;
        f.cells.daughter[i] = Some(vec![0x2E]);
        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::Split);
        let report = f.resolve();
        assert_eq!(report.births, 0);
        assert_eq!(report.failed_splits, 1);
    }

    #[test]
    fn a_split_without_a_bud_does_nothing() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::Split);
        assert_eq!(f.resolve().births, 0);
    }

    #[test]
    fn a_genome_that_outgrows_its_nucleus_is_truncated_at_division() {
        // SPEC §4.1: genome bloat is selected against by physics, not by a rule.
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 2);
        let capacity = nucleus_capacity(&f.cells, i);
        assert_eq!(capacity, 16);
        f.cells.daughter[i] = Some(vec![0x2E; 100]);
        f.config.mutation = MutationRates::none();

        f.intents.begin_tick(f.cells.capacity());
        f.intents.push(i, Intent::Split);
        f.resolve();
        apply_births(
            &mut f.cells,
            &f.pool,
            &mut f.pending,
            &mut crate::phylogeny::Phylogeny::new(),
            1,
            1,
        );
        let daughter = f.cells.iter().find(|j| *j != i).unwrap();
        assert_eq!(f.cells.genome[daughter].len(), capacity);
    }

    #[test]
    fn death_returns_everything_to_the_fluid() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.cells.interior_mut(i)[6] = q10(30);
        f.cells.interior_mut(i)[11] = q10(20);
        let before = f.total();

        f.pending.deaths.push(f.cells.id_at(i));
        let died = apply_deaths(
            &mut f.cells,
            &mut f.substrate,
            &f.config,
            &mut f.ledger,
            &mut f.pending,
        );
        assert_eq!(died.deaths, 1);
        assert!(died.to_carrion > 0, "the corpse left no carrion");
        assert_eq!(f.cells.len(), 0);
        // Summed across chemicals rather than compared per species. Since M8 a corpse turns
        // part of its body into carrion, which is a balanced reaction through the ledger — the
        // per-chemical split legitimately moves and only the total is invariant.
        let total = |v: &[i64; CHEM_COUNT]| -> i64 { v.iter().sum() };
        assert_eq!(
            total(&f.total()),
            total(&before),
            "a corpse must not evaporate"
        );
        // Interior chemistry goes back to the water untouched: only the *body* becomes carrion.
        assert_eq!(f.substrate.chem_at(6, 8, 8), q10(30));
        assert_eq!(f.substrate.chem_at(11, 8, 8), q10(20));
        assert!(
            f.substrate.chem_at(crate::ecology::CARRION, 8, 8) > 0,
            "the body left no corpse behind"
        );
    }

    #[test]
    fn a_corpse_on_a_full_square_spills_rather_than_vanishing() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.cells.interior_mut(i)[6] = q10(1000);
        // Fill the square the cell is standing on right to the brim.
        f.substrate
            .set_chem(6, 8, 8, crate::substrate::MAX_QUANTITY);
        let before = f.total();

        f.pending.deaths.push(f.cells.id_at(i));
        apply_deaths(
            &mut f.cells,
            &mut f.substrate,
            &f.config,
            &mut f.ledger,
            &mut f.pending,
        );
        // Anything the neighbourhood could not take is recorded as evicted rather than
        // quietly not adding up: I4 has no exception for a full square.
        let evicted: i64 = f.ledger.evicted().iter().sum();
        // Summed rather than per-species, for the same reason as the test above: since M8 part
        // of the body becomes carrion through the ledger, so the split moves and the total does
        // not. What has to hold is that everything is accounted for — what reached the water
        // plus what was evicted equals what the cell held.
        let sum = |v: &[i64; CHEM_COUNT]| -> i64 { v.iter().sum() };
        assert_eq!(
            sum(&f.total()),
            sum(&before) - evicted,
            "a corpse lost matter without saying so"
        );
        let spilled: i32 = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
            .filter(|(dx, dy)| *dx != 0 || *dy != 0)
            .map(|(dx, dy)| f.substrate.chem_at(6, 8 + dx, 8 + dy))
            .sum();
        assert!(
            spilled > 0 || evicted > 0,
            "the corpse neither spilled nor was accounted for"
        );
    }

    #[test]
    fn deaths_are_applied_in_a_fixed_order_however_they_were_detected() {
        let mut f = Fixture::new();
        let a = f.spawn(vec![0x2E]);
        let b = f.spawn(vec![0x2E]);
        f.pending.deaths.push(f.cells.id_at(b));
        f.pending.deaths.push(f.cells.id_at(a));
        f.pending.deaths.push(f.cells.id_at(b)); // and a duplicate
        let died = apply_deaths(
            &mut f.cells,
            &mut f.substrate,
            &f.config,
            &mut f.ledger,
            &mut f.pending,
        );
        assert_eq!(died.deaths, 2, "a duplicated death must be applied once");
        assert_eq!(f.cells.len(), 0);
    }

    #[test]
    fn a_running_genome_reaches_the_world_through_intents_only() {
        // EAT under execute must record a request and change nothing.
        let mut f = Fixture::new();
        // IMM 8 (amount), IMM 5 (chem), EAT, HALT
        let bytes = vec![
            0x02, 0x00, 0x00, 0x00, 0x01, 0x02, 0x01, 0x00, 0x01, 0x35, 0x2E,
        ];
        let i = f.spawn(bytes);
        f.substrate.set_chem(5, 8, 8, q10(100));
        let before = f.substrate.chem_at(5, 8, 8);

        f.intents.begin_tick(f.cells.capacity());
        let index = crate::neighbours::NeighbourIndex::default();
        execute(
            &mut f.cells,
            &f.substrate,
            &index,
            &mut f.intents,
            &VmConfig::DEFAULT,
            0,
            1,
            f.config.ecology.spike_damage,
            f.config.ecology.em_range,
            f.config.metabolism.catalogue.metabolism,
        );
        assert_eq!(
            f.substrate.chem_at(5, 8, 8),
            before,
            "execute must not move matter"
        );
        assert!(
            f.intents
                .for_slot(i)
                .iter()
                .any(|x| matches!(x, Intent::Eat { .. })),
            "the genome's EAT did not become an intent: {:?}",
            f.intents.for_slot(i)
        );
    }

    #[test]
    fn a_genome_is_told_about_itself_through_the_membrane() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.cells.energy[i] = q10(1234);
        let mut intents = IntentBuffer::new();
        intents.begin_tick(f.cells.capacity());
        let index = crate::neighbours::NeighbourIndex::default();
        let slot = intents.slots_mut().nth(i).expect("slot in range");
        let mut host = CellHost::new(
            i,
            &f.cells,
            &f.substrate,
            &index,
            slot,
            0,
            f.config.ecology.spike_damage,
            f.config.ecology.em_range,
            f.config.metabolism.catalogue.metabolism,
        );
        assert_eq!(host.oget(1, 0), 1234, "energy");
        assert_eq!(host.oget(0, 0), 20, "mass");
        assert_eq!(host.otype(0), OrganelleType::Membrane.number());
        assert_eq!(host.otype(7), OrganelleType::Empty.number());
    }

    #[test]
    fn eating_twice_in_one_tick_does_not_see_the_same_food_twice() {
        let mut f = Fixture::new();
        let i = f.spawn(vec![0x2E]);
        f.substrate.set_chem(5, 8, 8, q10(10));
        let mut intents = IntentBuffer::new();
        intents.begin_tick(f.cells.capacity());
        let index = crate::neighbours::NeighbourIndex::default();
        let slot = intents.slots_mut().nth(i).expect("slot in range");
        let mut host = CellHost::new(
            i,
            &f.cells,
            &f.substrate,
            &index,
            slot,
            0,
            f.config.ecology.spike_damage,
            f.config.ecology.em_range,
            f.config.metabolism.catalogue.metabolism,
        );
        assert_eq!(host.eat(10, 5), 10);
        assert_eq!(host.eat(10, 5), 0, "the square was already spoken for");
    }
}
