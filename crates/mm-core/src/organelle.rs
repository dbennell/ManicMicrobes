//! Organelles: the machinery a genome builds and controls (SPEC §6.2).
//!
//! A genome does not express behaviour directly. It builds and operates *machinery*, and the
//! machinery is what touches the world. That indirection is the whole design: what a cell can
//! do is bounded by what it has built and paid for, so capability costs matter and energy
//! rather than being a property of the code.
//!
//! # Sixteen slots, sixteen types
//!
//! Both are 4-bit operands. A mutation to an organelle reference is therefore a small local
//! perturbation — slot 7 becomes slot 6 — rather than a one-in-a-hundred lottery, which is
//! what makes the loadout something evolution can hill-climb.
//!
//! Slot 0 is always the membrane and cannot be torn down or retyped: a cell without a
//! boundary is not a cell, and making that structural means nothing downstream has to check.
//!
//! # The catalogue is append-only
//!
//! Unimplemented entries are [`OrganelleType::Reserved`] and behave as no-ops. New types fill
//! a reserved slot; existing numbers are never reused or renumbered. An archived genome from
//! a million ticks ago says `BUILD 3` and must still mean chloroplast, or the phylogeny is
//! reading a different language than the one it was written in.
//!
//! # No cell-type enum
//!
//! There is none here and there must never be one. "Skin", "muscle" and "neuron" are labels
//! the analysis layer infers from loadouts (SPEC §6.3). Differentiation has to emerge from
//! expression gated on internal chemical state, and a type field would let it be faked.

use crate::chem::CHEM_COUNT;
use crate::fixed::{q10, sat_i16, Q10_ONE};
use crate::state_hash::{StateHash, StateHasher};

/// Organelle slots per cell. Addressed `slot % 16` (SPEC §6.2).
pub const SLOT_COUNT: usize = 16;

/// Slot 0 is always the membrane.
pub const MEMBRANE_SLOT: usize = 0;

/// Reduce an arbitrary slot operand into range. Addressing wraps (SPEC §3).
#[inline(always)]
#[must_use]
pub const fn slot_index(slot: i16) -> usize {
    (slot as u16 as usize) % SLOT_COUNT
}

/// The catalogue (SPEC §6.2). Numbers are part of the ISA and must never be renumbered.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
pub enum OrganelleType {
    /// Nothing built here.
    #[default]
    Empty = 255,

    /// The boundary, and the self-sensor: mass, energy, age, radius, internal chemistry and
    /// damage are all read through `OGET` on slot 0.
    Membrane = 0,
    /// Holds the genome. Its capacity bounds genome length and its copy fidelity sets the
    /// mutation rate, which is what makes mutation rate an evolvable, physically costly trait.
    Nucleus = 1,
    /// `substrate + O -> energy + waste`.
    Mitochondrion = 2,
    /// `waste + light -> substrate`. The primary producer, and the only reason the matter
    /// loop closes rather than running down into an all-waste equilibrium.
    Chloroplast = 3,
    /// Internal storage above what the cytoplasm holds.
    Vacuole = 4,
    /// Moves one chemical across the membrane against its gradient, for a price.
    Pump = 5,

    /// M3.
    Cilium = 6,
    /// M3.
    Chemosensor = 7,
    /// M3.
    Photosensor = 8,
    /// M3.
    TouchSensor = 9,
    /// M7.
    JunctionPort = 10,
    /// M8.
    Lysosome = 11,
    /// M8.
    Spike = 12,
    /// M3.
    Oscillator = 13,
    /// Grips a barrier, so a cell can hold station in moving water (SPEC §17.1, §17.6).
    ///
    /// The first thing in the engine that can *resist* the fluid rather than be carried by it.
    /// Until it existed, every body on the slide was in free fall with the current — a cluster
    /// drifted as one, and staying put was not a strategy that went unrewarded so much as a
    /// strategy that was unavailable.
    Holdfast = 14,
    /// Unimplemented; a no-op until a later milestone fills it.
    ReservedB = 15,
}

const CATALOGUE: [OrganelleType; SLOT_COUNT] = [
    OrganelleType::Membrane,
    OrganelleType::Nucleus,
    OrganelleType::Mitochondrion,
    OrganelleType::Chloroplast,
    OrganelleType::Vacuole,
    OrganelleType::Pump,
    OrganelleType::Cilium,
    OrganelleType::Chemosensor,
    OrganelleType::Photosensor,
    OrganelleType::TouchSensor,
    OrganelleType::JunctionPort,
    OrganelleType::Lysosome,
    OrganelleType::Spike,
    OrganelleType::Oscillator,
    OrganelleType::Holdfast,
    OrganelleType::ReservedB,
];

