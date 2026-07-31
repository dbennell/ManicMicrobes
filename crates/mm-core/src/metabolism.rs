//! The matter loop and the energy ledger it runs through (SPEC §7.2, §7.3).
//!
//! # Why the loop has to close
//!
//! Matter is exactly conserved, which means a world with only consumers in it runs down into
//! an all-waste equilibrium and dies — not as a balancing failure but as an arithmetic
//! certainty. Closing it needs a primary producer:
//!
//! ```text
//! mitochondrion:  substrate + oxidant  ->  waste + waste   + energy
//! chloroplast:    waste + waste + light ->  substrate + oxidant
//! ```
//!
//! Two units of matter in, two units out, in both directions. Light is the only thing
//! entering from outside, and it is the only reason the biosphere does not equilibrate. That
//! is the entropy story the whole simulation exists to display: a cell is a dissipative
//! structure, maintaining local order by consuming a gradient and exporting disorder.
//!
//! # Energy is accounted, not conserved
//!
//! I5 says `energy_in == energy_out + Δenergy_stored`, exactly, in integer units. Energy is
//! *not* conserved — it degrades — so the claim is bookkeeping rather than physics, and it
//! only means anything if "stored" is defined precisely enough to recompute independently.
//!
//! Stored energy lives in two places:
//!
//! * the energy each living cell holds, and
//! * the **latent energy** of the substrate chemical, wherever it is — inside a cell or
//!   dissolved in the fluid. A unit of sugar is energy that has not been spent yet, and
//!   pretending otherwise would make photosynthesis look like it created energy from nothing
//!   and respiration look like it destroyed it.
//!
//! So every transaction here moves energy between those two pots and the ledger, and
//! [`recompute_stored`] adds them up from the world so the claim can be checked against
//! something that is not itself.
//!
//! Both conversions are lossy, and deliberately: photosynthesis banks less than the light it
//! catches, respiration recovers less than the substrate holds, and the difference is
//! dissipated as heat. Without that there would be no dissipation rate to plot and no reason
//! for a cell to be anything other than a battery.

use crate::cell::CellArena;
use crate::chem::{ChemTable, CHEM_COUNT};
use crate::fixed::{q10, q10_scale, Q10_ONE};
use crate::ledger::Ledger;
use crate::organelle::{MetabolicChemistry, OrganelleCatalogue, OrganelleType};
use crate::substrate::Substrate;

/// How much of what it catches each conversion keeps, `Q10`.
///
/// Scenario data at M8; named constants here so the numbers are visible rather than buried in
/// the arithmetic that uses them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MetabolicRates {
    /// Fraction of absorbed light that ends up banked as substrate rather than heat.
    pub photosynthesis_efficiency: i32,
    /// Fraction of a substrate's latent energy a mitochondrion recovers.
    pub respiration_efficiency: i32,
    /// Fraction of respiration's exhaust that comes out reactive rather than inert, `Q10`.
    ///
    /// This is the cost of breathing. Matter still balances exactly — the same two units come
    /// out either way — but some of them come out as a poison, so a cell that respires is
    /// slowly damaging itself and has to excrete or repair to keep up. It is why a well-fed
    /// cell is not immortal, and therefore why a population at carrying capacity still turns
    /// over and selection still has something to act on.
    pub reactive_fraction: i32,
    /// Matter one unit of `param` can convert per tick, `Q10`.
    pub throughput_per_param: i32,
    /// Latent energy per unit of substrate chemical, `Q10` energy per `Q10` matter.
    ///
    /// Read from the chemical table's `energy_yield` in a scenario; kept here as the fallback
    /// so the loop is well-defined even for a table that says nothing.
    pub latent_per_substrate: i32,
    /// How much of a toxin a cell tolerates before it starts taking damage, `Q10`.
    ///
    /// SPEC §7.1 defines `toxicity` as "membrane damage per unit **above threshold**"; this
    /// is that threshold. Below it a cell is coping, above it the toxin is doing harm — which
    /// is what makes a toxin something to avoid or excrete rather than a flat tax on being
    /// alive.
    pub toxicity_threshold: i32,
    /// Structural matter a cell moves from its cytoplasm into its body per tick, `Q10`.
    ///
    /// This is growth, and without it a cell is stuck at whatever mass it was born with.
    /// Division halves mass, so a lineage that could not put matter back would shrink by half
    /// every generation and stop dividing after five or six — which is exactly what happened
    /// before this existed, and it looked like a carrying capacity rather than like a bug.
    pub growth_rate: i32,
    /// Damage a cell repairs per tick, `Q10`. A rate, not a fraction.
    ///
    /// Fixed capacity rather than a proportion, and the difference decides whether anything
    /// ever ages. Repairing a *fraction* of accumulated damage means damage asymptotes at
    /// `inflicted / fraction`: a cell either sits below its tolerance forever or crosses it
    /// immediately, with nothing in between and no lifespan to speak of. A fixed capacity
    /// means damage grows at `inflicted - repair`, so a cell that is poisoning itself faster
    /// than it can mend has a definite and finite life — and one that respires harder has a
    /// shorter one. That is senescence with a cause.
    ///
    /// Repair costs energy: see `repair_energy_per_unit`.
    pub repair_per_tick: i32,

    /// Energy to mend one `Q10` unit of damage.
    ///
    /// Repair used to be free, which made the whole damage mechanism a one-way filter: a cell
    /// either out-repaired its poison or it did not, and the choice cost it nothing either
    /// way. Charging for it turns maintenance into part of the metabolic budget, so a cell
    /// that is only just breaking even ages, and a cell that is doing well does not.
    pub repair_energy_per_unit: i32,

    /// Damage every cell takes per tick, whatever it is doing, `Q10`.
    ///
    /// Wear. Nothing in the engine aged a cell that did not respire: peroxide is respiration's
    /// byproduct, so a cell with no mitochondrion made no poison, took no damage and lived
    /// forever. Measured, that was between 44% and 80% of a soup — inert hulls with about one
    /// organelle, ages in the tens of thousands and energy still climbing.
    ///
    /// This is deliberately *not* an age limit. Nothing counts a cell's birthdays and nothing
    /// kills it for reaching a number. Damage accrues at a flat rate and repair costs energy,
    /// so a cell lives exactly as long as it can afford to keep mending itself — which makes
    /// lifespan a consequence of how well it earns rather than a constant somebody chose. A
    /// thriving cell pays it out of pocket and never notices; one that has stopped earning
    /// falls behind and eventually fails its own membrane.
    pub background_damage: i32,

    /// The cost of being alive at all, `Q10` energy per tick, before any organelle is paid
    /// for.
    ///
    /// Charged per *cell*, not per organelle, and that is the whole point. Organelle upkeep
    /// scales with what a cell is carrying, so a cell that sheds its organelles sheds its
    /// costs: a bare membrane at `param 24` pays 80 `Q10` a tick and nothing else, which it
    /// can meet forever. Measured, between 55% and 80% of a soup was cells with roughly one
    /// organelle, no nucleus, ages in the thousands and energy still climbing — immortal
    /// because doing nothing had no floor under it.
    ///
    /// Attaching the cost to the membrane instead would have been evadable by shrinking its
    /// `param` towards zero, which is the same hole one step along.
    ///
    /// Deliberately small: a nudge towards oblivion rather than a cull. A working cell should
    /// not notice it; a hull that has stopped doing anything should take thousands of ticks to
    /// go. It is a first value and expected to move — it is also the single biggest dial on
    /// the carrying capacity of every scenario at once, so it wants measuring rather than
    /// guessing at.
    pub metabolic_floor: i32,
}

