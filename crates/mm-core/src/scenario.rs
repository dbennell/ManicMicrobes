//! Scenario configuration (SPEC §16).
//!
//! A scenario is a `.ron` file holding the full parameter set. It is the unit of
//! reproducibility: same scenario, same seed, same input events gives bit-identical state at
//! every tick (I1), so the scenario file and the seed together *are* the experiment.
//!
//! The ISA version is stamped into every scenario. Changing the opcode table changes the
//! meaning of every stored genome, so a scenario that names a different ISA version is
//! refused rather than run with a genome that means something else now than it did when it
//! evolved.

use serde::{Deserialize, Serialize};

use crate::chem::{ChemTable, CHEM_COUNT};
use crate::config::VmConfig;
use crate::fixed::{q10, Q10_ONE};
use crate::isa::ISA_VERSION;
use crate::light::{CurrentField, LightRegime};
use crate::state_hash::{StateHash, StateHasher};

/// How much matter of one chemical to place, and where.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Seeding {
    /// The same amount in every square.
    Uniform { chemical: usize, per_square: i32 },
    /// A linear ramp across the slide — the steep initial gradient the conservation test
    /// wants.
    Gradient {
        chemical: usize,
        low: i32,
        high: i32,
        horizontal: bool,
    },
    /// Everything in one square. The hardest case for a diffusion solver to keep exact.
    Spike {
        chemical: usize,
        x: u32,
        y: u32,
        amount: i32,
    },
    /// A filled rectangle.
    Patch {
        chemical: usize,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        per_square: i32,
    },
}

/// Matter crossing the boundary of the slide, every fluid step.
///
/// A slide has been closed to matter and open only to light. That is one habitat, and it is not
/// the only interesting one: a deep-sea vent is a slide in the dark with inorganic matter
/// welling up through it, and marine snow is a slide in the dark with organic matter falling
/// through. Both need matter to arrive from somewhere that is not the slide.
///
/// **A source needs a drain, or the only question a slide can answer is "how long until it is
/// full".** Matter that arrives and never leaves counts up to the quantity cap and stays there,
/// and a population under a ceiling set by arithmetic is not a population under a carrying
/// capacity. The pair is what makes a slide a *flow-through* system, which is where an
/// equilibrium between energy in, energy spent and space available can actually sit.
///
/// Both go through the ledger, in both directions and in both currencies. Matter is exact (I4):
/// what `Substrate::add_chem` reports as actually moved is what is recorded, so a source
/// pointed at a barrier or at a square already full records what it managed rather than what it
/// intended. Energy is exact too (I5): matter carrying a metabolic substrate carries its latent
/// energy with it, so an inflow of sulphide that did not say so would appear as stored energy
/// nobody let in and the next tick's check would fail.
///
/// Neither touches cells. A cell that swims into an outflow is not deleted — a drain removes
/// dissolved matter from the water and nothing else, because "the current washed it off the
/// slide" and "it was eaten by the edge of the world" are different claims and only one of them
/// is one this engine should make.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Flux {
    /// Matter arriving in every square of a rectangle, per fluid step.
    ///
    /// A vent is a small rectangle, marine snow is one covering the slide, and the inlet of a
    /// channel is a column one square wide at the upstream edge.
    Source {
        chemical: usize,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        per_tick: i32,
    },
    /// A fraction of whatever is in a rectangle leaves the slide, `Q10` per fluid step.
    ///
    /// A fraction rather than an amount, so a drain cannot take what is not there and an
    /// outflow settles into balance with whatever reaches it instead of scouring the last of
    /// it away.
    Drain {
        chemical: usize,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rate: i32,
    },
}

/// Who to put on the slide, and how many.
///
/// A scenario has always described a *world* and never its inhabitants, so opening one produced
/// an empty dish that the caller had to populate by hand — `mm-cli` with `--genome`, the front
/// end by seeding the ancestor whatever the file said. Which meant a scenario written around a
/// strategy could not say so: `the_drift.ron` is a channel built for filter feeders, and opening
/// it seeded photosynthesisers that had no use for it.
///
/// The genome is a **path and not bytes**, and it is resolved by the caller rather than here.
/// `mm-core` has no filesystem and no assembler by design and is not getting either; what a
/// scenario carries is the declaration, and turning a name into a program is a job for whoever
/// has `mm-asm` linked. So this is data that `World::new` deliberately does not act on.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Inhabitant {
    /// A genome source file, resolved relative to `genomes/`.
    pub genome: String,
    /// How many founders to place.
    pub count: u32,
    /// Where they go, and in what arrangement.
    #[serde(default)]
    pub place: Placement,
}

