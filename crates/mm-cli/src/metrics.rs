//! Metric export (SPEC §13).
//!
//! > The entropy story requires it to be measurable, not merely narrated.
//!
//! One JSON object per line, one line per sample. NDJSON because a run is a stream: it can be
//! tailed while the simulation is still going, truncated without corrupting what came before,
//! and fed to anything line-oriented without a parser that understands the whole file. A
//! single JSON array would have none of those properties and would only be finished when the
//! run was.
//!
//! Serialised by hand rather than through serde. The schema is small, it is a stable contract
//! that offline analysis depends on, and writing it out longhand means the field names are
//! visible in one place instead of being implied by a struct definition somewhere else.

use mm_core::chem::CHEM_COUNT;
use mm_core::organelle::OrganelleType;
use mm_core::World;

/// One sample of everything worth plotting.
#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub tick: u64,
    pub population: u64,
    pub births: u64,
    pub deaths: u64,

    /// Energy converted to heat since the last sample. The most direct statement of "life as
    /// a dissipative structure" (SPEC §13).
    pub dissipation: i64,
    pub energy_in: i64,
    pub energy_out: i64,
    pub energy_stored: i64,

    pub mean_age: i64,
    pub mean_energy: i64,
    pub mean_mass: i64,
    pub mean_genome_len: i64,
    /// How much interning is saving: distinct genomes against population.
    pub distinct_genomes: u64,
    /// Distinct organelle loadouts present. The cheapest measure of organisational
    /// complexity, and the one that will show differentiation when M7 arrives.
    pub distinct_loadouts: u64,
    /// Mean nucleus copy fidelity, `Q10`. The plottable trait that makes mutation rate
    /// evolvable rather than a constant somebody chose (SPEC §9).
    pub mean_fidelity: i64,

    /// Per-chemical totals over fluid, cell interiors and cell mass.
    pub chemicals: [i64; CHEM_COUNT],
    /// Total matter across every species — the number that must never move.
    pub total_matter: i64,
}

impl Sample {
    /// Take a sample of the world as it is now.
    ///
    /// `previous` is the last sample, for the rates that are differences rather than levels.
    #[must_use]
    pub fn take(world: &World, previous: Option<&Sample>) -> Sample {
        let cells = world.cells();
        let n = cells.len().max(1) as i64;
        let report = world.report();

        let mut age = 0i64;
        let mut energy = 0i64;
        let mut mass = 0i64;
        let mut genome_len = 0i64;
        let mut fidelity = 0i64;
        let mut loadouts: Vec<u64> = Vec::with_capacity(cells.len());

        for i in cells.iter() {
            age += cells.age[i] as i64;
            energy += cells.energy[i] as i64;
            mass += cells.mass[i] as i64;
            genome_len += cells.genome[i].len() as i64;
            fidelity += mm_core::biology::nucleus_fidelity(cells, i) as i64;

            // A loadout is which types are in which slots, ignoring size: two cells with the
            // same organelles in the same places are the same kind of thing.
            let mut key = 0u64;
            for (s, o) in cells.slots(i).iter().enumerate() {
                let kind = if o.kind == OrganelleType::Empty {
                    15u64
                } else {
                    (o.kind.number() as u64) & 0xF
                };
                key |= kind << (s * 4);
            }
            loadouts.push(key);
        }
        loadouts.sort_unstable();
        loadouts.dedup();

        let chemicals = world.total_matter();
        let ledger = world.ledger();
        let dissipation = match previous {
            Some(p) => ledger.energy_out() - p.energy_out,
            None => 0,
        };

        Sample {
            tick: world.tick_count(),
            population: cells.len() as u64,
            births: report.biology.births as u64,
            deaths: report.biology.deaths as u64,
            dissipation,
            energy_in: ledger.energy_in(),
            energy_out: ledger.energy_out(),
            energy_stored: ledger.energy_stored(),
            mean_age: age / n,
            mean_energy: energy / n,
            mean_mass: mass / n,
            mean_genome_len: genome_len / n,
            distinct_genomes: cells.distinct_genomes() as u64,
            distinct_loadouts: loadouts.len() as u64,
            mean_fidelity: fidelity / n,
            chemicals,
            total_matter: chemicals.iter().sum(),
        }
    }

