//! What the parameter editor draws (M10.2).
//!
//! [`mm_core::params`] answers "what parameters are there and how do I change one" generically,
//! through the serialised form, so no code here has to know a field exists for it to be
//! editable. This module answers the questions that generic traversal cannot: what to *call*
//! each one, which group it belongs in, and what its number means.
//!
//! # On the notes
//!
//! They are condensed from the doc comments on the structs themselves, which are better than
//! anything that would be written here from scratch. That is a duplication and it can drift.
//! Rust has no way to read a doc comment at runtime without a proc macro or a build script,
//! and neither is worth carrying for hover text — but [`every_parameter_has_a_description`]
//! at least guarantees that a *new* parameter cannot appear without somebody writing one.

use mm_core::params::Value;

/// Which tab a parameter appears under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Division,
    Mutation,
    Metabolism,
    Chemistry,
    Junctions,
    Ecology,
}

impl Group {
    pub const ALL: [Group; 6] = [
        Group::Division,
        Group::Mutation,
        Group::Metabolism,
        Group::Chemistry,
        Group::Junctions,
        Group::Ecology,
    ];

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Group::Division => "division",
            Group::Mutation => "mutation",
            Group::Metabolism => "metabolism",
            Group::Chemistry => "chemistry",
            Group::Junctions => "junctions",
            Group::Ecology => "ecology",
        }
    }
}

/// What a parameter's number means, for the reading shown beside the raw value.
///
/// The editable field is always the raw integer, because that is what the scenario file holds
/// and what a person comparing two files will see. The reading is a courtesy: `20480` is
/// unreadable and `20.0` is obvious, but only one of them is the truth.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    /// Fixed point, 1024 = one.
    Q10,
    /// Fixed point where the whole range is a fraction: shown as a percentage.
    Fraction,
    /// Chances in the mutation scale, shown as one-in-N.
    Chance,
    /// Position fixed point, 256 = one square.
    Pos,
    /// An index into the chemical table: shown as the chemical's name.
    Chemical,
    /// A plain count.
    Count,
    /// On or off.
    Flag,
}

/// One editable parameter.
pub struct Field {
    /// The path [`mm_core::params`] knows it by, and the name the scenario file uses.
    pub path: &'static str,
    pub label: &'static str,
    pub group: Group,
    pub unit: Unit,
    pub note: &'static str,
}

impl Field {
    /// The human reading of a raw value, or `None` when the raw value is already the reading.
    #[must_use]
    pub fn reading(&self, value: Value, chemicals: &[String]) -> Option<String> {
        let v = value.as_int();
        match self.unit {
            Unit::Q10 => Some(format!("{:.2}", v as f64 / 1024.0)),
            Unit::Fraction => Some(format!("{:.1}%", v as f64 * 100.0 / 1024.0)),
            Unit::Chance if v <= 0 => Some("never".to_string()),
            // The mutation scale is chances in 2^16 per byte. One-in-N is the form a person can
            // reason about; "eighty-two" is not.
            Unit::Chance => Some(format!("1 in {}", 65_536 / v.max(1))),
            Unit::Pos => Some(format!("{:.2} squares", v as f64 / 256.0)),
            // `usize::try_from` rather than clamping to zero: a negative index is not chemical
            // zero, it is a value that names no chemical, and saying "carbon" for it would be
            // a confident lie about a field somebody has just mistyped.
            Unit::Chemical => usize::try_from(v)
                .ok()
                .and_then(|i| chemicals.get(i))
                .cloned(),
            Unit::Count | Unit::Flag => None,
        }
    }
}

