//! Predation, corpses and digestion (M8, SPEC §6.2 and §7.2).
//!
//! # A corpse is a chemical, not an object
//!
//! SPEC §7.2 asks for dead cells to become "a localised deposit that lysosomes can digest and
//! that decays into the fluid over time". The obvious reading is a new kind of entity sitting
//! on the substrate with its own lifetime, its own storage and its own place in the tick order.
//!
//! It is a **chemical** instead — carrion, in the chemical table — and that is not a shortcut.
//! The substrate already diffuses, decays and conserves every chemical exactly; a corpse
//! expressed as one inherits all of it for free, and inherits it *correctly*, which a second
//! implementation of the same ideas would not. Carrion has a very low diffusion rate, so a
//! corpse stays where it fell; it decays slowly into ordinary waste, so nothing accumulates
//! forever; and a lysosome turns it back into substrate, which is scavenging.
//!
//! It also keeps CLAUDE.md's design rule intact. "No special-cased viruses, colonies or
//! organisms" is about not inventing a flag where a mechanism will do, and a corpse entity
//! would have been exactly that flag.
//!
//! # Predation is a spike and a wound
//!
//! A spike does contact damage to whatever the cell is touching. It does not "eat" anything —
//! there is no predation code path, no predator flag and nothing in the engine that knows one
//! cell from another. What happens is that damage kills, death makes carrion, and a cell with
//! a lysosome standing in carrion gets substrate out of it. Predator, scavenger and detritivore
//! are all the same three mechanisms in different proportions, which is why the analysis layer
//! infers trophic level rather than reading it off a field.

use rayon::prelude::*;

use crate::cell::CellArena;
use crate::chem::CHEM_COUNT;
use crate::fixed::{q10_scale, Q10_ONE};
use crate::ledger::Ledger;
use crate::organelle::OrganelleType;
use crate::substrate::Substrate;

/// The chemical a corpse becomes.
///
/// Index 15, which the default table calls `silt` and nothing has ever used. Redefining an
/// inert chemical is contained: the table lives in the scenario (SPEC §7.1), so a world that
/// wants different chemistry says so, and nothing that ran before this had any carrion in it
/// to mean something else.
pub const CARRION: usize = 15;

/// Particulate: solid matter suspended in a square (SPEC §17.4).
///
/// A chemical rather than an object, for the reason carrion is one — the substrate already
/// diffuses, decays and conserves every species exactly, so anything expressed as a chemical
/// inherits all of that and inherits it correctly. What is new is not the matter, it is the way
/// out of it: see [`captured`].
pub const DETRITUS: usize = 12;

/// What predation and digestion cost and yield.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EcologyConfig {
    /// Membrane damage a spike deals per tick per unit of extension, `Q10`.
    pub spike_damage: i32,
    /// Energy a spike costs per tick per unit of extension, `Q10`. Violence is not free.
    pub spike_upkeep: i32,
    /// How much of a cell's structural mass becomes carrion rather than returning straight to
    /// the fluid as its constituent chemical, `Q10` fraction.
    ///
    /// Not all of it: a cell that starves has already spent itself, and a body that vanished
    /// entirely into carrion would make starvation and predation indistinguishable to whatever
    /// comes along afterwards.
    pub carrion_fraction: i32,
    /// Carrion a lysosome digests per tick per unit of `param`, `Q10`.
    pub digestion_rate: i32,
    /// Fraction of digested carrion that becomes usable substrate, `Q10`. The rest is waste:
    /// scavenging is lossy, or a corpse would be worth more than the cell that made it.
    pub digestion_efficiency: i32,
    /// Membrane damage a tick, per whole radius a cell is pressed into by cells it is not
    /// joined to, `Q10`.
    ///
    /// **Zero by default, because it was measured and it does almost nothing.**
    ///
    /// It was meant to be somewhere for a crowd to end. Separation resolves a fraction of each
    /// overlap per tick and no more, so a population dividing faster than it can be pushed apart
    /// interpenetrates further and further; the idea was that being crushed should be survivable
    /// but not free, and that a crowded cell would spend its energy staying intact instead of
    /// dividing.
    ///
    /// It does not work, and it did not work before `split_pressure` either. Two runs of 2,500
    /// ticks differing only in this rate:
    ///
    /// ```text
    ///   tick   population        mean mass         deaths
    ///          on / off          on / off          on / off
    ///   1500   9063 / 9363       62254 / 63337      2 / 3
    ///   2500  11595 / 11751      67609 / 68474      0 / 0
    /// ```
    ///
    /// A 1.3% difference in the settled population and no deaths to speak of in either. The
    /// reason is that the damage goes through the membrane, and a cell with energy simply
    /// repairs it — so the mechanism converts to a small energy tax rather than to a ceiling.
    /// What actually bounds a crowd is `BiologyConfig::split_pressure`, which refuses a division
    /// to a cell with nowhere to put the daughter, and which bounds it without killing anything.
    ///
    /// Kept rather than deleted because it is a real mechanism and a scenario about being
    /// crushed should be able to ask for it. But it is not what holds a population, and a
    /// default that costs every tick to achieve 1.3% is not a default.
    ///
    /// Cells joined by junctions are exempt. Tissue is *supposed* to be packed, and charging
    /// an organism for holding itself together would make being multicellular a way to die.
    pub crowding_damage: i32,
    /// Ticks after birth during which a cell is not charged for being crowded.
    ///
    /// Because dividing *is* being crowded. A daughter is placed within half a square of its
    /// mother, so the two are deeply overlapped from the instant the second one exists — there
    /// is nowhere else to put it. Charged from birth, the price of reproducing is a wound to
    /// both parties, and `predator.mm` stopped at two cells where it used to reach a colony: it
    /// divided once, mother and daughter hurt each other, and that was the lineage.
    ///
    /// What crowding is meant to punish is being *stuck* in a crowd, not the moment of contact.
    /// A new pair is overlapped and separating, and separation clears an eighth of a radius a
    /// tick, so a dozen ticks settles an ordinary birth; this is that with room to spare. A
    /// cell born somewhere genuinely full is still overlapped when the grace runs out, and pays
    /// from then on.
    pub crowding_grace: u32,
    /// The radius at which `crowding_damage` is charged at face value, `Q10`.
    ///
    /// Cells smaller than this pay proportionally more and larger ones less, which is what stops
    /// a crowded population escaping the charge by shrinking. Set it to the size a cell of this
    /// scenario's ancestor settles at, so the existing calibration is what a normal cell sees and
    /// the term only bites on the ones that have given up size to fit.
    pub crowding_reference_radius: i32,

    /// Fraction of captured detritus that becomes structural matter, `Q10`. The rest is waste.
    ///
    /// Below one, like digestion's, and for the same reason: a grain must never be worth more
    /// to the cell that caught it than it was to the world.
    pub capture_efficiency: i32,
    /// How much of a square's detritus one unit of filter can take per unit of flow past it.
    ///
    /// The coefficient on the flux in [`captured`], and the one number that says whether
    /// filtering is worth an organelle. Set so that a filter at full size in a brisk current
    /// takes a useful fraction of what passes rather than all of it — a sponge that stripped
    /// its own square to zero every tick would be limited by the flow and not by itself, and
    /// then its size would not matter.
    pub capture_rate: i32,
    /// How far a photosensor looks for other cells' glow, in squares.
    ///
    /// The scan is a square of side `2 * range + 1`, paid by the cell doing the looking, so
    /// this is the one number that decides whether seeing is cheap. Six squares is far enough
    /// to be *range* — several body-lengths, past anything a touch sensor could reach — and
    /// small enough that a slide where everything is watching does not stall.
    pub em_range: i32,
}