impl Default for MetabolicRates {
    fn default() -> Self {
        MetabolicRates {
            photosynthesis_efficiency: Q10_ONE / 2,
            respiration_efficiency: Q10_ONE * 3 / 4,
            reactive_fraction: Q10_ONE / 24,
            throughput_per_param: Q10_ONE / 16,
            latent_per_substrate: 64,
            growth_rate: q10(1) / 4,
            toxicity_threshold: q10(8),
            repair_per_tick: 100,
            // Chosen by measurement, in a soup at 40,000 ticks. The first value tried was 64,
            // which cost half a unit of energy a tick against an idle cell's income of about
            // 240 — noise, and every cell mended itself completely forever.
            //
            //   cost      population   inert   working
            //   none           4,615     56%     2,021
            //   64             3,547     49%     1,804
            //   q10(8)         4,764     40%     2,822
            //   q10(24)        1,628     37%     1,020
            //
            // `q10(8)` is the only one that buys anything. It cuts the inert share from 56% to
            // 40% while leaving the carrying capacity alone and raising the number of *working*
            // cells by 40% — the tax falls on the cells that were not earning. `q10(24)` buys
            // three more points of inert share for two thirds of the population, which is a
            // cull wearing a nudge's clothes.
            repair_energy_per_unit: q10(8),
            // Slow. A membrane at `param 24` tolerates 24 units of damage, so an unrepaired
            // cell fails after roughly 24 * 1024 / 8 = 3,000 ticks — thousands, as intended,
            // and far longer for anything that can pay to keep up.
            background_damage: 8,
            // About the same again as a bare membrane's own upkeep, so merely existing costs
            // roughly twice what it did and a working cell barely feels it.
            metabolic_floor: Q10_ONE / 32,
        }
    }
}

/// Everything the metabolic step needs that is not the world itself.
#[derive(Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Metabolism {
    pub rates: MetabolicRates,
    pub catalogue: OrganelleCatalogue,
}

/// What one tick of metabolism did, for the metrics of SPEC §13.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MetabolicReport {
    /// Light energy absorbed by chloroplasts.
    pub absorbed: i64,
    /// Energy dissipated as heat: conversion losses plus upkeep.
    pub dissipated: i64,
    /// Matter photosynthesised, `Q10`.
    pub fixed: i64,
    /// Matter respired, `Q10`.
    pub burned: i64,
    /// Reactive byproduct made by respiration, `Q10`.
    pub reactive: i64,
    /// Matter that decayed from one species into another, `Q10`.
    pub decayed: i64,
    /// Structural matter moved from cytoplasm into body, `Q10`.
    pub grown: i64,
    /// Cells whose energy ran out this tick.
    pub starved: u32,
    /// Cells whose membrane failed under toxic damage this tick.
    pub poisoned: u32,
    /// Damage inflicted by toxins this tick, `Q10`.
    pub damage: i64,
}

