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
use crate::organelle::{Organelle, OrganelleType};
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
    /// How many times a victim's own bulk a cell must be before it can swallow it whole, `Q10`.
    ///
    /// The gate is a *size comparison*, not a type — there is no predator flag anywhere in this,
    /// and there must not be (CLAUDE.md). What it makes true is that being large finally earns
    /// something. Everywhere else in this engine size is a bill — more upkeep, more neighbours,
    /// more matter tied up — and the only income it had was the filter's frontal area.
    ///
    /// A victim's shell counts towards its bulk, so armour is what a cell grows when it does not
    /// intend to be swallowed. That is the arms race the shell was put in the catalogue for; up
    /// to now it only blunted *damage*, which is the weaker of the two channels.
    pub engulf_ratio: i32,
    /// Energy to swallow one `Q10` of another cell's mass.
    pub engulf_energy: i32,
    /// Fraction of a swallowed cell's **charge** the eater keeps, `Q10`. The rest dissipates.
    ///
    /// A cell is four compartments and, until this existed, three of them crossed: the cytoplasm
    /// as itself, the body as carrion, the organelles' minerals as themselves — and the charge
    /// died with the victim. Measured against what the rest of the meal is worth, that is not a
    /// rounding: a median prey carries about **400** units of charge and its flesh yields about
    /// **37** units of energy once digested and burnt. **A predator was destroying an order of
    /// magnitude more energy than it gained.**
    ///
    /// # Why this is not a new kind of thing
    ///
    /// One cell's charge becoming another's is already in the engine and has been since M7:
    /// `JXFER` with `what == 0` moves `cells.energy` straight across a junction, bounded only by
    /// what the sender holds. So a parasite may drink a host's charge through a straw while a
    /// predator that swallows the same cell whole gets nothing, and there is no defence of that
    /// asymmetry — it is an omission, not a decision.
    ///
    /// # Why it is a fraction and a small one
    ///
    /// Because the physical reading is "some of the charge survives the meal", not all of it: a
    /// predator does not harvest its prey's ATP. At the whole 400 one meal is nine hundred ticks
    /// of upkeep, which is a runaway, and worse, it makes the lysosome optional — a predator that
    /// lives on charge alone needs no gut, which *removes* mechanism. An eighth puts a meal's
    /// charge on the same order as its flesh rather than ten times it.
    ///
    /// # What it makes true that was not
    ///
    /// `genomes/stalker.mm` hunts up the metabolic emission band, which reports how big and how
    /// taxed a neighbour is — the fattest cell on the slide. With this, the brightest cell is
    /// also the richest one, so the sensor and the payoff finally point at the same thing.
    ///
    /// Energy is *accounted* rather than conserved (I5) — it enters from light and leaves as
    /// dissipation — so moving charge from victim to eater breaks no invariant. It makes the
    /// dissipation entry smaller, which is the point.
    pub engulf_charge_recovery: i32,
    /// Fraction of a swallowed cell's mass that lands as usable structural matter, `Q10`.
    ///
    /// High on purpose, and that is the point of the whole mechanism rather than a generosity.
    /// `docs/FEEDING.md` §4 measured why predation does not pay: a corpse yields half its mass as
    /// carrion, digestion recovers two thirds of that, and the deposit lands where the *victim*
    /// died and diffuses from there — perhaps a sixth reaches the killer, and only if it is
    /// standing on it. Worse, what does arrive is a burnable substrate, and "food it cannot burn
    /// is not food" because the mitochondrion's capacity was already the binding term.
    ///
    /// Engulfment answers both halves at once: the matter arrives **inside** the predator, and it
    /// arrives as *structural* matter, which is built with rather than burnt and so steps around
    /// the conversion cap entirely.
    pub engulf_efficiency: i32,
    /// Fraction of its interior a wounded cell leaks each tick, per unit of damage, `Q10`.
    ///
    /// `docs/FEEDING.md` §8 ranks this first of everything on its list, and the reason is that it
    /// costs nothing new: "No new type, no new opcode, no new accounting — the chemosensor
    /// already reads gradients and the substrate already conserves."
    ///
    /// What it buys is the thing §3 says is missing outright:
    ///
    /// > **Damage is private.** `cells.damage[i]` is read by its owner and by nothing else. A
    /// > wounded cell looks exactly like a healthy one to every sensor in the catalogue. So there
    /// > is no blood in the water, and histophagy — the film's most vivid mechanism, the Coleps
    /// > arriving from a distance like piranhas — has nothing to arrive towards.
    ///
    /// Now there is something to arrive towards, and it is found with a sensor that already
    /// exists. It also fixes predation the right way round: what leaks is a cell's *interior*,
    /// which is structural matter and monomers rather than a corpse's burnable substrate, so it
    /// steps around the conversion cap §4 measured. And it is a *pack* mechanism — the leak is
    /// public, so the second attacker profits from the first one's work, which is the one thing
    /// on the whole list that rewards more than one predator at a time.
    pub bleed_rate: i32,
    /// How fast chemistry crosses a fully-open membrane, `Q10` of the gradient per tick.
    ///
    /// **The membrane's own control word, implemented at last.** SPEC §8 gives the membrane two
    /// controls and M2 lists passive transport as a deliverable; only `investment` was ever read.
    /// Until now a membrane was a perfect barrier, which is the one thing a membrane is not.
    ///
    /// What it changes is not one mechanism but five. A cell that leaks down its gradient is one
    /// that can be *starved* by dilute water, which is what gives the pump something to beat; one
    /// that can be *poisoned* from outside, which turns a toxin from a private housekeeping
    /// problem into a weapon; and one for which a vacuole is the only way to hold something
    /// without losing it, which is the job slot 4 has never had.
    ///
    /// # Why open is the right default for a membrane, and shut is right for everything else
    ///
    /// `default_control` starts a throttle open and an action shut, and permeability is a
    /// throttle: a newly built membrane is *leaky*, and closing it is what a lineage evolves.
    /// That way round because a sealed membrane is a derived state, not a starting one. If inside
    /// may differ from outside for free, the membrane has already done its work before the first
    /// division — and a simulation that begins at the answer cannot show you the question. It also
    /// makes closing a genuine trade rather than a free upgrade, since a shut membrane cannot
    /// take anything in either.
    ///
    /// # Zero for now
    ///
    /// The *rate* ships at zero, so the mechanism is present and inert until a scenario turns it
    /// up. Every archetype in `genomes/` was written against a perfect barrier and none of them
    /// closes its membrane, so switching this on globally is a question about whether the
    /// hand-written cells are viable at all — which is a thing to measure with the balance panel
    /// and `selection_guard` watching, not a default to change on the way past.
    pub permeability_rate: i32,
    /// How deep a wound has to be before it leaks, as a fraction of what the membrane tolerates.
    ///
    /// **A wound, not wear.** Without this, "leak in proportion to damage" means *every cell
    /// bleeds always*: peroxide is a byproduct of respiring at all, its toxicity charges membrane
    /// damage every tick, and repair only holds that at an asymptote rather than at zero. So a
    /// perfectly healthy cell carries a little damage forever.
    ///
    /// `m2_life::selection_guard` is what caught it. Bleeding from any damage at all took the
    /// tidy strain from winning outright to 57% — barely above a coin toss — because a universal
    /// drain uncorrelated with copy fidelity drowns out the thing that test measures. A quarter
    /// of the membrane's tolerance puts routine wear below the line and leaves a real wound
    /// above it.
    pub bleed_threshold: i32,
    /// Mass an exoenzyme dissolves per tick per unit of effort, `Q10`.
    pub dissolve_rate: i32,
    /// Carrion a lysosome digests per tick per unit of `param`, `Q10`.
    pub digestion_rate: i32,
    /// Fraction of digested carrion that becomes usable substrate, `Q10`. The rest is waste:
    /// scavenging is lossy, or a corpse would be worth more than the cell that made it.
    pub digestion_efficiency: i32,
    /// Of what digestion recovers, the share that lands as **structural matter** rather than as
    /// burnable substrate, `Q10`.
    ///
    /// # Why a predator could pay its bills forever and never grow
    ///
    /// Flesh is polymer, and a thing made of polymer is both fuel and brick. The lysosome only
    /// ever made it fuel: `CARRION -> pathway.substrate`, full stop. Measured on one median prey
    /// swallowed whole — mass 60, charge 400, 20 units of cytoplasm:
    ///
    /// ```text
    ///   eater before   energy 400   substrate  0   mass 200
    ///   eater after    energy 211   substrate 53   mass 200
    ///   gains from the meal:  substrate +53,  mass +0
    /// ```
    ///
    /// **Mass plus nothing.** So a predator is permanently fuel-rich and brick-poor, which is
    /// exactly what a late-introduced `genomes/engulfer.mm` looks like when it dies: energy 490
    /// and rising, mass stuck at 98, under its own divide weight of 140 and under the two-to-one
    /// bulk gate it needs to take another meal. It is not starving. It cannot grow.
    ///
    /// The holdfast's filter has had the mirror of this dial all along —
    /// [`EcologyConfig::capture_efficiency`], "fraction of captured detritus that becomes
    /// structural matter" — so the sessile guild eats bricks and the predatory guild eats fuel,
    /// and only one of them can build a body out of a meal. That asymmetry was never argued for;
    /// it fell out of the two organelles being written at different times.
    ///
    /// **This is a global rate, not a genome's choice.** A lysosome's `control[0]` is its throttle
    /// and `control[1]` its pathway, so there is no word left for a cell to say "make me bricks
    /// today". If that turns out to matter it wants a pathway field rather than a third control.
    ///
    /// A cell still has to *want* the mass: growth is capped at `q10(membrane.param) +
    /// membrane.control[1]`, and a predator whose organelles already outweigh that target grows
    /// by nothing however much structural matter it holds. The engine makes the brick available;
    /// raising the membrane's investment to have somewhere to put it is the genome's business.
    pub digestion_structural_share: i32,
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
            // Twice its bulk. Enough that swallowing is a real commitment to being big rather
            // than a thing any cell does to its neighbour, and low enough that the ordinary
            // spread of sizes on a grown slide puts some pairs within reach of it.
            engulf_ratio: Q10_ONE * 2,
            engulf_energy: Q10_ONE / 32,
            // On, not zero. `docs/ZOO.md` §2 blames the slide being a monoculture on exactly the
            // habit of landing a mechanism defaulted off, and this one has a measured asymmetry
            // behind it rather than a hope: `JXFER` already moves charge between cells.
            engulf_charge_recovery: Q10_ONE / 8,
            // Three quarters. Against the sixth or so a spike-and-scavenge kill returns, and
            // as *structure* rather than substrate — see the field's own note.
            engulf_efficiency: Q10_ONE * 3 / 4,
            // **Zero by default, on the evidence.** The same call `crowding_damage` makes, for
            // the same kind of reason.
            //
            // The mechanism works and `tests/senses.rs` holds it to working. What it also does,
            // switched on, is take `m2_life::selection_guard` from the tidy strain winning
            // outright to 53–61% — a coin toss. Peroxide is a byproduct of respiring at all and
            // repair holds damage at an asymptote rather than at zero, so *every* cell carries a
            // wound; bleeding from it is a universal drain uncorrelated with copy fidelity, and
            // that is precisely the signal the guard exists to detect. Raising `bleed_threshold`
            // to a quarter of the membrane's tolerance was not enough — routine wear still
            // crosses it.
            //
            // So this wants its own pass at the economy rather than a number chosen to make a
            // test go green: either damage has to stop being universal, or bleeding has to key
            // off something other than damage. Until then a scenario can switch it on and see
            // what happens, which is what a parameter is for.
            permeability_rate: 0,
            bleed_rate: 0,
            bleed_threshold: Q10_ONE / 4,
            dissolve_rate: Q10_ONE / 64,
            digestion_rate: Q10_ONE / 8,
            digestion_efficiency: (Q10_ONE * 2) / 3,
            // Half and half, because flesh is both and there is no reason in the physics to
            // prefer either. It is the first cut, not a result — `mm_core::balance` decides it.
            digestion_structural_share: Q10_ONE / 2,
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
    /// Cells swallowed whole this tick.
    pub engulfed: u32,
    /// Structural mass taken by swallowing, `Q10`.
    pub swallowed: i64,
    /// Structural mass dissolved out of living cells into the water, `Q10`.
    pub dissolved: i64,
    /// Matter leaked into the water by wounded cells, `Q10`. The size of the trail.
    pub bled: i64,
    /// Matter crossing membranes passively in either direction, `Q10`.
    pub crossed: i64,
    /// Charge taken from swallowed cells rather than dissipated, `Q10`. The size of the second
    /// income — see [`EcologyConfig::engulf_charge_recovery`].
    pub charge_taken: i64,
}

