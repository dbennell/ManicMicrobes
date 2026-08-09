//! Named parameter sets, and the three layers a scenario's rules are resolved from.
//!
//! # The problem this solves
//!
//! Every config struct in the tree carries `#[serde(default)]`, so a scenario file has always
//! been a *sparse override* — `the_thicket.ron` names two parameters out of about sixty and
//! inherits the rest. What it could not do is inherit them from anything other than the
//! hard-coded [`Default`]. So "the same world under a different economy" and "the same economy
//! in a different world" were not expressible as data: the first meant editing the scenario, and
//! the second meant copying two parameters into another file by hand.
//!
//! That matters most for balancing, which is exactly those two sweeps. `mm_core::balance` can
//! hold the rules fixed and vary the worlds; without this it could not hold the worlds fixed and
//! vary the rules, and `docs/ECONOMY.md` §9's counterfactual table had to be written as Rust
//! literals rather than as a panel.
//!
//! # Three layers
//!
//! ```text
//!     Rules::default()          the engine's own numbers
//!  →  the named ruleset          a diff, which may itself name a parent
//!  →  the scenario's own block    whatever the .ron file says inline
//! ```
//!
//! Each layer is sparse and each wins over the one above it.
//!
//! # A ruleset is a diff, not a document
//!
//! ```ron
//! (
//!     name: "lean light",
//!     of: "default",
//!     notes: "docs/ECONOMY.md §10.1 — the only dial that moved the answer.",
//!     set: {
//!         "biology.metabolism.rates.light_occlusion": 128,
//!         "biology.metabolism.rates.rigidity_gain": 16384,
//!     },
//! )
//! ```
//!
//! Dotted paths, in the vocabulary [`crate::params`] already enumerates and the parameter editor
//! already speaks — so "save the current parameters as a named ruleset" is a diff of two field
//! lists rather than a new serialisation format. It also sidesteps the one thing a sparse
//! *document* cannot express: RON arrays are positional, so a document override cannot name the
//! fourth organelle spec without writing all sixteen, and `biology.metabolism.catalogue.specs.3.build_energy`
//! is precisely the kind of thing a balance pass wants to move.
//!
//! # Resolve at load, store resolved, keep the name as a label
//!
//! This is the rule that keeps hard rule 7 intact, and the codebase has made the same call once
//! before: [`crate::biology::Intervention`] stores a whole configuration rather than a "set field
//! X to V" delta, because "storing the configuration makes replay a copy, which cannot be wrong".
//!
//! A scenario that stored only a *reference* to a ruleset would change meaning whenever somebody
//! edited that file, and every archived run and every snapshot taken under it would stop
//! reproducing. So [`Scenario::ruleset`] is **provenance**: it records which named set the
//! parameters came from, and it is never re-applied over values that are already there.
//!
//! That is safe rather than merely conventional, and the reason is worth stating because it is
//! what makes the whole design work. Resolution merges *over* a complete base, and
//! `Scenario::to_ron` writes every field — a saved scenario carries all sixty-odd parameters
//! explicitly. So re-resolving a saved scenario merges a complete block over the ruleset, the
//! ruleset is masked entirely, and the operation is a copy. Resolution is idempotent, and
//! `a_saved_scenario_does_not_move_when_its_ruleset_does` is the test that says so.

use std::collections::BTreeMap;

use crate::chem::ChemTable;
use crate::config::VmConfig;
use crate::params;
use crate::scenario::Scenario;

/// Everything a ruleset may set: the rules of the simulation, as against the terrain of a world.
///
/// Chemistry is in here rather than in the terrain, and that is a judgement worth stating. A
/// scenario posing a different metabolic loop — `the_vent` wanting a chemosynthetic pathway — is
/// changing what is *possible*, not where things are, and two worlds that differ in their
/// chemistry are running different rules however similar their maps look.
///
/// What is deliberately *not* here: `seeding`, `barriers`, `light`, `current`, `flux`,
/// `inhabitants`, `width`, `height`, `seed`. Those are the world.
#[derive(Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Rules {
    pub biology: crate::biology::BiologyConfig,
    pub vm: VmConfig,
    pub chemicals: ChemTable,
}