impl Metabolism {
    /// The latent energy of a quantity of substrate chemical.
    #[inline]
    #[must_use]
    pub fn latent(&self, chem: &ChemTable, m: &MetabolicChemistry, quantity: i64) -> i64 {
        let per = self.per_unit_latent(chem, m) as i64;
        (quantity * per) / Q10_ONE as i64
    }

    #[inline]
    fn per_unit_latent(&self, chem: &ChemTable, m: &MetabolicChemistry) -> i32 {
        let from_table = chem.get(m.substrate).energy_yield;
        if from_table > 0 {
            from_table
        } else {
            self.rates.latent_per_substrate
        }
    }

    /// Run one tick of metabolism over the whole population.
    ///
    /// Cells are visited in slot order, which is id order (I6). Nothing here depends on how
    /// the work is scheduled because nothing here is scheduled — metabolism is part of
    /// resolve, and resolve is sequential by design.
    pub fn step(
        &self,
        cells: &mut CellArena,
        substrate: &Substrate,
        chem: &ChemTable,
        ledger: &mut Ledger,
        starving: &mut Vec<crate::cell::CellId>,
    ) -> MetabolicReport {
        let m = self.catalogue.metabolism;
        let latent_per_unit = self.per_unit_latent(chem, &m);
        let mut report = MetabolicReport::default();

        for i in 0..cells.capacity() {
            if !cells.occupied(i) {
                continue;
            }

            // --- construction: an organelle takes time before it works ---
            //
            // SPEC §6.2: organelles take time to construct and a partially built one is
            // inert. This is where that time passes. Without it every organelle a cell ever
            // built would stay inert forever, which looks exactly like a metabolism that
            // does not work.
            for o in cells.slots_mut(i) {
                if o.remaining_build > 0 {
                    o.remaining_build = o.remaining_build.saturating_sub(1);
                }
            }

            // --- photosynthesis: waste + light -> substrate + oxidant ---
            let light = {
                let sq = substrate.index(
                    cells.x[i] >> crate::fixed::POS_BITS,
                    cells.y[i] >> crate::fixed::POS_BITS,
                );
                substrate.light().get(sq).copied().unwrap_or(0)
            };
            if light > 0 {
                let capacity = self.conversion_capacity(cells, i, OrganelleType::Chloroplast);
                if capacity > 0 {
                    // Bounded by machinery, by the waste on hand, and by the light falling on
                    // it. Two units of waste make one of substrate and one of oxidant.
                    let waste_available = cells.interior(i)[m.waste];
                    let by_light = q10_scale(capacity, light);
                    let pairs = by_light.min(waste_available / 2).max(0);
                    if pairs > 0 {
                        let gained_latent =
                            (pairs as i64 * latent_per_unit as i64) / Q10_ONE as i64;
                        // The light it took to bank that much, plus what was lost as heat.
                        let absorbed = if self.rates.photosynthesis_efficiency > 0 {
                            (gained_latent * Q10_ONE as i64)
                                / self.rates.photosynthesis_efficiency as i64
                        } else {
                            gained_latent
                        };
                        let interior = cells.interior_mut(i);
                        interior[m.waste] = interior[m.waste].saturating_sub(pairs * 2);
                        interior[m.substrate] = interior[m.substrate].saturating_add(pairs);
                        interior[m.oxidant] = interior[m.oxidant].saturating_add(pairs);
                        // Two units of waste became one of substrate and one of oxidant.
                        // Reported rather than done silently: an unaccounted transmutation is
                        // indistinguishable from a conservation bug (I4).
                        ledger.convert(m.waste, m.substrate, pairs as i64);
                        ledger.convert(m.waste, m.oxidant, pairs as i64);

                        ledger.absorb(absorbed);
                        let waste_heat = absorbed.saturating_sub(gained_latent);
                        report.dissipated += ledger.dissipate(waste_heat);
                        report.absorbed += absorbed;
                        report.fixed += pairs as i64;
                    }
                }
            }

            // --- respiration: substrate + oxidant -> waste + energy ---
            let capacity = self.conversion_capacity(cells, i, OrganelleType::Mitochondrion);
            if capacity > 0 {
                let (sub, ox) = {
                    let interior = cells.interior(i);
                    (interior[m.substrate], interior[m.oxidant])
                };
                let burn = capacity.min(sub).min(ox).max(0);
                if burn > 0 {
                    let released = (burn as i64 * latent_per_unit as i64) / Q10_ONE as i64;
                    let recovered =
                        (released * self.rates.respiration_efficiency as i64) / Q10_ONE as i64;
                    // Two units come out for the two that went in, but not all of it is
                    // inert: a share is reactive, and that share is what ages the cell.
                    let exhaust = burn.saturating_mul(2);
                    let reactive = q10_scale(exhaust, self.rates.reactive_fraction).min(exhaust);
                    let inert = exhaust.saturating_sub(reactive);

                    let interior = cells.interior_mut(i);
                    interior[m.substrate] = interior[m.substrate].saturating_sub(burn);
                    interior[m.oxidant] = interior[m.oxidant].saturating_sub(burn);
                    interior[m.waste] = interior[m.waste].saturating_add(inert);
                    interior[m.reactive] = interior[m.reactive].saturating_add(reactive);

                    // Reported as two balanced reactions, so the per-species claim stays
                    // exact and an unaccounted transmutation still shows up as drift.
                    let from_substrate = burn.min(inert);
                    ledger.convert(m.substrate, m.waste, from_substrate as i64);
                    ledger.convert(
                        m.substrate,
                        m.reactive,
                        burn.saturating_sub(from_substrate) as i64,
                    );
                    let oxidant_inert = inert.saturating_sub(from_substrate);
                    ledger.convert(m.oxidant, m.waste, oxidant_inert as i64);
                    ledger.convert(
                        m.oxidant,
                        m.reactive,
                        burn.saturating_sub(oxidant_inert) as i64,
                    );
                    report.reactive += reactive as i64;

                    cells.energy[i] =
                        cells.energy[i].saturating_add(crate::fixed::sat_i32(recovered));
                    // The latent energy left the substrate; part became cell energy and the
                    // rest became heat. Stored is unchanged by the first and reduced by the
                    // second, which is exactly what dissipating the difference says.
                    report.dissipated += ledger.dissipate(released - recovered);
                    report.burned += burn as i64;
                }
            }

            // --- growth: cytoplasm becoming body ---
            //
            // A cell puts structural matter into itself up to what its membrane can hold, and
            // its membrane's size is what says how big it means to be. This is the other half
            // of division: a daughter is born at half its parent's mass and has to earn the
            // rest back before it can divide in turn, which is what makes structural matter
            // the thing a population is ultimately limited by.
            //
            // Not a species change — the same chemical, moved from the cytoplasm to the body —
            // so it needs no ledger entry, only for both compartments to be counted.
            {
                let membrane = cells.slots(i)[0];
                let target =
                    q10(membrane.param as i32).saturating_add((membrane.control[1] as i32).max(0));
                let room = target.saturating_sub(cells.mass[i]);
                if room > 0 {
                    let sc = m.structural % CHEM_COUNT;
                    let available = cells.interior(i)[sc];
                    let grown = self.rates.growth_rate.min(room).min(available);
                    if grown > 0 {
                        cells.interior_mut(i)[sc] = cells.interior(i)[sc].saturating_sub(grown);
                        cells.mass[i] = cells.mass[i].saturating_add(grown);
                        report.grown += grown as i64;
                    }
                }
            }

            // Note what does *not* happen here: a cell does not decompose its own peroxide.
            //
            // It decomposes in the water, and only there. Real hydrogen peroxide needs
            // catalase to break down at any speed, and catalase is a lysosome — an M8
            // organelle this cell does not have. So the only way out of a cytoplasm is to
            // excrete it, and a cell that will not is stuck with it.
            //
            // This was the other way round to begin with, and the consequence was worth
            // recording: with interior decay, retaining peroxide was an *advantage*, because
            // it decayed into carbon dioxide right where photosynthesis needed it. The strain
            // that dutifully excreted its waste lost, every time, for having given away its
            // own food supply. A believable mechanism, an emergent result, and the exact
            // opposite of the physics it was meant to model.

            // --- toxicity: membrane damage from what the cell is carrying (SPEC §7.1) ---
            //
            // A toxin does harm only above a threshold, so carrying a little is survivable
            // and carrying a lot is not. That is what makes excreting it a strategy rather
            // than a formality, and what gives a `PUMP` something to be for.
            {
                let mut inflicted = 0i32;
                for (c, held) in cells.interior(i).iter().enumerate() {
                    let toxicity = chem.get(c).toxicity;
                    if toxicity <= 0 {
                        continue;
                    }
                    let excess = held.saturating_sub(self.rates.toxicity_threshold);
                    if excess > 0 {
                        inflicted = inflicted.saturating_add(q10_scale(excess, toxicity));
                    }
                }
                // Wear, on top of whatever the cell is poisoning itself with. Everything ages
                // now, including the cells that never respire.
                inflicted = inflicted.saturating_add(self.rates.background_damage.max(0));

                // Repair, as far as the cell can pay for it. Bounded by three things: the
                // damage there is to mend, the rate it can mend at, and what it can afford.
                let want = self.rates.repair_per_tick.min(cells.damage[i]).max(0);
                let repaired = if self.rates.repair_energy_per_unit > 0 && want > 0 {
                    let affordable = ((cells.energy[i] as i64 * Q10_ONE as i64)
                        / self.rates.repair_energy_per_unit as i64)
                        .min(want as i64) as i32;
                    let spent = q10_scale(affordable, self.rates.repair_energy_per_unit);
                    cells.energy[i] = cells.energy[i].saturating_sub(spent);
                    report.dissipated += ledger.dissipate(spent as i64);
                    affordable
                } else {
                    want
                };
                cells.damage[i] = cells.damage[i]
                    .saturating_sub(repaired)
                    .saturating_add(inflicted)
                    .max(0);
                report.damage += inflicted as i64;

                // A membrane fails when the damage exceeds what was invested in it. That is
                // why membrane investment is a real trade-off and not just a number: it is
                // the cell's tolerance for its own chemistry.
                let tolerance = q10(cells.slots(i)[0].param as i32).max(q10(1));
                if cells.damage[i] > tolerance {
                    starving.push(cells.id_at(i));
                    report.poisoned = report.poisoned.saturating_add(1);
                }
            }

            // --- upkeep: the cost of being alive ---
            //
            // The floor first, then what the body costs. A cell that has shed everything still
            // pays to be a cell.
            let upkeep = self
                .rates
                .metabolic_floor
                .max(0)
                .saturating_add(self.catalogue.upkeep(&cells.loadout(i)));
            if upkeep > 0 {
                let paid = cells.energy[i].min(upkeep);
                cells.energy[i] = cells.energy[i].saturating_sub(paid);
                report.dissipated += ledger.dissipate(paid as i64);
                if paid < upkeep {
                    // It could not pay. A cell that cannot meet its own upkeep is dying, and
                    // the bookkeeping phase is where that is acted on.
                    starving.push(cells.id_at(i));
                    report.starved = report.starved.saturating_add(1);
                }
            }

            cells.age[i] = cells.age[i].saturating_add(1);
        }

        report
    }

