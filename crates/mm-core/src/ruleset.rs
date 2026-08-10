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
//! # Four layers
//!
//! ```text
//!     Rules::default()          the engine's own numbers
//!  →  the named ruleset          a diff, which may itself name a parent
//!  →  the scenario's own block    whatever the .ron file says inline
//!  →  the scenario's `set`        its own dotted paths, the last word
//! ```
//!
//! Each layer is sparse and each wins over the one above it.
//!
//! The fourth is [`Scenario::set`], and it exists because the third cannot reach an array
//! element — the same positional problem described below, met from the scenario's side rather
//! than the ruleset's. It is also what [`Scenario::to_ron_sparse`] writes, which is what stopped
//! a scenario saved out of the microscope from being four hundred lines of the engine's own
//! numbers restated. Unlike `ruleset` it names no file outside itself, so `Scenario::from_ron`
//! applies it and a file carrying one still means exactly what it says.
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
//!
//! # Two forms, and the trade between them
//!
//! The paragraph above is about `Scenario::to_ron`, and it is why that function still writes
//! every field: a `.mmslide` embeds one and reads it back with `Scenario::from_ron`, which
//! applies no ruleset, so a saved run must not depend on any file outside itself.
//!
//! [`Scenario::to_ron_sparse`] is the other form, and it makes the opposite trade deliberately.
//! A file written that way names its ruleset and inherits from it, so **it means what that
//! ruleset says today** — edit `rulesets/rival_light.ron` and every scenario naming it moves.
//! That is the point: it is what makes one file's numbers reach a whole library. It is also
//! exactly what `the_thicket.ron` has done since rulesets landed, so the sparse form is not a
//! new hazard, it is the hand-written form with a writer behind it.
//!
//! The rule to keep the two straight: **a recipe inherits, a record does not.** Scenario files
//! are recipes. Snapshots and archived runs are records.

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

impl Rules {
    /// The rules half of a scenario, lifted out.
    ///
    /// The inverse of what [`RulesetLibrary::load_scenario_as`] puts back, and the thing to
    /// compare against a baseline when the question is "what does this world change".
    #[must_use]
    pub fn of(scenario: &Scenario) -> Rules {
        Rules {
            biology: scenario.biology.clone(),
            vm: scenario.vm,
            chemicals: scenario.chemicals.clone(),
        }
    }

    /// Write these rules into a scenario, leaving its terrain alone.
    pub fn apply_to(&self, scenario: &mut Scenario) {
        scenario.biology = self.biology.clone();
        scenario.vm = self.vm;
        scenario.chemicals = self.chemicals.clone();
    }
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

impl Ruleset {
    /// A ruleset from a computed [`params::diff`], ready to be written to `rulesets/<name>.ron`.
    ///
    /// What the module header said this format was chosen to make possible: "save the current
    /// parameters as a named ruleset is a diff of two field lists rather than a new serialisation
    /// format".
    #[must_use]
    pub fn from_diff(
        name: &str,
        of: &str,
        notes: &str,
        set: &BTreeMap<String, params::Value>,
    ) -> Ruleset {
        Ruleset {
            name: name.to_string(),
            of: of.to_string(),
            notes: notes.to_string(),
            set: set
                .iter()
                .map(|(path, value)| (path.clone(), value.to_ron()))
                .collect(),
        }
    }