impl OrganelleType {
    /// Decode a `BUILD` type operand. Total: the operand wraps into the catalogue.
    #[inline(always)]
    #[must_use]
    pub const fn from_operand(ty: i16) -> OrganelleType {
        CATALOGUE[(ty as u16 as usize) % SLOT_COUNT]
    }

    /// The catalogue number, or 255 for an empty slot — what `OTYPE` reports.
    #[inline(always)]
    #[must_use]
    pub const fn number(self) -> i16 {
        self as u8 as i16
    }

    /// Whether this milestone implements the type. Unimplemented types can still be built and
    /// paid for; they simply do nothing, which is what `RESERVED` means.
    ///
    /// That is now one reserved slot and nothing else: M2 brought the metabolic types, M3 the
    /// sensors and the cilium, M7 the junction port, M8 the lysosome and the spike, and §17.1
    /// the holdfast. `Empty` is not a type a cell can hold.
    #[inline]
    #[must_use]
    pub const fn is_implemented(self) -> bool {
        !matches!(self, OrganelleType::Empty | OrganelleType::ReservedB)
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            OrganelleType::Empty => "empty",
            OrganelleType::Membrane => "membrane",
            OrganelleType::Nucleus => "nucleus",
            OrganelleType::Mitochondrion => "mitochondrion",
            OrganelleType::Chloroplast => "chloroplast",
            OrganelleType::Vacuole => "vacuole",
            OrganelleType::Pump => "pump",
            OrganelleType::Cilium => "cilium",
            OrganelleType::Chemosensor => "chemosensor",
            OrganelleType::Photosensor => "photosensor",
            OrganelleType::TouchSensor => "touch sensor",
            OrganelleType::JunctionPort => "junction port",
            OrganelleType::Lysosome => "lysosome",
            OrganelleType::Spike => "spike",
            OrganelleType::Oscillator => "oscillator",
            OrganelleType::Holdfast => "holdfast",
            OrganelleType::ReservedB => "reserved_b",
        }
    }

    /// How many bands the emission spectrum is divided into.
    pub const EM_BANDS: usize = 2;

    /// Work done against the world: pushing, gripping, stabbing.
    ///
    /// The split is between *doing* and *being*, not between kinds of organ, and getting that
    /// wrong the first time is what taught it. Charging a spike's upkeep to this band made a
    /// sheathed spike glow exactly like a drawn one — which turns the signature into an
    /// inventory of what a cell carries, when the whole value of it is that it reports what a
    /// cell is *doing*. Maintenance is maintenance whatever it maintains, so upkeep is all
    /// metabolic, and only work reaches this band.
    ///
    /// The consequence is worth stating because it is the interesting one: a predator at rest
    /// is indistinguishable from anything else its size, and becomes unmistakable the instant
    /// it extends. Ambush is available; ambush while armed is not.
    pub const EM_MECHANICAL: usize = 0;

    /// Chemistry and housekeeping — everything a cell pays simply to go on existing.
    pub const EM_METABOLIC: usize = 1;

    /// All catalogue entries in order.
    #[must_use]
    pub const fn all() -> &'static [OrganelleType; SLOT_COUNT] {
        &CATALOGUE
    }
}

/// One organelle in one slot.
///
/// Sixteen of these per cell, so the layout matters: eight bytes each keeps the whole loadout
/// inside 128 bytes of the 512-byte per-cell budget (SPEC §6.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Organelle {
    pub kind: OrganelleType,
    /// Set at `BUILD`, `0..=255`. Scales both capability and cost — a bigger chloroplast
    /// catches more light and costs more to carry.
    pub param: u8,
    /// Structural matter still owed before it works. A partially built organelle is inert
    /// (SPEC §6.2), which is what stops a cell building a full body in a single tick.
    pub remaining_build: u16,
    /// The genome's control input, written by `OSET`. Meaning depends on the type.
    pub control: [i16; 2],
}

impl Organelle {
    #[must_use]
    pub const fn empty() -> Organelle {
        Organelle {
            kind: OrganelleType::Empty,
            param: 0,
            remaining_build: 0,
            control: [0, 0],
        }
    }

    /// A finished organelle of a given type and size, at full throttle.
    ///
    /// Full, not idle. A cell that has paid for a mitochondrion should get a mitochondrion;
    /// requiring a separate `OSET` before the machinery does anything would mean a genome had
    /// to find two mutations to gain one capability, and the second is invisible until the
    /// first exists. Throttling down is the refinement, and it is the one a genome has a
    /// reason to discover.
    #[must_use]
    pub const fn finished(kind: OrganelleType, param: u8) -> Organelle {
        Organelle {
            kind,
            param,
            remaining_build: 0,
            control: [Q10_ONE as i16, 0],
        }
    }

