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

/// Organelle slots per cell. Addressed `slot % SLOT_COUNT` (SPEC §6.2).
///
/// How many organelles one cell may hold. Nothing to do with how many *kinds* there are — see
/// [`CATALOGUE_SIZE`], which this was doing double duty for until they were separated.
///
/// Measured across every route in `benches/routes.rs`, cells run **25–35% occupied**: 128 bytes
/// of slot per cell of which some ninety-six are empty. So this is the number that is
/// over-provisioned, and it is the one that should not grow.
pub const SLOT_COUNT: usize = 16;

/// How many organelle *types* the catalogue defines. Type operands wrap `ty % CATALOGUE_SIZE`.
///
/// Separate from [`SLOT_COUNT`], which it was folded into until now — they are independent
/// quantities that happened to be equal, and `docs/FEEDING.md` §8 asks for exactly this split
/// whether or not the catalogue is ever widened, because it "makes §6's decision a one-constant
/// edit rather than a refactor".
///
/// The two want to move in opposite directions. Slots per cell are a quarter full; the catalogue
/// is *exhausted* — the holdfast took 14 at ISA 3 and the shell took 15 at ISA 6, and there is no
/// reserved entry left. Widening this alone costs one array per world of about half a kilobyte
/// and nothing per cell.
///
/// **A change here is an ISA bump** (hard rule 8). It renumbers nothing, but it changes what an
/// out-of-range operand means: `BUILD 19` reduces to the chloroplast at sixteen types and would
/// name type 19 at thirty-two. Mutation produces such operands constantly, so archived genomes
/// have to be replayed under their stamped version — which `genome_file.rs` already enforces.
pub const CATALOGUE_SIZE: usize = 32;

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
    /// A mineral test: armour, paid for in matter and in shade.
    ///
    /// The catalogue's first *defence*. Until it existed a spike met either bare membrane or
    /// nothing, so predation was free or worthless with nothing between the two and no arms race
    /// was possible from either end — `docs/FEEDING.md` §4 measures the predator's side of that
    /// and this is the prey's.
    ///
    /// It costs twice over, which is what keeps it a strategy rather than an upgrade every
    /// lineage grows. Matter to build, as everything does — and **shade**: a shell is opaque, so
    /// the light reaching the chloroplasts under it is reduced by exactly the fraction of the
    /// body it covers. Armour and photosynthesis are therefore rival, and a cell that wants both
    /// has to choose how much of each, on one control word, with no threshold anywhere in it.
    Shell = 15,
    // ---- the upper half: `n + 16` is the same job done a different way ----
    //
    // Laid out so that bit 4 of a type operand *means* something. A copy error is a single bit
    // flip, so `cilium` and `flagellum` are one mutation apart and evolution can hill-climb
    // between stirring and swimming rather than having to find it. Without the pairing, flipping
    // bit 4 would simply turn a working organelle into a no-op — one flip in eight on every type
    // byte in every genome — which is the cost `docs/FEEDING.md` §6 identifies and this layout is
    // the answer to.
    //
    // A `Reserved` entry up here therefore means "this organ has no variant yet", which is a
    // meaningful reservation rather than filler.
    Reserved16 = 16,
    Reserved17 = 17,
    /// Pairs with [`OrganelleType::Mitochondrion`]: the engine oxygen drives, and the engine
    /// oxygen stops.
    ///
    /// Turns dissolved nitrogen into body. Inhibited by local oxidant, which is what makes an
    /// anoxic corner worth living in and gives the shell's impermeability a second job.
    Diazosome = 18,
    /// Pairs with [`OrganelleType::Chloroplast`]: a producer that needs no light.
    ///
    /// The same reaction a chloroplast runs, driven by a reduced mineral instead of the light
    /// field. The only entry in the catalogue that can make a living in the dark, and the reason
    /// `the_vent` and `the_black_smoker` exist.
    Chemosynth = 19,
    /// Pairs with [`OrganelleType::Vacuole`]: a store, and a denser store.
    ///
    /// Holds energy above the ceiling a cell's membrane otherwise sets, so a lineage can carry a
    /// surplus through a night or a famine instead of spending it or losing it.
    LipidDroplet = 20,
    Reserved21 = 21,
    /// Pairs with [`OrganelleType::Cilium`]: a cilium stirs, a flagellum propels.
    ///
    /// The honest difference is where the thrust goes — more into the body and less into the
    /// water. A ciliate anchored on a holdfast pumps its own square and filter-feeds on it
    /// (`tests/ciliary_probe.rs`); a flagellate goes somewhere.
    Flagellum = 22,
    /// Pairs with [`OrganelleType::Chemosensor`]: taste a chemical, or taste the water itself.
    ///
    /// Reads pH — the ratio of carbonate to dissolved CO₂ at the square (`chem::ph_of`) — and its
    /// two gradients, on the same `index % 3` its sibling uses. One bit-flip retunes a lineage
    /// from tasting a substance to tasting acidity.
    ///
    /// **Without it the carbonate swing would select on lineages that cannot act on it**, which
    /// is a pressure with no strategy behind it. With it, "swim away from the acid" and "build
    /// armour only where the water is sweet" are both reachable by mutation.
    PhSensor = 23,
    Reserved24 = 24,
    Reserved25 = 25,
    Reserved26 = 26,
    Reserved27 = 27,
    /// Pairs with [`OrganelleType::Spike`]: stab it, or dissolve it.
    ///
    /// Digests a neighbour from the outside, into the square rather than into the digester — a
    /// leaky public good, and the answer to prey too large to swallow.
    Exoenzyme = 28,
    Reserved29 = 29,
    Reserved30 = 30,
    /// Pairs with [`OrganelleType::Shell`]: a test of glass, or a test of limestone.
    ///
    /// The same armour, made of the other mineral and on opposite terms. Silica is dear, slow and
    /// **pH-indifferent**; calcite is cheap, quick and **dissolves in acid** — so a calcite-shelled
    /// cell in a crowded respiring mat is paying for its neighbours' CO₂, and the same cell in
    /// bright open water is armoured for nearly nothing. Neither dominates, which is the test of
    /// whether a sibling was worth a slot.
    ///
    /// It has to be a sibling rather than an option on the shell's recipe, because
    /// [`OrganelleSpec::build_trace`] is an **AND**: every non-zero entry is required, charged and
    /// refunded together, so calcium beside silicon would make a shell needing both. See
    /// `docs/CHEMISTRY.md` §11.
    CalciteShell = 31,
}

const CATALOGUE: [OrganelleType; CATALOGUE_SIZE] = [
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
    OrganelleType::Shell,
    OrganelleType::Reserved16,
    OrganelleType::Reserved17,
    OrganelleType::Diazosome,
    OrganelleType::Chemosynth,
    OrganelleType::LipidDroplet,
    OrganelleType::Reserved21,
    OrganelleType::Flagellum,
    OrganelleType::PhSensor,
    OrganelleType::Reserved24,
    OrganelleType::Reserved25,
    OrganelleType::Reserved26,
    OrganelleType::Reserved27,
    OrganelleType::Exoenzyme,
    OrganelleType::Reserved29,
    OrganelleType::Reserved30,
    OrganelleType::CalciteShell,
];