/// Every parameter, in the order the editor lists them.
///
/// Grouped by what a person is trying to change rather than by which struct it lives in — the
/// organelle catalogue's chemistry sits with the metabolic rates because that is the question
/// it answers, not three levels down a path called `metabolism.catalogue.metabolism`.
pub const FIELDS: &[Field] = &[
    // --- division ---
    Field {
        path: "division_matter",
        label: "division matter",
        group: Group::Division,
        unit: Unit::Q10,
        note: "structural matter a daughter needs beyond half the parent's",
    },
    Field {
        path: "division_energy",
        label: "division energy",
        group: Group::Division,
        unit: Unit::Q10,
        note: "energy a division costs outright",
    },
    Field {
        path: "copy_energy_per_byte",
        label: "copy energy per byte",
        group: Group::Division,
        unit: Unit::Q10,
        note: "energy per genome byte copied at full fidelity — accuracy is not free",
    },
    Field {
        path: "structural_chemical",
        label: "structural chemical",
        group: Group::Division,
        unit: Unit::Chemical,
        note: "which chemical a body is built out of. Must match the metabolism's",
    },
    // --- mutation ---
    Field {
        path: "mutation.point",
        label: "point",
        group: Group::Mutation,
        unit: Unit::Chance,
        note: "a copied byte comes out different",
    },
    Field {
        path: "mutation.insertion",
        label: "insertion",
        group: Group::Mutation,
        unit: Unit::Chance,
        note: "a run of bytes appears",
    },
    Field {
        path: "mutation.deletion",
        label: "deletion",
        group: Group::Mutation,
        unit: Unit::Chance,
        note: "a run of bytes goes",
    },
    Field {
        path: "mutation.duplication",
        label: "duplication",
        group: Group::Mutation,
        unit: Unit::Chance,
        note: "a run of bytes is copied. The operator new genes come from, and CLAUDE.md \
               requires it to exist",
    },
    Field {
        path: "mutation.inversion",
        label: "inversion",
        group: Group::Mutation,
        unit: Unit::Chance,
        note: "a run of bytes is reversed",
    },
    Field {
        path: "mutation.translocation",
        label: "translocation",
        group: Group::Mutation,
        unit: Unit::Chance,
        note: "a run of bytes moves elsewhere",
    },
    Field {
        path: "mutation.max_segment",
        label: "max segment",
        group: Group::Mutation,
        unit: Unit::Count,
        note: "longest run a structural operator may act on, in bytes",
    },
    Field {
        path: "mutation.copy_error_max",
        label: "copy error ceiling",
        group: Group::Mutation,
        unit: Unit::Chance,
        note: "per-byte error rate at zero nucleus fidelity — what the nucleus is buying down",
    },
    // --- metabolism ---
    Field {
        path: "metabolism.rates.photosynthesis_efficiency",
        label: "photosynthesis efficiency",
        group: Group::Metabolism,
        unit: Unit::Fraction,
        note: "absorbed light that ends up banked as substrate rather than heat",
    },
    Field {
        path: "metabolism.rates.respiration_efficiency",
        label: "respiration efficiency",
        group: Group::Metabolism,
        unit: Unit::Fraction,
        note: "a substrate's latent energy a mitochondrion recovers",
    },
    Field {
        path: "metabolism.rates.reactive_fraction",
        label: "reactive fraction",
        group: Group::Metabolism,
        unit: Unit::Fraction,
        note: "respiration's exhaust that comes out as poison rather than inert. The cost of \
               breathing, and why a well-fed cell is not immortal",
    },
    Field {
        path: "metabolism.rates.throughput_per_param",
        label: "throughput per param",
        group: Group::Metabolism,
        unit: Unit::Q10,
        note: "matter one unit of an organelle's size can convert per tick",
    },
    Field {
        path: "metabolism.rates.latent_per_substrate",
        label: "latent per substrate",
        group: Group::Metabolism,
        unit: Unit::Q10,
        note: "energy per unit of substrate, when the chemical table says nothing",
    },
    Field {
        path: "metabolism.rates.toxicity_threshold",
        label: "toxicity threshold",
        group: Group::Metabolism,
        unit: Unit::Q10,
        note: "how much of a toxin a cell tolerates before it takes damage",
    },
    Field {
        path: "metabolism.rates.growth_rate",
        label: "growth rate",
        group: Group::Metabolism,
        unit: Unit::Q10,
        note: "structural matter moved from cytoplasm into body per tick. Without it a lineage \
               halves in mass every division and stops after five or six",
    },
    Field {
        path: "metabolism.rates.repair_per_tick",
        label: "repair per tick",
        group: Group::Metabolism,
        unit: Unit::Q10,
        note: "damage a cell can mend per tick. A fixed capacity, not a fraction — which is \
               what gives senescence a cause",
    },
    Field {
        path: "metabolism.rates.repair_energy_per_unit",
        label: "repair energy per unit",
        group: Group::Metabolism,
        unit: Unit::Q10,
        note: "energy each unit of repair costs. A cell that cannot pay cannot mend",
    },
    Field {
        path: "metabolism.rates.background_damage",
        label: "background damage",
        group: Group::Metabolism,
        unit: Unit::Q10,
        note: "wear per tick that nothing inflicts. The nudge into oblivion for a cell that \
               does not respire and would otherwise have no clock at all",
    },
    Field {
        path: "metabolism.rates.metabolic_floor",
        label: "metabolic floor",
        group: Group::Metabolism,
        unit: Unit::Q10,
        note: "upkeep every cell pays for being alive, before its organelles",
    },
    // --- chemistry ---
    Field {
        path: "metabolism.catalogue.metabolism.substrate",
        label: "substrate",
        group: Group::Chemistry,
        unit: Unit::Chemical,
        note: "burned by a mitochondrion for energy",
    },
    Field {
        path: "metabolism.catalogue.metabolism.oxidant",
        label: "oxidant",
        group: Group::Chemistry,
        unit: Unit::Chemical,
        note: "consumed alongside the substrate",
    },
    Field {
        path: "metabolism.catalogue.metabolism.waste",
        label: "waste",
        group: Group::Chemistry,
        unit: Unit::Chemical,
        note: "produced by burning. A chloroplast turns this back into substrate — the loop \
               has to close or the world ends as an all-waste equilibrium",
    },
    Field {
        path: "metabolism.catalogue.metabolism.byproduct",
        label: "byproduct",
        group: Group::Chemistry,
        unit: Unit::Chemical,
        note: "produced alongside the substrate by photosynthesis",
    },
    Field {
        path: "metabolism.catalogue.metabolism.structural",
        label: "structural",
        group: Group::Chemistry,
        unit: Unit::Chemical,
        note: "what a body is built out of",
    },
    Field {
        path: "metabolism.catalogue.metabolism.reactive",
        label: "reactive",
        group: Group::Chemistry,
        unit: Unit::Chemical,
        note: "respiration's toxic byproduct — reactive oxygen, in the real thing",
    },
    // --- junctions ---
    Field {
        path: "junctions.join_base_cost",
        label: "join base cost",
        group: Group::Junctions,
        unit: Unit::Q10,
        note: "energy to join with a matching key. Meant to be nearly free",
    },
    Field {
        path: "junctions.join_forced_penalty",
        label: "forced join penalty",
        group: Group::Junctions,
        unit: Unit::Q10,
        note: "extra energy per unit of the target's membrane when the key does not match. \
               What makes consent economic rather than absolute",
    },
    Field {
        path: "junctions.soft_max_range",
        label: "soft max range",
        group: Group::Junctions,
        unit: Unit::Pos,
        note: "a soft junction breaks beyond this",
    },
    Field {
        path: "junctions.breaking_strain",
        label: "breaking strain",
        group: Group::Junctions,
        unit: Unit::Pos,
        note: "a hard junction breaks this far past its rest length",
    },
    Field {
        path: "junctions.stiffness",
        label: "stiffness",
        group: Group::Junctions,
        unit: Unit::Fraction,
        note: "position error one solver iteration corrects",
    },
    Field {
        path: "junctions.iterations",
        label: "solver iterations",
        group: Group::Junctions,
        unit: Unit::Count,
        note: "Gauss-Seidel passes per tick. SPEC §8.4 says two or three",
    },
    Field {
        path: "junctions.muscle_range",
        label: "muscle range",
        group: Group::Junctions,
        unit: Unit::Pos,
        note: "how far JLEN may move a rest length from its natural value",
    },
    Field {
        path: "junctions.transfer_cost",
        label: "transfer cost",
        group: Group::Junctions,
        unit: Unit::Q10,
        note: "energy per unit moved across a soft junction",
    },
    Field {
        path: "junctions.probe_leaks_distance",
        label: "probe leaks distance",
        group: Group::Junctions,
        unit: Unit::Flag,
        note: "whether a failed JOIN reveals how close the key was. SPEC §8.2 is explicit that \
               this makes the key hill-climbable in about seven probes and parasitism trivial. \
               A knob for watching that happen deliberately, and off by default",
    },
    // --- ecology ---
    Field {
        path: "ecology.spike_damage",
        label: "spike damage",
        group: Group::Ecology,
        unit: Unit::Q10,
        note: "membrane damage a spike deals per tick per unit of extension",
    },
    Field {
        path: "ecology.spike_upkeep",
        label: "spike upkeep",
        group: Group::Ecology,
        unit: Unit::Q10,
        note: "energy a spike costs per tick per unit of extension. Violence is not free",
    },
    Field {
        path: "ecology.carrion_fraction",
        label: "carrion fraction",
        group: Group::Ecology,
        unit: Unit::Fraction,
        note: "a dead cell's structural mass that becomes carrion rather than returning \
               straight to the fluid",
    },
    Field {
        path: "ecology.digestion_rate",
        label: "digestion rate",
        group: Group::Ecology,
        unit: Unit::Q10,
        note: "carrion a lysosome digests per tick per unit of size",
    },
    Field {
        path: "ecology.digestion_efficiency",
        label: "digestion efficiency",
        group: Group::Ecology,
        unit: Unit::Fraction,
        note: "digested carrion that becomes usable substrate. The rest is waste, or a corpse \
               would be worth more than the cell that made it",
    },
];