/// A named set of parameter changes.
#[derive(Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Ruleset {
    /// What it is called. The file's stem is used when this is empty.
    pub name: String,
    /// The ruleset this one is a diff against. Empty means the engine's own defaults.
    ///
    /// A `String` rather than an `Option<String>` because these files are written by hand, and
    /// RON spells an absent option `None` and a present one `Some("x")` — so the honest type
    /// would make the common case read `of: Some("default")`. Empty-as-absent costs one line of
    /// code here and saves it in every file.
    pub of: String,
    /// Why it exists. Free text, for the wiki and for whoever reads it next.
    pub notes: String,
    /// The changes, as dotted paths into [`Rules`]. A `BTreeMap` because iteration order must
    /// never reach a simulation outcome (hard rule 6) — and because a sorted file diffs better.
    pub set: BTreeMap<String, ron::Value>,
}

/// What went wrong resolving a ruleset.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RulesetError {
    /// The file did not parse.
    Parse(String),
    /// A scenario or a ruleset named one that is not in the library.
    Unknown(String),
    /// `of` chains round to itself.
    Cycle(String),
    /// A path that names no parameter, or a value that will not fit it.
    ///
    /// Refused rather than ignored: a ruleset with a typo in a path would otherwise be a set of
    /// changes that silently did nothing, which is the worst possible failure for a file whose
    /// whole job is to change numbers.
    BadPath { ruleset: String, path: String },
}

impl std::fmt::Display for RulesetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RulesetError::Parse(e) => write!(f, "ruleset does not parse: {e}"),
            RulesetError::Unknown(n) => write!(f, "no ruleset named `{n}`"),
            RulesetError::Cycle(n) => write!(f, "ruleset `{n}` inherits from itself"),
            RulesetError::BadPath { ruleset, path } => write!(
                f,
                "ruleset `{ruleset}` sets `{path}`, which is not a parameter (or the value does \
                 not fit it)"
            ),
        }
    }
}

impl std::error::Error for RulesetError {}

/// Every ruleset that has been loaded, by name.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RulesetLibrary {
    sets: BTreeMap<String, Ruleset>,
}

/// How deep an `of` chain may go before it is called a cycle.
///
/// Sixteen. A chain longer than that is a mistake whatever it is, and a bound is what makes
/// resolution total rather than a stack overflow waiting for a badly-written pair of files.
const MAX_DEPTH: usize = 16;

impl RulesetLibrary {
    #[must_use]
    pub fn new() -> RulesetLibrary {
        RulesetLibrary::default()
    }