/// One thing a cell did to another cell this tick, for the picture to show.
///
/// # Why the renderer needs this and cannot derive it
///
/// A spike wound moves `cells.damage[j]` and leaves nothing behind that says who dealt it; a
/// swallowed cell is simply gone next frame. So the microscope showed cells vanishing with no
/// cause and no culprit, which is the complaint that produced this: *"it's not obvious what cell
/// its attacking and what really happened."* Both facts exist for exactly the length of one
/// ecology phase and are then unrecoverable, so they are published while they are true.
///
/// Scratch on the same terms as the `eaten` channel: cleared and refilled every ecology phase,
/// never carried between ticks, and therefore excluded from equality, hashing and snapshots
/// (hard rule 7 gains no surface). Determinism is inherited rather than argued — the loop that
/// fills this is the sequential one, in slot order over cells and then neighbour order, which is
/// the discipline `step`'s own note describes.
///
/// **Nothing in the simulation reads it.** It is a one-way channel out, and it must stay one:
/// a genome that could sense a deed would be sensing something no organelle reports.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Deed {
    /// The cell that acted. [`crate::cell::CellId::NONE`] when nothing did — a cell that starves
    /// is killed by arithmetic, not by anybody.
    pub actor: crate::cell::CellId,
    /// The cell it acted on.
    pub target: crate::cell::CellId,
    /// Where it happened, `POS`, which is the target's position at the moment it happened.
    ///
    /// Carried rather than looked up because the commonest use is a cell that no longer exists:
    /// by the time a frame is built the swallowed and the dead are out of the arena and there is
    /// nothing left to ask.
    pub x: i32,
    pub y: i32,
    pub kind: DeedKind,
}