impl OrganelleType {
    /// Decode a `BUILD` type operand. Total: the operand wraps into the catalogue.
    #[inline(always)]
    #[must_use]
    pub const fn from_operand(ty: i16) -> OrganelleType {
        CATALOGUE[(ty as u16 as usize) % CATALOGUE_SIZE]
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
    /// The lower sixteen are all implemented: M2 brought the metabolic types, M3 the sensors and
    /// the cilium, M7 the junction port, M8 the lysosome and the spike, §17.1 the holdfast, and
    /// the shell took the last one at ISA 6.
    ///
    /// The upper sixteen are the variants — see the enum's own note on the `n + 16` pairing — and
    /// most of them are still `Reserved`, which up here means "this organ has no variant yet"
    /// rather than "this number is spare".
    ///
    /// The pump is the exception in the other direction: a type with a number, a name and a
    /// catalogue entry that nothing reads. Declared by SPEC §6.2 and unimplemented, which is not
    /// the same as reserved.
    #[inline]
    #[must_use]
    pub const fn is_implemented(self) -> bool {
        !matches!(
            self,
            OrganelleType::Empty
                | OrganelleType::Reserved16
                | OrganelleType::Reserved17
                | OrganelleType::Reserved21
                | OrganelleType::Reserved24
                | OrganelleType::Reserved25
                | OrganelleType::Reserved26
                | OrganelleType::Reserved27
                | OrganelleType::Reserved29
                | OrganelleType::Reserved30
        )
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
            OrganelleType::Shell => "shell",
            OrganelleType::CalciteShell => "calcite shell",
            OrganelleType::PhSensor => "pH sensor",
            OrganelleType::Diazosome => "diazosome",
            OrganelleType::Chemosynth => "chemosynthetic granule",
            OrganelleType::LipidDroplet => "lipid droplet",
            OrganelleType::Flagellum => "flagellum",
            OrganelleType::Exoenzyme => "exoenzyme vesicle",
            OrganelleType::Reserved16 => "reserved_16",
            OrganelleType::Reserved17 => "reserved_17",
            OrganelleType::Reserved21 => "reserved_21",
            OrganelleType::Reserved24 => "reserved_24",
            OrganelleType::Reserved25 => "reserved_25",
            OrganelleType::Reserved26 => "reserved_26",
            OrganelleType::Reserved27 => "reserved_27",
            OrganelleType::Reserved29 => "reserved_29",
            OrganelleType::Reserved30 => "reserved_30",
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
    pub const fn all() -> &'static [OrganelleType; CATALOGUE_SIZE] {
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
            control: default_control(kind),
        }
    }

