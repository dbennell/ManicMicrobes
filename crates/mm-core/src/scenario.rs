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
        let s: Scenario = ron::from_str(text).map_err(|e| ScenarioError::Parse(e.to_string()))?;
        s.check_isa()?;
        Ok(s)
    }

    /// Render to `.ron`, pretty-printed for a human to edit.
    ///
    /// # Errors
    ///
    /// Serialisation failure, which should not happen for a well-formed scenario.
    pub fn to_ron(&self) -> Result<String, ScenarioError> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| ScenarioError::Parse(e.to_string()))
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
        }
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
                },
                Inhabitant {
                    genome: "drifter.mm".to_string(),
                    count: 4,
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
}