    /// How much matter one cell's organelles of a given type can convert this tick.
    ///
    /// Only finished organelles count: a half-built chloroplast is matter the cell is
    /// carrying, not machinery it can use (SPEC §6.2).
    fn conversion_capacity(&self, cells: &CellArena, i: usize, kind: OrganelleType) -> i32 {
        let mut total = 0i32;
        for o in cells.slots(i) {
            if o.kind != kind || !o.is_active() {
                continue;
            }
            let size = self
                .rates
                .throughput_per_param
                .saturating_mul(o.param as i32);
            total = total.saturating_add(q10_scale(size, o.throttle()));
        }
        total
    }
}

/// Recompute the world's stored energy from what is actually there.
///
/// The independent check that makes I5 mean something: the ledger claims a figure, and this
/// derives one from the cells and the fluid. Two different calculations agreeing is evidence;
/// a ledger agreeing with itself is not.
#[must_use]
pub fn recompute_stored(
    cells: &CellArena,
    substrate: &Substrate,
    chem: &ChemTable,
    metabolism: &Metabolism,
) -> i64 {
    let m = metabolism.catalogue.metabolism;
    let per_unit = metabolism.per_unit_latent(chem, &m) as i64;

    let cell_energy = cells.total_energy();
    let substrate_in_cells = cells.total_interior()[m.substrate % CHEM_COUNT];
    let substrate_in_fluid = substrate.total_chem()[m.substrate % CHEM_COUNT];
    let latent = ((substrate_in_cells + substrate_in_fluid) * per_unit) / Q10_ONE as i64;

    cell_energy + latent
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{CellId, CellSeed};
    use crate::fixed::q10;
    use crate::genome::GenomePool;
    use crate::organelle::Organelle;

    fn world() -> (
        CellArena,
        Substrate,
        ChemTable,
        Ledger,
        Metabolism,
        GenomePool,
    ) {
        (
            CellArena::new(),
            Substrate::new(8, 8).unwrap(),
            ChemTable::spec_default(),
            Ledger::new(),
            Metabolism::default(),
            GenomePool::new(),
        )
    }

    fn spawn(cells: &mut CellArena, pool: &GenomePool) -> usize {
        let id = cells.spawn(CellSeed {
            x: crate::fixed::pos(4),
            y: crate::fixed::pos(4),
            mass: q10(10),
            energy: q10(100),
            membrane: 16,
            key: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome: pool.intern(vec![0x2E]).unwrap(),
        });
        cells.index(id).unwrap()
    }

    /// Per-species totals across cells and fluid — what the ledger claims to know.
    fn total_matter(cells: &CellArena, substrate: &Substrate) -> [i64; CHEM_COUNT] {
        let a = cells.total_interior();
        let b = substrate.total_chem();
        std::array::from_fn(|c| a[c] + b[c])
    }

    /// Total matter across every species — the quantity no reaction may move.
    fn grand_total(cells: &CellArena, substrate: &Substrate) -> i64 {
        total_matter(cells, substrate).iter().sum()
    }

    #[test]
    fn an_idle_cell_wears_out_even_though_nothing_poisons_it() {
        // The gap this closes: peroxide is respiration's byproduct, so a cell with no
        // mitochondrion made no poison, took no damage and lived forever. Between 44% and 80%
        // of a soup was exactly that — inert hulls with about one organelle and ages in the
        // tens of thousands.
        //
        // Deliberately not an age limit. Nothing counts birthdays. Damage accrues and repair
        // costs energy, so a cell lives as long as it can afford to keep mending itself.
        let (mut cells, sub, chem, mut ledger, met, pool) = world();
        assert!(met.rates.background_damage > 0, "nothing wears out");

        let i = spawn(&mut cells, &pool);
        cells.energy[i] = 0;
        let mut starving = Vec::new();
        met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
        assert_eq!(
            cells.damage[i], met.rates.background_damage,
            "a cell carrying no toxin at all took no wear"
        );

        // With nothing to pay with, damage only goes up, and the membrane eventually fails on
        // its own tolerance rather than on a clock.
        let tolerance = q10(cells.slots(i)[0].param as i32).max(q10(1));
        let mut ticks = 0u32;
        while cells.damage[i] <= tolerance && ticks < 100_000 {
            cells.energy[i] = 0;
            met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
            ticks += 1;
        }
        assert!(
            cells.damage[i] > tolerance,
            "an unmaintained cell never failed in 100,000 ticks"
        );
        // Thousands of ticks, not hundreds: a nudge into oblivion, as asked for.
        assert!(
            ticks > 1_000,
            "an unmaintained cell died in {ticks} ticks; that is a cull, not wear"
        );
    }

    #[test]
    fn repair_costs_energy_and_a_cell_that_can_pay_stays_whole() {
        let (mut cells, sub, chem, mut ledger, met, pool) = world();
        let i = spawn(&mut cells, &pool);
        cells.damage[i] = q10(5);
        cells.energy[i] = q10(10_000);
        let before = cells.energy[i];

        let mut starving = Vec::new();
        met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);

        assert!(
            cells.damage[i] < q10(5),
            "a cell that could pay did not mend"
        );
        assert!(
            cells.energy[i] < before,
            "mending cost nothing, so maintenance is not part of the budget"
        );
    }

    #[test]
    fn a_cell_with_no_energy_cannot_mend_at_all() {
        // The other half of charging for it: repair is not something a dying cell gets for
        // free. This is what makes falling behind irreversible rather than a plateau.
        let (mut cells, sub, chem, mut ledger, met, pool) = world();
        let i = spawn(&mut cells, &pool);
        cells.damage[i] = q10(5);
        cells.energy[i] = 0;

        let mut starving = Vec::new();
        met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
        assert!(
            cells.damage[i] >= q10(5),
            "a cell with no energy repaired itself anyway"
        );
    }

    #[test]
    fn an_idle_cell_pays_a_floor_and_eventually_runs_out() {
        // The floor exists because organelle upkeep scales with what a cell carries, so a cell
        // that has shed everything sheds its costs and lives forever doing nothing. It is
        // meant to be a nudge, not a cull: a bare hull should take a long time to go, and a
        // working cell should not notice it.
        let rates = MetabolicRates::default();
        assert!(rates.metabolic_floor > 0, "there is no floor at all");

        let catalogue = OrganelleCatalogue::balanced();
        let mut bare = [Organelle::empty(); crate::organelle::SLOT_COUNT];
        bare[0] = Organelle::finished(OrganelleType::Membrane, 24);
        let hull_upkeep = catalogue.upkeep(&bare);
        let hull_total = hull_upkeep + rates.metabolic_floor;

        // A working body: membrane, nucleus, mitochondrion, chloroplast.
        let mut working = bare;
        working[1] = Organelle::finished(OrganelleType::Nucleus, 40);
        working[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
        working[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
        let working_total = catalogue.upkeep(&working) + rates.metabolic_floor;

        // The floor has to be a real share of an idle cell's costs, or it changes nothing...
        assert!(
            rates.metabolic_floor * 4 > hull_total,
            "the floor is lost in the noise of a bare membrane's own upkeep"
        );
        // ...and a small share of a working one's, or it is a cull rather than a nudge.
        assert!(
            rates.metabolic_floor * 3 < working_total,
            "the floor is a large share of a working cell's upkeep; that is a cull"
        );
        // Which together means an idle hull is a long time dying. Thousands of ticks from a
        // typical energy reserve, not hundreds.
        let reserve = q10(400);
        assert!(
            reserve / hull_total > 1_000,
            "a hull with a full reserve dies in {} ticks; too abrupt",
            reserve / hull_total
        );
    }

    #[test]
    fn respiration_conserves_matter_and_yields_energy() {
        let (mut cells, sub, chem, mut ledger, met, pool) = world();
        let i = spawn(&mut cells, &pool);
        cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 200);
        let m = met.catalogue.metabolism;
        cells.interior_mut(i)[m.substrate] = q10(50);
        cells.interior_mut(i)[m.oxidant] = q10(50);

        ledger.set_baseline(total_matter(&cells, &sub));
        let before_total = grand_total(&cells, &sub);
        let before_energy = cells.energy[i];
        let mut starving = Vec::new();
        let report = met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);

        assert!(report.burned > 0, "nothing was respired");
        assert!(
            cells.energy[i] > before_energy,
            "respiration yielded no energy"
        );
        assert_eq!(
            grand_total(&cells, &sub),
            before_total,
            "respiration created or destroyed matter"
        );
        // A reaction moves matter between species, so the per-species claim only holds if
        // the reaction reported itself.
        ledger
            .check_matter(&total_matter(&cells, &sub))
            .expect("respiration did not account for what it transmuted");
        assert!(ledger.converted() > 0);
    }

    #[test]
    fn photosynthesis_conserves_matter_and_costs_light() {
        let (mut cells, mut sub, chem, mut ledger, met, pool) = world();
        for y in 0..8 {
            for x in 0..8 {
                let idx = sub.index(x, y);
                sub.light_mut()[idx] = Q10_ONE;
            }
        }
        let i = spawn(&mut cells, &pool);
        cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 200);
        let m = met.catalogue.metabolism;
        cells.interior_mut(i)[m.waste] = q10(100);

        ledger.set_baseline(total_matter(&cells, &sub));
        let before = grand_total(&cells, &sub);
        let mut starving = Vec::new();
        let report = met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);

        assert!(report.fixed > 0, "nothing was photosynthesised");
        assert!(report.absorbed > 0, "light was free");
        assert!(cells.interior(i)[m.substrate] > 0);
        assert_eq!(
            grand_total(&cells, &sub),
            before,
            "photosynthesis created or destroyed matter"
        );
        ledger
            .check_matter(&total_matter(&cells, &sub))
            .expect("photosynthesis did not account for what it transmuted");
    }

    #[test]
    fn the_loop_closes_over_many_ticks() {
        // The property the whole design rests on: a cell with both organelles cycles matter
        // between substrate and waste indefinitely, and the totals never move.
        let (mut cells, mut sub, chem, mut ledger, met, pool) = world();
        for y in 0..8 {
            for x in 0..8 {
                let idx = sub.index(x, y);
                sub.light_mut()[idx] = Q10_ONE;
            }
        }
        let i = spawn(&mut cells, &pool);
        cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 120);
        cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 120);
        let m = met.catalogue.metabolism;
        cells.interior_mut(i)[m.substrate] = q10(200);
        cells.interior_mut(i)[m.oxidant] = q10(200);
        cells.interior_mut(i)[m.waste] = q10(200);
        cells.energy[i] = q10(10_000);

        ledger.set_baseline(total_matter(&cells, &sub));
        let before = grand_total(&cells, &sub);
        let mut starving = Vec::new();
        for tick in 0..2_000 {
            met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
            assert_eq!(
                grand_total(&cells, &sub),
                before,
                "total matter moved at tick {tick}"
            );
            ledger
                .check_matter(&total_matter(&cells, &sub))
                .unwrap_or_else(|e| panic!("unaccounted transmutation at tick {tick}: {e}"));
        }
        assert!(ledger.energy_in() > 0, "no light was ever absorbed");
        assert!(ledger.energy_out() > 0, "no heat was ever exported");
    }

    #[test]
    fn energy_accounting_holds_against_an_independent_recomputation() {
        // I5 checked the way it has to be checked: the ledger's claim against a figure
        // derived from the world rather than from the ledger.
        let (mut cells, mut sub, chem, mut ledger, met, pool) = world();
        for y in 0..8 {
            for x in 0..8 {
                let idx = sub.index(x, y);
                sub.light_mut()[idx] = Q10_ONE;
            }
        }
        let i = spawn(&mut cells, &pool);
        cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 90);
        cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 90);
        let m = met.catalogue.metabolism;
        cells.interior_mut(i)[m.substrate] = q10(100);
        cells.interior_mut(i)[m.oxidant] = q10(100);
        cells.interior_mut(i)[m.waste] = q10(100);

        // Adopt the world's starting energy as the baseline, the way World::new does for
        // matter: what was there at the start was not "absorbed".
        let baseline = recompute_stored(&cells, &sub, &chem, &met);
        ledger.set_energy_baseline(baseline);

        let mut starving = Vec::new();
        for tick in 0..1_000 {
            met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
            ledger
                .check_energy()
                .unwrap_or_else(|e| panic!("identity broke at tick {tick}: {e}"));
            let actual = recompute_stored(&cells, &sub, &chem, &met);
            assert_eq!(
                ledger.energy_stored(),
                actual,
                "at tick {tick}: the ledger claims {} but the world holds {actual}",
                ledger.energy_stored()
            );
        }
    }

    #[test]
    fn a_toxin_damages_a_membrane_only_above_its_threshold() {
        // SPEC §7.1: "membrane damage per unit above threshold". Below it a cell is coping,
        // which is what makes excreting a toxin a strategy rather than a formality.
        //
        // Background wear switched off, so this measures the toxin and only the toxin.
        // Everything ages now — see `an_idle_cell_wears_out_even_though_nothing_poisons_it` —
        // and leaving it on here would make "a survivable dose did no harm" a claim about the
        // sum of two mechanisms rather than about toxicity.
        let (mut cells, sub, chem, mut ledger, mut met, pool) = world();
        met.rates.background_damage = 0;
        let toxin = chem
            .all()
            .iter()
            .position(|d| d.toxicity > 0)
            .expect("the default table has a toxin");

        let tolerable = spawn(&mut cells, &pool);
        cells.interior_mut(tolerable)[toxin] = met.rates.toxicity_threshold / 2;
        let poisoned = spawn(&mut cells, &pool);
        cells.interior_mut(poisoned)[toxin] = met.rates.toxicity_threshold * 20;

        ledger.absorb(q10(100_000) as i64);
        let mut starving = Vec::new();
        let report = met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);

        assert_eq!(cells.damage[tolerable], 0, "a survivable dose did harm");
        assert!(cells.damage[poisoned] > 0, "an overdose did none");
        assert!(report.damage > 0);
    }

    #[test]
    fn a_membrane_fails_when_damage_exceeds_what_was_invested_in_it() {
        // Membrane investment is the cell's tolerance for its own chemistry, which is what
        // makes it a real trade-off rather than a number.
        let (mut cells, sub, chem, mut ledger, met, pool) = world();
        let toxin = chem.all().iter().position(|d| d.toxicity > 0).unwrap();
        let i = spawn(&mut cells, &pool);
        cells.interior_mut(i)[toxin] = met.rates.toxicity_threshold * 200;
        ledger.absorb(q10(100_000) as i64);

        let mut starving = Vec::new();
        let mut ticks = 0;
        while starving.is_empty() && ticks < 10_000 {
            met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
            ticks += 1;
        }
        assert!(!starving.is_empty(), "a poisoned cell never died");
        assert_eq!(starving[0], cells.id_at(i));
    }

    #[test]
    fn damage_heals_when_the_toxin_is_gone() {
        // Otherwise the first whiff of a toxin would eventually be a death sentence, and
        // there would be no point in a cell ever clearing itself out.
        let (mut cells, sub, chem, mut ledger, met, pool) = world();
        let i = spawn(&mut cells, &pool);
        cells.damage[i] = q10(10);
        ledger.absorb(q10(100_000) as i64);
        let before = cells.damage[i];
        let mut starving = Vec::new();
        for _ in 0..200 {
            met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
        }
        assert!(cells.damage[i] < before, "damage never healed");
    }

    #[test]
    fn a_world_with_no_toxin_takes_no_damage() {
        let (mut cells, sub, mut chem, mut ledger, mut met, pool) = world();
        let defs: Vec<_> = chem
            .all()
            .iter()
            .map(|d| crate::chem::ChemicalDef {
                toxicity: 0,
                ..d.clone()
            })
            .collect();
        chem = ChemTable::new(defs);
        // As above: this is a claim about toxins, so wear is switched off for it.
        met.rates.background_damage = 0;
        let i = spawn(&mut cells, &pool);
        for c in 0..CHEM_COUNT {
            cells.interior_mut(i)[c] = q10(10_000);
        }
        ledger.absorb(q10(100_000) as i64);
        let mut starving = Vec::new();
        let report = met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
        assert_eq!(report.damage, 0);
        assert_eq!(cells.damage[i], 0);
    }

    #[test]
    fn upkeep_starves_a_cell_that_cannot_pay() {
        let (mut cells, sub, chem, mut ledger, met, pool) = world();
        let i = spawn(&mut cells, &pool);
        cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 255);
        cells.energy[i] = 1;

        let mut starving = Vec::new();
        ledger.absorb(1_000_000);
        let report = met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
        assert_eq!(report.starved, 1);
        assert_eq!(starving, vec![cells.id_at(i)]);
        assert_eq!(cells.energy[i], 0, "it spent everything it had trying");
    }

    #[test]
    fn a_cell_with_no_machinery_still_ages_and_pays() {
        let (mut cells, sub, chem, mut ledger, met, pool) = world();
        let i = spawn(&mut cells, &pool);
        ledger.absorb(q10(1000) as i64);
        let mut starving = Vec::new();
        met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
        assert_eq!(cells.age[i], 1);
        assert!(
            cells.energy[i] < q10(100),
            "the membrane's upkeep is the floor on the cost of being alive"
        );
    }

    #[test]
    fn an_unfinished_organelle_does_nothing_but_still_costs() {
        let (mut cells, sub, chem, mut ledger, met, pool) = world();
        let i = spawn(&mut cells, &pool);
        cells.slots_mut(i)[2] = Organelle {
            remaining_build: 5,
            ..Organelle::finished(OrganelleType::Mitochondrion, 200)
        };
        let m = met.catalogue.metabolism;
        cells.interior_mut(i)[m.substrate] = q10(50);
        cells.interior_mut(i)[m.oxidant] = q10(50);

        let mut starving = Vec::new();
        let report = met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
        assert_eq!(report.burned, 0, "a half-built mitochondrion must be inert");
        assert!(cells.energy[i] < q10(100), "but it is still being carried");
    }

    #[test]
    fn the_throttle_controls_the_rate() {
        // The genome's only handle on its own metabolism. Without this a cell could not
        // choose to idle, and dormancy would not be an evolvable strategy.
        let (mut cells, sub, chem, mut ledger, met, pool) = world();
        let m = met.catalogue.metabolism;
        let mut burned = Vec::new();
        for throttle in [0, Q10_ONE / 4, Q10_ONE] {
            let i = spawn(&mut cells, &pool);
            let mut organelle = Organelle::finished(OrganelleType::Mitochondrion, 200);
            organelle.control[0] = throttle as i16;
            cells.slots_mut(i)[2] = organelle;
            cells.interior_mut(i)[m.substrate] = q10(50);
            cells.interior_mut(i)[m.oxidant] = q10(50);
            let mut starving = Vec::new();
            let report = met.step(&mut cells, &sub, &chem, &mut ledger, &mut starving);
            burned.push(report.burned);
            cells.despawn(cells.id_at(i));
        }
        assert_eq!(burned[0], 0, "a closed throttle should burn nothing");
        assert!(burned[1] > 0 && burned[1] < burned[2], "{burned:?}");
    }
}