/// How a scenario's founders are arranged on the slide.
///
/// # Why this is not one pair of coordinates
///
/// It was. `Inhabitant` carried `at: Option<(u32, u32)>` with a doc comment arguing exactly the
/// right case — "the whole reason to place a cell by hand is that you want it somewhere the grid
/// would not put it: against a wall, in the mouth of a channel, on one side of a barrier and not
/// the other" — and **neither front end read it**. `mm-cli`'s `seed_inhabitants` and `mm-app`'s
/// `seed_into` both called `place_founders`, which spreads. The field was declared, documented,
/// round-trip tested, and inert.
///
/// The cost of that shows up in `the_drift.ron`, whose own comment says: *"Twelve rather than
/// sixteen: the founders are spread over a square grid and a channel is not square, so some of
/// them would land in the walls."* A scenario author reducing the population to work around a
/// placement rule is the sign that the placement rule is the thing to fix.
///
/// # Nothing is ever placed inside a wall
///
/// Every variant is filtered through the barriers: a slot that lands on a blocked square is moved
/// to the nearest free one, and a founder with nowhere within reach is not placed at all — the
/// count a scenario asks for is a request, and [`crate::World::place_inhabitants`] returns how
/// many actually landed. That is what lets a channel scenario ask for sixteen and get sixteen.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Placement {
    /// Spread over the whole slide on a square lattice.
    ///
    /// The default, and what an `Inhabitant` that says nothing gets. Deliberately the same
    /// lattice `World::place_founders` has always used, to the position — every acceptance
    /// number in the tree was taken on it.
    #[default]
    Spread,

    /// All of them around one square, spiralling outward as the count needs.
    ///
    /// What `at: Some((x, y))` meant, now that something acts on it. A count of one is exactly
    /// that square; more than one rings it, because a dozen founders dropped on a single square
    /// is one cell and eleven refusals.
    At { x: u32, y: u32 },

    /// A square lattice inside a rectangle.
    ///
    /// The walled-off-habitat case: two of these in two rooms is two populations that cannot
    /// reach each other, which is an experiment the scenario format could not previously
    /// describe.
    Grid {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },

    /// A hexagonal lattice inside a rectangle: every other row offset by half a step.
    ///
    /// Closer packing than a square grid at the same spacing, and the arrangement a settled
    /// monolayer relaxes into anyway — so a colony seeded this way starts where a square-packed
    /// one would spend a few hundred ticks getting to.
    Hex {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },

    /// Scattered inside a rectangle, no two founders closer than `spacing` squares.
    ///
    /// Deterministic: positions come from `hash(seed, k, purpose)` like every other random draw
    /// in this engine (hard rule 5), so a scenario scatters the same way on every machine and at
    /// every thread count. `spacing` of zero means no minimum.
    ///
    /// This is the variant to reach for when two species should start *interleaved* rather than
    /// in separate rooms — two entries over the same rectangle mix, where two `Grid`s would sit
    /// on top of each other.
    Scatter {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        spacing: u32,
    },
}

impl Placement {
    /// How to say where these founders go, for the inspector and the wiki.
    ///
    /// Here rather than in the front end so that a new variant is a compile error in one place
    /// and a sentence in one place, instead of a match somebody forgets to extend.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Placement::Spread => "spread over the slide".to_string(),
            Placement::At { x, y } => format!("at ({x}, {y})"),
            Placement::Grid {
                x,
                y,
                width,
                height,
            } => format!("on a grid in {width}x{height} at ({x}, {y})"),
            Placement::Hex {
                x,
                y,
                width,
                height,
            } => format!("hex-packed in {width}x{height} at ({x}, {y})"),
            Placement::Scatter {
                x,
                y,
                width,
                height,
                spacing,
            } => format!("scattered in {width}x{height} at ({x}, {y}), {spacing} apart"),
        }
    }
}

/// Where barriers go.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Barrier {
    /// A single square.
    Square { x: u32, y: u32 },
    /// A filled rectangle.
    Rect {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    /// A wall with a gap in it, which is what makes a slide into two habitats that still
    /// exchange — the archipelago scenario of M8 in miniature.
    WallWithGap {
        /// Column for a vertical wall, row for a horizontal one.
        at: u32,
        vertical: bool,
        gap_start: u32,
        gap_len: u32,
    },
}