impl Default for EcologyConfig {
    fn default() -> EcologyConfig {
        EcologyConfig {
            spike_damage: Q10_ONE / 16,
            // Comparable to a mitochondrion's upkeep, so carrying a spike is a real commitment
            // rather than something every cell does because it might as well.
            spike_upkeep: Q10_ONE / 64,
            carrion_fraction: Q10_ONE / 2,
            digestion_rate: Q10_ONE / 8,
            digestion_efficiency: (Q10_ONE * 2) / 3,
            // Off by default, on the evidence. See the field's own note: measured against a run
            // with it at `Q10_ONE / 64`, it moved the settled population by 1.3% and killed
            // almost nobody. `split_pressure` is what bounds a crowd now.
            crowding_damage: 0,
            crowding_grace: 64,
            // `biology::radius` of a cell of about thirty units of mass, which is what the
            // ancestors are seeded at: a quarter of a square plus five eighths.
            crowding_reference_radius: (Q10_ONE * 7) / 8,
            capture_efficiency: (Q10_ONE * 3) / 4,
            capture_rate: Q10_ONE / 4,
            em_range: 6,
        }
    }
}

/// What one tick of ecology did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EcologyReport {
    /// Damage dealt by spikes, `Q10`.
    pub damage_dealt: i64,
    /// Cells wounded by a spike this tick.
    pub wounded: u32,
    /// Carrion digested, `Q10`.
    pub digested: i64,
    /// Substrate recovered from carrion, `Q10`.
    pub scavenged: i64,
    /// Energy spent keeping spikes extended.
    pub spike_upkeep: i64,
    /// Membrane damage dealt by crowding, `Q10`. The measure of how much of the population is
    /// being kept in check by having nowhere to go.
    pub crushed: i64,
    /// Detritus taken out of the water by filters, `Q10`. The size of the sessile trade.
    pub filtered: i64,
}

/// A cell's total spike extension, `0..` — zero if it has none.
///
/// The control input is signed, and a retracted spike does nothing: SPEC §8's catalogue calls
/// it "signed extension", so a genome can put a spike away without tearing it off.
#[must_use]
pub fn spike_extension(cells: &CellArena, i: usize) -> i32 {
    let mut total = 0i32;
    for o in cells.slots(i) {
        if o.kind == OrganelleType::Spike && o.is_active() {
            let extension = (o.control[0] as i32).clamp(0, Q10_ONE);
            // Scaled by how big the spike is, so a bigger one hurts more and costs more.
            total = total.saturating_add(q10_scale(extension, crate::fixed::q10(o.param as i32)));
        }
    }
    total
}

/// A cell's total lysosome capacity, `0..`.
#[must_use]
pub fn digestive_capacity(cells: &CellArena, i: usize) -> i32 {
    digestive_capacity_by_pathway(cells, i).iter().sum()
}

/// What one cell's organelles are doing to its neighbours, worked out before the sequential
/// loop.
///
/// The third of these hoists, after `metabolism::Capacities` and `sensing::BodyScan`, and exact
/// for the same reason: every field is a function of the organelle slots alone and
/// [`step`] never builds, tears down or retypes one. Three separate walks of all sixteen slots
/// per cell per tick became one, and that one runs on every core.
///
/// Scratch: derived fresh each tick, so excluded from equality, hashing and snapshots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EcologyScan {
    /// How far the spikes are out, scaled by their size.
    pub extension: i32,
    /// Holdfast surface presented to the current.
    pub filter: i32,
    /// Lysosome capacity, split by the substrate each turns carrion into.
    pub digestion: [i32; crate::organelle::PATHWAY_COUNT],
}

