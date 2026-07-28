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

use crate::cell::{CellArena, CellSeed};
use crate::chem::{ChemTable, CHEM_COUNT};
use crate::config::VmConfig;
use crate::fixed::{cell_to_q10, pos_to_square, q10, q10_scale, sat_i16, Q10_ONE};
use crate::genome::{Genome, GenomePool};
use crate::host::Host;
use crate::intent::{Intent, IntentBuffer, Pending, PendingBirth};
use crate::ledger::Ledger;
use crate::metabolism::Metabolism;
use crate::mutation::{copy_error, copy_error_rate, mutate_structural, MutationRates};
use crate::organelle::{
    slot_index, MembraneReading, Organelle, OrganelleType, MEMBRANE_SLOT, SLOT_COUNT,
};
use crate::rng::{Purpose, RandCtx};
use crate::substrate::Substrate;

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
    intents: &'a mut IntentBuffer,
    /// The square the cell is standing on.
    square: usize,
    /// How much of each chemical the cell has already promised itself this tick, so that a
    /// genome calling `EAT` twice is not told the same food twice.
    claimed: [i32; CHEM_COUNT],
}

impl<'a> CellHost<'a> {
    #[must_use]
    pub fn new(
        slot: usize,
        cells: &'a CellArena,
        substrate: &'a Substrate,
        intents: &'a mut IntentBuffer,
    ) -> CellHost<'a> {
        let square = substrate.index(pos_to_square(cells.x[slot]), pos_to_square(cells.y[slot]));
        CellHost {
            slot,
            cells,
            substrate,
            intents,
            square,
            claimed: [0; CHEM_COUNT],
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
    // Integer square-root-ish: radius rises with mass but not linearly, so a cell twice as
    // heavy is not twice as wide. Cheap and monotonic, which is all anything needs of it.
    let m = (cells.mass[i] / Q10_ONE).max(0) as u32;
    let mut r = 0u32;
    while (r + 1) * (r + 1) <= m {
        r += 1;
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
        self.intents.push(
            self.slot,
            Intent::Build {
                slot: slot_index(slot) as u8,
                kind: (ty as u16 % SLOT_COUNT as u16) as u8,
                param: (param as u16 & 0xFF) as u8,
            },
        );
    }

    fn tear(&mut self, slot: i16) {
        self.intents.push(
            self.slot,
            Intent::Tear {
                slot: slot_index(slot) as u8,
            },
        );
    }

    fn oset(&mut self, v: i16, idx: i16, slot: i16) {
        self.intents.push(
            self.slot,
            Intent::Control {
                slot: slot_index(slot) as u8,
                index: (idx as u16 % 2) as u8,
                value: v,
            },
        );
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
            OrganelleType::Mitochondrion => {
                let m = OrganelleType::Mitochondrion;
                let _ = m;
                match (idx as u16) % 2 {
                    0 => sat_i16(o.param as i32),
                    // Substrate available, so a cell can tell starvation from idleness.
                    _ => q10_to_visible(self.cells.interior(self.slot)[8 % CHEM_COUNT]),
                }
            }
            OrganelleType::Vacuole => match (idx as u16) % 2 {
                0 => q10_to_visible(interior_capacity(self.cells, self.slot)),
                _ => q10_to_visible(self.cells.interior(self.slot).iter().copied().sum::<i32>()),
            },
            // Built, paid for, and not yet implemented. A `RESERVED` organelle reads as
            // nothing rather than as an error, because there is no error state.
            _ => 0,
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
        self.intents.push(
            self.slot,
            Intent::Eat {
                chem: c as u8,
                promised,
            },
        );
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
        self.intents.push(
            self.slot,
            Intent::Emit {
                chem: c as u8,
                amount: sending,
            },
        );
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
        self.intents
            .push(self.slot, Intent::Bud { size: want as u16 });
        1
    }

    fn copy_byte(&mut self, dst: u16, src: u8) {
        self.intents.push(self.slot, Intent::CopyByte { dst, src });
    }

    fn split(&mut self) {
        self.intents.push(self.slot, Intent::Split);
    }

    fn set_key(&mut self, key: u8) {
        self.intents.push(self.slot, Intent::SetKey { key });
    }
}

/// Everything resolve needs that is not the world.
#[derive(Clone, Debug)]
pub struct BiologyConfig {
    pub metabolism: Metabolism,
    pub mutation: MutationRates,
    /// Structural matter a daughter needs beyond half the parent's, `Q10`.
    pub division_matter: i32,
    /// Energy a division costs outright, `Q10`.
    pub division_energy: i32,
    /// Which chemical structural mass is made of.
    pub structural_chemical: usize,
    /// Energy per genome byte copied at full fidelity, `Q10`. Accuracy is not free.
    pub copy_energy_per_byte: i32,
}

impl Default for BiologyConfig {
    fn default() -> Self {
        BiologyConfig {
            metabolism: Metabolism::default(),
            mutation: MutationRates::default(),
            division_matter: q10(4),
            division_energy: q10(20),
            structural_chemical: 4,
            copy_energy_per_byte: Q10_ONE / 64,
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
    intents: &IntentBuffer,
    config: &BiologyConfig,
    chem: &ChemTable,
    ledger: &mut Ledger,
    pending: &mut Pending,
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
                    if s == MEMBRANE_SLOT {
                        // Slot 0 is always the membrane and cannot be retyped. A cell without
                        // a boundary is not a cell.
                        continue;
                    }
                    let kind = OrganelleType::from_operand(kind as i16);
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
                    let fidelity = nucleus_fidelity(cells, i);
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
                    if try_split(cells, config, ledger, pending, &ctx, i, &mut report) {
                        report.births = report.births.saturating_add(1);
                    } else {
                        report.failed_splits = report.failed_splits.saturating_add(1);
                    }
                }

                Intent::SetKey { key } => {
                    cells.key[i] = key & 0x7F;
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
pub fn nucleus_fidelity(cells: &CellArena, i: usize) -> i32 {
    for o in cells.slots(i) {
        if o.kind == OrganelleType::Nucleus && o.is_active() {
            return (o.control[0] as i32).clamp(0, Q10_ONE);
        }
    }
    0
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

    cells.energy[i] = cells.energy[i].saturating_sub(config.division_energy);
    ledger.dissipate(config.division_energy as i64);

    let mass = cells.mass[i] / 2;
    cells.mass[i] = cells.mass[i].saturating_sub(mass);
    let energy = cells.energy[i] / 2;
    cells.energy[i] = cells.energy[i].saturating_sub(energy);

    let mut interior = vec![0i32; CHEM_COUNT];
    for (c, share) in interior.iter_mut().enumerate() {
        let half = cells.interior(i)[c] / 2;
        *share = half;
        cells.interior_mut(i)[c] = cells.interior(i)[c].saturating_sub(half);
    }

    // Structural mutation happens here, once, on the daughter's copy.
    let mut genome = buffer;
    let _ = mutate_structural(&mut genome, &config.mutation, ctx);

    // A cell whose genome outgrew its nucleus is truncated at division (SPEC §4.1).
    let capacity = nucleus_capacity(cells, i);
    if capacity > 0 && genome.len() > capacity {
        genome.truncate(capacity);
    }
    if genome.is_empty() {
        return false;
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

        let id = cells.spawn(CellSeed {
            x: birth.x.saturating_add(jitter_x),
            y: birth.y.saturating_add(jitter_y),
            mass: birth.mass,
            energy: birth.energy,
            membrane: birth.membrane,
            key: birth.key,
            // Lineage marker, inherited from the parent. Real speciation — forking on
            // fingerprint distance from a founder — is M5's; until then this is what lets a
            // run tell two seeded strains apart.
            species: birth.species,
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
) -> u32 {
    let mut died = 0;
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

        // Structural mass returns as the chemical it was built from.
        let mut unplaced = [0i32; CHEM_COUNT];
        let sc = config.structural_chemical % CHEM_COUNT;
        let mass = cells.mass[i];
        cells.mass[i] = 0;
        unplaced[sc] = mass.saturating_sub(deposit(substrate, sc, sx, sy, mass));

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
        died += 1;
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

/// Give one cell its instruction budget.
///
/// Sequential over cells at M2. The VM is taken out of the arena for the duration so that the
/// host can hold a shared reference to everything else; parallelising this is M9's scale work
/// and needs the arena split differently, not the semantics changed.
pub fn execute(
    cells: &mut CellArena,
    substrate: &Substrate,
    intents: &mut IntentBuffer,
    cfg: &VmConfig,
    tick: u64,
    seed: u64,
) {
    for i in 0..cells.capacity() {
        if !cells.occupied(i) {
            continue;
        }
        let id = cells.id_at(i);
        let genome: Arc<Genome> = Arc::clone(&cells.genome[i]);
        let mut vm = std::mem::take(&mut cells.vm[i]);
        {
            let mut host = CellHost::new(i, cells, substrate, intents);
            let ctx = RandCtx::new(seed, tick, id.ordering_key());
            vm.tick(&genome, cfg, &ctx, &mut host);
        }
        cells.vm[i] = vm;
    }
}

#[cfg(test)]
mod tests {
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
                &self.intents,
                &self.config,
                &self.chem,
                &mut self.ledger,
                &mut self.pending,
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

        let born = apply_births(&mut f.cells, &f.pool, &mut f.pending, 1, 1);
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
        apply_births(&mut f.cells, &f.pool, &mut f.pending, 1, 1);
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
        assert_eq!(died, 1);
        assert_eq!(f.cells.len(), 0);
        assert_eq!(f.total(), before, "a corpse must not evaporate");
        assert_eq!(f.substrate.chem_at(6, 8, 8), q10(30));
        assert_eq!(f.substrate.chem_at(11, 8, 8), q10(20));
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
        assert_eq!(
            f.total(),
            {
                let mut expected = before;
                expected[6] -= f.ledger.evicted()[6];
                expected
            },
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
        assert_eq!(died, 2, "a duplicated death must be applied once");
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
        execute(
            &mut f.cells,
            &f.substrate,
            &mut f.intents,
            &VmConfig::DEFAULT,
            0,
            1,
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
        let mut host = CellHost::new(i, &f.cells, &f.substrate, &mut intents);
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
        let mut host = CellHost::new(i, &f.cells, &f.substrate, &mut intents);
        assert_eq!(host.eat(10, 5), 10);
        assert_eq!(host.eat(10, 5), 0, "the square was already spoken for");
    }
}