/// The full parameter set for a run.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Scenario {
    /// The ISA the genomes in this scenario were written or evolved under.
    pub isa_version: u16,
    pub name: String,
    pub seed: u64,

    pub width: u32,
    pub height: u32,

    pub chemicals: ChemTable,
    pub light: LightRegime,
    pub current: CurrentField,

    /// Cell ticks between fluid steps. The fluid runs at `fluid_hz`, decoupled from and
    /// typically lower than the cell tick rate (SPEC §7.4).
    pub fluid_interval: u32,
    /// Fraction of a cilium impulse that survives each fluid step, `Q10`.
    pub impulse_retain: i32,
    /// Brownian jitter, `Q10` of a square per tick.
    ///
    /// The reason a cell that does nothing still ends up somewhere. Small enough that it is
    /// noise rather than transport, large enough that a population spreads without needing to
    /// swim — which matters, because a chemotaxis experiment has to be able to tell swimming
    /// apart from drifting.
    pub jitter: i32,
    /// A steady pull towards the middle of the slide, `Q10` of a square per tick per tick.
    ///
    /// A body force, deliberately, and not a current. `CurrentField::Convergent` will also
    /// gather a population, but the fluid adds its drift straight to the position step without
    /// ever touching velocity — so drag cannot damp it and neither can the contact solver, and
    /// a crowd that has nowhere left to go is shoved inward and pushed back out on every tick
    /// for as long as the current runs. Measured on the packing bench at a sixteenth of a square
    /// per cell per tick while `vx` and `vy` read exactly zero, which is a picture that never
    /// stops trembling and a metric that says everything is fine.
    ///
    /// This goes in with thrust and Brownian jitter instead, so it is subject to drag on the way
    /// in and to velocity reconciliation on the way out. A cell pressed against neighbours it
    /// cannot move loses the part of its motion that was driving it into them, and the pack
    /// settles.
    #[serde(default)]
    pub gravity: i32,

    pub seeding: Vec<Seeding>,
    pub barriers: Vec<Barrier>,

    /// Who lives here. See [`Inhabitant`] — the caller resolves and places these, not `World`.
    #[serde(default)]
    pub inhabitants: Vec<Inhabitant>,

    /// Matter crossing the edge of the slide every fluid step. See [`Flux`].
    #[serde(default)]
    pub flux: Vec<Flux>,

    /// The named parameter set these rules came from, or `None` for the engine's own.
    ///
    /// **Provenance, not a reference.** It records where the numbers came from and is never
    /// re-applied over values that are already here — see [`crate::ruleset`] for why that is what
    /// keeps hard rule 7 intact, and for the test that says so.
    ///
    /// Resolution happens once, at load, in [`crate::ruleset::RulesetLibrary::load_scenario`].
    /// [`Scenario::from_ron`] carries this through and applies nothing, which is right for a
    /// saved scenario (whose parameters are already complete) and wrong for a hand-written one —
    /// so a front end loads through the library.
    /// A `String` rather than an `Option<String>`, empty meaning "the engine's own", for the
    /// reason [`crate::ruleset::Ruleset::of`] is one: RON spells a present option `Some("x")`,
    /// and these files are written by hand.
    pub ruleset: String,

    /// Parameter changes by dotted path, applied last of all — the same vocabulary
    /// [`crate::ruleset::Ruleset::set`] uses, in the scenario's own file.
    ///
    /// # Why a scenario needs this as well as its `biology` block
    ///
    /// A sparse `biology: (…)` block can reach any *struct* field, and that is what
    /// `the_marbles.ron` uses. It cannot reach an array element: RON sequences are positional, so
    /// naming the fourth organelle spec means writing all sixteen, and naming one chemical means
    /// writing the whole four-hundred-line table. [`crate::ruleset`] met the same wall and chose
    /// dotted paths for exactly this reason; this is that choice, available to a world as well as
    /// to a named set of rules.
    ///
    /// It is also what [`Scenario::to_ron_sparse`] writes, so a scenario saved out of the
    /// microscope is a short list of what it changed rather than four hundred lines of the
    /// engine's own numbers restated.
    ///
    /// # Applied last, and applied by the parser
    ///
    /// Last, so it beats both the named ruleset and the inline block — it is the most specific
    /// thing the file says. By the parser rather than by the resolver, because unlike `ruleset`
    /// this names no file outside itself: a scenario carrying a `set` still means exactly what it
    /// says, which is what lets [`Scenario::from_ron`] stay the honest way to read a snapshot's
    /// embedded copy.
    ///
    /// Applying is idempotent — the values here are the values that end up in the config — so a
    /// scenario keeps its `set` after resolution rather than being emptied, and re-resolving a
    /// saved file is still a copy.
    #[serde(default)]
    pub set: std::collections::BTreeMap<String, ron::Value>,

    pub vm: VmConfig,

    /// Costs, rates, mutation, junctions, ecology and the organelle catalogue (M10.2).
    ///
    /// This lived on `World` alone until M10.2, reachable through `World::set_biology` and from
    /// nowhere else — so every scenario in `scenarios/` ran on the compiled-in defaults, and
    /// the balancing numbers arrived at by measurement in M9 were constants no scenario could
    /// vary and no user could touch. A parameter that is not in the file is not a parameter,
    /// it is a decision somebody else made.
    ///
    /// Moving it here also deletes about sixty lines of hand-written serialisation from
    /// `snapshot.rs`, which had been the reason the snapshot format version moved three times
    /// in two milestones for changes that should have been free.
    pub biology: crate::biology::BiologyConfig,
}

impl Default for Scenario {
    fn default() -> Self {
        Scenario {
            isa_version: ISA_VERSION,
            name: "untitled".to_string(),
            seed: 0,
            width: 128,
            height: 128,
            chemicals: ChemTable::spec_default(),
            light: LightRegime::default(),
            current: CurrentField::default(),
            fluid_interval: 1,
            impulse_retain: Q10_ONE * 15 / 16,
            jitter: 24,
            gravity: 0,
            seeding: Vec::new(),
            barriers: Vec::new(),
            inhabitants: Vec::new(),
            flux: Vec::new(),
            ruleset: String::new(),
            set: std::collections::BTreeMap::new(),
            vm: VmConfig::DEFAULT,
            biology: crate::biology::BiologyConfig::default(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ScenarioError {
    Parse(String),
    /// Refusing to run genomes under an ISA that is not the one they mean.
    IsaMismatch {
        scenario: u16,
        engine: u16,
    },
    Substrate(crate::substrate::SubstrateError),
    /// A `set` entry naming no parameter, or holding a value that will not fit one.
    ///
    /// Refused rather than ignored, for the reason [`crate::ruleset::RulesetError::BadPath`] is:
    /// a typo in a block whose whole job is to change numbers would otherwise be a change that
    /// silently did nothing, which is the worst failure such a block can have.
    BadPath(String),
}

impl std::fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScenarioError::Parse(e) => write!(f, "{e}"),
            ScenarioError::IsaMismatch { scenario, engine } => write!(
                f,
                "scenario was written for ISA version {scenario}, this engine is version \
                 {engine}; every stored genome means something different under a different \
                 opcode table, so it will not be run"
            ),
            ScenarioError::Substrate(e) => write!(f, "{e}"),
            ScenarioError::BadPath(path) => write!(
                f,
                "scenario sets `{path}`, which is not a parameter (or the value does not fit it)"
            ),
        }
    }
}

impl std::error::Error for ScenarioError {}

impl Scenario {
    /// Parse a `.ron` scenario.
    ///
    /// # Errors
    ///
    /// A syntax error, or an ISA version this engine cannot honour.
    pub fn from_ron(text: &str) -> Result<Scenario, ScenarioError> {
        let mut s: Scenario =
            ron::from_str(text).map_err(|e| ScenarioError::Parse(e.to_string()))?;
        s.apply_set()?;
        s.check_isa()?;
        Ok(s)
    }