    /// An organelle under construction, at full throttle for when it finishes.
    #[must_use]
    pub const fn building(kind: OrganelleType, param: u8, ticks: u16) -> Organelle {
        Organelle {
            kind,
            param,
            remaining_build: ticks,
            control: default_control(kind),
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

    /// How hard this organelle is being worked, `Q10`, regardless of which way.
    ///
    /// What [`OrganelleSpec::upkeep_throttled`] is charged against, and **not** the same as
    /// [`Organelle::throttle`] — the difference is a cilium, and it is not academic.
    /// [`crate::sensing::cilium_power`] clamps to `-Q10..=Q10` because a beat has a direction:
    /// a cilium at `-Q10_ONE` is driving at full power in reverse. `throttle` reports that as
    /// zero, so pricing upkeep on it would have let a cell swim backwards for nothing, which is
    /// the same shape of bug as pricing a chemosensor by which chemical it watches. Magnitude is
    /// what a bill wants; sign is what the propulsion wants; they are different questions asked
    /// of one word.
    ///
    /// Everything else is one-sided already and gets the plain clamp.
    #[inline]
    #[must_use]
    pub fn effort(&self) -> i32 {
        match self.kind {
            OrganelleType::Cilium | OrganelleType::Flagellum => {
                (self.control[0] as i32).clamp(-Q10_ONE, Q10_ONE).abs()
            }
            _ => self.throttle(),
        }
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
    /// Energy per tick to keep it, at `param == 0`, in [`UPKEEP_SCALE`]ths of `Q10`.
    pub upkeep: i32,
    /// Extra upkeep per unit of `param`, in [`UPKEEP_SCALE`]ths of `Q10`.
    pub upkeep_per_param: i32,
    /// How much of this type's upkeep follows `control[0]`, `Q10`.
    ///
    /// The dormancy dial. An organelle turned down costs less to keep, so a cell in the dark can
    /// halve its burn by closing the machinery it cannot use — which is what makes sleeping
    /// through a night an evolvable strategy rather than a slow death (SPEC §5). The rest of the
    /// bill is basal and is paid whatever the control says: a closed organelle is still protein a
    /// cell keeps folded, and if carrying were free there would be no pressure against carrying
    /// sixteen of everything.
    ///
    /// **Zero — the `Default` — is exactly the old behaviour**, and that polarity is load-bearing
    /// three times over. A scenario that names some fields of a spec and omits this one is
    /// unchanged; the `Empty` slot's `Default` is unchanged; and a *snapshot* written before this
    /// field existed loads as a record of the catalogue it was written under, which is what hard
    /// rule 7 asks for. At the other end the arithmetic is an identity — a control at full
    /// throttle pays `upkeep + upkeep_per_param * param` exactly, with no rounding to argue about
    /// — so every genome in `genomes/`, which leaves its metabolic controls wide open, is priced
    /// today as it was yesterday by construction rather than by measurement.
    ///
    /// # This may only be non-zero where `control[0]` actually gates the service
    ///
    /// The rule, and the reason this is per-type data rather than one formula in the upkeep
    /// block: **`control[0]` is not a throttle everywhere.** It is four different things.
    ///
    /// * A *throttle* on the mitochondrion, chloroplast, chemosynthetic granule, lysosome and
    ///   diazosome — [`crate::metabolism::Metabolism::capacity_by_pathway`],
    ///   [`crate::ecology::digestive_capacity_by_pathway`] and
    ///   [`crate::biology::fixation_capacity`] all scale their output by it. Closing one buys the
    ///   discount and surrenders the work, which is the trade this field is for.
    /// * An *effort or extension* on the spike, holdfast, exoenzyme, cilium, flagellum and shell.
    ///   Same bargain, and they already pay separately for the work itself in
    ///   [`crate::ecology`], so their catalogue line is more carrying than doing.
    /// * A **selector** on the chemosensor and the pH sensor (*which chemical*), the oscillator
    ///   (*phase*), the nucleus (*copy fidelity*) and the membrane (*permeability*). This is why
    ///   a blanket rule would have been a bug rather than a balance question:
    ///   [`crate::sensing::chem_index`] reads a raw chemical index, so a sensor watching chemical
    ///   3 carries `control[0] == 3`, which as a `Q10` fraction is three parts in a thousand. It
    ///   would have slept for free, priced by which chemical it happened to watch.
    /// * *Unread* on the vacuole, the photosensor, the touch sensor, the junction port and the
    ///   lipid droplet — nothing to surrender, so a discount here is a discount for nothing.
    ///
    /// The vacuole is the one worth naming, because it looks throttleable and is not. Its two
    /// services — [`crate::biology::sequestered`] and the room it adds in
    /// [`crate::biology::interior_capacity`] — read `param` and ignore `control[0]` entirely, so
    /// a closed vacuole would keep its solute out of the turgor reckoning and keep its cytoplasm
    /// and pay less for both. Making it honest means gating *both* of those on the control
    /// together, which is a change to what every cell can hold and does not belong in this one.
    ///
    /// [`THROTTLEABLE`] states the answer as a table and `the_dormancy_dial_is_only_set_where_the_control_gates_the_service`
    /// checks the catalogue against it, so switching one on later is a deliberate act.
    pub upkeep_throttled: i32,
    /// Fraction of its structural matter recovered by `TEAR`, `Q10`. The rest is lost to the
    /// fluid as waste — dismantling is not free, or a cell would rebuild itself every tick.
    pub teardown_recovery: i32,
    /// What else it takes to build one, per chemical, `Q10` at `param == 0`.
    ///
    /// The recipe. Until this existed an organelle cost one number of one chemical — whatever
    /// `MetabolicChemistry::structural` names — so the table's other three monomers were flagged
    /// `structural: true` and could not be built from, and there was no way for a type to need
    /// something the rest of the catalogue did not.
    ///
    /// **This matter is not turned into mass.** Structural matter becomes `cells.mass` and comes
    /// back out as the structural chemical when the cell dies; a trace cost stays what it is, is
    /// held *in the organelle*, and is returned as itself. That distinction is the whole reason
    /// the field exists rather than being folded into `build_matter`: routing silicon through
    /// mass would hand it back as carbon, which is the one-way conversion `carrion`'s decay used
    /// to be and which took a population of twelve thousand down to a hundred and ninety.
    ///
    /// It scales with `param` the way everything else does — see [`OrganelleSpec::trace_cost`] —
    /// and `total_matter` counts it, so a body under construction has not made anything vanish.
    ///
    /// All zeroes by default, so a catalogue that says nothing behaves exactly as it did.
    pub build_trace: [i32; crate::chem::CHEM_COUNT],
}

/// What an organelle's control words start at, by type.
///
/// **One constant used to serve every type**, and it was `[Q10_ONE, 0]` — the first word wide
/// open. That is right for a throttle and wrong for anything that acts on the world, and the
/// difference was not academic: putting engulfment's appetite on the vacuole's first word turned
/// every vacuole in `genomes/` into a mouth, and `m2_life::selection_guard` caught it by watching
/// the tidy strain's advantage collapse. The membrane's own note has been warning about the same
/// trap for milestones — it is why `permeability` was left unimplemented rather than done
/// quickly, because a permeability control at full throttle means *wide open*.
///
/// The rule, and it is the whole of the design here:
///
/// * a control that **acts on the world or spends energy doing so** starts at zero. An organelle
///   a genome has not wired up is a cost it is carrying, not a free action it is taking — which
///   is also exactly the premise of M3's chemotaxis experiment, where `drifter.mm` carries every
///   part it needs and the only thing missing is four instructions connecting them.
/// * everything else keeps the throttle open. A mitochondrion that has to be switched on before
///   it burns anything is a mitochondrion nobody builds, and a cell that has to discover its own
///   metabolism before it can respire does not get a second tick.
///
/// Every shipped genome sets the controls it cares about explicitly — checked, not assumed — so
/// this changes nothing any archetype does. What it changes is what a *mutation* gets for free.
#[must_use]
pub const fn default_control(kind: OrganelleType) -> [i16; 2] {
    match kind {
        OrganelleType::Cilium
        | OrganelleType::Flagellum
        | OrganelleType::Spike
        | OrganelleType::Holdfast
        | OrganelleType::Shell
        | OrganelleType::CalciteShell
        | OrganelleType::Exoenzyme => [0, 0],
        _ => [Q10_ONE as i16, 0],
    }
}

/// A recipe that asks for nothing but structural matter — what every entry had before recipes
/// existed, and what all but the shell still say.
pub const NO_TRACE: [i32; crate::chem::CHEM_COUNT] = [0; crate::chem::CHEM_COUNT];

/// Nitrogen, as a recipe of `n` `Q10` units and nothing else.
///
/// # The stoichiometry is not an approximation of the biology, it *is* the biology
///
/// Organisms hold carbon, nitrogen and phosphorus at a fairly rigid ratio — roughly 106 : 16 : 1,
/// the Redfield ratio — and they do so because that is the composition of the *machinery*, not
/// because anything prefers it. Nitrogen sits in proteins; phosphorus sits in nucleic acids;
/// carbon is the bulk and the energy store. [`OrganelleSpec::build_trace`] is a stoichiometry
/// table, so writing those proportions into it is not a model of the ratio, it is the thing the
/// ratio describes.
///
/// So the costs below are 16/106 of each type's carbon for nitrogen and 1/106 for phosphorus,
/// and they are put where the biology puts them: nitrogen on the enzymatic machinery, phosphorus
/// on the nucleus, silicon on the shell.
///
/// **What this does not settle is whether any of them binds.** A requirement is not a scarcity —
/// real phosphorus limits ecosystems because the *supply* is minute, not because the requirement
/// is large — and §6's lesson is that the level has to be swept rather than guessed. These
/// numbers say what a body is made of; what a world holds is a separate question.
#[must_use]
pub const fn nitrogen_trace(n: i32) -> [i32; crate::chem::CHEM_COUNT] {
    let mut r = NO_TRACE;
    r[NITROGEN] = n;
    r
}

/// Chemical 5, the monomer proteins are built from. See [`nitrogen_trace`].
pub const NITROGEN: usize = 5;
/// Chemical 6, the monomer a nucleus is built from.
pub const PHOSPHORUS: usize = 6;
/// Chemical 7, the mineral a shell is built from.
pub const SILICON: usize = 7;

impl OrganelleSpec {
    /// Structural matter to build one at a given size.
    #[inline]
    #[must_use]
    pub fn matter_cost(&self, param: u8) -> i32 {
        self.build_matter
            .saturating_add(self.build_matter_per_param.saturating_mul(param as i32))
    }

    /// How much of one trace chemical it takes to build one at a given size, `Q10`.
    ///
    /// Doubles across the `param` range, which is the same shape `matter_cost` has: a bigger
    /// organelle needs proportionally more of everything it is made of.
    #[inline]
    #[must_use]
    pub fn trace_cost(&self, c: usize, param: u8) -> i32 {
        let base = self.build_trace[c % crate::chem::CHEM_COUNT];
        if base == 0 {
            return 0;
        }
        base.saturating_add(base.saturating_mul(param as i32) / 255)
    }

    /// Whether this type costs anything beyond structural matter.
    #[inline]
    #[must_use]
    pub fn has_trace(&self) -> bool {
        self.build_trace.iter().any(|v| *v != 0)
    }

    /// Energy per tick to keep one at a given size, running flat out, `Q10`.
    ///
    /// The full bill, which is what every caller outside the upkeep block wants: what this
    /// organelle costs to have. [`OrganelleSpec::upkeep_cost_at`] is the same thing asked about
    /// an organelle that is idling.
    #[inline]
    #[must_use]
    pub fn upkeep_cost(&self, param: u8) -> i32 {
        self.upkeep_cost_at(param, Q10_ONE)
    }

    /// Energy per tick to keep one at a given size and a given [`Organelle::effort`], `Q10`.
    ///
    /// `basal + variable * effort`, where the split is [`OrganelleSpec::upkeep_throttled`] of the
    /// whole. At `effort == Q10_ONE` this is `basal + variable`, which is the whole, so the full
    /// bill is an identity rather than a rounding of one — see the field's note for why that
    /// matters more than it looks.
    #[inline]
    #[must_use]
    pub fn upkeep_cost_at(&self, param: u8, effort: i32) -> i32 {
        // Summed in the fine unit and divided once, so `upkeep_per_param` keeps its resolution
        // all the way through: five of the catalogue's eight entries used to sit at 1 `Q10`,
        // which is a rate that cannot be lowered without switching the mechanism off. The
        // throttle split happens up here for the same reason — a quarter of a coarse bill of 2
        // is 0, and a quarter of the fine 41 behind it is 10.
        let fine = self
            .upkeep
            .saturating_add(self.upkeep_per_param.saturating_mul(param as i32));
        let dial = self.upkeep_throttled.clamp(0, Q10_ONE);
        if dial == 0 {
            return fine / UPKEEP_SCALE;
        }
        let variable = crate::fixed::q10_scale(fine, dial);
        let basal = fine.saturating_sub(variable);
        basal.saturating_add(crate::fixed::q10_scale(variable, effort.clamp(0, Q10_ONE)))
            / UPKEEP_SCALE
    }
}

/// What [`OrganelleSpec::upkeep`] and [`OrganelleSpec::upkeep_per_param`] are denominated in.
///
/// The catalogue's upkeep is written as `q10(1) / N` for a smallish `N`, which put five of the
/// eight entries' `upkeep_per_param` at exactly **1** — the integer floor. A bill that cannot be
/// halved without reaching zero is not a bill that can be balanced, and `docs/ECONOMY.md` is
/// largely an argument about that column.
///
/// Sixteenths, matching [`crate::sensing::THRUST_PER_PARAM`] and
/// `MetabolicRates::throughput_per_param`. Multiplying every catalogue value by sixteen and
/// dividing once at the end is exact for the values that were there — `(16a + 16bp)/16 == a + bp`
/// — so this unit change on its own moves nothing. What it buys is four more halvings of headroom
/// before the floor, which is what the tempo work in this commit spends one of.
const UPKEEP_SCALE: i32 = 16;

/// How much of a throttleable organelle's upkeep the throttle can reach, `Q10`.
///
/// Three quarters, so a quarter is basal. The quarter is the point as much as the three: a cell
/// that could park sixteen closed organelles for nothing would face no pressure against carrying
/// every capability it might one day want, and `docs/ECONOMY.md` §12.1 — 818 cells at four
/// organelles, 666 at six, none at all at eight — is a measurement of that pressure working.
/// Dormancy should change the slope of the bill, not repeal it.
///
/// A number to be settled by `mm_core::balance` rather than by argument, which is why it is named
/// here instead of written out five times in the catalogue below.
const DORMANT_SHARE: i32 = Q10_ONE * 3 / 4;

/// The organelle types whose upkeep may follow `control[0]`, and the whole of the audit behind
/// [`OrganelleSpec::upkeep_throttled`].
///
/// Five, and every one of them scales its *output* by the same word: closing one is a real
/// surrender, not a discount. That is the property the field's note requires and the one this
/// table exists to pin down, because it cannot be checked mechanically — nothing in the type
/// system knows that `capacity_by_pathway` reads `control[0]`.
///
/// **The reaching organelles are deliberately absent**, and they are the near miss. A spike, a
/// holdfast, a cilium and an exoenzyme all gate their work on `control[0]` too, so they pass the
/// rule. What they do not pass is the question this change is answering. They are already
/// retracted when idle — `default_control` starts them at zero — so putting them here would not
/// help a sleeping cell at all; it would hand a permanent discount to every armed cell that is
/// not currently stabbing. Whether a sheathed spike should be cheap to carry is a real question
/// and `crates/mm-core/src/ecology.rs` has an opinion about it, but it is a question about
/// ambush, not about night, and answering both in one commit would make the balance harness's
/// before-and-after unreadable.
pub const THROTTLEABLE: [OrganelleType; 5] = [
    OrganelleType::Mitochondrion,
    OrganelleType::Chloroplast,
    OrganelleType::Chemosynth,
    OrganelleType::Lysosome,
    OrganelleType::Diazosome,
];

/// The costs and capabilities of every catalogue entry.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OrganelleCatalogue {
    specs: [OrganelleSpec; CATALOGUE_SIZE],
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
            upkeep: q10(8) / 64,
            upkeep_per_param: q10(8) / 1024,
            upkeep_throttled: 0,
            teardown_recovery: Q10_ONE / 2,
            build_trace: NO_TRACE,
        };
        let mut specs = [cheap; CATALOGUE_SIZE];

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
            upkeep: q10(8) / 32,
            upkeep_per_param: q10(8) / 512,
            upkeep_throttled: 0,
            teardown_recovery: 0,
            build_trace: NO_TRACE,
        };
        // A nucleus is expensive to carry, which is what makes genome bloat cost something
        // (SPEC §4.1) without any rule saying so.
        specs[OrganelleType::Nucleus as usize] = OrganelleSpec {
            build_matter: q10(6),
            build_matter_per_param: q10(1) / 2,
            build_energy: q10(12),
            build_ticks: 12,
            upkeep: q10(8) / 32,
            upkeep_per_param: q10(8) / 256,
            upkeep_throttled: 0,
            teardown_recovery: Q10_ONE / 2,
            build_trace: {
                // Phosphorus, and only the nucleus takes it: a genome is a nucleic acid and
                // that is where the phosphate backbone is. One part in 106 of its carbon, which
                // is a tiny requirement — the scarcity, when it comes, will come from the
                // supply and from phosphorus being the one chemical that does not move.
                let mut r = NO_TRACE;
                r[PHOSPHORUS] = 58;
                r
            },
        };
        specs[OrganelleType::Mitochondrion as usize] = OrganelleSpec {
            build_matter: q10(5),
            build_matter_per_param: q10(1) / 8,
            build_energy: q10(10),
            build_ticks: 10,
            upkeep: q10(8) / 48,
            upkeep_per_param: q10(8) / 768,
            upkeep_throttled: DORMANT_SHARE,
            teardown_recovery: Q10_ONE / 2,
            build_trace: nitrogen_trace(773),
        };
        specs[OrganelleType::Chloroplast as usize] = OrganelleSpec {
            build_matter: q10(7),
            build_matter_per_param: q10(1) / 6,
            build_energy: q10(14),
            build_ticks: 14,
            upkeep: q10(8) / 40,
            upkeep_per_param: q10(8) / 640,
            upkeep_throttled: DORMANT_SHARE,
            teardown_recovery: Q10_ONE / 2,
            build_trace: nitrogen_trace(1082),
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
            upkeep: q10(8) / 24,
            upkeep_per_param: q10(8) / 384,
            upkeep_throttled: 0,
            teardown_recovery: Q10_ONE / 2,
            build_trace: NO_TRACE,
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
            upkeep: q10(8) / 48,
            upkeep_per_param: q10(8) / 768,
            upkeep_throttled: 0,
            teardown_recovery: Q10_ONE / 4,
            build_trace: NO_TRACE,
        };