    /// Add one, parsed from its file's text. `name` is the file's stem, used when the document
    /// does not name itself.
    ///
    /// # Errors
    ///
    /// The document did not parse.
    pub fn insert(&mut self, name: &str, text: &str) -> Result<(), RulesetError> {
        let mut set: Ruleset =
            ron::from_str(text).map_err(|e| RulesetError::Parse(format!("{name}: {e}")))?;
        if set.name.is_empty() {
            set.name = name.to_string();
        }
        self.sets.insert(name.to_string(), set);
        Ok(())
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Ruleset> {
        self.sets.get(name)
    }

    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.sets.keys().map(String::as_str).collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// The rules a named set resolves to, with its `of` chain applied from the root down.
    ///
    /// # Errors
    ///
    /// An unknown name, a cycle, or a path that names no parameter.
    pub fn rules(&self, name: &str) -> Result<Rules, RulesetError> {
        // Walk up to the root first, then apply downwards, so a child's change beats its
        // parent's rather than being overwritten by it.
        let mut chain: Vec<&Ruleset> = Vec::new();
        let mut cursor = Some(name.to_string());
        while let Some(n) = cursor {
            if chain.len() >= MAX_DEPTH || chain.iter().any(|s| s.name == n) {
                return Err(RulesetError::Cycle(n));
            }
            let set = self.sets.get(&n).ok_or(RulesetError::Unknown(n.clone()))?;
            chain.push(set);
            cursor = if set.of.is_empty() {
                None
            } else {
                Some(set.of.clone())
            };
        }

        let mut rules = Rules::default();
        for set in chain.iter().rev() {
            for (path, value) in &set.set {
                let v = as_param(value).ok_or_else(|| RulesetError::BadPath {
                    ruleset: set.name.clone(),
                    path: path.clone(),
                })?;
                rules = params::set(&rules, path, v).ok_or_else(|| RulesetError::BadPath {
                    ruleset: set.name.clone(),
                    path: path.clone(),
                })?;
            }
        }
        Ok(rules)
    }

    /// Load a scenario from `.ron`, resolving whatever ruleset it names.
    ///
    /// **This is the entry point a front end should use**, not [`Scenario::from_ron`]. The
    /// difference is only visible for a file that names a ruleset — `from_ron` carries the name
    /// through and applies nothing, which for a hand-written file means the ruleset silently does
    /// not take effect.
    ///
    /// # Errors
    ///
    /// The scenario did not parse, or it names a ruleset this library does not have.
    pub fn load_scenario(&self, text: &str) -> Result<Scenario, RulesetError> {
        self.load_scenario_as(text, None)
    }

    /// The same, with the scenario's own choice of ruleset overridden.
    ///
    /// What `mm-cli balance --ruleset lean-light` runs on: hold the worlds fixed and vary the
    /// rules, which is the half of balancing the panel could not do before.
    ///
    /// # Errors
    ///
    /// The scenario did not parse, or the ruleset is not in this library.
    pub fn load_scenario_as(
        &self,
        text: &str,
        override_with: Option<&str>,
    ) -> Result<Scenario, RulesetError> {
        let mut scenario =
            Scenario::from_ron(text).map_err(|e| RulesetError::Parse(e.to_string()))?;
        let named = match override_with {
            Some(n) => n.to_string(),
            None => scenario.ruleset.clone(),
        };
        if named.is_empty() {
            // No ruleset in play: the file means exactly what it says, which is what it has
            // always meant.
            return Ok(scenario);
        }

        // Layer two, complete: the engine's defaults with the named set applied.
        let mut merged = to_value(&self.rules(&named)?)?;

        // Layer three, sparse: whatever the scenario names inline, merged over it.
        //
        // Only the three config sub-trees go through `ron::Value`, and that restriction is
        // load-bearing rather than tidiness. `ron::Value` does not carry an enum's variant name,
        // so a whole `Scenario` cannot survive the round trip — `CurrentField::Still` comes back
        // as a bare unit value and `into_rust` has nothing to turn it into. `Rules` holds only
        // structs, numbers, flags and options, none of which have that problem, so the merge
        // happens there and the terrain is never taken apart at all.
        let document: ron::Value =
            ron::from_str(text).map_err(|e| RulesetError::Parse(e.to_string()))?;
        for key in ["biology", "vm", "chemicals"] {
            let Some(over) = child(&document, key) else {
                continue;
            };
            let under = child(&merged, key).unwrap_or_else(|| over.clone());
            put(&mut merged, key, merge(under, over));
        }

        let rules: Rules = merged
            .into_rust()
            .map_err(|e| RulesetError::Parse(e.to_string()))?;
        scenario.biology = rules.biology;
        scenario.vm = rules.vm;
        scenario.chemicals = rules.chemicals;
        // Provenance, recorded now that it has been applied. Never read again — see the module
        // header, and `a_saved_scenario_does_not_move_when_its_ruleset_does`.
        scenario.ruleset = named;
        Ok(scenario)
    }
}

/// A `ron::Value` as a parameter value. Numbers and flags only, which is all a config holds.
fn as_param(v: &ron::Value) -> Option<params::Value> {
    match v {
        ron::Value::Number(n) => Some(params::Value::Int(n.into_f64() as i64)),
        ron::Value::Bool(b) => Some(params::Value::Bool(*b)),
        _ => None,
    }
}

fn to_value<T: serde::Serialize>(v: &T) -> Result<ron::Value, RulesetError> {
    let text = ron::to_string(v).map_err(|e| RulesetError::Parse(e.to_string()))?;
    ron::from_str(&text).map_err(|e| RulesetError::Parse(e.to_string()))
}

fn child(v: &ron::Value, key: &str) -> Option<ron::Value> {
    match v {
        ron::Value::Map(m) => m.get(&ron::Value::String(key.to_string())).cloned(),
        _ => None,
    }
}

fn put(v: &mut ron::Value, key: &str, value: ron::Value) {
    if let ron::Value::Map(m) = v {
        m.insert(ron::Value::String(key.to_string()), value);
    }
}

/// Deep-merge `over` onto `under`, `over` winning.
///
/// * **Maps** merge key by key, recursively. That is what makes a sparse override sparse.
/// * **Sequences of equal length** merge element by element, so a ruleset or a scenario can move
///   one organelle spec or one pathway without restating the other fifteen. Unequal lengths
///   replace outright, because two lists of different lengths are two different lists and there
///   is no honest way to line them up.
/// * **Everything else** is replaced.
fn merge(under: ron::Value, over: ron::Value) -> ron::Value {
    match (under, over) {
        (ron::Value::Map(mut u), ron::Value::Map(o)) => {
            for (key, value) in o.iter() {
                let merged = match u.get(key) {
                    Some(existing) => merge(existing.clone(), value.clone()),
                    None => value.clone(),
                };
                u.insert(key.clone(), merged);
            }
            ron::Value::Map(u)
        }
        (ron::Value::Seq(u), ron::Value::Seq(o)) if u.len() == o.len() => ron::Value::Seq(
            u.into_iter()
                .zip(o)
                .map(|(a, b)| merge(a, b))
                .collect(),
        ),
        (_, over) => over,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEAN: &str = r#"(
        name: "lean light",
        notes: "the measured setting from docs/ECONOMY.md",
        set: {
            "biology.metabolism.rates.light_occlusion": 128,
            "biology.metabolism.rates.rigidity_gain": 16384,
        },
    )"#;

    const LEANER: &str = r#"(
        name: "leaner",
        of: "lean",
        set: {
            "biology.metabolism.rates.light_occlusion": 256,
            "biology.division_energy": 4096,
        },
    )"#;