/// What a [`Deed`] was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeedKind {
    /// A spike landed. Carries the damage dealt, `Q10`, so the picture can show a graze
    /// differently from a killing blow.
    Struck { damage: i32 },
    /// A cell took in food it had to work for — a lysosome digesting carrion, a filter catching
    /// detritus. Carries what was taken, `Q10`.
    ///
    /// **Gated at a whole unit.** Every lysosome in a crowd digests a trickle every tick, and a
    /// deed per cell per tick would be fifty thousand entries describing nothing anybody can see.
    /// A whole unit is the smallest amount the picture could show.
    Fed { amount: i32 },
    /// A cell was swallowed whole. The target is gone by the end of the tick.
    Swallowed,
    /// A cell left the arena. Reported for every death however caused, so the picture can show
    /// something leaving rather than a cell simply not being in the next frame — which is what
    /// "cells just disappear" was.
    ///
    /// The cause is not carried and deliberately so: by the time `apply_deaths` runs, starvation,
    /// poisoning and being eaten have been merged into one list and the distinction is genuinely
    /// gone. A renderer that wants it can pair this with the `Swallowed` deed naming the same
    /// cell, which is the only cause that has a culprit worth drawing.
    Died,
}

/// How far one spike is out, `Q10` of its full travel — zero if it is sheathed, still building,
/// or not a spike.
///
/// The control input is signed, and a retracted spike does nothing: SPEC §8's catalogue calls
/// it "signed extension", so a genome can put a spike away without tearing it off.
///
/// Separate from [`spike_extension_of`], which multiplies this by the size, because they answer
/// different questions and only one of them is *how far out it is*. The renderer needs this one:
/// a spike's drawn length is its travel and its drawn thickness is its `param`, and folding the
/// two together would draw a large sheathed spike as a small drawn one.
#[must_use]
pub fn spike_reach(o: &Organelle) -> i32 {
    if o.kind != OrganelleType::Spike || !o.is_active() {
        return 0;
    }
    (o.control[0] as i32).clamp(0, Q10_ONE)
}

/// What one spike contributes to its cell's total extension.
///
/// [`spike_reach`] scaled by how big the spike is, so a bigger one hurts more and costs more.
#[must_use]
pub fn spike_extension_of(o: &Organelle) -> i32 {
    q10_scale(spike_reach(o), crate::fixed::q10(o.param as i32))
}

/// How wide open one exoenzyme vesicle is, `Q10` — zero if it is shut, still building, or not an
/// exoenzyme.
///
/// The sibling of [`spike_reach`], [`crate::sensing::cilium_power`] and
/// [`crate::sensing::holdfast_effort`].
#[must_use]
pub fn exoenzyme_throttle(o: &Organelle) -> i32 {
    if o.kind != OrganelleType::Exoenzyme || !o.is_active() {
        return 0;
    }
    (o.control[0] as i32).clamp(0, Q10_ONE)
}

/// What one exoenzyme vesicle is dissolving with, `Q10`. Throttle against the size `param` bought.
#[must_use]
pub fn exoenzyme_output_of(o: &Organelle) -> i32 {
    let throttle = exoenzyme_throttle(o);
    if throttle == 0 {
        return 0;
    }
    q10_scale(crate::fixed::q10(o.param as i32), throttle)
}