    /// An organelle under construction, at full throttle for when it finishes.
    #[must_use]
    pub const fn building(kind: OrganelleType, param: u8, ticks: u16) -> Organelle {
        Organelle {
            kind,
            param,
            remaining_build: ticks,
            control: [Q10_ONE as i16, 0],
        }
    }

    #[inline(always)]
    #[must_use]
    pub const fn is_present(&self) -> bool {
        !matches!(self.kind, OrganelleType::Empty)
    }

    /// A built organelle that is not still under construction. Only these do anything.
    #[inline(always)]
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.is_present() && self.remaining_build == 0
    }

    /// Control input, clamped to a `Q10` fraction of `0..=1`. Most organelles take a throttle.
    #[inline]
    #[must_use]
    pub fn throttle(&self) -> i32 {
        (self.control[0] as i32).clamp(0, Q10_ONE)
    }
}

/// What an organelle type costs and what it can do.
///
/// Data-driven, so balancing (M8) is a matter of editing numbers rather than code, and so a
/// scenario can pose a different economy without a different engine.
/// `Default` is "costs nothing, does nothing" — the `Empty` slot's entry, and the right thing
/// for a scenario that names some fields of a spec and leaves the rest out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OrganelleSpec {
    /// Structural matter to build one, at `param == 0`, `Q10`.
    pub build_matter: i32,
    /// Extra structural matter per unit of `param`, `Q10`.
    pub build_matter_per_param: i32,
    /// Energy to build one, `Q10`.
    pub build_energy: i32,
    /// Ticks of construction before it becomes active.
    pub build_ticks: u16,
    /// Energy per tick to keep it, at `param == 0`, `Q10`.
    pub upkeep: i32,
    /// Extra upkeep per unit of `param`, `Q10`.
    pub upkeep_per_param: i32,
    /// Fraction of its structural matter recovered by `TEAR`, `Q10`. The rest is lost to the
    /// fluid as waste — dismantling is not free, or a cell would rebuild itself every tick.
    pub teardown_recovery: i32,
}

impl OrganelleSpec {
    /// Structural matter to build one at a given size.
    #[inline]
    #[must_use]
    pub fn matter_cost(&self, param: u8) -> i32 {
        self.build_matter
            .saturating_add(self.build_matter_per_param.saturating_mul(param as i32))
    }

    /// Energy per tick to keep one at a given size.
    #[inline]
    #[must_use]
    pub fn upkeep_cost(&self, param: u8) -> i32 {
        self.upkeep
            .saturating_add(self.upkeep_per_param.saturating_mul(param as i32))
    }
}

/// The costs and capabilities of every catalogue entry.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OrganelleCatalogue {
    specs: [OrganelleSpec; SLOT_COUNT],
    /// Which chemical a mitochondrion oxidises, which a chloroplast produces, and so on.
    pub metabolism: MetabolicChemistry,
}

/// How many ways there are to make a living in a world.
///
/// Four. A power of two so the selector is a mask, and enough that the default chemical table's
/// three energy substrates each get a pathway with one spare. Raising it costs a slot in the
/// per-cell capacity tally in `Metabolism::step` and nothing else.
pub const PATHWAY_COUNT: usize = 4;

/// One metabolic reaction, in both directions (SPEC §7.2).
///
/// ```text
/// respiration     substrate + oxidant  ->  waste (+ reactive) + energy
/// photosynthesis  2 waste + light      ->  substrate + oxidant
/// ```
///
/// The pair has to close: what a mitochondrion turns into waste, a chloroplast must be able to
/// turn back into substrate, or matter conservation guarantees the world ends as an all-waste
/// equilibrium. Naming the chemicals rather than hard-coding them lets a scenario pose a
/// different chemistry, and lets the closure be checked rather than assumed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Pathway {
    /// Burned by a mitochondrion for energy.
    pub substrate: usize,
    /// Consumed alongside the substrate, and produced alongside it by photosynthesis.
    ///
    /// This was two fields, `oxidant` and `byproduct`, and `closes()` required them to be
    /// equal — the same chemical named twice because the reaction runs in both directions.
    pub oxidant: usize,
    /// Produced by burning. A chloroplast turns this back into substrate.
    pub waste: usize,
    /// Respiration's toxic byproduct — reactive oxygen, in the real thing.
    ///
    /// A fraction of what a mitochondrion exhales comes out as this rather than as ordinary
    /// waste. It is what gives ageing a *cause*: a cell that respires accumulates a poison it
    /// must excrete or repair away, and one that cannot keep up eventually fails. Without it
    /// a well-fed cell is immortal, and a population with no turnover has no differential
    /// reproduction for selection to be made of.
    pub reactive: usize,
}