        // A wall is expensive to raise and cheap to keep: nearly twice a holdfast's matter and
        // the slowest thing in the catalogue to finish, against an upkeep below a cilium's. That
        // shape is the point — armour is a commitment made in advance, not a running cost that
        // can be dropped the moment something bites. `teardown_recovery` is the lowest here for
        // the same reason: mineral put down does not come back up.
        specs[OrganelleType::Shell as usize] = OrganelleSpec {
            build_matter: q10(13),
            build_matter_per_param: q10(1) / 4,
            build_energy: q10(20),
            build_ticks: 28,
            upkeep: q10(8) / 96,
            upkeep_per_param: q10(8) / 1024,
            upkeep_throttled: 0,
            teardown_recovery: Q10_ONE / 8,
            // Silicon, and it is the only entry in the catalogue that asks for anything but
            // carbon. A test is mineral, and the table has carried silicon since the beginning
            // with nothing able to build from it — `docs/CHEMISTRY.md` §2 lists it among the
            // three monomers "flagged `structural: true`... nothing can be built out of them".
            //
            // What it buys is a second axis of competition. Carbon is contested by everything
            // alive; silicon is contested only by whatever is armoured, so a shelled lineage is
            // limited by something its prey is not spending, and depleting it locally is a
            // pressure that falls on one strategy rather than on the whole slide. That is the
            // argument `CHEMISTRY.md` §3 makes for a second structural chemical, and this is the
            // first thing to take it up.
            build_trace: {
                let mut r = NO_TRACE;
                r[7] = q10(6);
                r
            },
        };