/// The prefix the organelle catalogue's per-entry costs live under.
///
/// Drawn as a grid rather than as rows in a tab — sixteen entries of seven numbers each is a
/// hundred and twelve fields, which is a table, not a form.
pub const CATALOGUE_PREFIX: &str = "metabolism.catalogue.specs.";

/// The seven numbers each catalogue entry carries, as (suffix, column heading).
pub const CATALOGUE_COLUMNS: [(&str, &str); 7] = [
    ("build_matter", "matter"),
    ("build_matter_per_param", "matter/size"),
    ("build_energy", "energy"),
    ("build_ticks", "ticks"),
    ("upkeep", "upkeep"),
    ("upkeep_per_param", "upkeep/size"),
    ("teardown_recovery", "recovered"),
];

/// The fields in one group, in table order.
#[must_use]
pub fn group(group: Group) -> Vec<&'static Field> {
    FIELDS.iter().filter(|f| f.group == group).collect()
}

/// Look a parameter's description up by path.
#[must_use]
pub fn describe(path: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_core::biology::BiologyConfig;

    #[test]
    fn every_parameter_has_a_description() {
        // The drift guard, and the reason the generic traversal is worth having. A parameter
        // added to `BiologyConfig` appears in `mm_core::params::fields` for free — and this
        // fails until somebody has said what it is, rather than it appearing in the editor as
        // a nameless number or, worse, not appearing at all.
        let config = BiologyConfig::default();
        let missing: Vec<String> = mm_core::params::fields(&config)
            .into_iter()
            .map(|(path, _)| path)
            .filter(|path| !path.starts_with(CATALOGUE_PREFIX))
            .filter(|path| describe(path).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "parameters with no description in params.rs: {missing:#?}"
        );
    }

    #[test]
    fn every_description_names_a_real_parameter() {
        // The other direction: a field renamed in `mm-core` must not leave a row here that
        // edits nothing. Without this the editor would silently stop applying it.
        let config = BiologyConfig::default();
        let real: Vec<String> = mm_core::params::fields(&config)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        for field in FIELDS {
            assert!(
                real.iter().any(|p| p == field.path),
                "`{}` is described here but does not exist",
                field.path
            );
        }
    }

    #[test]
    fn every_catalogue_column_exists() {
        let config = BiologyConfig::default();
        let real: Vec<String> = mm_core::params::fields(&config)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        for (suffix, _) in CATALOGUE_COLUMNS {
            let path = format!("{CATALOGUE_PREFIX}0.{suffix}");
            assert!(real.contains(&path), "`{path}` does not exist");
        }
        // And between the two the whole catalogue is covered, so nothing in it is uneditable.
        let catalogue: Vec<&String> = real
            .iter()
            .filter(|p| p.starts_with(CATALOGUE_PREFIX))
            .collect();
        assert_eq!(
            catalogue.len(),
            mm_core::organelle::SLOT_COUNT * CATALOGUE_COLUMNS.len(),
            "the catalogue has fields no column covers"
        );
    }

    #[test]
    fn every_field_is_in_exactly_one_group_and_no_group_is_empty() {
        for g in Group::ALL {
            assert!(!group(g).is_empty(), "{} has no parameters", g.title());
        }
        let grouped: usize = Group::ALL.iter().map(|g| group(*g).len()).sum();
        assert_eq!(grouped, FIELDS.len(), "a field is in no group, or in two");
    }

    #[test]
    fn no_two_fields_share_a_path() {
        let mut paths: Vec<&str> = FIELDS.iter().map(|f| f.path).collect();
        let count = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), count, "two rows edit the same parameter");
    }

    #[test]
    fn readings_are_offered_where_a_raw_number_is_unreadable_and_not_otherwise() {
        let names: Vec<String> = (0..16).map(|i| format!("chem{i}")).collect();

        let q10 = describe("division_energy").unwrap();
        assert_eq!(
            q10.reading(Value::Int(20_480), &names).as_deref(),
            Some("20.00")
        );

        let fraction = describe("metabolism.rates.reactive_fraction").unwrap();
        assert_eq!(
            fraction.reading(Value::Int(512), &names).as_deref(),
            Some("50.0%")
        );

        let chemical = describe("structural_chemical").unwrap();
        assert_eq!(
            chemical.reading(Value::Int(4), &names).as_deref(),
            Some("chem4")
        );

        let chance = describe("mutation.point").unwrap();
        assert_eq!(
            chance.reading(Value::Int(64), &names).as_deref(),
            Some("1 in 1024")
        );
        // Zero is off, not one-in-infinity or a division by zero.
        assert_eq!(
            chance.reading(Value::Int(0), &names).as_deref(),
            Some("never")
        );

        // A count is already its own reading, and repeating it would be noise.
        let count = describe("junctions.iterations").unwrap();
        assert_eq!(count.reading(Value::Int(3), &names), None);
    }

    #[test]
    fn a_chemical_index_out_of_range_reads_as_nothing_rather_than_panicking() {
        let names: Vec<String> = vec!["only".to_string()];
        let chemical = describe("structural_chemical").unwrap();
        assert_eq!(chemical.reading(Value::Int(9), &names), None);
        assert_eq!(chemical.reading(Value::Int(-1), &names), None);
    }
}