impl Default for Pathway {
    fn default() -> Self {
        // Indices into `ChemTable::spec_default`: sugar, an inert filler standing in for
        // dissolved oxygen, carbon dioxide, and peroxide.
        Pathway {
            substrate: 8,
            oxidant: 14,
            waste: 11,
            reactive: 13,
        }
    }
}

impl Pathway {
    /// Whether this reaction closes and names real chemicals.
    #[must_use]
    pub fn closes(&self) -> bool {
        self.substrate < CHEM_COUNT
            && self.oxidant < CHEM_COUNT
            && self.waste < CHEM_COUNT
            && self.reactive < CHEM_COUNT
            && self.substrate != self.waste
    }
}

/// Every way of making a living that a world offers, and what bodies are built from.
///
/// # Why there is more than one
///
/// Until M10.3 this was a single reaction, so there was exactly one way to make a living and
/// every cell in every scenario made it the same way. The chemical table already described a
/// richer world than the engine implemented — `lipid` and `sulphide` carried energy yields
/// that nothing could burn, because a mitochondrion burned *the* substrate, one index.
///
/// With several, an organelle chooses which reaction it runs, by its `control[1]`. A
/// mitochondrion on pathway 1 can burn only pathway 1's substrate, so a lineage must either
/// pair its own chloroplast and mitochondrion onto the same pathway or **eat something that
/// makes what it burns**. That is cross-feeding, and it is the first mechanism here that turns
/// one lineage's waste into another's food by evolution rather than by construction.
///
/// See `docs/CHEMISTRY.md` for the measurements this came out of.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MetabolicChemistry {
    /// The reactions on offer. Selected by an organelle's `control[1]`, modulo this length, so
    /// every value a genome can write names a real pathway (hard rule 4).
    pub pathways: [Pathway; PATHWAY_COUNT],
    /// What a body is built out of.
    ///
    /// Shared across pathways rather than per-pathway: a cell is one body whatever it eats, and
    /// a world where what you are made of depends on what you had for lunch is a different and
    /// much stranger design than this one.
    pub structural: usize,
}

impl Default for MetabolicChemistry {
    fn default() -> Self {
        // Pathway 0 is the world M2 through M9 ran on, unchanged, so a scenario that says
        // nothing about chemistry behaves exactly as it did.
        //
        // The other three are the substrates the default table has always carried yields for
        // and nothing could ever eat: lipid at 1536, sulphide at 768. All four share oxygen as
        // the oxidant and carbon dioxide as the waste, which is what makes them *alternatives*
        // rather than four disjoint worlds — they compete for one pool of oxidant and feed one
        // pool of waste, so which substrate a lineage runs on is a choice with consequences
        // for everybody else.
        //
        // The fourth is sugar again, deliberately: a duplicate is the cheapest thing for an
        // unlucky mutation to land on, and it costs nothing to make one of the four slots
        // harmless rather than leaving it to alias a pathway by accident.
        MetabolicChemistry {
            pathways: [
                Pathway {
                    substrate: 8,
                    oxidant: 14,
                    waste: 11,
                    reactive: 13,
                },
                Pathway {
                    substrate: 9,
                    oxidant: 14,
                    waste: 11,
                    reactive: 13,
                },
                Pathway {
                    substrate: 10,
                    oxidant: 14,
                    waste: 11,
                    reactive: 13,
                },
                Pathway {
                    substrate: 8,
                    oxidant: 14,
                    waste: 11,
                    reactive: 13,
                },
            ],
            structural: 4,
        }
    }
}

impl MetabolicChemistry {
    /// Pathway zero: the one an organelle runs when its genome has never said otherwise.
    ///
    /// The default set makes this the reaction M2 through M9 ran on, unchanged, so a world
    /// that says nothing about chemistry behaves exactly as it always did.
    #[must_use]
    pub fn primary(&self) -> &Pathway {
        &self.pathways[0]
    }

    /// The pathway an organelle's control word selects.
    ///
    /// Reduced modulo the count, so every value a genome can write names a real reaction and
    /// none of them is an error (hard rule 4: addressing wraps). The cast through `u16` is what
    /// makes a negative control word wrap rather than saturate at zero — the same treatment a
    /// cilium's mount angle gets in `sensing.rs`.
    #[must_use]
    pub fn pathway(&self, control: i16) -> &Pathway {
        let n = (control as u16 as usize) % PATHWAY_COUNT;
        // `PATHWAY_COUNT` is the array's length, so this cannot miss; the fallback is here
        // because indexing that *could* panic has no business on a path a genome reaches.
        self.pathways.get(n).unwrap_or(&self.pathways[0])
    }