        // --- the upper half ---
        //
        // Each priced against the entry it pairs with, because that is the comparison a genome is
        // actually making: bit 4 of the type operand is one mutation away, so these are not new
        // organs competing with the whole catalogue but variants competing with one sibling.

        // Dearer than a mitochondrion and slower to raise: fixing is expensive machinery, and
        // the whole point of it is that it is worth carrying only where the alternative is worse.
        specs[OrganelleType::Diazosome as usize] = OrganelleSpec {
            build_matter: q10(9),
            build_matter_per_param: q10(1) / 5,
            build_energy: q10(16),
            build_ticks: 22,
            upkeep: q10(8) / 40,
            upkeep_per_param: q10(8) / 640,
            upkeep_throttled: DORMANT_SHARE,
            teardown_recovery: Q10_ONE / 3,
            build_trace: nitrogen_trace(1391),
        };

        // A chloroplast's price, near enough. The two are alternatives and neither should win on
        // cost — which one pays is a question about the world, not about the catalogue.
        specs[OrganelleType::Chemosynth as usize] = OrganelleSpec {
            build_matter: q10(7),
            build_matter_per_param: q10(1) / 6,
            build_energy: q10(12),
            build_ticks: 18,
            upkeep: q10(8) / 44,
            upkeep_per_param: q10(8) / 700,
            upkeep_throttled: DORMANT_SHARE,
            teardown_recovery: Q10_ONE / 3,
            build_trace: nitrogen_trace(1082),
        };

        // Cheap to keep, which is the whole of what a store is for.
        specs[OrganelleType::LipidDroplet as usize] = OrganelleSpec {
            build_matter: q10(5),
            build_matter_per_param: q10(1) / 6,
            build_energy: q10(8),
            build_ticks: 12,
            upkeep: q10(8) / 128,
            upkeep_per_param: q10(8) / 2048,
            upkeep_throttled: 0,
            teardown_recovery: Q10_ONE / 2,
            build_trace: NO_TRACE,
        };

        // Dearer than a cilium and slower to build. A flagellum is one large organ where cilia
        // are many small ones, and the catalogue should say so before the physics does.
        specs[OrganelleType::Flagellum as usize] = OrganelleSpec {
            build_matter: q10(8),
            build_matter_per_param: q10(1) / 5,
            build_energy: q10(14),
            build_ticks: 18,
            upkeep: q10(8) / 40,
            upkeep_per_param: q10(8) / 512,
            upkeep_throttled: 0,
            teardown_recovery: Q10_ONE / 3,
            build_trace: NO_TRACE,
        };

        // About a spike, and it should be: they are the two ways of attacking a neighbour and the
        // choice between them is meant to be about the neighbour, not about the bill.
        specs[OrganelleType::Exoenzyme as usize] = OrganelleSpec {
            build_matter: q10(6),
            build_matter_per_param: q10(1) / 6,
            build_energy: q10(11),
            build_ticks: 14,
            upkeep: q10(8) / 48,
            upkeep_per_param: q10(8) / 768,
            upkeep_throttled: 0,
            teardown_recovery: Q10_ONE / 3,
            build_trace: nitrogen_trace(927),
        };

        // Scavenging is the cheaper trade and the lower-yield one: a lysosome costs about what
        // a mitochondrion costs, and what it digests has already been through someone else.
        specs[OrganelleType::Lysosome as usize] = OrganelleSpec {
            build_matter: q10(6),
            build_matter_per_param: q10(1) / 8,
            build_energy: q10(12),
            build_ticks: 12,
            upkeep: q10(8) / 56,
            upkeep_per_param: q10(8) / 896,
            upkeep_throttled: DORMANT_SHARE,
            teardown_recovery: Q10_ONE / 2,
            build_trace: nitrogen_trace(927),
        };