    /// Fold [`Scenario::set`] into the three rules blocks.
    ///
    /// Called by [`Scenario::from_ron`], and again by
    /// [`crate::ruleset::RulesetLibrary::load_scenario_as`] once the named ruleset and the inline
    /// block have been merged underneath it — `set` is the last word, so it has to be applied
    /// after anything it might have to beat.
    ///
    /// Idempotent: it writes the values it names, so applying it twice writes them twice.
    ///
    /// # Errors
    ///
    /// [`ScenarioError::BadPath`] for a path that names no parameter, or a value that will not
    /// fit one.
    pub fn apply_set(&mut self) -> Result<(), ScenarioError> {
        if self.set.is_empty() {
            return Ok(());
        }
        let mut rules = crate::ruleset::Rules::of(self);
        let mut normalised = std::collections::BTreeMap::new();
        for (path, value) in &self.set {
            let value = crate::params::Value::from_ron(value)
                .ok_or_else(|| ScenarioError::BadPath(path.clone()))?;
            rules = crate::params::set(&rules, path, value)
                .ok_or_else(|| ScenarioError::BadPath(path.clone()))?;
            // Written back through the same conversion it came in by. `ron::Value::Number` is
            // width-tagged — 4096 parses as a `U16` and is constructed as something else — so two
            // scenarios that say the same thing would otherwise compare unequal depending on
            // whether they came off a disk or out of a diff.
            normalised.insert(path.clone(), value.to_ron());
        }
        self.set = normalised;
        rules.apply_to(self);
        Ok(())
    }

