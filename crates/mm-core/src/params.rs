//! Reading and writing configuration parameters by name (M10.2).
//!
//! The parameter editor has to enumerate every knob in [`crate::biology::BiologyConfig`] and
//! change one, without a line of code per knob. Sixty hand-written getters and setters is sixty
//! chances for one to be missing, and the missing one is invisible: the parameter simply does
//! not appear in the editor, and nobody notices until they go looking for it.
//!
//! So the traversal goes through the serialised form. Every config struct already derives
//! `Serialize` and `Deserialize`, because they live in the scenario file — which means the
//! names here and the names in the file are the same names by construction, and a parameter
//! added to the struct appears in the editor without anybody remembering to add it.
//!
//! Paths are dotted, and array elements are indexed: `division_energy`,
//! `metabolism.rates.repair_per_tick`, `metabolism.catalogue.specs.3.build_energy`.

use serde::{de::DeserializeOwned, Serialize};

/// A parameter's value. Everything in a config is an integer or a flag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Value {
    Int(i64),
    Bool(bool),
}

impl Value {
    /// The integer, or zero for a flag — for the places that only draw numbers.
    #[must_use]
    pub fn as_int(self) -> i64 {
        match self {
            Value::Int(v) => v,
            Value::Bool(v) => i64::from(v),
        }
    }

    #[must_use]
    pub fn is_bool(self) -> bool {
        matches!(self, Value::Bool(_))
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(v) => write!(f, "{v}"),
            Value::Bool(v) => write!(f, "{v}"),
        }
    }
}