    /// Which pathway index a control word selects.
    #[must_use]
    pub fn pathway_index(control: i16) -> usize {
        (control as u16 as usize) % PATHWAY_COUNT
    }

    /// Whether every loop closes: everything a mitochondrion consumes must be something a
    /// chloroplast can produce, and vice versa.
    ///
    /// If this is false the world runs down and dies, however good the cells are. It is worth
    /// asserting rather than discovering after a million ticks.
    #[must_use]
    pub fn closes(&self) -> bool {
        self.structural < CHEM_COUNT && self.pathways.iter().all(Pathway::closes)
    }

    /// Every distinct substrate, once each.
    ///
    /// For the energy accounting of I5: latent energy is held by the chemicals a mitochondrion
    /// could release it from, and two pathways sharing a substrate must not count it twice.
    #[must_use]
    pub fn substrates(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.pathways.iter().map(|p| p.substrate).collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

impl Default for OrganelleCatalogue {
    fn default() -> Self {
        Self::balanced()
    }
}

impl OrganelleCatalogue {
    /// The M2 starting economy.
    ///
    /// These numbers are a first pass, not a result. The balancing milestone is M8, and the
    /// interesting output of this project is knowing which of them matter — so they live here
    /// as named data rather than scattered through the code that reads them.
    #[must_use]
    pub fn balanced() -> OrganelleCatalogue {
        let cheap = OrganelleSpec {
            build_matter: q10(4),
            build_matter_per_param: q10(1) / 8,
            build_energy: q10(8),
            build_ticks: 8,
            upkeep: q10(1) / 64,
            upkeep_per_param: q10(1) / 1024,
            teardown_recovery: Q10_ONE / 2,
        };
        let mut specs = [cheap; SLOT_COUNT];

        // The membrane is the one thing every cell has, so its upkeep is the floor on the
        // cost of being alive.
        //
        // NOT YET IMPLEMENTED: control input 0, `permeability`. SPEC §8 gives the membrane
        // two controls and only the second, `investment`, is read (by the growth step in
        // `metabolism`). So passive transport — chemistry crossing the membrane on its own,
        // down its gradient, without an `EAT` — does not happen, and M2's deliverable list
        // asks for it. Today a membrane is a perfect barrier, which is the one thing a
        // membrane is not.
        //
        // It is left undone rather than done quickly because of where the default sits.
        // `Organelle::finished` starts every control at full throttle, which for a
        // permeability control means *wide open*: switching this on would make every existing
        // ancestor leak its sugar into the water and absorb the peroxide it just excreted.
        // That is a change to whether the hand-written ancestors are viable at all, so it
        // needs its own pass at M2's persistence and selection runs — not a late addition on
        // top of results already gathered.
        specs[OrganelleType::Membrane as usize] = OrganelleSpec {
            build_matter: q10(8),
            build_matter_per_param: q10(1) / 4,
            build_energy: q10(16),
            build_ticks: 0,
            upkeep: q10(1) / 32,
            upkeep_per_param: q10(1) / 512,
            teardown_recovery: 0,
        };
        // A nucleus is expensive to carry, which is what makes genome bloat cost something
        // (SPEC §4.1) without any rule saying so.
        specs[OrganelleType::Nucleus as usize] = OrganelleSpec {
            build_matter: q10(6),
            build_matter_per_param: q10(1) / 2,
            build_energy: q10(12),
            build_ticks: 12,
            upkeep: q10(1) / 32,
            upkeep_per_param: q10(1) / 256,
            teardown_recovery: Q10_ONE / 2,
        };
        specs[OrganelleType::Mitochondrion as usize] = OrganelleSpec {
            build_matter: q10(5),
            build_matter_per_param: q10(1) / 8,
            build_energy: q10(10),
            build_ticks: 10,
            upkeep: q10(1) / 48,
            upkeep_per_param: q10(1) / 768,
            teardown_recovery: Q10_ONE / 2,
        };
        specs[OrganelleType::Chloroplast as usize] = OrganelleSpec {
            build_matter: q10(7),
            build_matter_per_param: q10(1) / 6,
            build_energy: q10(14),
            build_ticks: 14,
            upkeep: q10(1) / 40,
            upkeep_per_param: q10(1) / 640,
            teardown_recovery: Q10_ONE / 2,
        };

        // A spike is the dearest thing in the catalogue to build and the dearest to carry —
        // dearer than a chloroplast, before the per-tick cost of actually extending it. This is
        // the main dial on whether the food web has a second level: on the `cheap` default a
        // spike costs less than a mitochondrion, and if violence is cheaper than metabolism
        // then every lineage grows one, eats everything and starves. Predation has to be a
        // commitment that only pays when there is enough prey about to repay it.
        specs[OrganelleType::Spike as usize] = OrganelleSpec {
            build_matter: q10(9),
            build_matter_per_param: q10(1) / 4,
            build_energy: q10(20),
            build_ticks: 20,
            upkeep: q10(1) / 24,
            upkeep_per_param: q10(1) / 384,
            teardown_recovery: Q10_ONE / 2,
        };

        // A holdfast is a commitment to a place. Dear to build and slow, because deciding to
        // stop moving should not be a thing a cell does casually and reverses next tick;
        // ordinary to carry, because cement does not cost much to keep — the price of holding
        // is charged per tick against the force actually resisted, in `sensing::step_physics`,
        // so a cell anchored in still water pays only upkeep and one in a torrent pays for the
        // torrent.
        //
        // `teardown_recovery` is a quarter where everything else is a half: letting go leaves
        // the attachment behind. Small, and deliberate — it is the only asymmetry that makes
        // "stay" and "leave" different decisions rather than the same decision twice.
        specs[OrganelleType::Holdfast as usize] = OrganelleSpec {
            build_matter: q10(7),
            build_matter_per_param: q10(1) / 6,
            build_energy: q10(14),
            build_ticks: 16,
            upkeep: q10(1) / 48,
            upkeep_per_param: q10(1) / 768,
            teardown_recovery: Q10_ONE / 4,
        };

        // Scavenging is the cheaper trade and the lower-yield one: a lysosome costs about what
        // a mitochondrion costs, and what it digests has already been through someone else.
        specs[OrganelleType::Lysosome as usize] = OrganelleSpec {
            build_matter: q10(6),
            build_matter_per_param: q10(1) / 8,
            build_energy: q10(12),
            build_ticks: 12,
            upkeep: q10(1) / 56,
            upkeep_per_param: q10(1) / 896,
            teardown_recovery: Q10_ONE / 2,
        };

        OrganelleCatalogue {
            specs,
            metabolism: MetabolicChemistry::default(),
        }
    }

    /// Every spec, in catalogue order. For serialisation (hard rule 7).
    #[must_use]
    pub fn specs(&self) -> &[OrganelleSpec; SLOT_COUNT] {
        &self.specs
    }

    /// Replace every spec, in catalogue order. For restoring a snapshot.
    pub fn set_specs(&mut self, specs: [OrganelleSpec; SLOT_COUNT]) {
        self.specs = specs;
    }

    #[inline(always)]
    #[must_use]
    pub fn spec(&self, kind: OrganelleType) -> &OrganelleSpec {
        match kind {
            OrganelleType::Empty => &self.specs[0],
            other => &self.specs[(other as u8 as usize) % SLOT_COUNT],
        }
    }

    /// Total upkeep for a whole loadout, `Q10` energy per tick.
    ///
    /// Charged whether or not an organelle is finished: a half-built mitochondrion is still
    /// matter the cell is carrying around.
    #[must_use]
    pub fn upkeep(&self, slots: &[Organelle; SLOT_COUNT]) -> i32 {
        let mut total = 0i32;
        for o in slots {
            if o.is_present() {
                total = total.saturating_add(self.spec(o.kind).upkeep_cost(o.param));
            }
        }
        total
    }
}

impl StateHash for Organelle {
    fn hash_state(&self, h: &mut StateHasher) {
        h.u8(self.kind as u8);
        h.u8(self.param);
        h.u16(self.remaining_build);
        h.i16(self.control[0]);
        h.i16(self.control[1]);
    }
}

/// What `OGET` reports for the membrane, which is the cell's self-sensor (SPEC §5.1).
///
/// Index operands wrap, so every value a genome asks for is one of these.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum MembraneReading {
    Mass = 0,
    Energy = 1,
    Age = 2,
    Radius = 3,
    Damage = 4,
    /// `5..=20` read internal chemical `idx - 5`.
    Chemical = 5,
    /// This cell's own surface badge, at the index right after the chemicals.
    ///
    /// *After* them, not before, so that adding it renumbers nothing: every genome written
    /// under ISA 3 reads the same chemical from the same index it always did.
    ///
    /// Readable at all because recognition has to survive the badge changing. A genome that
    /// compared a neighbour's badge to a hard-coded immediate would stop recognising its own
    /// kin the moment a mutation moved the badge — so lineages could never drift their colours.
    /// Comparing *neighbour to self* means a lineage that changes its badge changes what it
    /// answers to in the same stroke, and diverges from its cousins as it goes.
    Badge = 21,
}

impl MembraneReading {
    /// Decode an `OGET` index operand. Total for any input.
    ///
    /// The sixteen chemical readings sit immediately after the five scalars, so a genome that
    /// walks the index space finds its own chemistry rather than falling off the end.
    #[inline]
    #[must_use]
    pub fn decode(idx: i16) -> MembraneReading {
        match (idx as u16 as usize) % (6 + CHEM_COUNT) {
            0 => MembraneReading::Mass,
            1 => MembraneReading::Energy,
            2 => MembraneReading::Age,
            3 => MembraneReading::Radius,
            4 => MembraneReading::Damage,
            n if n == 5 + CHEM_COUNT => MembraneReading::Badge,
            _ => MembraneReading::Chemical,
        }
    }