    /// Render to `.ron`, pretty-printed for a human to edit.
    ///
    /// **Every field, always.** That is what makes this the archival form: [`crate::snapshot`]
    /// embeds this string and reads it back with [`Scenario::from_ron`], which applies no
    /// ruleset, so a saved slide must not depend on any file outside itself. It is also what
    /// makes re-resolving a saved scenario a copy rather than a merge — see [`crate::ruleset`]
    /// and `a_saved_scenario_does_not_move_when_its_ruleset_does`.
    ///
    /// For the form a person reads and edits, see [`Scenario::to_ron_sparse`].
    ///
    /// # Errors
    ///
    /// Serialisation failure, which should not happen for a well-formed scenario.
    pub fn to_ron(&self) -> Result<String, ScenarioError> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| ScenarioError::Parse(e.to_string()))
    }

    /// Render to `.ron`, writing only what `base` does not already say.
    ///
    /// # Why there are two of these
    ///
    /// [`Scenario::to_ron`] writes all eighteen fields, of which the chemical table alone is
    /// four hundred lines. That is right for a snapshot and wrong for a file a person opens: a
    /// scenario saved out of the microscope came back as four hundred and twenty lines of which
    /// four hundred were the engine's own defaults restated, and from that moment on editing a
    /// ruleset could no longer reach it. The hand-written files in `scenarios/` never had that
    /// problem — `soup.ron` is fifteen lines — and the only thing standing between them and the
    /// saved ones was this function.
    ///
    /// So: what a person writes by hand is what Save writes back.
    ///
    /// # What `base` should be
    ///
    /// [`crate::ruleset::RulesetLibrary::baseline`] — [`Scenario::default`] with whatever ruleset
    /// this scenario names resolved into it. Then the output is exactly the three layers
    /// `mm_core::ruleset` describes, written down: the engine's numbers are silence, the
    /// ruleset's are its name, and this scenario's own are the only ones on the page.
    ///
    /// # What this costs, and it is worth saying plainly
    ///
    /// A file written this way **means what its ruleset says today**. Edit `rulesets/foo.ron`
    /// and every scenario naming it moves with it — which is the point, and is also why the
    /// archival form is the other one. `the_thicket.ron` has had exactly this property since
    /// rulesets landed; this puts the app's output in the same category as the files it ships
    /// beside, no more and no less.
    ///
    /// # The rules half is written as dotted paths
    ///
    /// Into [`Scenario::set`], not as a nested `biology: (…)` block, and for two reasons. The
    /// small one is that `"biology.metabolism.rates.rigidity_gain": 1024` greps and diffs better
    /// than the same thing three levels down a tree. The load-bearing one is that a nested block
    /// cannot name an array element — RON sequences are positional, so one changed chemical would
    /// mean writing all sixteen, which is the four hundred lines this whole function exists to
    /// avoid. It is also the vocabulary `rulesets/*.ron` already speaks, so the two file formats
    /// say a parameter change the same way.
    ///
    /// # Errors
    ///
    /// Serialisation failure, which should not happen for a well-formed scenario.
    pub fn to_ron_sparse(&self, base: &Scenario) -> Result<String, ScenarioError> {
        let mut w = Sparse::new();
        // The stamp, then who and where. Written whatever the baseline says, because a scenario
        // file with no name and no size is a puzzle rather than a short file.
        w.changed("isa_version", &self.isa_version, &base.isa_version)?;
        w.always("name", &self.name)?;
        w.always("seed", &self.seed)?;
        w.always("width", &self.width)?;
        w.always("height", &self.height)?;

        // Light and current are written whatever they are. `docs/UI.md` §9.6 calls them the two
        // settings that decide the most about a world, and a file that leaves the reader to know
        // that silence means full daylight and still water is a file that has to be checked
        // against the engine to be read.
        w.gap();
        w.always("light", &self.light)?;
        w.always("current", &self.current)?;
        w.changed("fluid_interval", &self.fluid_interval, &base.fluid_interval)?;
        w.changed("impulse_retain", &self.impulse_retain, &base.impulse_retain)?;
        w.changed("jitter", &self.jitter, &base.jitter)?;
        w.changed("gravity", &self.gravity, &base.gravity)?;

        w.gap();
        w.changed("seeding", &self.seeding, &base.seeding)?;
        w.changed("barriers", &self.barriers, &base.barriers)?;
        w.changed("inhabitants", &self.inhabitants, &base.inhabitants)?;
        w.changed("flux", &self.flux, &base.flux)?;

        // The rules half last, which is the order every hand-written file already reads in: what
        // kind of world this is, then what a cell may do in it.
        w.gap();
        if !self.ruleset.is_empty() {
            w.always("ruleset", &self.ruleset)?;
        }
        w.set(&crate::params::diff(
            &crate::ruleset::Rules::of(base),
            &crate::ruleset::Rules::of(self),
        ));
        Ok(w.finish())
    }

    /// # Errors
    ///
    /// [`ScenarioError::IsaMismatch`] if the stamp is not this engine's version.
    pub fn check_isa(&self) -> Result<(), ScenarioError> {
        if self.isa_version == ISA_VERSION {
            Ok(())
        } else {
            Err(ScenarioError::IsaMismatch {
                scenario: self.isa_version,
                engine: ISA_VERSION,
            })
        }
    }

    /// A stirred, barrier-fragmented, steeply-graded slide — the hostile case the matter
    /// conservation test wants. Everything about it is chosen to make a leak visible.
    #[must_use]
    pub fn stress(width: u32, height: u32) -> Scenario {
        Scenario {
            name: "conservation stress".to_string(),
            width,
            height,
            current: CurrentField::Rotational {
                strength: Q10_ONE * 3 / 4,
            },
            light: LightRegime::DayNight {
                period_ticks: 4096,
                day: Q10_ONE,
                night: 0,
            },
            seeding: vec![
                Seeding::Gradient {
                    chemical: 0,
                    low: 0,
                    high: q10(200_000),
                    horizontal: true,
                },
                Seeding::Gradient {
                    chemical: 4,
                    low: q10(150_000),
                    high: 0,
                    horizontal: false,
                },
                // The minerals every recipe in the catalogue is costed in. Uniform rather than
                // graded, deliberately: this scenario exists to stress the *fluid*, and a cell
                // that cannot build a nucleus because it stands on the wrong end of a gradient
                // makes it a test of the chemistry instead.
                Seeding::Uniform {
                    chemical: crate::organelle::NITROGEN,
                    per_square: q10(2_000),
                },
                Seeding::Uniform {
                    chemical: crate::organelle::PHOSPHORUS,
                    per_square: q10(200),
                },
                Seeding::Spike {
                    chemical: 8,
                    x: width / 3,
                    y: height / 3,
                    amount: i32::MAX / 2,
                },
                Seeding::Patch {
                    chemical: 13,
                    x: 0,
                    y: 0,
                    width: width / 4,
                    height: height / 4,
                    per_square: q10(90_000),
                },
                Seeding::Uniform {
                    chemical: 15,
                    per_square: q10(1_000),
                },
            ],
            barriers: vec![
                Barrier::WallWithGap {
                    at: width / 2,
                    vertical: true,
                    gap_start: height / 2,
                    gap_len: 2,
                },
                Barrier::Rect {
                    x: width / 5,
                    y: height * 3 / 4,
                    width: width / 6,
                    height: 3,
                },
                Barrier::Square { x: 1, y: 1 },
            ],
            ..Scenario::default()
        }
    }
}

/// How the world half of a sparse scenario is printed.
///
/// `compact_structs` is what makes `Uniform(chemical: 11, per_square: 409600)` one line instead
/// of four, which is how every file in `scenarios/` already spells it. `compact_arrays` stays
/// off: a seeding list one entry to a line is the form that diffs.
fn pretty() -> ron::ser::PrettyConfig {
    ron::ser::PrettyConfig::default()
        .compact_structs(true)
        .compact_arrays(false)
}

/// Builds the text of a sparse scenario, one field at a time.
///
/// Two kinds of field, written by different machinery for a reason.
///
/// * The **world** half holds enums — `Uniform(intensity: 1024)`, `Still`, `Spread` — and only
///   the derived serialiser knows their variant names. It also holds fixed-size arrays, which
///   RON spells `(1, 2, 3)` where a `Vec` is `[1, 2, 3]` and which it will not read the other
///   way round. `ron::Value` carries neither distinction, so anything printed from one would be
///   guessing; these go through `serde` and come out right by construction.
/// * The **rules** half is written as dotted paths, where there is no shape to get wrong and an
///   array element can be named on its own.
struct Sparse {
    out: String,
    gap: bool,
}

impl Sparse {
    fn new() -> Sparse {
        Sparse {
            out: String::new(),
            gap: false,
        }
    }

    /// A blank line before the next field that is written — and none at all if none is, so a
    /// scenario that says nothing about its weather does not save with a hole where it would
    /// have gone.
    fn gap(&mut self) {
        self.gap = !self.out.is_empty();
    }