    /// Render to `.ron`.
    ///
    /// # Errors
    ///
    /// Serialisation failure, which should not happen for a well-formed ruleset.
    pub fn to_ron(&self) -> Result<String, RulesetError> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| RulesetError::Parse(e.to_string()))
    }
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
    /// A fault in the scenario itself, met while resolving it — its own `set` block naming no
    /// parameter, most likely.
    ///
    /// Its own variant rather than a `Parse`, because the message that came out of that was
    /// "ruleset does not parse: scenario sets `biology.divisian_energy`…", which sends whoever
    /// made the typo to look at the wrong file.
    Scenario(String),
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
            RulesetError::Scenario(m) => write!(f, "{m}"),
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
                let v = params::Value::from_ron(value).ok_or_else(|| RulesetError::BadPath {
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

    /// The scenario a file naming `name` starts from: [`Scenario::default`] with that ruleset
    /// resolved into it. An empty name means the engine's own numbers.
    ///
    /// **What [`Scenario::to_ron_sparse`] should be given.** Written against this, a saved file
    /// says exactly what this scenario adds to the layers underneath it and nothing else — which
    /// is the same file a person would have written by hand.
    ///
    /// # Errors
    ///
    /// An unknown name, a cycle, or a path that names no parameter.
    pub fn baseline(&self, name: &str) -> Result<Scenario, RulesetError> {
        let mut scenario = Scenario::default();
        if !name.is_empty() {
            self.rules(name)?.apply_to(&mut scenario);
            scenario.ruleset = name.to_string();
        }
        Ok(scenario)
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
        // Only a genuine syntax error is a parse error. An ISA mismatch, a slide that cannot
        // exist, or a `set` block with a typo in it are all faults in the scenario, and calling
        // them "ruleset does not parse" sends whoever made one to look at the wrong file.
        let mut scenario = Scenario::from_ron(text).map_err(|e| match e {
            crate::scenario::ScenarioError::Parse(m) => RulesetError::Parse(m),
            other => RulesetError::Scenario(other.to_string()),
        })?;
        let named = match override_with {
            Some(n) => n.to_string(),
            None => scenario.ruleset.clone(),
        };
        if named.is_empty() {
            // No ruleset in play: the file means exactly what it says, which is what it has
            // always meant. `from_ron` has already applied the scenario's own `set`.
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

        // Layer four, and the last word: the scenario's own dotted paths. `from_ron` has already
        // applied these once, over the engine's defaults — and the merge above has just
        // overwritten that with the ruleset's numbers, so they go on again here. Applying twice
        // is harmless: `set` writes the values it names.
        //
        // Last because it is the most specific thing the file says, and because it is what
        // `Scenario::to_ron_sparse` writes — a saved world must come back meaning what it meant.
        scenario
            .apply_set()
            .map_err(|e| RulesetError::Scenario(e.to_string()))?;

        // Provenance, recorded now that it has been applied. Never read again — see the module
        // header, and `a_saved_scenario_does_not_move_when_its_ruleset_does`.
        scenario.ruleset = named;
        Ok(scenario)
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
    fn a_world_saved_against_its_ruleset_does_not_restate_it() {
        // The whole point of the sparse form, in the case that matters most: `the_thicket.ron`
        // names `rival_light` and adds nothing of its own, so saving it should say the name and
        // stop. Before this it came back as four hundred and thirty-six lines with those two
        // rates written out — and from then on editing the ruleset could not reach it.
        let lib = library();
        let world = lib.load_scenario(WORLD).expect("scenario");
        let text = world
            .to_ron_sparse(&lib.baseline("lean").expect("baseline"))
            .expect("to_ron_sparse");

        assert!(text.contains(r#"ruleset: "lean""#), "lost the name:\n{text}");
        assert!(
            // By line rather than by substring, because `ruleset:` ends in `set:`.
            !text.lines().any(|l| l.trim_start().starts_with("set:")),
            "restated the ruleset it inherits:\n{text}"
        );
        assert!(text.lines().count() < 12, "not sparse:\n{text}");

        // And it still runs the economy it asked for when it comes back.
        let back = lib.load_scenario(&text).expect("reload");
        assert_eq!(back.biology, world.biology);
        assert_eq!(back.width, world.width);
    }

    #[test]
    fn a_world_that_adds_to_its_ruleset_saves_only_what_it_added() {
        let lib = library();
        let mut world = lib.load_scenario(WORLD).expect("scenario");
        world.biology.division_energy = 777;

        let text = world
            .to_ron_sparse(&lib.baseline("lean").expect("baseline"))
            .expect("to_ron_sparse");
        assert!(
            text.contains(r#""biology.division_energy": 777"#),
            "the addition did not reach the page:\n{text}"
        );
        assert!(
            !text.contains("light_occlusion"),
            "the ruleset's own numbers came along too:\n{text}"
        );

        let back = lib.load_scenario(&text).expect("reload");
        assert_eq!(back.biology.division_energy, 777, "the scenario's own");
        assert_eq!(
            back.biology.metabolism.rates.light_occlusion, 128,
            "the ruleset's, inherited rather than written down"
        );
    }

    #[test]
    fn a_sparse_world_follows_its_ruleset_when_that_ruleset_moves() {
        // The cost of the sparse form, asserted rather than left implied — and the reason
        // `to_ron` still writes everything. A file that inherits its numbers *means what its
        // ruleset says today*: that is what makes editing one file change a library, and it is
        // exactly what a snapshot must never do.
        let text = library()
            .load_scenario(WORLD)
            .expect("scenario")
            .to_ron_sparse(&library().baseline("lean").expect("baseline"))
            .expect("to_ron_sparse");

        let mut later = RulesetLibrary::new();
        later
            .insert(
                "lean",
                r#"( name: "lean light", set: {
                    "biology.metabolism.rates.light_occlusion": 1,
                } )"#,
            )
            .expect("lean");
        let moved = later.load_scenario(&text).expect("reload");
        assert_eq!(moved.biology.metabolism.rates.light_occlusion, 1);
    }

    #[test]
    fn a_worlds_parameters_can_be_lifted_out_as_a_named_ruleset() {
        // "Save these as a ruleset" — the operation the module header said this format was chosen
        // to make possible, end to end: diff two field lists, write the file, read it back, and
        // get the same rules.
        let lib = library();
        let mut world = lib.load_scenario(WORLD).expect("scenario");
        world.biology.division_energy = 777;
        world.chemicals = {
            let mut defs: Vec<crate::chem::ChemicalDef> = world.chemicals.clone().into();
            defs[8].energy_yield = 2_048;
            crate::chem::ChemTable::new(defs)
        };

        let changes = params::diff(&Rules::default(), &Rules::of(&world));
        let file = Ruleset::from_diff("thicket economy", "", "measured", &changes)
            .to_ron()
            .expect("to_ron");

        let mut saved = RulesetLibrary::new();
        saved.insert("thicket_economy", &file).expect("parses");
        let rules = saved.rules("thicket_economy").expect("rules");
        assert_eq!(
            rules,
            Rules::of(&world),
            "a world's rules did not survive being written out as a named set"
        );
    }

    #[test]
    fn a_fault_in_the_scenario_is_not_reported_as_a_fault_in_the_ruleset() {
        // A typo in a world's own `set` block came out as "ruleset does not parse: scenario sets
        // `biology.divisian_energy`…", which names two files and blames the wrong one. Whoever
        // made the typo has to be sent to the file they made it in.
        let text = r#"(
            name: "typo",
            ruleset: "lean",
            set: { "biology.divisian_energy": 4096 },
        )"#;
        let e = library().load_scenario(text).expect_err("should refuse");
        assert!(matches!(e, RulesetError::Scenario(_)), "got {e:?}");
        let said = e.to_string();
        assert!(said.contains("biology.divisian_energy"), "unhelpful: {said}");
        assert!(!said.contains("does not parse"), "blames the ruleset: {said}");
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