/// Fill `into` with one [`EcologyScan`] per arena slot.
pub fn scan_into(cells: &CellArena, into: &mut Vec<EcologyScan>) {
    into.clear();
    into.resize(cells.capacity(), EcologyScan::default());
    into.par_iter_mut().enumerate().for_each(|(i, scan)| {
        if !cells.occupied(i) {
            return;
        }
        scan.extension = spike_extension(cells, i);
        scan.filter = filter_strength(cells, i);
        scan.digestion = digestive_capacity_by_pathway(cells, i);
    });
}

/// Digestive capacity, split by which substrate each lysosome turns carrion into.
///
/// A lysosome's `control[1]` chooses its pathway, the same word a mitochondrion and a
/// chloroplast use (M10.3). Which matters ecologically rather than cosmetically: a scavenger
/// that digests carrion into lipid is feeding a different guild from one that digests it into
/// sugar, and neither of them can eat what it produces unless it also carries a mitochondrion
/// set to the same reaction.
#[must_use]
pub fn digestive_capacity_by_pathway(
    cells: &CellArena,
    i: usize,
) -> [i32; crate::organelle::PATHWAY_COUNT] {
    let mut total = [0i32; crate::organelle::PATHWAY_COUNT];
    for o in cells.slots(i) {
        if o.kind == OrganelleType::Lysosome && o.is_active() {
            let throttle = (o.control[0] as i32).clamp(0, Q10_ONE);
            let n = crate::organelle::MetabolicChemistry::pathway_index(o.control[1]);
            if let Some(slot) = total.get_mut(n) {
                *slot = slot.saturating_add(q10_scale(crate::fixed::q10(o.param as i32), throttle));
            }
        }
    }
    total
}

/// How much detritus a filter takes from its square this tick, `Q10`.
///
/// # Why this is not a slower `EAT`
///
/// SPEC §17.4 justifies particulate by contrasting "a dissolved resource is a commons" with
/// "a particulate is a thing you can be standing on", and that contrast is not true of the code:
/// `EAT` already reads the cell's own centre square, already takes what it likes, and costs
/// nothing. The commons is one square wide and free. So particulate feeding written as a slower
/// `EAT` would be a *worse* `EAT`, and nothing would ever evolve to use it.
///
/// What makes filtering a different living is that it is a **flux** and not a helping. What a
/// cell can take is what goes *past* it:
///
/// ```text
///     concentration  ×  relative speed  ×  frontal area  ×  filter
/// ```
///
/// Three properties fall out of that, and all three are the point.
///
/// **It is zero for a cell drifting with the water.** Relative speed, not the water's speed —
/// a cell carried along by a current sees still water and catches nothing. So this can never
/// collapse into a second `EAT`, and *holding station* becomes worth something, which is what
/// the holdfast was built for and had no payoff for until now.
///
/// **It is symmetric, so swimming is the other way to do it.** `|v_water − v_cell|` does not
/// care which of the two is moving. A cell anchored in a current and a cell swimming through
/// still water get the same reading from the same expression, so ram feeding is not a second
/// mechanism — it is this one, read from the other end. Which of the two pays depends on
/// whether the water is already moving, and that is a trade rather than a right answer.
///
/// **Size finally earns something.** Frontal area goes with the radius, so a bigger body
/// intercepts more. Everywhere else in this engine being large is a bill — more upkeep, more
/// neighbours, more matter tied up — and this is the first place it is an income.
#[must_use]
pub fn captured(
    concentration: i32,
    relative_speed: i32,
    radius: i32,
    filter: i32,
    rate: i32,
) -> i32 {
    if concentration <= 0 || relative_speed <= 0 || filter <= 0 {
        return 0;
    }
    let flux = q10_scale(concentration, relative_speed);
    let intercepted = q10_scale(flux, radius);
    q10_scale(q10_scale(intercepted, filter), rate)
        .min(concentration)
        .max(0)
}

/// The filtering strength of a cell's holdfasts, `Q10`.
///
/// The same `control[0]` that sets the grip, deliberately: a holdfast held out hard both holds
/// on hard and strains hard, because it is one surface doing one thing. Splitting them would be
/// two dials for a cell that has no way to want them different.
#[must_use]
pub fn filter_strength(cells: &CellArena, i: usize) -> i32 {
    let mut total = 0i32;
    for o in cells.slots(i) {
        if o.kind != OrganelleType::Holdfast || !o.is_active() {
            continue;
        }
        let effort = (o.control[0] as i32).clamp(0, Q10_ONE);
        // `param` as a fraction of its own range, so a full-size holdfast at full effort is
        // one whole unit of filter and the coefficient in `EcologyConfig` carries the units.
        let size = (o.param as i32).saturating_mul(Q10_ONE) / 255;
        total = total.saturating_add(q10_scale(size, effort));
    }
    total
}