    fn field(&mut self, key: &str, rendered: &str) {
        if self.gap {
            self.out.push('\n');
            self.gap = false;
        }
        self.out.push_str("    ");
        self.out.push_str(key);
        self.out.push_str(": ");
        // A multi-line value is printed at depth zero and moved under its key here, so neither
        // printer has to know how deep it ended up.
        for (i, line) in rendered.lines().enumerate() {
            if i > 0 {
                self.out.push_str("\n    ");
            }
            self.out.push_str(line);
        }
        self.out.push_str(",\n");
    }

    fn always<T: Serialize>(&mut self, key: &str, value: &T) -> Result<(), ScenarioError> {
        let text = ron::ser::to_string_pretty(value, pretty())
            .map_err(|e| ScenarioError::Parse(e.to_string()))?;
        self.field(key, &text);
        Ok(())
    }

    fn changed<T: Serialize + PartialEq>(
        &mut self,
        key: &str,
        now: &T,
        was: &T,
    ) -> Result<(), ScenarioError> {
        if now == was {
            return Ok(());
        }
        self.always(key, now)
    }

    /// The rules half: every parameter this world moves, one to a line.
    ///
    /// Written by hand rather than through the serialiser because a `BTreeMap` prints as a RON
    /// map — `{"a": 1}` — which is what the format wants here, but the pretty-printer spreads a
    /// nested map over three lines a key and this reads as a table.
    fn set(&mut self, changes: &std::collections::BTreeMap<String, crate::params::Value>) {
        if changes.is_empty() {
            return;
        }
        let mut text = String::from("{\n");
        for (path, value) in changes {
            text.push_str(&format!("    {path:?}: {value},\n"));
        }
        text.push('}');
        self.field("set", &text);
    }

    fn finish(self) -> String {
        format!("(\n{})\n", self.out)
    }
}