    fn library() -> RulesetLibrary {
        let mut lib = RulesetLibrary::new();
        lib.insert("lean", LEAN).expect("lean");
        lib.insert("leaner", LEANER).expect("leaner");
        lib
    }

    #[test]
    fn a_ruleset_changes_what_it_names_and_nothing_else() {
        let rules = library().rules("lean").expect("rules");
        assert_eq!(rules.biology.metabolism.rates.light_occlusion, 128);
        assert_eq!(rules.biology.metabolism.rates.rigidity_gain, 16_384);
        // Everything it did not name is the engine's own number.
        let base = Rules::default();
        assert_eq!(rules.biology.division_energy, base.biology.division_energy);
        assert_eq!(rules.vm, base.vm);
        assert_eq!(rules.chemicals, base.chemicals);
    }

    #[test]
    fn a_child_beats_its_parent() {
        let rules = library().rules("leaner").expect("rules");
        // Overridden by the child...
        assert_eq!(rules.biology.metabolism.rates.light_occlusion, 256);
        // ...inherited from the parent...
        assert_eq!(rules.biology.metabolism.rates.rigidity_gain, 16_384);
        // ...and the child's own addition.
        assert_eq!(rules.biology.division_energy, 4_096);
    }

    #[test]
    fn a_path_that_names_no_parameter_is_refused_rather_than_ignored() {
        // The worst failure for a file whose whole job is to change numbers would be a typo that
        // silently changed nothing.
        let mut lib = RulesetLibrary::new();
        lib.insert(
            "typo",
            r#"( set: { "biology.metabolism.rates.light_occulsion": 128 } )"#,
        )
        .expect("parses");
        let err = lib.rules("typo").expect_err("should refuse");
        assert!(
            matches!(err, RulesetError::BadPath { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn an_unknown_name_and_a_cycle_are_both_errors() {
        let lib = library();
        assert_eq!(
            lib.rules("nope"),
            Err(RulesetError::Unknown("nope".to_string()))
        );

        let mut looped = RulesetLibrary::new();
        looped
            .insert("a", r#"( name: "a", of: "b", set: {} )"#)
            .expect("a");
        looped
            .insert("b", r#"( name: "b", of: "a", set: {} )"#)
            .expect("b");
        assert!(matches!(looped.rules("a"), Err(RulesetError::Cycle(_))));
    }

    const WORLD: &str = r#"(
        name: "test world",
        seed: 7,
        width: 32,
        height: 32,
        ruleset: "lean",
    )"#;

    #[test]
    fn a_scenario_inherits_the_ruleset_it_names() {
        let s = library().load_scenario(WORLD).expect("scenario");
        assert_eq!(s.biology.metabolism.rates.light_occlusion, 128);
        assert_eq!(s.ruleset, "lean");
        assert_eq!(s.width, 32, "the world's own fields survived resolution");
    }

    #[test]
    fn a_scenario_beats_the_ruleset_it_names() {
        // Layer three. The whole point of the three of them.
        let text = r#"(
            name: "test world",
            ruleset: "lean",
            biology: (
                metabolism: (
                    rates: ( light_occlusion: 999 ),
                ),
            ),
        )"#;
        let s = library().load_scenario(text).expect("scenario");
        assert_eq!(s.biology.metabolism.rates.light_occlusion, 999, "inline lost");
        assert_eq!(
            s.biology.metabolism.rates.rigidity_gain,
            16_384,
            "naming one rate should not discard the rest of the ruleset"
        );
    }

    #[test]
    fn a_scenario_that_names_no_ruleset_is_untouched() {
        let text = r#"( name: "plain", width: 8, height: 8 )"#;
        let s = library().load_scenario(text).expect("scenario");
        assert_eq!(s.ruleset, "");
        assert_eq!(s.biology, crate::biology::BiologyConfig::default());
    }

    #[test]
    fn an_unknown_ruleset_stops_the_load_rather_than_being_skipped() {
        let text = r#"( name: "w", ruleset: "does-not-exist" )"#;
        assert!(matches!(
            library().load_scenario(text),
            Err(RulesetError::Unknown(_))
        ));
    }

    #[test]
    fn the_override_wins_over_the_scenarios_own_choice() {
        let s = library()
            .load_scenario_as(WORLD, Some("leaner"))
            .expect("scenario");
        assert_eq!(s.biology.metabolism.rates.light_occlusion, 256);
        assert_eq!(s.ruleset, "leaner");
    }

    #[test]
    fn a_saved_scenario_does_not_move_when_its_ruleset_does() {
        // **The load-bearing one** (hard rule 7). A scenario stores its parameters resolved and
        // keeps the ruleset's name only as a label, so a run saved today reproduces exactly even
        // if somebody edits that ruleset tomorrow. If this ever fails, every archived run and
        // every snapshot taken under a named ruleset has stopped being reproducible.
        let saved = library().load_scenario(WORLD).expect("scenario");
        let text = saved.to_ron().expect("to_ron");

        // The world moves on: the ruleset is edited to say something quite different.
        let mut later = RulesetLibrary::new();
        later
            .insert(
                "lean",
                r#"( name: "lean light", set: {
                    "biology.metabolism.rates.light_occlusion": 1,
                    "biology.division_energy": 1,
                } )"#,
            )
            .expect("lean");

        // Both routes back must give the scenario that was saved, not the one the file now
        // describes: `from_ron` because it applies nothing, and `load_scenario` because a saved
        // document is *complete*, so merging it over the ruleset masks the ruleset entirely.
        let plain = Scenario::from_ron(&text).expect("from_ron");
        let resolved = later.load_scenario(&text).expect("load_scenario");
        assert_eq!(plain.biology, saved.biology, "from_ron drifted");
        assert_eq!(
            resolved.biology, saved.biology,
            "re-resolving a saved scenario picked up the edited ruleset"
        );
        assert_eq!(resolved.ruleset, "lean", "lost the label");
    }

    #[test]
    fn resolving_twice_is_the_same_as_resolving_once() {
        // The property the test above rests on, on its own.
        let lib = library();
        let once = lib.load_scenario(WORLD).expect("once");
        let twice = lib
            .load_scenario(&once.to_ron().expect("to_ron"))
            .expect("twice");
        assert_eq!(once, twice);
    }

    #[test]
    fn a_ruleset_can_reach_one_organelle_and_one_pathway() {
        // The reason a ruleset is a set of dotted paths and not a sparse document: RON arrays are
        // positional, so a document override cannot name the fourth spec without writing all
        // sixteen, and this is exactly what a balance pass wants to move.
        let mut lib = RulesetLibrary::new();
        lib.insert(
            "cheap-spikes",
            r#"( set: {
                "biology.metabolism.catalogue.specs.12.build_energy": 4096,
                "biology.metabolism.catalogue.metabolism.pathways.1.substrate": 10,
                "chemicals.8.energy_yield": 2048,
                "vm.instr_per_tick": 32,
            } )"#,
        )
        .expect("parses");
        let rules = lib.rules("cheap-spikes").expect("rules");
        assert_eq!(
            rules.biology.metabolism.catalogue.specs()
                [crate::organelle::OrganelleType::Spike as usize]
                .build_energy,
            4_096
        );
        assert_eq!(
            rules.biology.metabolism.catalogue.metabolism.pathways[1].substrate,
            10
        );
        assert_eq!(rules.chemicals.get(8).energy_yield, 2_048);
        assert_eq!(rules.vm.instr_per_tick, 32);
        // And the fifteen specs it did not name are untouched.
        let base = Rules::default();
        assert_eq!(
            rules.biology.metabolism.catalogue.specs()[3],
            base.biology.metabolism.catalogue.specs()[3]
        );
    }
}