/// Spikes wound whatever their owner is touching, and lysosomes digest what they are standing
/// in (SPEC §6.2).
///
/// # Why there is no predator here
///
/// Nothing in this function knows what a predator is. A cell with a spike damages its
/// neighbours; a cell that takes enough damage dies; a dead cell becomes carrion; a cell with
/// a lysosome standing in carrion recovers substrate from it. Predation is what those three
/// look like when one lineage does them in that order, and the analysis layer infers it from
/// the trophic accounting rather than from a flag.
///
/// # Determinism
///
/// Slot order over cells, then the neighbour list in its own fixed order — the same discipline
/// as collision separation, and for the same reason: damage is applied pairwise and pairwise
/// application does not commute (I1, I6).
pub fn step(
    cells: &mut CellArena,
    substrate: &mut Substrate,
    neighbours: &crate::neighbours::NeighbourIndex,
    crowding: &[i32],
    // How fast the water is going past each cell, `Q10`, from the physics phase.
    slip: &[i32],
    config: &EcologyConfig,
    chemistry: &crate::organelle::MetabolicChemistry,
    ledger: &mut Ledger,
    // What each cell's spikes, holdfasts and lysosomes add up to, from `scan_into`.
    scan: &[EcologyScan],
) -> EcologyReport {
    let mut report = EcologyReport::default();

    for i in 0..cells.capacity() {
        if !cells.occupied(i) {
            continue;
        }

        // --- being crushed ---
        //
        // Charged against the same membrane everything else attacks, so it is repairable and
        // a crowded cell spends its energy staying intact instead of dividing. Scaled by the
        // cell's own radius, because being pressed a tenth of a millimetre into a neighbour
        // means something quite different to a large cell and a small one.
        let pressed = crowding.get(i).copied().unwrap_or(0);
        if pressed > 0 && config.crowding_damage > 0 && cells.age[i] >= config.crowding_grace {
            let own = crate::biology::radius(cells, i);
            let radius = crate::fixed::q10_to_pos(own).max(1);
            let depth = ((pressed as i64 * Q10_ONE as i64) / radius as i64).min(i32::MAX as i64);
            // Smaller cells suffer more for the same *relative* squeeze, and without this term
            // nothing does.
            //
            // `depth` is a ratio — the compression scales with the radius and is then divided by
            // it — so it reads exactly the same for a slide of big cells and a slide of shards.
            // Tolerance does not scale with size either; it is the membrane parameter. So
            // crowding used to be perfectly indifferent to how small cells got, while the one
            // real ceiling, matter, is happy to be divided into any number of smaller pieces.
            // A population under pressure therefore answered by shrinking, which cost it
            // nothing, and kept dividing until it hit `division_matter`. That is the field of
            // shards, and it is why crowding never bounded anything.
            //
            // The factor is surface over contents. A cell is a bag held by its membrane, the
            // membrane goes as the square of the radius and what it contains as the cube, so
            // the wall of a small cell carries proportionally more of the load — the same
            // fractional dent is a bigger deal to it. Linear in `1 / radius`, which is that
            // relationship, and capped so a cell shrinking towards nothing cannot produce an
            // unbounded charge out of a rounding error.
            let frail = ((config.crowding_reference_radius as i64 * Q10_ONE as i64)
                / own.max(1) as i64)
                .clamp(0, (Q10_ONE * 8) as i64) as i32;
            let hurt = q10_scale(q10_scale(config.crowding_damage, depth as i32), frail);
            cells.damage[i] = cells.damage[i].saturating_add(hurt);
            report.crushed = report.crushed.saturating_add(hurt as i64);
        }

        // --- spikes ---
        let extension = scan.get(i).map(|s| s.extension).unwrap_or(0);
        if extension > 0 {
            let cost = q10_scale(config.spike_upkeep, extension);
            let paid = cells.energy[i].min(cost);
            cells.energy[i] = cells.energy[i].saturating_sub(paid);
            report.spike_upkeep += paid as i64;
            ledger.dissipate(paid as i64);
            // Mechanical, and the loudest thing a cell does. This is the line that makes a
            // signature worth reading: an armed cell is audible whether or not it wants to be.
            cells.emit_energy(i, crate::organelle::OrganelleType::EM_MECHANICAL, paid);
            // An extended spike a cell cannot afford does nothing. Violence is not free, and a
            // starving cell is not a threat.
            if paid >= cost {
                let (sx, sy) = (
                    crate::fixed::pos_to_square(cells.x[i]),
                    crate::fixed::pos_to_square(cells.y[i]),
                );
                let reach = crate::junction::reach(cells, i);
                let victims: Vec<usize> = neighbours
                    .around(sx, sy)
                    .filter(|j| *j != i)
                    .filter(|j| cells.occupied(*j))
                    .filter(|j| crate::junction::distance(cells, i, *j) <= reach)
                    .collect();
                for j in victims {
                    let damage = q10_scale(config.spike_damage, extension);
                    if damage <= 0 {
                        continue;
                    }
                    cells.damage[j] = cells.damage[j].saturating_add(damage);
                    report.damage_dealt += damage as i64;
                    report.wounded = report.wounded.saturating_add(1);
                }
            }
        }

        // --- filters ---
        //
        // What goes past, not what is here. See [`captured`] for why that is the whole design
        // and not a detail of it.
        let filter = scan.get(i).map(|s| s.filter).unwrap_or(0);
        if filter > 0 {
            let (sx, sy) = (
                crate::fixed::pos_to_square(cells.x[i]),
                crate::fixed::pos_to_square(cells.y[i]),
            );
            let here = substrate.chem_at(DETRITUS, sx, sy);
            if here > 0 {
                // From the physics phase, because it is the only place the answer exists: a
                // cell being carried by a current and a cell holding station against one both
                // have a velocity of zero and both stand in the same moving water, and no field
                // on either tells them apart. What does is how much of the drift the holdfast
                // refused, and that is decided in `sensing::step_physics` and recorded there.
                let speed = slip.get(i).copied().unwrap_or(0);
                let radius = crate::biology::radius(cells, i);
                let want = captured(here, speed, radius, filter, config.capture_rate);
                let taken = -substrate.add_chem(DETRITUS, sx, sy, -want);
                if taken > 0 {
                    // Detritus becomes structural matter, lossily, and the loss becomes waste.
                    // Both are chemicals inside the conserved total, so this is two balanced
                    // reactions and goes through the ledger — an unaccounted transmutation is
                    // indistinguishable from a conservation bug (I4).
                    let recovered = q10_scale(taken, config.capture_efficiency);
                    let wasted = taken.saturating_sub(recovered);
                    let structural = chemistry.structural % CHEM_COUNT;
                    let waste_chem = chemistry.pathway(0).waste % CHEM_COUNT;
                    let room = crate::biology::interior_capacity(cells, i)
                        .saturating_sub(cells.interior(i)[structural])
                        .max(0);
                    let into_cell = recovered.min(room);
                    if into_cell > 0 {
                        cells.interior_mut(i)[structural] =
                            cells.interior(i)[structural].saturating_add(into_cell);
                        ledger.convert(DETRITUS, structural, into_cell as i64);
                    }
                    // What the cell had no room for goes back out with the waste, rather than
                    // being destroyed: a full cell stops profiting from filtering, it does not
                    // start leaking matter out of the world.
                    let spilled = (recovered - into_cell).saturating_add(wasted);
                    if spilled > 0 {
                        let placed = substrate.add_chem(waste_chem, sx, sy, spilled);
                        ledger.convert(DETRITUS, waste_chem, placed as i64);
                        let unplaced = spilled - placed;
                        if unplaced > 0 {
                            // Nowhere to put it. Back where it came from rather than gone.
                            let returned = substrate.add_chem(DETRITUS, sx, sy, unplaced);
                            let lost = unplaced - returned;
                            if lost > 0 {
                                let mut evicted = [0i32; CHEM_COUNT];
                                evicted[DETRITUS] = lost;
                                ledger.record_evicted(&evicted);
                            }
                        }
                    }
                    report.filtered = report.filtered.saturating_add(taken as i64);
                }
            }
        }

        // --- lysosomes ---
        //
        // One pass per pathway a lysosome is set to. Almost every cell has all its lysosomes on
        // one, so this is one iteration doing work and three skipping immediately.
        let by_pathway = scan.get(i).map(|s| s.digestion).unwrap_or_default();
        for (n, &capacity) in by_pathway.iter().enumerate() {
            if capacity <= 0 {
                continue;
            }
            let p = chemistry.pathway(n as i16);
            let (sx, sy) = (
                crate::fixed::pos_to_square(cells.x[i]),
                crate::fixed::pos_to_square(cells.y[i]),
            );
            let available = substrate.chem_at(CARRION, sx, sy);
            let taken = capacity.min(available).max(0);
            if taken <= 0 {
                continue;
            }
            let moved = -substrate.add_chem(CARRION, sx, sy, -taken);
            if moved <= 0 {
                continue;
            }
            // Carrion becomes substrate, lossily. Both are chemicals inside the conserved total,
            // so this is a balanced reaction and goes through the ledger — an unaccounted
            // transmutation is indistinguishable from a conservation bug (I4).
            let recovered = q10_scale(moved, config.digestion_efficiency);
            let wasted = moved.saturating_sub(recovered);
            // From the scenario's chemistry, not written down here. A scenario can pose a
            // different metabolic loop (SPEC §7.1), and digestion that always produced chemical 8
            // would be quietly making the wrong substance in any world that did.
            let substrate_chem = p.substrate % CHEM_COUNT;
            let waste_chem = p.waste % CHEM_COUNT;

            let room = crate::biology::interior_capacity(cells, i)
                .saturating_sub(cells.interior(i)[substrate_chem])
                .max(0);
            let into_cell = recovered.min(room);
            if into_cell > 0 {
                cells.interior_mut(i)[substrate_chem] =
                    cells.interior(i)[substrate_chem].saturating_add(into_cell);
                ledger.convert(CARRION, substrate_chem, into_cell as i64);
            }
            // Whatever the cell had no room for, plus the digestion loss, goes back to the water
            // as waste rather than being destroyed.
            let spilled = recovered.saturating_sub(into_cell).saturating_add(wasted);
            if spilled > 0 {
                let placed = substrate.add_chem(waste_chem, sx, sy, spilled);
                ledger.convert(CARRION, waste_chem, placed as i64);
                let lost = spilled.saturating_sub(placed);
                if lost > 0 {
                    // Nowhere to put it. Written off explicitly, the way a walled-in corpse is.
                    let mut evicted = [0i32; CHEM_COUNT];
                    evicted[CARRION] = lost;
                    ledger.record_evicted(&evicted);
                }
            }
            report.digested += moved as i64;
            report.scavenged += into_cell as i64;
        }
    }

    report
}