impl StateHash for Scenario {
    fn hash_state(&self, h: &mut StateHasher) {
        h.u16(self.isa_version);
        h.u64(self.seed);
        h.u32(self.width);
        h.u32(self.height);
        self.chemicals.hash_state(h);
        self.light.hash_state(h);
        self.current.hash_state(h);
        h.u32(self.fluid_interval);
        h.i32(self.impulse_retain);
        h.i32(self.jitter);
        h.i32(self.gravity);
        h.u16(self.vm.instr_per_tick);
        h.u16(self.vm.template_search_range);
        h.u16(self.vm.promoter_bind_threshold);
        h.u64(self.seeding.len() as u64);
        h.u64(self.barriers.len() as u64);
        for i in &self.inhabitants {
            h.bytes(i.genome.as_bytes());
            h.u32(i.count);
            // The arrangement, as a discriminant and its numbers. Two slides that put the same
            // founders in different places are different slides.
            let (tag, a, b, c, d) = match i.place {
                crate::Placement::Spread => (0u8, 0, 0, 0, 0),
                crate::Placement::At { x, y } => (1, x, y, 0, 0),
                crate::Placement::Grid { x, y, width, height } => (2, x, y, width, height),
                crate::Placement::Hex { x, y, width, height } => (3, x, y, width, height),
                crate::Placement::Scatter { x, y, width, height, spacing } => {
                    h.u32(spacing);
                    (4, x, y, width, height)
                }
            };
            h.u8(tag);
            h.u32(a);
            h.u32(b);
            h.u32(c);
            h.u32(d);
        }
        h.u64(self.flux.len() as u64);
        self.biology.hash_state(h);
        for c in 0..CHEM_COUNT {
            h.i32(self.chemicals.get(c).diffusion);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_scenario_round_trips_through_ron() {
        let s = Scenario::default();
        let text = s.to_ron().unwrap();
        let back = Scenario::from_ron(&text).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn who_lives_here_survives_the_round_trip() {
        let s = Scenario {
            inhabitants: vec![
                Inhabitant {
                    genome: "sponge.mm".to_string(),
                    count: 12,
                    place: Placement::Spread,
                },
                Inhabitant {
                    genome: "drifter.mm".to_string(),
                    count: 4,
                    place: Placement::At { x: 9, y: 40 },
                },
            ],
            ..Scenario::default()
        };
        let back = Scenario::from_ron(&s.to_ron().unwrap()).unwrap();
        assert_eq!(back.inhabitants, s.inhabitants);
    }

    /// The field is `#[serde(default)]`, so every scenario written before it existed still
    /// loads — and says, correctly, that nobody in particular lives there.
    #[test]
    fn a_scenario_written_before_anyone_lived_here_still_loads() {
        let mut text = Scenario::default().to_ron().unwrap();
        text = text
            .lines()
            .filter(|l| !l.contains("inhabitants"))
            .collect::<Vec<_>>()
            .join("\n");
        let back = Scenario::from_ron(&text).expect("an older file should still parse");
        assert!(back.inhabitants.is_empty());
    }

    #[test]
    fn the_stress_scenario_round_trips_through_ron() {
        let s = Scenario::stress(64, 48);
        let back = Scenario::from_ron(&s.to_ron().unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn every_biology_parameter_survives_the_file() {
        // The point of M10.2. These lived on `World` and nowhere else, so a scenario could not
        // express them and the numbers arrived at by measurement were constants. Each one is
        // moved off its default here, because a round trip of the defaults would pass just as
        // well against a field that serialises as a constant.
        let mut s = Scenario::default();
        s.biology.division_matter = 12_345;
        s.biology.division_energy = 6_789;
        s.biology.structural_chemical = 9;
        s.biology.copy_energy_per_byte = 77;
        s.biology.mutation.point = 4_242;
        s.biology.mutation.duplication = 31;
        s.biology.metabolism.rates.repair_energy_per_unit = 555;
        s.biology.metabolism.rates.background_damage = 13;
        s.biology.metabolism.rates.metabolic_floor = 21;
        s.biology.metabolism.catalogue.metabolism.pathways[1].substrate = 3;
        s.biology.junctions.join_forced_penalty = 999;
        s.biology.junctions.probe_leaks_distance = true;
        s.biology.ecology.spike_damage = 246;
        s.biology.ecology.digestion_efficiency = 802;

        let mut specs = *s.biology.metabolism.catalogue.specs();
        specs[3].build_energy = 4_096;
        specs[3].upkeep_per_param = 17;
        s.biology.metabolism.catalogue.set_specs(specs);

        let back = Scenario::from_ron(&s.to_ron().unwrap()).unwrap();
        assert_eq!(
            back, s,
            "a biology parameter did not survive the round trip"
        );
    }

    #[test]
    fn a_scenario_that_says_nothing_about_biology_still_loads() {
        // Every scenario in `scenarios/` was written before the parameters existed in the file,
        // and none of them mention it. They have to keep working, and they have to keep meaning
        // what they meant.
        let text = r#"(name: "terse", width: 8, height: 8)"#;
        let s = Scenario::from_ron(text).unwrap();
        assert_eq!(s.biology, crate::biology::BiologyConfig::default());
    }

    #[test]
    fn two_scenarios_differing_only_in_a_parameter_do_not_share_a_hash() {
        // A world that costs more to divide in is a different world. A hash that could not say
        // so would let a determinism test pass across a parameter change, which is the one
        // thing the hash is for.
        use crate::state_hash::StateHasher;
        let a = Scenario::default();
        let mut b = Scenario::default();
        b.biology.division_energy += 1;

        let mut ha = StateHasher::new();
        a.hash_state(&mut ha);
        let mut hb = StateHasher::new();
        b.hash_state(&mut hb);
        assert_ne!(ha.finish(), hb.finish());
    }

    #[test]
    fn omitted_fields_take_their_defaults() {
        // Scenario authors should not have to spell out sixteen chemicals to change a seed.
        let s = Scenario::from_ron("(name: \"tiny\", seed: 7, width: 32, height: 32)").unwrap();
        assert_eq!(s.name, "tiny");
        assert_eq!(s.seed, 7);
        assert_eq!(s.width, 32);
        assert_eq!(s.chemicals, ChemTable::spec_default());
        assert_eq!(s.isa_version, ISA_VERSION);
    }

    #[test]
    fn a_foreign_isa_version_is_refused() {
        let text = format!("(isa_version: {}, name: \"old\")", ISA_VERSION + 1);
        let e = Scenario::from_ron(&text).unwrap_err();
        assert_eq!(
            e,
            ScenarioError::IsaMismatch {
                scenario: ISA_VERSION + 1,
                engine: ISA_VERSION
            }
        );
        assert!(e.to_string().contains("will not be run"));
    }

    #[test]
    fn a_syntax_error_is_reported_rather_than_panicking() {
        let e = Scenario::from_ron("(this is not ron").unwrap_err();
        assert!(matches!(e, ScenarioError::Parse(_)));
    }

    /// A world like `soup.ron`: a size, a light, some chemistry, and no opinion at all about
    /// what a cell costs to run.
    fn a_world() -> Scenario {
        Scenario {
            name: "primordial soup".to_string(),
            seed: 20_250_728,
            width: 64,
            height: 64,
            seeding: vec![Seeding::Uniform {
                chemical: 4,
                per_square: 409_600,
            }],
            ..Scenario::default()
        }
    }

    #[test]
    fn a_sparse_scenario_says_what_it_changes_and_stays_silent_about_the_rest() {
        let text = a_world()
            .to_ron_sparse(&Scenario::default())
            .expect("to_ron_sparse");

        // The point of the whole exercise. The archival form is four hundred and thirty-six
        // lines, of which four hundred are the chemical table restated.
        assert!(
            text.lines().count() < 20,
            "a world that changes nothing about the rules should be short:\n{text}"
        );
        // By line rather than by substring: `ruleset:` ends in `set:`, and the trap is worth
        // avoiding in a test whose whole job is to notice what is on the page.
        for block in ["biology", "chemicals", "vm", "set"] {
            assert!(
                !text
                    .lines()
                    .any(|l| l.trim_start().starts_with(&format!("{block}:"))),
                "`{block}` was written out unchanged:\n{text}"
            );
        }
        // And it still says who and where, whatever the baseline holds.
        for said in ["name:", "seed:", "width:", "height:", "light:", "current:"] {
            assert!(text.contains(said), "`{said}` is missing:\n{text}");
        }
    }

    #[test]
    fn a_sparse_scenario_reloads_to_the_scenario_it_came_from() {
        // The load-bearing one for this form. Everything left off the page has to come back from
        // `#[serde(default)]` exactly as it went in, or a save is a quiet edit.
        let mut s = a_world();
        s.biology.division_energy = 4_096;
        s.fluid_interval = 8;
        s.gravity = 12;
        s.inhabitants = vec![Inhabitant {
            genome: "ancestor.mm".to_string(),
            count: 16,
            place: Placement::At { x: 3, y: 4 },
        }];

        let text = s.to_ron_sparse(&Scenario::default()).expect("to_ron_sparse");
        let mut back = Scenario::from_ron(&text).expect("a sparse scenario should parse");

        // `set` is what the file said, kept the way `ruleset` is kept: provenance, already
        // applied. The scenario that was saved never had one, so it is cleared before the
        // comparison and checked on its own.
        assert_eq!(
            back.set.keys().collect::<Vec<_>>(),
            ["biology.division_energy"],
            "the rules delta is not what changed:\n{text}"
        );
        back.set.clear();
        assert_eq!(back, s, "sparse save changed the scenario:\n{text}");

        // And the archival form of what came back is stable, so a sparse file opened and
        // re-saved as a snapshot does not drift.
        let archived = Scenario::from_ron(&back.to_ron().expect("to_ron")).expect("re-read");
        assert_eq!(archived, back);
    }

    #[test]
    fn one_changed_chemical_is_one_line_and_not_the_whole_table() {
        // The case that decided the format. A nested `chemicals: (…)` block cannot name entry
        // eight — RON sequences are positional — so a document override would have had to write
        // all sixteen, which is the four hundred lines this whole function exists to avoid.
        let mut s = a_world();
        let mut defs: Vec<crate::chem::ChemicalDef> = s.chemicals.clone().into();
        defs[8].energy_yield = 2_048;
        s.chemicals = ChemTable::new(defs);

        let text = s.to_ron_sparse(&Scenario::default()).expect("to_ron_sparse");
        assert!(
            text.contains(r#""chemicals.8.energy_yield": 2048"#),
            "the change did not reach the page as a path:\n{text}"
        );
        assert!(text.lines().count() < 25, "still writing the table:\n{text}");

        let back = Scenario::from_ron(&text).expect("parses");
        assert_eq!(back.chemicals.get(8).energy_yield, 2_048);
        assert_eq!(
            back.chemicals, s.chemicals,
            "the fifteen chemicals it did not change came back different"
        );
    }

    #[test]
    fn every_rules_parameter_survives_a_sparse_save() {
        // The guard that makes this format safe to leave alone. Move *every* parameter there is,
        // save, reload and compare — so a field added to any config, of any shape, is either
        // carried by this form or fails here, rather than silently reverting to its default the
        // next time somebody saves. It is also the promise a shape-aware nested writer could not
        // have made: `set` names leaves, and a leaf has no shape to get wrong.
        let base = Scenario::default();
        let mut rules = crate::ruleset::Rules::of(&base);
        for (path, value) in crate::params::fields(&rules) {
            let moved = match value {
                crate::params::Value::Int(v) => crate::params::Value::Int(v + 1),
                crate::params::Value::Bool(b) => crate::params::Value::Bool(!b),
            };
            // Not every field takes every neighbouring value — a diffusion rate is clamped on the
            // way in, an index is bounded. Whatever it does take is what has to survive.
            if let Some(next) = crate::params::set(&rules, &path, moved) {
                rules = next;
            }
        }

        let mut s = a_world();
        rules.apply_to(&mut s);
        assert_ne!(
            crate::ruleset::Rules::of(&s),
            crate::ruleset::Rules::of(&base),
            "the probe moved nothing"
        );

        let text = s.to_ron_sparse(&base).expect("to_ron_sparse");
        let back = Scenario::from_ron(&text).expect("a sparse scenario should parse");
        assert_eq!(back.biology, s.biology, "biology drifted across a sparse save");
        assert_eq!(back.vm, s.vm, "vm drifted across a sparse save");
        assert_eq!(
            back.chemicals, s.chemicals,
            "chemistry drifted across a sparse save"
        );
    }

    #[test]
    fn a_set_entry_that_names_no_parameter_is_refused_rather_than_ignored() {
        // The same call `mm_core::ruleset` makes, for the same reason: the worst failure for a
        // block whose whole job is to change numbers is a typo that silently changes none.
        let text = r#"(
            name: "typo",
            set: { "biology.metabolism.rates.light_occulsion": 128 },
        )"#;
        assert!(matches!(
            Scenario::from_ron(text),
            Err(ScenarioError::BadPath(_))
        ));
    }

    #[test]
    fn a_set_block_beats_the_inline_block_beside_it() {
        // `set` is the last word, which is what lets a sparse save be trusted: whatever else the
        // file says, what the microscope wrote is what comes back.
        let text = r#"(
            name: "both",
            biology: ( division_energy: 111 ),
            set: { "biology.division_energy": 222 },
        )"#;
        let s = Scenario::from_ron(text).expect("parses");
        assert_eq!(s.biology.division_energy, 222);
    }

    #[test]
    fn a_sparse_scenario_against_itself_is_almost_empty() {
        // Not literally empty: name, seed, size, light and current are always written. Nothing
        // else should be, and this is what says so if a field is ever added to the always list
        // by accident.
        let s = a_world();
        let text = s.to_ron_sparse(&s).expect("to_ron_sparse");
        let fields: Vec<&str> = text
            .lines()
            .filter_map(|l| l.trim().split_once(':'))
            .map(|(k, _)| k)
            .collect();
        assert_eq!(fields, ["name", "seed", "width", "height", "light", "current"]);
    }

    #[test]
    fn the_archival_form_is_still_every_field() {
        // `Snapshot` embeds `to_ron` and reads it back with `from_ron`, which applies no ruleset.
        // If this ever starts writing a delta, every saved slide silently begins depending on
        // files outside itself. The two forms are two forms on purpose.
        let text = Scenario::default().to_ron().expect("to_ron");
        for block in ["biology:", "chemicals:", "vm:", "isa_version:"] {
            assert!(text.contains(block), "`{block}` is not in the archival form");
        }
    }
}