/// Every scalar in a config, as a dotted path and its current value.
///
/// Sorted by path, because that is the order the underlying map keeps and an unstable order
/// would make the editor's rows jump about. Grouping them sensibly is the caller's job; this
/// answers only "what is there".
#[must_use]
pub fn fields<T: Serialize>(config: &T) -> Vec<(String, Value)> {
    let Ok(text) = ron::to_string(config) else {
        return Vec::new();
    };
    let Ok(root) = ron::from_str::<ron::Value>(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk(&root, &mut String::new(), &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Read one parameter.
#[must_use]
pub fn get<T: Serialize>(config: &T, path: &str) -> Option<Value> {
    fields(config)
        .into_iter()
        .find(|(p, _)| p == path)
        .map(|(_, v)| v)
}

/// Set one parameter, returning the changed config.
///
/// `None` if there is no such path, or if the value does not fit the field — which is not an
/// error worth a type for, because the editor's job is to offer what `fields` reported and
/// anything else is a caller bug.
///
/// The value is *validated by deserialisation*: it goes back through the same parser the
/// scenario file uses, so a number too large for an `i32` field is refused here rather than
/// silently truncated, and a config that comes out of this is a config that would have loaded
/// from a file.
#[must_use]
pub fn set<T: Serialize + DeserializeOwned>(config: &T, path: &str, value: Value) -> Option<T> {
    let text = ron::to_string(config).ok()?;
    let mut root: ron::Value = ron::from_str(&text).ok()?;
    if !place(&mut root, path, value) {
        return None;
    }
    // Straight from the value tree, not back through text. `ron::to_string` of a `Value::Map`
    // emits a *map* literal — `{"a": 1}` — and RON, unlike JSON, will not read one of those
    // into a struct: it wants `(a: 1)`. The tree does not know it came from a struct, so the
    // only way round is not to ask it to.
    root.into_rust().ok()
}

/// Collect every scalar leaf, depth first, building the dotted path as it goes.
fn walk(node: &ron::Value, path: &mut String, out: &mut Vec<(String, Value)>) {
    match node {
        ron::Value::Map(map) => {
            for (key, child) in map.iter() {
                let ron::Value::String(name) = key else {
                    // A map keyed by anything but a string is not a struct, and nothing in a
                    // config is one. Skipped rather than guessed at.
                    continue;
                };
                let mark = path.len();
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(name);
                walk(child, path, out);
                path.truncate(mark);
            }
        }
        ron::Value::Seq(items) => {
            for (i, child) in items.iter().enumerate() {
                let mark = path.len();
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(&i.to_string());
                walk(child, path, out);
                path.truncate(mark);
            }
        }
        ron::Value::Number(n) => {
            out.push((path.clone(), Value::Int(number_as_i64(n))));
        }
        ron::Value::Bool(b) => {
            out.push((path.clone(), Value::Bool(*b)));
        }
        // Strings, chars, options and units are not parameters. A config has none of them
        // today; if one appears, it is not editable as a number and this is where to say so.
        _ => {}
    }
}

/// Replace the scalar at `path`. `false` if the path does not lead to one.
fn place(node: &mut ron::Value, path: &str, value: Value) -> bool {
    let (head, rest) = match path.split_once('.') {
        Some((h, r)) => (h, Some(r)),
        None => (path, None),
    };
    match node {
        ron::Value::Map(map) => {
            let key = ron::Value::String(head.to_string());
            let Some(child) = map.get_mut(&key) else {
                return false;
            };
            match rest {
                Some(rest) => place(child, rest, value),
                None => assign(child, value),
            }
        }
        ron::Value::Seq(items) => {
            let Ok(i) = head.parse::<usize>() else {
                return false;
            };
            let Some(child) = items.get_mut(i) else {
                return false;
            };
            match rest {
                Some(rest) => place(child, rest, value),
                None => assign(child, value),
            }
        }
        _ => false,
    }
}

/// Write a scalar over a leaf, keeping its kind.
///
/// A number stays a number and a flag stays a flag. Writing an integer over a boolean would
/// produce a document that no longer deserialises, and the failure would surface as "that
/// parameter cannot be set" three layers away from the cause.
fn assign(leaf: &mut ron::Value, value: Value) -> bool {
    match (&leaf, value) {
        (ron::Value::Number(_), Value::Int(v)) => {
            *leaf = ron::Value::Number(ron::value::Number::new(v));
            true
        }
        (ron::Value::Bool(_), Value::Bool(v)) => {
            *leaf = ron::Value::Bool(v);
            true
        }
        // An integer offered for a flag is taken as one, because a form that draws every
        // parameter as a number will do exactly that.
        (ron::Value::Bool(_), Value::Int(v)) => {
            *leaf = ron::Value::Bool(v != 0);
            true
        }
        _ => false,
    }
}

fn number_as_i64(n: &ron::value::Number) -> i64 {
    // `Number` is an enum over every width serde might have produced — a field holding 20480
    // comes back as a `U16` and the same field holding -1 as an `I8`. Going through the float
    // accessor rather than matching eleven variants is exact for everything a config holds,
    // which is integers well inside a f64's 53 bits of mantissa.
    n.into_f64() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biology::BiologyConfig;

    #[test]
    fn every_parameter_is_reachable_by_name() {
        let config = BiologyConfig::default();
        let found = fields(&config);
        assert!(!found.is_empty());

        // The ones a person is most likely to want, spelled the way the file spells them.
        for path in [
            "division_energy",
            "division_matter",
            "copy_energy_per_byte",
            "mutation.point",
            "mutation.duplication",
            "metabolism.rates.repair_per_tick",
            "metabolism.rates.background_damage",
            "metabolism.rates.metabolic_floor",
            "metabolism.catalogue.metabolism.structural",
            "metabolism.catalogue.metabolism.pathways.0.substrate",
            "metabolism.catalogue.metabolism.pathways.2.waste",
            "junctions.join_forced_penalty",
            "junctions.probe_leaks_distance",
            "ecology.spike_damage",
        ] {
            assert!(
                found.iter().any(|(p, _)| p == path),
                "{path} is not reachable"
            );
        }
    }

    #[test]
    fn the_organelle_catalogue_is_reachable_entry_by_entry() {
        // A sequence, not a struct, so it exercises the indexed half of the path grammar. The
        // catalogue is where balancing actually happens, so being unable to reach it would
        // make the editor decorative.
        let config = BiologyConfig::default();
        let found = fields(&config);
        let entries: Vec<&(String, Value)> = found
            .iter()
            .filter(|(p, _)| p.starts_with("metabolism.catalogue.specs."))
            .collect();
        assert!(
            entries.len() >= crate::organelle::SLOT_COUNT,
            "only {} catalogue fields found",
            entries.len()
        );
    }

    #[test]
    fn setting_a_parameter_changes_that_parameter_and_no_other() {
        let before = BiologyConfig::default();
        let after = set(&before, "division_energy", Value::Int(12_345)).expect("set");
        assert_eq!(after.division_energy, 12_345);

        // Everything else is untouched, checked by comparing the whole field list rather than
        // by spot checks — a setter that reset the catalogue while writing one number would
        // pass any number of spot checks.
        let a = fields(&before);
        let b = fields(&after);
        assert_eq!(a.len(), b.len());
        for ((path, was), (_, now)) in a.iter().zip(b.iter()) {
            if path == "division_energy" {
                continue;
            }
            assert_eq!(was, now, "{path} changed too");
        }
    }

    #[test]
    fn a_nested_parameter_and_a_catalogue_entry_can_both_be_set() {
        let config = BiologyConfig::default();
        let config = set(&config, "metabolism.rates.repair_per_tick", Value::Int(7)).expect("rate");
        assert_eq!(config.metabolism.rates.repair_per_tick, 7);

        let config = set(
            &config,
            "metabolism.catalogue.specs.3.build_energy",
            Value::Int(4_096),
        )
        .expect("catalogue");
        assert_eq!(config.metabolism.catalogue.specs()[3].build_energy, 4_096);
        // And the first change survived the second.
        assert_eq!(config.metabolism.rates.repair_per_tick, 7);
    }

    #[test]
    fn a_negative_value_survives() {
        // Config numbers are unsigned in the file only because their defaults happen to be
        // positive. A parameter that can go below zero has to be able to.
        let config = BiologyConfig::default();
        let config = set(&config, "metabolism.rates.growth_rate", Value::Int(-42)).expect("set");
        assert_eq!(config.metabolism.rates.growth_rate, -42);
    }

    #[test]
    fn a_flag_can_be_set_either_way_it_is_offered() {
        let config = BiologyConfig::default();
        assert!(!config.junctions.probe_leaks_distance);

        let by_bool =
            set(&config, "junctions.probe_leaks_distance", Value::Bool(true)).expect("bool");
        assert!(by_bool.junctions.probe_leaks_distance);

        // A form that draws everything as a number will offer an integer, and that has to work
        // rather than silently failing to apply.
        let by_int = set(&config, "junctions.probe_leaks_distance", Value::Int(1)).expect("int");
        assert!(by_int.junctions.probe_leaks_distance);
    }

    #[test]
    fn a_value_that_does_not_fit_is_refused_rather_than_truncated() {
        // The fields are `i32`. Offering more than one holds must not wrap round to a small
        // number and be applied as though it were what was asked for.
        let config = BiologyConfig::default();
        assert!(set(
            &config,
            "division_energy",
            Value::Int(i64::from(i32::MAX) + 1)
        )
        .is_none());
        assert_eq!(
            BiologyConfig::default().division_energy,
            config.division_energy,
            "a refused set changed the config anyway"
        );
    }

    #[test]
    fn a_path_that_does_not_exist_is_refused() {
        let config = BiologyConfig::default();
        assert!(set(&config, "no_such_field", Value::Int(1)).is_none());
        assert!(set(&config, "mutation.no_such_field", Value::Int(1)).is_none());
        assert!(
            set(&config, "mutation", Value::Int(1)).is_none(),
            "set a whole struct"
        );
        assert!(set(
            &config,
            "metabolism.catalogue.specs.999.upkeep",
            Value::Int(1)
        )
        .is_none());
        assert!(get(&config, "no_such_field").is_none());
    }

    #[test]
    fn a_config_round_trips_through_its_own_field_list() {
        // Read every parameter and write every one back unchanged. Anything that does not
        // survive that is a field this module can enumerate but not preserve, which would show
        // up in the editor as a number that resets itself when you touch a different one.
        let original = BiologyConfig::default();
        let mut rebuilt = original.clone();
        for (path, value) in fields(&original) {
            rebuilt = set(&rebuilt, &path, value).unwrap_or_else(|| panic!("{path} did not set"));
        }
        assert_eq!(rebuilt, original);
    }
}