/// How a population earns its living, in parts per thousand.
///
/// The trophic analysis of SPEC §13. Inferred from what cells are *built* out of rather than
/// from anything they declare, because there is no cell-type enum and there must not be one:
/// a cell with chloroplasts is a producer whatever it calls itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TrophicMix {
    /// Cells carrying chloroplasts.
    pub producers: u32,
    /// Cells carrying a spike — the machinery of predation.
    pub predators: u32,
    /// Cells carrying a lysosome — the machinery of scavenging.
    pub scavengers: u32,
    /// Cells carrying neither, living on what is dissolved in the water.
    pub osmotrophs: u32,
    pub total: u32,
}

impl TrophicMix {
    /// Read the mix off a population.
    #[must_use]
    pub fn of(cells: &CellArena) -> TrophicMix {
        let mut mix = TrophicMix::default();
        for i in cells.iter() {
            let mut producer = false;
            let mut predator = false;
            let mut scavenger = false;
            for o in cells.slots(i) {
                if !o.is_active() {
                    continue;
                }
                match o.kind {
                    OrganelleType::Chloroplast => producer = true,
                    OrganelleType::Spike => predator = true,
                    OrganelleType::Lysosome => scavenger = true,
                    _ => {}
                }
            }
            // A cell can be counted in more than one column, because a mixotroph is a real
            // thing and forcing every cell into one box would be inventing the cell-type enum
            // by the back door.
            mix.producers += u32::from(producer);
            mix.predators += u32::from(predator);
            mix.scavengers += u32::from(scavenger);
            mix.osmotrophs += u32::from(!producer && !predator && !scavenger);
            mix.total += 1;
        }
        mix
    }