        // --- the sensors ---
        //
        // Protein, and costed as protein: they take the `cheap` default for everything else, so
        // this is the one line that distinguishes them. At `q10(4)` of carbon the Redfield share
        // is 618, which sits alongside the mitochondrion's 773 — not a rounding difference, and
        // that is the point of noting it here rather than folding it in silently.
        //
        // **Sweep this entry on its own.** Everything else carrying nitrogen is metabolic
        // machinery; the sensors are the cost of *perceiving at all*, and taxing them creates a
        // pressure the rest do not: blind fast breeders against perceptive slow ones, contested
        // inside the same sixteen slots. That is an axis worth having and it is also a confound —
        // if behaviour changes when nitrogen arrives, this is the entry that has to be ruled in
        // or out separately, because §6's lesson is that the one you did not measure alone is the
        // one that was two orders out.
        // A test of limestone: the shell's sibling, priced against *it* rather than against the
        // catalogue, because bit 4 of the type operand is one mutation away and that is the
        // comparison a genome is actually making.
        //
        // Cheaper and quicker on every axis that is about *laying it down* — limestone
        // precipitates where glass has to be spun — and identical on the two that are about
        // having laid it: upkeep, because a wall is cheap to keep whatever it is made of, and
        // `teardown_recovery`, because mineral put down does not come back up.
        //
        // The recipe is the trade. Silicon is scarce, immobile-ish and contested only by whatever
        // is armoured; calcium and carbonate are abundant, well mixed, and the carbonate half is
        // the same pool that buffers the water — so a lineage that armours itself in calcite is
        // drawing down its own neighbourhood's buffer, and a crowd of them makes the water that
        // dissolves them. Silica has no such loop and costs more for not having one.
        specs[OrganelleType::CalciteShell as usize] = OrganelleSpec {
            build_matter: q10(11),
            build_matter_per_param: q10(1) / 4,
            build_energy: q10(14),
            build_ticks: 18,
            upkeep: q10(8) / 96,
            upkeep_per_param: q10(8) / 1024,
            upkeep_throttled: 0,
            teardown_recovery: Q10_ONE / 8,
            build_trace: {
                let mut r = NO_TRACE;
                r[crate::chem::CALCIUM] = q10(4);
                r[crate::chem::CARBONATE] = q10(4);
                r
            },
        };

        for kind in [
            OrganelleType::Chemosensor,
            OrganelleType::Photosensor,
            OrganelleType::TouchSensor,
            // The fourth sensor, on the same terms as the other three — and it wants the same
            // caveat. Sensors are the cost of *perceiving at all*, taxing them is a distinct
            // pressure from taxing metabolism, and a fourth is a fourth call on the same sixteen
            // slots. If behaviour changes when the carbonate system lands, this entry has to be
            // ruled in or out separately.
            OrganelleType::PhSensor,
        ] {
            specs[kind as usize].build_trace = nitrogen_trace(618);
        }