    /// Which chemical a `Chemical` reading refers to.
    #[inline]
    #[must_use]
    pub fn chemical_of(idx: i16) -> usize {
        ((idx as u16 as usize) % (5 + CHEM_COUNT)).saturating_sub(5) % CHEM_COUNT
    }
}

/// Convert a `Q10` internal quantity to what a genome sees. Saturates (SPEC §3).
#[inline]
#[must_use]
pub fn to_cell_visible(q: i32) -> i16 {
    sat_i16(q / Q10_ONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_operands_wrap() {
        for s in i16::MIN..=i16::MAX {
            assert!(slot_index(s) < SLOT_COUNT);
        }
        assert_eq!(slot_index(0), 0);
        assert_eq!(slot_index(16), 0);
        assert_eq!(slot_index(-1), 15);
    }

    #[test]
    fn type_operands_wrap_into_the_catalogue() {
        for t in i16::MIN..=i16::MAX {
            let kind = OrganelleType::from_operand(t);
            assert!(
                kind != OrganelleType::Empty,
                "BUILD must always build something"
            );
        }
        assert_eq!(OrganelleType::from_operand(3), OrganelleType::Chloroplast);
        assert_eq!(OrganelleType::from_operand(19), OrganelleType::Chloroplast);
        assert_eq!(OrganelleType::from_operand(-13), OrganelleType::Chloroplast);
    }

    #[test]
    fn catalogue_numbers_are_stable() {
        // Renumbering these changes the meaning of every archived genome (hard rule 8).
        assert_eq!(OrganelleType::Membrane.number(), 0);
        assert_eq!(OrganelleType::Nucleus.number(), 1);
        assert_eq!(OrganelleType::Mitochondrion.number(), 2);
        assert_eq!(OrganelleType::Chloroplast.number(), 3);
        assert_eq!(OrganelleType::Vacuole.number(), 4);
        assert_eq!(OrganelleType::Pump.number(), 5);
        assert_eq!(OrganelleType::Cilium.number(), 6);
        assert_eq!(OrganelleType::Oscillator.number(), 13);
        assert_eq!(OrganelleType::ReservedB.number(), 15);
        assert_eq!(OrganelleType::Empty.number(), 255);
        for (i, kind) in OrganelleType::all().iter().enumerate() {
            assert_eq!(kind.number() as usize, i);
        }
    }

    #[test]
    fn only_the_reserved_slots_are_unimplemented() {
        // The list is written out rather than derived, so that filling a `RESERVED` slot has to
        // be a deliberate edit here as well as in the mechanism.
        let unimplemented: Vec<&str> = OrganelleType::all()
            .iter()
            .filter(|k| !k.is_implemented())
            .map(|k| k.name())
            .collect();
        assert_eq!(unimplemented, vec!["reserved_b"]);
        let implemented: Vec<&str> = OrganelleType::all()
            .iter()
            .filter(|k| k.is_implemented())
            .map(|k| k.name())
            .collect();
        assert_eq!(
            implemented,
            vec![
                "membrane",
                "nucleus",
                "mitochondrion",
                "chloroplast",
                "vacuole",
                "pump",
                "cilium",
                "chemosensor",
                "photosensor",
                "touch sensor",
                "junction port",
                "lysosome",
                "spike",
                "oscillator",
                "holdfast",
            ]
        );
    }

    #[test]
    fn a_new_organelle_runs_without_having_to_be_switched_on() {
        // Otherwise a genome would need two mutations to gain one capability, and the second
        // would be invisible until the first existed.
        let o = Organelle::finished(OrganelleType::Mitochondrion, 100);
        assert_eq!(o.throttle(), Q10_ONE);
        assert_eq!(
            Organelle::building(OrganelleType::Chloroplast, 50, 9).throttle(),
            Q10_ONE
        );
        // and an empty slot has nothing to throttle
        assert_eq!(Organelle::empty().throttle(), 0);
    }

    #[test]
    fn a_partially_built_organelle_is_inert() {
        let mut o = Organelle::finished(OrganelleType::Chloroplast, 100);
        assert!(o.is_active());
        o.remaining_build = 3;
        assert!(o.is_present());
        assert!(!o.is_active(), "it must do nothing until it is finished");
    }

    #[test]
    fn every_metabolic_loop_closes() {
        // If one does not, matter conservation guarantees the world ends as an all-waste
        // equilibrium however good the cells are.
        assert!(MetabolicChemistry::default().closes());

        // A pathway that burns its own waste is a perpetual motion machine. It must be
        // refused wherever it sits in the set, not only in the first slot — an unclosed
        // reaction three pathways down is exactly as fatal and much harder to notice.
        for slot in 0..PATHWAY_COUNT {
            let mut chemistry = MetabolicChemistry::default();
            chemistry.pathways[slot].waste = chemistry.pathways[slot].substrate;
            assert!(
                !chemistry.closes(),
                "an unclosed loop in pathway {slot} was accepted"
            );
        }

        let mut out_of_range = MetabolicChemistry::default();
        out_of_range.pathways[2].reactive = CHEM_COUNT + 1;
        assert!(!out_of_range.closes());
    }

    #[test]
    fn a_control_word_always_names_a_real_pathway() {
        // Hard rule 4: addressing wraps. A genome can write any `i16` into `control[1]`, and
        // every one of them has to select a reaction rather than fail.
        let chemistry = MetabolicChemistry::default();
        for control in [0i16, 1, 3, 4, 255, -1, -4, i16::MIN, i16::MAX] {
            let n = MetabolicChemistry::pathway_index(control);
            assert!(n < PATHWAY_COUNT, "{control} selected pathway {n}");
            assert_eq!(chemistry.pathway(control), &chemistry.pathways[n]);
        }
        // Zero is the primary, so an organelle whose control was never written runs the
        // reaction the world has always run.
        assert_eq!(chemistry.pathway(0), chemistry.primary());
    }

    #[test]
    fn the_default_set_offers_more_than_one_way_to_make_a_living() {
        // The point of M10.3. If every pathway named the same substrate there would be a
        // choice in the type system and none in the world.
        let chemistry = MetabolicChemistry::default();
        assert!(
            chemistry.substrates().len() >= 3,
            "only {} distinct substrates: {:?}",
            chemistry.substrates().len(),
            chemistry.substrates()
        );
        // And they share an oxidant and a waste, which is what makes them alternatives
        // competing for one pool rather than four worlds that never meet.
        let first = chemistry.primary();
        for p in &chemistry.pathways {
            assert_eq!(p.oxidant, first.oxidant);
            assert_eq!(p.waste, first.waste);
        }
    }

    #[test]
    fn distinct_substrates_are_counted_once() {
        // What `recompute_stored` relies on for I5: two pathways sharing a substrate must not
        // let the world claim its latent energy twice.
        let mut chemistry = MetabolicChemistry::default();
        for p in chemistry.pathways.iter_mut() {
            p.substrate = 8;
        }
        assert_eq!(chemistry.substrates(), vec![8]);
    }

    #[test]
    fn costs_scale_with_param_and_never_overflow() {
        let cat = OrganelleCatalogue::balanced();
        for kind in *OrganelleType::all() {
            let spec = cat.spec(kind);
            let small = spec.matter_cost(0);
            let large = spec.matter_cost(255);
            assert!(large >= small, "{} got cheaper with size", kind.name());
            assert!(spec.upkeep_cost(255) >= spec.upkeep_cost(0));
        }
    }

    #[test]
    fn upkeep_counts_unfinished_organelles() {
        // A half-built mitochondrion is still matter the cell is carrying around.
        let cat = OrganelleCatalogue::balanced();
        let mut slots = [Organelle::empty(); SLOT_COUNT];
        slots[0] = Organelle::finished(OrganelleType::Membrane, 10);
        let bare = cat.upkeep(&slots);
        slots[1] = Organelle {
            remaining_build: 5,
            ..Organelle::finished(OrganelleType::Nucleus, 40)
        };
        assert!(cat.upkeep(&slots) > bare);
    }

    #[test]
    fn membrane_readings_cover_the_scalars_then_the_chemistry() {
        assert_eq!(MembraneReading::decode(0), MembraneReading::Mass);
        assert_eq!(MembraneReading::decode(4), MembraneReading::Damage);
        assert_eq!(MembraneReading::decode(5), MembraneReading::Chemical);
        assert_eq!(MembraneReading::chemical_of(5), 0);
        assert_eq!(MembraneReading::chemical_of(20), 15);
        // and every operand a genome can produce decodes to something
        for idx in i16::MIN..=i16::MAX {
            let r = MembraneReading::decode(idx);
            if r == MembraneReading::Chemical {
                assert!(MembraneReading::chemical_of(idx) < CHEM_COUNT);
            }
        }
    }
}