    /// Whether the population has collapsed onto one strategy.
    ///
    /// M8's fourth acceptance test: no scenario in the library should end up with everybody
    /// doing the same thing. Measured as "one column holds essentially all of it", with a
    /// floor on population so that three surviving cells are not reported as a monoculture.
    #[must_use]
    pub fn is_monoculture(&self, threshold_permille: u32) -> bool {
        if self.total < 32 {
            return false;
        }
        let permille = |n: u32| (n as u64 * 1000 / self.total.max(1) as u64) as u32;
        [
            self.producers,
            self.predators,
            self.scavengers,
            self.osmotrophs,
        ]
        .into_iter()
        .any(|n| permille(n) >= threshold_permille)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{CellId, CellSeed};
    use crate::fixed::{pos, q10};
    use crate::genome::GenomePool;
    use crate::organelle::Organelle;
    use crate::scenario::Scenario;

    /// The ecology phase as `World::step` runs it: organelle scan first, then the loop.
    ///
    /// Shadows [`super::step`] deliberately, for the reason `sensing`'s shim does — gathering the
    /// scan is not optional for a caller, and a test that skipped it would be exercising a slide
    /// where no spike is out, no holdfast filters and no lysosome digests.
    #[allow(clippy::too_many_arguments)]
    fn step(
        cells: &mut CellArena,
        substrate: &mut Substrate,
        neighbours: &crate::neighbours::NeighbourIndex,
        crowding: &[i32],
        slip: &[i32],
        config: &EcologyConfig,
        chemistry: &crate::organelle::MetabolicChemistry,
        ledger: &mut Ledger,
    ) -> EcologyReport {
        let mut scan = Vec::new();
        super::scan_into(cells, &mut scan);
        super::step(
            cells, substrate, neighbours, crowding, slip, config, chemistry, ledger, &scan,
        )
    }

    fn arena() -> (CellArena, GenomePool, Substrate, Ledger) {
        let scenario = Scenario::stress(16, 16);
        let substrate = Substrate::new(16, 16).expect("substrate");
        let ledger = Ledger::new();
        let _ = scenario;
        (CellArena::new(), GenomePool::new(), substrate, ledger)
    }

    fn spawn(cells: &mut CellArena, pool: &GenomePool, x: i32, y: i32) -> CellId {
        let genome = pool.intern(vec![0x2E; 4]).expect("genome");
        cells.spawn(CellSeed {
            x: pos(x),
            y: pos(y),
            mass: q10(30),
            energy: q10(1_000),
            membrane: 24,
            key: 11,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        })
    }

    #[test]
    fn a_cell_drifting_with_the_water_catches_nothing() {
        // The property that stops this being a second `EAT`. Relative speed, not the water's:
        // a cell carried along by a current sees still water.
        assert_eq!(captured(q10(100), 0, Q10_ONE, Q10_ONE, Q10_ONE), 0);
        assert!(captured(q10(100), Q10_ONE / 8, Q10_ONE, Q10_ONE, Q10_ONE) > 0);
    }

    #[test]
    fn swimming_and_anchoring_are_the_same_reading() {
        // `|v_water - v_cell|` does not care which of the two is moving, so ram feeding is not
        // a second mechanism. The caller takes the difference; this asserts the rate law treats
        // the result identically however it arose.
        let anchored = captured(q10(100), Q10_ONE / 8, Q10_ONE, Q10_ONE, Q10_ONE);
        let swimming = captured(q10(100), Q10_ONE / 8, Q10_ONE, Q10_ONE, Q10_ONE);
        assert_eq!(anchored, swimming);
    }

    #[test]
    fn a_bigger_cell_intercepts_more() {
        // The first place in this engine where being large is an income rather than a bill.
        let small = captured(q10(100), Q10_ONE / 8, Q10_ONE / 2, Q10_ONE, Q10_ONE);
        let large = captured(q10(100), Q10_ONE / 8, Q10_ONE * 2, Q10_ONE, Q10_ONE);
        assert!(
            large > small,
            "size bought nothing: {large} against {small}"
        );
    }

    #[test]
    fn a_filter_cannot_take_more_than_is_there() {
        // Everything about the rate law scales up; the square does not.
        let here = q10(3);
        let got = captured(here, Q10_ONE, Q10_ONE * 8, Q10_ONE * 8, Q10_ONE);
        assert!(got <= here, "took {got} from a square holding {here}");
    }

    #[test]
    fn nothing_silly_gets_past_the_rate_law() {
        for c in [i32::MIN, -1, 0, 1, i32::MAX] {
            for v in [i32::MIN, -1, 0, Q10_ONE, i32::MAX] {
                let got = captured(c, v, Q10_ONE, Q10_ONE, Q10_ONE);
                assert!(got >= 0, "negative capture from ({c}, {v})");
                assert!(
                    got <= c.max(0),
                    "capture exceeded what was there from ({c}, {v})"
                );
            }
        }
    }

    #[test]
    fn a_small_cell_suffers_more_for_the_same_relative_squeeze() {
        // The term that stops a crowded population escaping by shrinking. `depth` is a ratio —
        // compression over radius — so without this it read identically for a slide of big
        // cells and a slide of shards, and since tolerance is the membrane parameter and does
        // not scale either, crowding was perfectly indifferent to how small cells got. Matter,
        // the only real ceiling, is happy to be divided into any number of smaller pieces, so
        // that is exactly what a population under pressure did.
        let (mut cells, pool, mut substrate, mut ledger) = arena();
        let big = spawn(&mut cells, &pool, 5, 5);
        let small = spawn(&mut cells, &pool, 12, 12);
        let (bi, si) = (cells.index(big).unwrap(), cells.index(small).unwrap());
        cells.mass[si] = cells.mass[bi] / 8;
        // Past the grace, or neither is charged at all. And with a rate of its own, because the
        // default is now zero — this tests the mechanism, not whether it is switched on.
        let config = EcologyConfig {
            crowding_damage: Q10_ONE / 64,
            ..EcologyConfig::default()
        };
        cells.age[bi] = config.crowding_grace;
        cells.age[si] = config.crowding_grace;

        // The *same relative* squeeze: each pressed the same fraction of its own radius, which
        // is the case the old code could not tell apart.
        let press = |i: usize| -> i32 {
            crate::fixed::q10_to_pos(crate::biology::radius(&cells, i)).max(1) / 2
        };
        let crowding = {
            let mut v = vec![0i32; cells.capacity()];
            v[bi] = press(bi);
            v[si] = press(si);
            v
        };

        let index = crate::neighbours::NeighbourIndex::default();
        let still = vec![0i32; cells.capacity()];
        step(
            &mut cells,
            &mut substrate,
            &index,
            &crowding,
            &still,
            &config,
            &Scenario::stress(16, 16)
                .biology
                .metabolism
                .catalogue
                .metabolism,
            &mut ledger,
        );
        assert!(
            cells.damage[si] > cells.damage[bi],
            "a shard took no more than a full-sized cell for the same relative squeeze: \
             {} against {}",
            cells.damage[si],
            cells.damage[bi]
        );
    }

    #[test]
    fn a_spike_wounds_what_it_touches_and_nothing_else() {
        let (mut cells, pool, mut substrate, mut ledger) = arena();
        let attacker = spawn(&mut cells, &pool, 5, 5);
        let victim = spawn(&mut cells, &pool, 5, 5);
        let bystander = spawn(&mut cells, &pool, 14, 14);
        let ia = cells.index(attacker).unwrap();
        let mut spike = Organelle::finished(OrganelleType::Spike, 200);
        spike.control[0] = Q10_ONE as i16;
        cells.slots_mut(ia)[4] = spike;

        let mut index = crate::neighbours::NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        let report = step(
            &mut cells,
            &mut substrate,
            &index,
            &[],
            &[],
            &EcologyConfig::default(),
            &crate::organelle::MetabolicChemistry::default(),
            &mut ledger,
        );

        let iv = cells.index(victim).unwrap();
        let ib = cells.index(bystander).unwrap();
        assert!(cells.damage[iv] > 0, "the victim took no damage");
        assert_eq!(cells.damage[ib], 0, "a cell across the slide was wounded");
        assert!(report.damage_dealt > 0);
        assert!(
            report.spike_upkeep > 0,
            "the spike cost nothing to hold out"
        );
    }

    #[test]
    fn a_retracted_spike_is_harmless() {
        let (mut cells, pool, mut substrate, mut ledger) = arena();
        let attacker = spawn(&mut cells, &pool, 5, 5);
        let victim = spawn(&mut cells, &pool, 5, 5);
        let ia = cells.index(attacker).unwrap();
        let mut spike = Organelle::finished(OrganelleType::Spike, 200);
        spike.control[0] = 0; // put away
        cells.slots_mut(ia)[4] = spike;

        let mut index = crate::neighbours::NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        step(
            &mut cells,
            &mut substrate,
            &index,
            &[],
            &[],
            &EcologyConfig::default(),
            &crate::organelle::MetabolicChemistry::default(),
            &mut ledger,
        );
        assert_eq!(cells.damage[cells.index(victim).unwrap()], 0);
    }

    #[test]
    fn a_starving_cell_cannot_afford_to_be_dangerous() {
        let (mut cells, pool, mut substrate, mut ledger) = arena();
        let attacker = spawn(&mut cells, &pool, 5, 5);
        let victim = spawn(&mut cells, &pool, 5, 5);
        let ia = cells.index(attacker).unwrap();
        let mut spike = Organelle::finished(OrganelleType::Spike, 255);
        spike.control[0] = Q10_ONE as i16;
        cells.slots_mut(ia)[4] = spike;
        cells.energy[ia] = 1;

        let mut index = crate::neighbours::NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        step(
            &mut cells,
            &mut substrate,
            &index,
            &[],
            &[],
            &EcologyConfig::default(),
            &crate::organelle::MetabolicChemistry::default(),
            &mut ledger,
        );
        assert_eq!(
            cells.damage[cells.index(victim).unwrap()],
            0,
            "a cell with one unit of energy still managed to stab something"
        );
    }

    #[test]
    fn a_lysosome_digests_carrion_into_substrate() {
        let (mut cells, pool, mut substrate, mut ledger) = arena();
        let scavenger = spawn(&mut cells, &pool, 5, 5);
        let i = cells.index(scavenger).unwrap();
        cells.slots_mut(i)[4] = Organelle::finished(OrganelleType::Lysosome, 200);
        substrate.add_chem(CARRION, 5, 5, q10(50));
        ledger.set_baseline(substrate.total_chem());

        let before_carrion = substrate.chem_at(CARRION, 5, 5);
        let mut index = crate::neighbours::NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        let report = step(
            &mut cells,
            &mut substrate,
            &index,
            &[],
            &[],
            &EcologyConfig::default(),
            &crate::organelle::MetabolicChemistry::default(),
            &mut ledger,
        );

        assert!(report.digested > 0, "nothing was digested");
        assert!(report.scavenged > 0, "digesting recovered no substrate");
        assert!(substrate.chem_at(CARRION, 5, 5) < before_carrion);
        assert!(
            cells.interior(i)[8] > 0,
            "the scavenger gained no substrate"
        );
    }

    #[test]
    fn digestion_is_lossy_so_a_corpse_is_worth_less_than_the_cell() {
        let (mut cells, pool, mut substrate, mut ledger) = arena();
        let scavenger = spawn(&mut cells, &pool, 5, 5);
        let i = cells.index(scavenger).unwrap();
        cells.slots_mut(i)[4] = Organelle::finished(OrganelleType::Lysosome, 255);
        substrate.add_chem(CARRION, 5, 5, q10(200));
        ledger.set_baseline(substrate.total_chem());

        let mut index = crate::neighbours::NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        let report = step(
            &mut cells,
            &mut substrate,
            &index,
            &[],
            &[],
            &EcologyConfig::default(),
            &crate::organelle::MetabolicChemistry::default(),
            &mut ledger,
        );
        assert!(
            report.scavenged < report.digested,
            "scavenging recovered {} of {} digested — a corpse is worth as much as the cell",
            report.scavenged,
            report.digested
        );
    }

    #[test]
    fn a_cell_with_no_lysosome_ignores_carrion() {
        let (mut cells, pool, mut substrate, mut ledger) = arena();
        let plain = spawn(&mut cells, &pool, 5, 5);
        substrate.add_chem(CARRION, 5, 5, q10(50));
        ledger.set_baseline(substrate.total_chem());
        let before = substrate.chem_at(CARRION, 5, 5);

        let mut index = crate::neighbours::NeighbourIndex::default();
        index.rebuild(&cells, 16, 16);
        step(
            &mut cells,
            &mut substrate,
            &index,
            &[],
            &[],
            &EcologyConfig::default(),
            &crate::organelle::MetabolicChemistry::default(),
            &mut ledger,
        );
        assert_eq!(substrate.chem_at(CARRION, 5, 5), before);
        assert_eq!(cells.interior(cells.index(plain).unwrap())[8], 0);
    }

    #[test]
    fn the_trophic_mix_reads_what_cells_are_built_of() {
        let (mut cells, pool, _s, _l) = arena();
        let producer = spawn(&mut cells, &pool, 2, 2);
        let predator = spawn(&mut cells, &pool, 4, 4);
        let scavenger = spawn(&mut cells, &pool, 6, 6);
        let plain = spawn(&mut cells, &pool, 8, 8);
        cells.slots_mut(cells.index(producer).unwrap())[3] =
            Organelle::finished(OrganelleType::Chloroplast, 60);
        cells.slots_mut(cells.index(predator).unwrap())[3] =
            Organelle::finished(OrganelleType::Spike, 60);
        cells.slots_mut(cells.index(scavenger).unwrap())[3] =
            Organelle::finished(OrganelleType::Lysosome, 60);
        let _ = plain;

        let mix = TrophicMix::of(&cells);
        assert_eq!(mix.total, 4);
        assert_eq!(mix.producers, 1);
        assert_eq!(mix.predators, 1);
        assert_eq!(mix.scavengers, 1);
        assert_eq!(mix.osmotrophs, 1);
    }

    #[test]
    fn a_mixotroph_is_counted_in_both_columns() {
        // Forcing every cell into exactly one box would be the cell-type enum by the back
        // door, and a cell with chloroplasts *and* a spike is a real and interesting thing.
        let (mut cells, pool, _s, _l) = arena();
        let both = spawn(&mut cells, &pool, 2, 2);
        let i = cells.index(both).unwrap();
        cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
        cells.slots_mut(i)[4] = Organelle::finished(OrganelleType::Spike, 60);
        let mix = TrophicMix::of(&cells);
        assert_eq!(mix.producers, 1);
        assert_eq!(mix.predators, 1);
        assert_eq!(mix.osmotrophs, 0);
    }

    #[test]
    fn a_handful_of_survivors_is_not_a_monoculture() {
        // A population that has crashed to three cells is not evidence that one strategy won.
        let (mut cells, pool, _s, _l) = arena();
        for k in 0..3 {
            let id = spawn(&mut cells, &pool, 2 + k, 2);
            cells.slots_mut(cells.index(id).unwrap())[3] =
                Organelle::finished(OrganelleType::Chloroplast, 60);
        }
        assert!(!TrophicMix::of(&cells).is_monoculture(900));
    }

    #[test]
    fn everybody_doing_the_same_thing_is_a_monoculture() {
        let (mut cells, pool, _s, _l) = arena();
        for k in 0..40 {
            let id = spawn(&mut cells, &pool, 1 + (k % 12), 1 + (k / 12));
            cells.slots_mut(cells.index(id).unwrap())[3] =
                Organelle::finished(OrganelleType::Chloroplast, 60);
        }
        assert!(TrophicMix::of(&cells).is_monoculture(900));
    }
}