/// A cell's total spike extension, `0..` — zero if it has none.
#[must_use]
pub fn spike_extension(cells: &CellArena, i: usize) -> i32 {
    let mut total = 0i32;
    for o in cells.slots(i) {
        total = total.saturating_add(spike_extension_of(o));
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
    // The whole catalogue rather than just its `metabolism`, because engulfment has to ask what
    // the victim's organelles were *made of* — a swallowed nucleus's phosphorus belongs to the
    // eater, and only `OrganelleSpec::trace_cost` knows how much of it there was.
    catalogue: &crate::organelle::OrganelleCatalogue,
    ledger: &mut Ledger,
    // What each cell's spikes, holdfasts and lysosomes add up to, from `scan_into`.
    scan: &[EcologyScan],
    // Whatever was swallowed whole this tick, for the caller to bury. Same channel starvation
    // uses — `ecology` has no business despawning anything, and a victim whose mass has already
    // moved must go through `apply_deaths` like any other corpse so the books close in one place.
    eaten: &mut Vec<crate::cell::CellId>,
    // What was done to whom, for the picture. Appended to, never cleared: the tick owns this
    // channel and clears it, because this phase does not always run — an empty slide skips it,
    // and a `clear` in here left one tool-driven kill being re-announced on every tick forever.
    // Write-only from the simulation's point of view — see [`Deed`].
    deeds: &mut Vec<Deed>,
) -> EcologyReport {
    let chemistry = &catalogue.metabolism;
    let mut report = EcologyReport::default();
    eaten.clear();

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
            // What the shell lets through. A wall is a wall whatever is pressing on it, so this
            // is the same reduction a spike meets — being crushed by neighbours and being stabbed
            // by one are the same insult arriving from different directions.
            let hurt = q10_scale(
                hurt,
                crate::organelle::shell_admits(crate::organelle::shell_cover(cells, i)),
            );
            if hurt <= 0 {
                continue;
            }
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
                    // Charged against the *victim's* shell, which is the whole of what the slot
                    // is for: until it existed a spike met bare membrane or nothing, and an arms
                    // race needs both ends to have somewhere to go.
                    let damage = q10_scale(
                        damage,
                        crate::organelle::shell_admits(crate::organelle::shell_cover(cells, j)),
                    );
                    if damage <= 0 {
                        continue;
                    }
                    cells.damage[j] = cells.damage[j].saturating_add(damage);
                    report.damage_dealt += damage as i64;
                    report.wounded = report.wounded.saturating_add(1);
                    deeds.push(Deed {
                        actor: cells.id_at(i),
                        target: cells.id_at(j),
                        x: cells.x[j],
                        y: cells.y[j],
                        kind: DeedKind::Struck { damage },
                    });
                }
            }
        }

        // --- engulfment ---
        //
        // Swallowing another cell whole, which is the one way of eating that satisfies both of
        // `docs/FEEDING.md` §4's conclusions at once: it delivers *structural* matter, and the
        // deposit lands in the predator rather than in the water where the victim happened to
        // die. Nothing else on that list does both.
        //
        // On the vacuole, whose two control words were free, and gated on a size comparison
        // rather than on any kind of predator flag. Appetite is a behaviour a genome chooses tick
        // by tick, not a property of having grown an organ.
        //
        // **`control[1]`, and that is not arbitrary.** `Organelle::finished` starts an organelle
        // at `[Q10_ONE, 0]` — the first word wide open, the second shut. Appetite on `control[0]`
        // therefore made every vacuole ever built a mouth: ten genomes in `genomes/` grow one,
        // and all ten quietly became predators. `m2_life::selection_guard` caught it — the tidy
        // strain's advantage fell from over 90% to 57%, because cells were now dying of being
        // eaten rather than of copying badly, and mortality uncorrelated with fidelity is exactly
        // what a selection test measures the absence of.
        //
        // The membrane's own note has been warning about this the whole time: it is why
        // `permeability` was left unimplemented rather than done quickly, because a permeability
        // control at full throttle means *wide open*.
        let appetite = cells
            .slots(i)
            .iter()
            .filter(|o| o.kind == OrganelleType::Vacuole && o.is_active())
            .map(|o| (o.control[1] as i32).clamp(0, Q10_ONE))
            .max()
            .unwrap_or(0);
        if appetite > 0 {
            let (sx, sy) = (
                crate::fixed::pos_to_square(cells.x[i]),
                crate::fixed::pos_to_square(cells.y[i]),
            );
            let reach = crate::junction::reach(cells, i);
            let mine = cells.mass[i];
            // In neighbour order and stopping at the first that goes down, so a cell swallows at
            // most one thing a tick and two predators reaching for the same prey are settled by
            // slot order — this loop is sequential for exactly that reason.
            let candidates: Vec<usize> = neighbours
                .around(sx, sy)
                .filter(|j| *j != i)
                .filter(|j| cells.occupied(*j))
                .filter(|j| crate::junction::distance(cells, i, *j) <= reach)
                .collect();
            for j in candidates {
                // A shell counts towards the bulk that has to be got round. Armour is not a
                // damage reduction here — it is simply more of the victim to swallow.
                let cover = crate::organelle::shell_cover(cells, j);
                let bulk = cells.mass[j].saturating_add(q10_scale(cells.mass[j], cover));
                if bulk <= 0 {
                    continue;
                }
                let needed = q10_scale(bulk, config.engulf_ratio);
                if mine < needed {
                    continue;
                }
                let cost = q10_scale(cells.mass[j], config.engulf_energy);
                if cells.energy[i] < cost {
                    continue;
                }
                cells.energy[i] = cells.energy[i].saturating_sub(cost);

                // The fourth compartment. Taken from the victim before `apply_deaths` sees it, so
                // what it goes on to dissipate is exactly the remainder and the books still
                // balance — energy is accounted rather than conserved (I5), and this only makes
                // the dissipation entry smaller.
                let charge = cells.energy[j].max(0);
                let kept = q10_scale(charge, config.engulf_charge_recovery.clamp(0, Q10_ONE));
                if kept > 0 {
                    cells.energy[j] = cells.energy[j].saturating_sub(kept);
                    cells.energy[i] = cells.energy[i].saturating_add(kept);
                    report.charge_taken = report.charge_taken.saturating_add(kept as i64);
                }
                ledger.dissipate(cost as i64);

                // **You get what it had.**
                //
                // This took only `mass` and left everything else, and the everything else is
                // where the food was. The victim then went through `apply_deaths` with its mass
                // already zeroed, which deposits its cytoplasm and its organelles' minerals into
                // the *water* — so an eater got the bricks and the square got the groceries.
                // `genomes/engulfer.mm` is the demonstration: it swallowed and starved.
                //
                // A cell is four compartments and each means something different to whatever
                // eats it, so each is treated as what it is rather than folded into one number:
                //
                //   cytoplasm  already-digested food. Crosses as itself, no loss, no conversion.
                //   body       raw flesh. Becomes carrion *inside* the eater; needs a lysosome.
                //   minerals   what the organelles were made of. Cross as themselves.
                //   energy     its charge. A share crosses; the rest dissipates. See
                //              `EcologyConfig::engulf_charge_recovery` — it was all dissipated
                //              until that existed, which destroyed about ten times the energy the
                //              flesh went on to yield.
                //
                // `carrion_fraction` is deliberately **not** applied. That split describes how a
                // corpse rots in water — half flesh, half plain carbon anything can absorb — and
                // being eaten is not rotting. A swallowed body is flesh, whole.
                //
                // `engulf_efficiency` applies to the body alone. Taxing the cytoplasm would tax
                // it twice: it is already dissolved, and its owner already paid to dissolve it.
                let structural = chemistry.structural % crate::chem::CHEM_COUNT;
                let (vx, vy) = (
                    crate::fixed::pos_to_square(cells.x[j]),
                    crate::fixed::pos_to_square(cells.y[j]),
                );

                // The organelles let go of their minerals into the victim's own cytoplasm — the
                // same release `apply_deaths` performs — so that the transfer below carries them
                // without needing a second rule. The slots are emptied as they are drained, or
                // `apply_deaths` would release the same matter a second time and mint it.
                for s in 0..crate::organelle::SLOT_COUNT {
                    let o = cells.slots(j)[s];
                    if !o.is_present() {
                        continue;
                    }
                    let spec = *catalogue.spec(o.kind);
                    if !spec.has_trace() {
                        continue;
                    }
                    for c in 0..crate::chem::CHEM_COUNT {
                        if c == structural {
                            continue;
                        }
                        let held = spec.trace_cost(c, o.param);
                        if held > 0 {
                            cells.interior_mut(j)[c] =
                                cells.interior(j)[c].saturating_add(held);
                        }
                    }
                    cells.slots_mut(j)[s] = Organelle::empty();
                }

                // The body, as carrion, inside the eater.
                let taken = cells.mass[j];
                cells.mass[j] = 0;
                let usable = q10_scale(taken, config.engulf_efficiency);
                let lost = taken.saturating_sub(usable);
                let room = crate::biology::interior_capacity(cells, i)
                    .saturating_sub(cells.interior(i)[CARRION])
                    .max(0);
                let into_cell = usable.min(room);
                if into_cell > 0 {
                    cells.interior_mut(i)[CARRION] =
                        cells.interior(i)[CARRION].saturating_add(into_cell);
                    ledger.convert(structural, CARRION, into_cell as i64);
                }
                // What would not fit, plus the share the swallowing wasted, lands on the square
                // as carrion — which is where a kill too big to finish belongs, and is the one
                // part of a swallowed meal anybody else can reach.
                let spill = usable.saturating_sub(into_cell).saturating_add(lost);
                if spill > 0 {
                    let placed = substrate.add_chem(CARRION, vx, vy, spill);
                    ledger.convert(structural, CARRION, placed as i64);
                    let stuck = spill.saturating_sub(placed);
                    if stuck > 0 {
                        // Nowhere to put it: it stays in the husk as plain carbon and
                        // `apply_deaths` will try again. Not converted, because it never became
                        // carrion.
                        cells.interior_mut(j)[structural] =
                            cells.interior(j)[structural].saturating_add(stuck);
                    }
                }

                // And the cytoplasm, as itself. Same chemical on both sides, so this is a move
                // and not a reaction — nothing to report to the ledger. Whatever the eater has
                // no room for stays in the husk and reaches the water through `apply_deaths`,
                // which is what makes a stomach worth having: eat more than you can hold and you
                // are feeding the neighbourhood.
                for c in 0..crate::chem::CHEM_COUNT {
                    let held = cells.interior(j)[c];
                    if held <= 0 {
                        continue;
                    }
                    let room = crate::biology::interior_capacity(cells, i)
                        .saturating_sub(cells.interior(i)[c])
                        .max(0);
                    let moved = held.min(room);
                    if moved > 0 {
                        cells.interior_mut(j)[c] = held.saturating_sub(moved);
                        cells.interior_mut(i)[c] =
                            cells.interior(i)[c].saturating_add(moved);
                    }
                }
                report.engulfed = report.engulfed.saturating_add(1);
                report.swallowed = report.swallowed.saturating_add(taken as i64);
                eaten.push(cells.id_at(j));
                deeds.push(Deed {
                    actor: cells.id_at(i),
                    target: cells.id_at(j),
                    x: cells.x[j],
                    y: cells.y[j],
                    kind: DeedKind::Swallowed,
                });
                break;
            }
        }

        // --- passive transport: chemistry crossing the membrane on its own ---
        //
        // Down the gradient, at a rate set by the membrane's own `control[0]`. Both directions
        // out of one subtraction: a cell richer than its square loses, a cell poorer gains, and a
        // cell in equilibrium does neither without needing a case for it.
        //
        // Bounded on the way in by what the cell can hold and on the way out by what the square
        // will take, and both use the amount actually moved rather than the amount intended —
        // `add_chem` returns what it placed, and the difference between asking and achieving is
        // where a conservation bug would live.
        let permeability = cells
            .slots(i)
            .first()
            .map_or(0, |m| (m.control[0] as i32).clamp(0, Q10_ONE));
        let crossing = q10_scale(config.permeability_rate, permeability);
        if crossing > 0 {
            let (sx, sy) = (
                crate::fixed::pos_to_square(cells.x[i]),
                crate::fixed::pos_to_square(cells.y[i]),
            );
            // A vacuole's contents are sequestered and do not cross. That is what a vacuole is
            // for, and until passive transport existed it was the one thing it could not offer:
            // it held solute out of the *osmotic* reckoning and nothing else.
            let room = crate::biology::interior_capacity(cells, i);
            for c in 0..crate::chem::CHEM_COUNT {
                let inside = cells.interior(i)[c];
                let outside = substrate.chem_at(c, sx, sy);
                let gradient = inside.saturating_sub(outside);
                let want = q10_scale(gradient, crossing);
                if want > 0 {
                    // Out, down the gradient, bounded by what is actually held.
                    let out = want.min(inside);
                    let placed = substrate.add_chem(c, sx, sy, out);
                    if placed > 0 {
                        cells.interior_mut(i)[c] = cells.interior(i)[c].saturating_sub(placed);
                        report.crossed = report.crossed.saturating_add(placed as i64);
                    }
                } else if want < 0 {
                    // In, bounded by the room left and by what the square has.
                    let headroom = room.saturating_sub(cells.interior(i)[c]).max(0);
                    let take = (-want).min(outside).min(headroom);
                    if take > 0 {
                        let moved = -substrate.add_chem(c, sx, sy, -take);
                        if moved > 0 {
                            cells.interior_mut(i)[c] =
                                cells.interior(i)[c].saturating_add(moved);
                            report.crossed = report.crossed.saturating_add(moved as i64);
                        }
                    }
                }
            }
        }

        // --- blood in the water ---
        //
        // A wounded cell leaks, in proportion to how wounded it is. Placed after everything that
        // can hurt it this tick — crowding and spikes above — so a cell bleeds from the damage it
        // actually carries rather than from last tick's.
        //
        // Out of the interior rather than off the mass: a cut leaks what a cell is *holding*, and
        // what it is holding is monomers and stores. Losing them is a real cost to the victim on
        // top of the wound, which is what makes fleeing worth more than standing.
        // What the membrane tolerates before it is a wound rather than wear — see
        // `bleed_threshold`. A thick wall resists leaking for the same reason it resists
        // everything else.
        let tolerance = crate::fixed::q10(
            cells
                .slots(i)
                .first()
                .map_or(0, |m| i32::from(m.param)),
        );
        let hurt = cells.damage[i].saturating_sub(q10_scale(tolerance, config.bleed_threshold));
        if hurt > 0 && config.bleed_rate > 0 {
            let share = q10_scale(config.bleed_rate, hurt).clamp(0, Q10_ONE);
            if share > 0 {
                let (sx, sy) = (
                    crate::fixed::pos_to_square(cells.x[i]),
                    crate::fixed::pos_to_square(cells.y[i]),
                );
                for c in 0..crate::chem::CHEM_COUNT {
                    let held = cells.interior(i)[c];
                    if held <= 0 {
                        continue;
                    }
                    let out = q10_scale(held, share).min(held);
                    if out <= 0 {
                        continue;
                    }
                    // Whatever the square will not take stays in the cell. Nothing evaporates.
                    let placed = substrate.add_chem(c, sx, sy, out);
                    if placed > 0 {
                        cells.interior_mut(i)[c] = cells.interior(i)[c].saturating_sub(placed);
                        report.bled = report.bled.saturating_add(placed as i64);
                    }
                }
            }
        }

        // --- exoenzymes ---
        //
        // The spike's pair: stab it, or dissolve it. Digests a neighbour from the outside and
        // puts the result in the *square*, where anyone standing there can take it — a leaky
        // public good, and the answer to prey too large to swallow.
        //
        // What makes it a different living from engulfment rather than a worse one is exactly
        // that leak. A swallowed cell is yours; a dissolved one is everybody's, so an exoenzyme
        // pays best where the digester is the only thing nearby and worst in a crowd. It also has
        // no size gate at all — a small cell can dissolve a large one, slowly, which is the
        // position on the board that engulfment cannot occupy.
        let enzyme = cells
            .slots(i)
            .iter()
            .map(exoenzyme_output_of)
            .fold(0i32, i32::saturating_add);
        if enzyme > 0 {
            let cost = q10_scale(enzyme, config.spike_upkeep);
            let paid = cells.energy[i].min(cost);
            cells.energy[i] = cells.energy[i].saturating_sub(paid);
            ledger.dissipate(paid as i64);
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
                let structural = chemistry.structural % crate::chem::CHEM_COUNT;
                for j in victims {
                    // A shell is what an exoenzyme has to get through, the same as a spike —
                    // mineral does not dissolve.
                    let admits =
                        crate::organelle::shell_admits(crate::organelle::shell_cover(cells, j));
                    let bite = q10_scale(q10_scale(enzyme, config.dissolve_rate), admits)
                        .min(cells.mass[j])
                        .max(0);
                    if bite <= 0 {
                        continue;
                    }
                    cells.mass[j] = cells.mass[j].saturating_sub(bite);
                    let (vx, vy) = (
                        crate::fixed::pos_to_square(cells.x[j]),
                        crate::fixed::pos_to_square(cells.y[j]),
                    );
                    // Into the water at the victim, not into the digester. That is the whole
                    // design and the reason this is not simply a slower engulfment.
                    let placed = substrate.add_chem(structural, vx, vy, bite);
                    let stuck = bite.saturating_sub(placed);
                    if stuck > 0 {
                        cells.interior_mut(j)[structural] =
                            cells.interior(j)[structural].saturating_add(stuck);
                    }
                    report.dissolved = report.dissolved.saturating_add(bite as i64);
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
            // What the cell has already caught, before what is going past.
            //
            // [`captured`] is about *interception* — how much of the drift a holdfast can take
            // out of the water — and matter that is already inside has been intercepted. So it
            // is converted at the filter's own throughput without asking the flow for permission
            // a second time, which is also why `speed` does not appear in this half.
            //
            // Without it, `EAT` on detritus was the same dead end carrion was: the opcode would
            // put it in the cytoplasm and nothing could ever take it out again.
            // **The filter deliberately does *not* read the interior, and the lysosome does.**
            //
            // It was given an interior draw in the same pass that gave the lysosome one, on the
            // symmetry that both organelles transform something a cell might be carrying. The
            // symmetry is false and `tests/sponge.rs` caught it inside one run:
            //
            //   a_cell_carried_by_the_water_catches_nothing
            //   anchored took 38,805,955 -- adrift took 41,290,393
            //
            // A holdfast's income is *interception* — `captured` scales with the slip the
            // holdfast refused, so a cell carried along by the current catches nothing, and that
            // is the whole design rather than a detail of it. An interior draw is by definition
            // flow-free, and passive transport quietly fills every cell's cytoplasm with whatever
            // it is standing in, so the drifting cell simply converted its own leakage and
            // out-earned the anchored one. Holding station stopped buying anything.
            //
            // The lysosome has no such property to lose: it digests a pool, and where the pool is
            // does not change what digestion means. So the interior route belongs to it alone,
            // which is also the right answer for engulfment — a swallowed body is digested, not
            // strained out of the water.
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
                    if taken >= Q10_ONE {
                        deeds.push(Deed {
                            actor: cells.id_at(i),
                            target: cells.id_at(i),
                            x: cells.x[i],
                            y: cells.y[i],
                            kind: DeedKind::Fed { amount: taken },
                        });
                    }
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
            // **What the cell is carrying, before what it is standing in.**
            //
            // A lysosome that could only reach the square was the reason `EAT` on carrion was a
            // dead end: the opcode takes any chemical into the cytoplasm, and carrion arriving
            // there could never be turned into anything again. Matter went in and did not come
            // out, which is a sink wearing the costume of a meal.
            //
            // It is also what stopped a cell eating another cell. Engulfment puts a body inside
            // the eater, and a stomach that can only digest the floor is not a stomach —
            // `genomes/engulfer.mm` starved holding its dinner.
            //
            // Inside first rather than in some proportion, because the alternative loses a
            // swallowed meal to whatever else is standing on the same square, and the whole
            // point of swallowing is that the meal is yours. The square is the remainder.
            let held = cells.interior(i)[CARRION].max(0);
            let from_inside = capacity.min(held);
            if from_inside > 0 {
                cells.interior_mut(i)[CARRION] =
                    cells.interior(i)[CARRION].saturating_sub(from_inside);
            }
            let headroom = capacity.saturating_sub(from_inside);
            let available = substrate.chem_at(CARRION, sx, sy);
            let taken = headroom.min(available).max(0);
            let from_square = if taken > 0 {
                (-substrate.add_chem(CARRION, sx, sy, -taken)).max(0)
            } else {
                0
            };
            let moved = from_inside.saturating_add(from_square);
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
            let structural_chem = chemistry.structural % CHEM_COUNT;

            // Flesh is polymer, so it is both fuel and brick — see
            // `EcologyConfig::digestion_structural_share`. The brick half is taken off first and
            // the remainder is fuel, so the two shares always sum to what was recovered and no
            // rounding can mint a unit between them.
            let as_brick = q10_scale(recovered, config.digestion_structural_share);
            let as_fuel = recovered.saturating_sub(as_brick);

            let mut into_cell = 0i32;
            // Both halves are capacity-bounded independently, because a cell full of fuel may
            // still have room for structure and the reverse. If the structural chemical *is* the
            // substrate — a scenario is free to pose that — the second draw simply finds the
            // capacity the first one left, which is correct rather than a special case.
            for (chem, amount) in [(structural_chem, as_brick), (substrate_chem, as_fuel)] {
                if amount <= 0 {
                    continue;
                }
                let room = crate::biology::interior_capacity(cells, i)
                    .saturating_sub(cells.interior(i)[chem])
                    .max(0);
                let took = amount.min(room);
                if took > 0 {
                    cells.interior_mut(i)[chem] = cells.interior(i)[chem].saturating_add(took);
                    ledger.convert(CARRION, chem, took as i64);
                    into_cell = into_cell.saturating_add(took);
                }
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
            if into_cell >= Q10_ONE {
                deeds.push(Deed {
                    actor: cells.id_at(i),
                    target: cells.id_at(i),
                    x: cells.x[i],
                    y: cells.y[i],
                    kind: DeedKind::Fed { amount: into_cell },
                });
            }
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
        catalogue: &crate::organelle::OrganelleCatalogue,
        ledger: &mut Ledger,
    ) -> EcologyReport {
        let mut scan = Vec::new();
        super::scan_into(cells, &mut scan);
        // These tests are about spikes, filters and digestion; nothing here swallows, so the
        // list is discarded. A test that wants engulfment has `tests/engulf.rs`, which drives a
        // whole `World` and therefore gets the deaths buried for it.
        let mut eaten = Vec::new();
        let mut deeds = Vec::new();
        super::step(
            cells, substrate, neighbours, crowding, slip, config, catalogue, ledger, &scan,
            &mut eaten,
            &mut deeds,
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
            &Scenario::stress(16, 16).biology.metabolism.catalogue,
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
    fn effort_and_size_are_separable_and_the_per_cell_sum_is_unchanged() {
        // The split the renderer needs and the property that makes it safe. A limb's drawn
        // *length* is how far out it is and its drawn *thickness* is what the cell built, so
        // folding the two together — which is all `spike_extension` ever reported — would draw a
        // large sheathed spike as a small drawn one.
        //
        // The sums must be to the byte what they were, because the ecology reads them.
        let (mut cells, pool, _substrate, _ledger) = arena();
        let id = spawn(&mut cells, &pool, 5, 5);
        let i = cells.index(id).unwrap();

        let mut spike = Organelle::finished(OrganelleType::Spike, 200);
        spike.control[0] = Q10_ONE as i16 / 2;
        cells.slots_mut(i)[4] = spike;
        let mut sheathed = Organelle::finished(OrganelleType::Spike, 255);
        sheathed.control[0] = -1;
        cells.slots_mut(i)[5] = sheathed;
        let mut unbuilt = Organelle::building(OrganelleType::Spike, 255, 4);
        unbuilt.control[0] = Q10_ONE as i16;
        cells.slots_mut(i)[6] = unbuilt;

        assert_eq!(spike_reach(&cells.slots(i)[4]), Q10_ONE / 2, "half out");
        assert_eq!(spike_reach(&cells.slots(i)[5]), 0, "a sheathed spike is away");
        assert_eq!(spike_reach(&cells.slots(i)[6]), 0, "and an unfinished one");
        // The biggest spike on the cell is the one that shows nothing, which is the whole point.
        assert!(cells.slots(i)[5].param > cells.slots(i)[4].param);

        let summed: i32 = cells
            .slots(i)
            .iter()
            .map(spike_extension_of)
            .fold(0, i32::saturating_add);
        assert_eq!(spike_extension(&cells, i), summed);

        // And the same shape for the other three, so a limb never has to reach past these for a
        // number and reimplement the semantics on the way.
        let mut enzyme = Organelle::finished(OrganelleType::Exoenzyme, 100);
        enzyme.control[0] = Q10_ONE as i16 / 4;
        assert_eq!(exoenzyme_throttle(&enzyme), Q10_ONE / 4);
        assert_eq!(exoenzyme_throttle(&sheathed), 0, "not an exoenzyme");

        let mut anchor = Organelle::finished(OrganelleType::Holdfast, 128);
        anchor.control[0] = Q10_ONE as i16;
        assert_eq!(crate::sensing::holdfast_effort(&anchor), Q10_ONE);
        anchor.control[0] = -5;
        assert_eq!(
            crate::sensing::holdfast_effort(&anchor),
            0,
            "a negative grip is letting go, not gripping backwards"
        );
        assert_eq!(crate::sensing::holdfast_grip_of(&anchor), 0);

        let mut beater = Organelle::finished(OrganelleType::Cilium, 64);
        beater.control[0] = -(Q10_ONE as i16);
        assert_eq!(
            crate::sensing::cilium_power(&beater),
            -Q10_ONE,
            "a cilium can beat backwards and the picture has to be able to say so"
        );
        assert!(crate::sensing::cilium_thrust(&beater) < 0);
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
            &crate::organelle::OrganelleCatalogue::balanced(),
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
            &crate::organelle::OrganelleCatalogue::balanced(),
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
            &crate::organelle::OrganelleCatalogue::balanced(),
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
            &crate::organelle::OrganelleCatalogue::balanced(),
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
            &crate::organelle::OrganelleCatalogue::balanced(),
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
            &crate::organelle::OrganelleCatalogue::balanced(),
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
    fn a_strike_and_a_swallow_are_reported_with_both_ends() {
        // The channel the microscope was missing. A wound moves `damage[j]` and leaves nothing
        // saying who dealt it; a swallowed cell is simply gone. Both are published while true.
        use crate::cell::CellSeed;
        let mut world = crate::World::new(crate::Scenario {
            seed: 3,
            width: 16,
            height: 16,
            ..crate::Scenario::default()
        })
        .expect("world");
        world.set_biology(crate::biology::BiologyConfig {
            mutation: crate::MutationRates::none(),
            ..crate::biology::BiologyConfig::default()
        });
        let genome = world.genomes().intern(vec![0x2E]).expect("genome");
        let seed = |mass: i32| CellSeed {
            x: crate::fixed::pos(8),
            y: crate::fixed::pos(8),
            mass: crate::fixed::q10(mass),
            energy: crate::fixed::q10(400),
            membrane: 24,
            key: 11,
            badge: 0,
            species: 0,
            parent: crate::cell::CellId::NONE,
            birth_tick: 0,
            genome: genome.clone(),
        };
        let hunter = world.spawn_cell(seed(200));
        let prey = world.spawn_cell(seed(60));
        let i = world.cells().index(hunter).expect("alive");
        let mut spike = Organelle::finished(OrganelleType::Spike, 200);
        spike.control[0] = Q10_ONE as i16; // drawn — a sheathed spike wounds nothing
        world.cells_mut().slots_mut(i)[5] = spike;
        world.adopt_current_contents_as_baseline();

        world.run(1);
        let struck: Vec<_> = world
            .deeds()
            .iter()
            .filter(|d| matches!(d.kind, DeedKind::Struck { .. }))
            .collect();
        assert!(!struck.is_empty(), "a drawn spike on a neighbour reported nothing");
        assert_eq!(struck[0].actor, hunter, "the strike names the wrong attacker");
        assert_eq!(struck[0].target, prey, "the strike names the wrong victim");

        // And a swallow, from the same channel.
        let i = world.cells().index(hunter).expect("alive");
        let mut vac = Organelle::finished(OrganelleType::Vacuole, 120);
        vac.control[1] = Q10_ONE as i16; // appetite, which is shut on a fresh vacuole
        world.cells_mut().slots_mut(i)[4] = vac;
        for _ in 0..8 {
            world.run(1);
            if let Some(d) = world
                .deeds()
                .iter()
                .find(|d| d.kind == DeedKind::Swallowed)
            {
                assert_eq!(d.actor, hunter);
                assert_eq!(d.target, prey);
                return;
            }
        }
        panic!("nothing was swallowed, so the channel was not exercised");
    }

    #[test]
    fn the_deed_channel_is_cleared_every_tick_and_never_accumulates() {
        // Scratch, not state. If it accumulated it would be world state by the back door, and
        // hard rule 7 would have a surface it does not know about.
        let mut world = crate::World::new(crate::Scenario {
            seed: 3,
            width: 16,
            height: 16,
            ..crate::Scenario::default()
        })
        .expect("world");
        world.run(20);
        assert!(
            world.deeds().is_empty(),
            "an empty slide reported deeds: {:?}",
            world.deeds()
        );
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