        OrganelleCatalogue {
            specs,
            metabolism: MetabolicChemistry::default(),
        }
    }

    /// Every spec, in catalogue order. For serialisation (hard rule 7).
    #[must_use]
    pub fn specs(&self) -> &[OrganelleSpec; CATALOGUE_SIZE] {
        &self.specs
    }

    /// Replace every spec, in catalogue order. For restoring a snapshot.
    pub fn set_specs(&mut self, specs: [OrganelleSpec; CATALOGUE_SIZE]) {
        self.specs = specs;
    }

    #[inline(always)]
    #[must_use]
    pub fn spec(&self, kind: OrganelleType) -> &OrganelleSpec {
        match kind {
            OrganelleType::Empty => &self.specs[0],
            other => &self.specs[(other as u8 as usize) % CATALOGUE_SIZE],
        }
    }

    /// Total upkeep for a whole loadout, `Q10` energy per tick.
    ///
    /// Charged whether or not an organelle is finished: a half-built mitochondrion is still
    /// matter the cell is carrying around.
    #[must_use]
    /// A slice rather than `&[Organelle; CATALOGUE_SIZE]`, so that the caller can pass
    /// [`crate::cell::CellArena::slots`] straight in.
    ///
    /// The array form obliged the one hot caller — `metabolism::step`, once per cell per tick —
    /// to go through `CellArena::loadout`, which copies all sixteen organelles into a temporary
    /// so that a reference to a fixed-size array exists to take. That is 128 bytes memcpy'd per
    /// cell per tick, six megabytes at fifty thousand cells, to read a field from each and throw
    /// the copy away. A slice needs no copy and the loop below does not care about the length.
    pub fn upkeep(&self, slots: &[Organelle]) -> i32 {
        let mut total = 0i32;
        for o in slots {
            if o.is_present() {
                // At the effort it is actually running, which for every organelle in every
                // shipped genome is full and therefore the same number it has always been.
                //
                // **An unfinished one pays full price whatever its control says**, and that is
                // the one place the rule needs stating rather than following. Everywhere else the
                // bargain is self-enforcing: closing a throttle surrenders the output, so a cell
                // cannot idle and go on earning. A half-built organelle produces nothing either
                // way, so there is nothing to surrender and the discount would be free — small,
                // eight ticks of three quarters of one line, and a hole is a hole. Construction is
                // not something a cell gets to sleep through.
                let effort = if o.is_active() { o.effort() } else { Q10_ONE };
                total = total.saturating_add(self.spec(o.kind).upkeep_cost_at(o.param, effort));
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
    /// `5..=(4 + CHEM_COUNT)` read internal chemical `idx - 5`.
    ///
    /// **The discriminants below are labels, not wire indices.** `decode` places `Badge` at
    /// `5 + CHEM_COUNT` and `Crowding` at `6 + CHEM_COUNT`, so where they actually sit moves every
    /// time the table grows — 21 and 22 when it held sixteen, 22 and 23 at ISA 11, 24 and 25 at
    /// ISA 12. A genome reading its own badge or its own crowding at a hard-coded index reads
    /// something else afterwards, which is exactly what the version stamp is for (hard rule 8) and
    /// why archived genomes replay under the version they evolved in. This comment said `5..=20`
    /// for two ISA versions after that stopped being true.
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
    /// How hard this cell is being pressed on by its neighbours, `Q10` of a radius.
    ///
    /// Appended after the badge for the same reason the badge was appended after the chemicals:
    /// it renumbers nothing, and only the indices that used to wrap change meaning.
    ///
    /// The number was already there. `neighbours::resolve_collisions` computes crowding every
    /// tick and it drives real consequences — `split_pressure` refuses a division to a cell with
    /// nowhere to put the daughter, and `crowding_damage` charges for being squeezed — but until
    /// now nothing could *read* it. SPEC §17.8 says being buried is the best place to be, and no
    /// cell on the slide could tell whether it was buried.
    ///
    /// That asymmetry is the whole reason this is worth a reading. A pressure a cell suffers and
    /// cannot sense is weather; one it can sense is a reason to move, to stop dividing, to grow a
    /// holdfast, or to stay exactly where it is.
    Crowding = 22,
}

impl MembraneReading {
    /// Decode an `OGET` index operand. Total for any input.
    ///
    /// The sixteen chemical readings sit immediately after the five scalars, so a genome that
    /// walks the index space finds its own chemistry rather than falling off the end.
    #[inline]
    #[must_use]
    pub fn decode(idx: i16) -> MembraneReading {
        match (idx as u16 as usize) % (7 + CHEM_COUNT) {
            0 => MembraneReading::Mass,
            1 => MembraneReading::Energy,
            2 => MembraneReading::Age,
            3 => MembraneReading::Radius,
            4 => MembraneReading::Damage,
            n if n == 5 + CHEM_COUNT => MembraneReading::Badge,
            n if n == 6 + CHEM_COUNT => MembraneReading::Crowding,
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

/// Shell per unit of `param`, `Q10` of the body covered.
///
/// Sized so that a single full-size shell at full closure covers rather less than half a body: a
/// cell that wants to be sealed has to spend several slots on it, which is the trade the slot
/// exists to pose. Coverage is capped below one in [`shell_cover`] whatever is built.
pub const SHELL_PER_PARAM: i32 = Q10_ONE / 640;

/// The most of a body any amount of shell can cover, `Q10`.
///
/// Seven eighths, and the missing eighth is not a rounding allowance. Total immunity is a cliff
/// of exactly the kind SPEC §3 keeps out of the landscape — above it nothing a predator evolves
/// matters at all, so the arms race stops having a gradient to climb from either end. It also
/// keeps a shelled cell answerable to crowding, which is what stops a sealed lineage simply
/// tiling the slide.
pub const SHELL_MAX_COVER: i32 = Q10_ONE * 7 / 8;

/// How much of a body one shell covers, `Q10`, before the whole-cell cap.
///
/// The same `control[0]` closes the shell and shades the cell beneath it, deliberately, and for
/// the same reason the holdfast's one word both grips and strains: it is one surface doing one
/// thing. A genome that wants the light back opens up, and opening up is what a spike is for.
///
/// Uncapped, unlike [`shell_cover`]: [`SHELL_MAX_COVER`] is a limit on the *cell*, not on a slot,
/// and applying it here would let two half-shells cover more than the pair of them can.
#[must_use]
pub fn shell_cover_of(o: &Organelle) -> i32 {
    if !matches!(
        o.kind,
        OrganelleType::Shell | OrganelleType::CalciteShell
    ) || !o.is_active()
    {
        return 0;
    }
    let closed = (o.control[0] as i32).clamp(0, Q10_ONE);
    crate::fixed::q10_scale(SHELL_PER_PARAM.saturating_mul(o.param as i32), closed)
}

/// How much of a cell is behind a shell, `Q10`.
#[must_use]
pub fn shell_cover(cells: &crate::cell::CellArena, i: usize) -> i32 {
    let mut cover = 0i32;
    for o in cells.slots(i) {
        cover = cover.saturating_add(shell_cover_of(o));
    }
    cover.clamp(0, SHELL_MAX_COVER)
}

/// What survives a shell, `Q10` of the damage offered to it.
#[inline]
#[must_use]
pub fn shell_admits(cover: i32) -> i32 {
    Q10_ONE.saturating_sub(cover.clamp(0, SHELL_MAX_COVER))
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
        // 19 reduced to the chloroplast until ISA 7 and now names its pair. That change of
        // meaning is what the version stamp on an archived genome is for — mutation produces
        // out-of-range type operands constantly, so this is the widest-reaching consequence of
        // the widening even though it takes nothing away.
        assert_eq!(OrganelleType::from_operand(19), OrganelleType::Chemosynth);
        assert_eq!(OrganelleType::from_operand(35), OrganelleType::Chloroplast);
        assert_eq!(OrganelleType::from_operand(-29), OrganelleType::Chloroplast);
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
        assert_eq!(OrganelleType::Shell.number(), 15);
        assert_eq!(OrganelleType::Empty.number(), 255);
        for (i, kind) in OrganelleType::all().iter().enumerate() {
            assert_eq!(kind.number() as usize, i);
        }
    }

    #[test]
    fn every_reservation_is_in_the_upper_half_and_means_something() {
        // The lower sixteen are the organs and every one of them does something. The upper
        // sixteen are *variants* — see the enum's note on the `n + 16` pairing — and a `Reserved`
        // entry up there means "this organ has no variant yet", which is a real statement rather
        // than filler.
        //
        // So the invariant is not "nothing is reserved". It is that nothing in the lower half
        // ever becomes reserved again: a future change that quietly re-reserved an organ to make
        // room for itself would break every archived genome that built one, which is what
        // `drifter_blind.mm` was bitten by twice.
        let lower_unimplemented: Vec<&str> = OrganelleType::all()
            .iter()
            .take(16)
            .filter(|k| !k.is_implemented())
            .map(|k| k.name())
            .collect();
        assert!(
            lower_unimplemented.is_empty(),
            "the lower half has {lower_unimplemented:?} unimplemented; those numbers are spoken \
             for by archived genomes and cannot be handed back"
        );
        // And every reservation that does exist is a variant slot, named so it cannot be mistaken
        // for an organ somebody forgot to write.
        for kind in OrganelleType::all().iter().skip(16) {
            if !kind.is_implemented() {
                assert!(
                    kind.name().starts_with("reserved_"),
                    "{kind:?} is unimplemented but not named as a reservation"
                );
            }
        }
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
                "shell",
                "diazosome",
                "chemosynthetic granule",
                "lipid droplet",
                "flagellum",
                "pH sensor",
                "exoenzyme vesicle",
                "calcite shell",
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
    fn the_dormancy_arithmetic_is_total() {
        // Hard rule 3. `param` is bounded by its type but `effort` is `control[0]`, which a genome
        // writes with `OSET` and can therefore make any `i16`; and a ruleset can put any `i32` in
        // any catalogue column. Every product here goes through `q10_mul`, which promotes to
        // `i64` and saturates, so this is a check that nothing was added outside it.
        for upkeep in [i32::MIN, -1, 0, 1, 16, i32::MAX] {
            for per in [i32::MIN, 0, 1, i32::MAX] {
                for dial in [i32::MIN, 0, 1, Q10_ONE, Q10_ONE * 4, i32::MAX] {
                    let spec = OrganelleSpec {
                        upkeep,
                        upkeep_per_param: per,
                        upkeep_throttled: dial,
                        ..OrganelleSpec::default()
                    };
                    for param in [0u8, 1, 128, 255] {
                        for effort in [i32::MIN, -1, 0, 1, Q10_ONE, i32::MAX] {
                            let _ = spec.upkeep_cost_at(param, effort);
                        }
                    }
                }
            }
        }
        // And `effort` itself, over every control word a genome can write.
        for kind in [OrganelleType::Cilium, OrganelleType::Mitochondrion] {
            for control in [i16::MIN, -1, 0, 1, i16::MAX] {
                let mut o = Organelle::finished(kind, 200);
                o.control[0] = control;
                let e = o.effort();
                assert!(
                    (0..=Q10_ONE).contains(&e),
                    "{} {control} -> {e}",
                    kind.name()
                );
            }
        }
    }

    #[test]
    fn a_control_at_full_throttle_pays_exactly_what_it_always_did() {
        // The property the whole design rests on, and the reason it can ship without re-running
        // a single balance measurement: every genome in `genomes/` leaves its metabolic controls
        // wide open, so if full effort is an identity then nothing any of them does is repriced.
        // Asserted against the arithmetic rather than against a recorded table, over every type
        // and every `param` a `u8` can hold, because a rounding error that only appears at
        // param 173 is exactly the kind this would otherwise ship.
        let cat = OrganelleCatalogue::balanced();
        for kind in OrganelleType::all() {
            let spec = cat.spec(*kind);
            for param in 0..=255u8 {
                let expected = spec
                    .upkeep
                    .saturating_add(spec.upkeep_per_param.saturating_mul(param as i32))
                    / UPKEEP_SCALE;
                assert_eq!(
                    spec.upkeep_cost_at(param, Q10_ONE),
                    expected,
                    "{} at param {param} is repriced at full throttle",
                    kind.name()
                );
                assert_eq!(spec.upkeep_cost(param), expected);
            }
        }
    }

    #[test]
    fn the_dormancy_dial_is_only_set_where_the_control_gates_the_service() {
        // `THROTTLEABLE` is the audit and this is the only thing that can check it: whether
        // `control[0]` really gates a type's output is a fact about `capacity_by_pathway` and
        // `digestive_capacity_by_pathway`, not about anything the type system can see. So the
        // table is written by hand and the catalogue is held to it — switching one on later
        // means editing the list, which is the point.
        let cat = OrganelleCatalogue::balanced();
        for kind in OrganelleType::all() {
            let dial = cat.spec(*kind).upkeep_throttled;
            let listed = THROTTLEABLE.contains(kind);
            assert_eq!(
                dial > 0,
                listed,
                "{} has a dormancy dial of {dial} and is {}in THROTTLEABLE",
                kind.name(),
                if listed { "" } else { "not " }
            );
        }
        // And the ones that would have been bugs rather than balance questions, named so that
        // the reasoning survives the table: a sensor's `control[0]` is a chemical index, a
        // nucleus's is copy fidelity, a membrane's is permeability, and a vacuole's is unread.
        for kind in [
            OrganelleType::Chemosensor,
            OrganelleType::PhSensor,
            OrganelleType::Oscillator,
            OrganelleType::Nucleus,
            OrganelleType::Membrane,
            OrganelleType::Vacuole,
        ] {
            assert_eq!(cat.spec(kind).upkeep_throttled, 0, "{}", kind.name());
        }
    }

    #[test]
    fn a_closed_organelle_is_cheaper_to_keep_and_not_free() {
        // Both halves matter. Cheaper, or there is no dormancy; not free, or there is no
        // pressure against carrying sixteen of everything shut — which is the pressure
        // `docs/ECONOMY.md` §12.1 measures.
        let cat = OrganelleCatalogue::balanced();
        for kind in THROTTLEABLE {
            let spec = cat.spec(kind);
            let open = spec.upkeep_cost_at(100, Q10_ONE);
            let shut = spec.upkeep_cost_at(100, 0);
            assert!(
                shut < open,
                "{} does not sleep: {shut} vs {open}",
                kind.name()
            );
            assert!(shut > 0, "{} sleeps for nothing", kind.name());
            // Monotone in between, so a genome that half-closes gets half the discount and
            // there is a gradient for selection to climb rather than a cliff.
            let half = spec.upkeep_cost_at(100, Q10_ONE / 2);
            assert!(
                shut <= half && half <= open,
                "{} {shut} {half} {open}",
                kind.name()
            );
        }
    }

    #[test]
    fn an_organelle_under_construction_cannot_be_slept_through() {
        // The one asymmetry the self-enforcing bargain does not cover: an unfinished organelle
        // produces nothing open or shut, so a discount for closing it would be free.
        let cat = OrganelleCatalogue::balanced();
        let mut slots = [Organelle::empty(); SLOT_COUNT];
        slots[0] = Organelle::finished(OrganelleType::Membrane, 24);
        slots[2] = Organelle::building(OrganelleType::Chloroplast, 100, 5);
        let building = cat.upkeep(&slots);
        slots[2].control[0] = 0;
        assert_eq!(
            cat.upkeep(&slots),
            building,
            "a half-built chloroplast was slept through"
        );
        // And once it finishes, the same closed control does buy the discount.
        slots[2].remaining_build = 0;
        assert!(cat.upkeep(&slots) < building);
    }

    #[test]
    fn a_cilium_driving_backwards_is_not_asleep() {
        // `cilium_power` clamps to `-Q10..=Q10` because a beat has a direction, so `throttle`
        // reports full reverse as zero. Pricing upkeep on that would have sold backwards
        // swimming for nothing. `effort` is the magnitude, and this is the only place the two
        // differ.
        let mut back = Organelle::finished(OrganelleType::Cilium, 100);
        back.control[0] = -(Q10_ONE as i16);
        assert_eq!(back.throttle(), 0);
        assert_eq!(back.effort(), Q10_ONE);

        let mut idle = Organelle::finished(OrganelleType::Mitochondrion, 100);
        idle.control[0] = 0;
        assert_eq!(idle.effort(), 0, "a shut throttle is a shut throttle");
    }

    #[test]
    fn a_spec_that_says_nothing_about_dormancy_is_priced_as_it_always_was() {
        // The polarity that lets an old snapshot keep its old physics, and a scenario that
        // names three fields of a spec keep the rest: zero means "does not respond".
        let quiet = OrganelleSpec {
            upkeep: 16 * 8,
            upkeep_per_param: 16,
            ..OrganelleSpec::default()
        };
        assert_eq!(quiet.upkeep_throttled, 0);
        for effort in [0, Q10_ONE / 3, Q10_ONE, i32::MAX, i32::MIN] {
            assert_eq!(quiet.upkeep_cost_at(200, effort), (16 * 8 + 16 * 200) / 16);
        }
    }

    #[test]
    fn a_loadout_sleeps_by_what_it_shuts_and_no_more() {
        let cat = OrganelleCatalogue::balanced();
        let mut slots = [Organelle::empty(); SLOT_COUNT];
        slots[0] = Organelle::finished(OrganelleType::Membrane, 24);
        slots[1] = Organelle::finished(OrganelleType::Nucleus, 56);
        slots[2] = Organelle::finished(OrganelleType::Chloroplast, 100);
        slots[3] = Organelle::finished(OrganelleType::Mitochondrion, 50);
        slots[4] = Organelle::finished(OrganelleType::Chemosensor, 40);
        let awake = cat.upkeep(&slots);

        // Shutting the sensor buys nothing — it is a selector, not a throttle, and this is the
        // free lunch the per-type table exists to refuse.
        slots[4].control[0] = 0;
        assert_eq!(cat.upkeep(&slots), awake, "a sensor slept for free");

        // Shutting the two engines buys three quarters of their line and nothing of the
        // membrane's or the nucleus's, which cannot be surrendered and are not offered.
        slots[2].control[0] = 0;
        slots[3].control[0] = 0;
        let asleep = cat.upkeep(&slots);
        let engines = cat.spec(OrganelleType::Chloroplast).upkeep_cost(100)
            + cat.spec(OrganelleType::Mitochondrion).upkeep_cost(50);
        assert!(asleep < awake);
        assert!(
            awake - asleep <= engines,
            "slept for more than the engines cost: {awake} -> {asleep}, engines {engines}"
        );
        assert!(
            awake - asleep >= engines / 2,
            "the discount is not worth having: {awake} -> {asleep}, engines {engines}"
        );
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