    /// One NDJSON line, without the newline.
    #[must_use]
    pub fn to_json(&self) -> String {
        let chems: Vec<String> = self.chemicals.iter().map(|v| v.to_string()).collect();
        format!(
            concat!(
                r#"{{"tick":{},"population":{},"births":{},"deaths":{},"#,
                r#""dissipation":{},"energy_in":{},"energy_out":{},"energy_stored":{},"#,
                r#""mean_age":{},"mean_energy":{},"mean_mass":{},"mean_genome_len":{},"#,
                r#""distinct_genomes":{},"distinct_loadouts":{},"mean_fidelity":{},"#,
                r#""total_matter":{},"chemicals":[{}]}}"#
            ),
            self.tick,
            self.population,
            self.births,
            self.deaths,
            self.dissipation,
            self.energy_in,
            self.energy_out,
            self.energy_stored,
            self.mean_age,
            self.mean_energy,
            self.mean_mass,
            self.mean_genome_len,
            self.distinct_genomes,
            self.distinct_loadouts,
            self.mean_fidelity,
            self.total_matter,
            chems.join(",")
        )
    }

    /// A fixed-width line for a terminal, for watching a run go by.
    #[must_use]
    pub fn to_row(&self) -> String {
        format!(
            "{:>10} {:>8} {:>6} {:>6} {:>12} {:>9} {:>8} {:>6}",
            self.tick,
            self.population,
            self.births,
            self.deaths,
            self.dissipation,
            self.mean_energy,
            self.mean_genome_len,
            self.distinct_genomes,
        )
    }

    /// Column headings matching [`Sample::to_row`].
    #[must_use]
    pub fn header() -> String {
        format!(
            "{:>10} {:>8} {:>6} {:>6} {:>12} {:>9} {:>8} {:>6}",
            "tick", "population", "births", "deaths", "dissipation", "energy", "genome", "distinct"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_core::Scenario;

    #[test]
    fn a_sample_of_an_empty_world_is_well_formed() {
        // Division by population must not blow up before anything is alive.
        let world = World::new(Scenario::stress(8, 8)).unwrap();
        let s = Sample::take(&world, None);
        assert_eq!(s.population, 0);
        assert_eq!(s.mean_age, 0);
        assert!(s.to_json().starts_with('{'));
        assert!(s.to_json().ends_with('}'));
    }

    #[test]
    fn the_json_line_has_no_newlines_in_it() {
        // NDJSON's one rule. A sample that embedded one would corrupt every reader.
        let world = World::new(Scenario::stress(8, 8)).unwrap();
        let line = Sample::take(&world, None).to_json();
        assert!(!line.contains('\n'));
        assert_eq!(line.matches("\"tick\"").count(), 1);
    }

    #[test]
    fn dissipation_is_a_rate_and_the_rest_are_levels() {
        let world = World::new(Scenario::stress(8, 8)).unwrap();
        let first = Sample::take(&world, None);
        assert_eq!(
            first.dissipation, 0,
            "the first sample has nothing to differ from"
        );

        let mut world2 = World::new(Scenario::stress(8, 8)).unwrap();
        world2.ledger_mut().absorb(1000);
        world2.ledger_mut().dissipate(500);
        let second = Sample::take(&world2, Some(&first));
        assert_eq!(second.dissipation, 500);
    }

    #[test]
    fn every_chemical_is_reported() {
        let world = World::new(Scenario::stress(16, 16)).unwrap();
        let s = Sample::take(&world, None);
        assert_eq!(s.chemicals.len(), CHEM_COUNT);
        assert_eq!(s.total_matter, s.chemicals.iter().sum::<i64>());
        assert!(s.to_json().matches(',').count() >= CHEM_COUNT);
    }
}
