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

use crate::chem::CHEM_COUNT;
use crate::organelle::OrganelleType;
use crate::World;

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
    /// Energy that entered the world since the last sample — the other side of the same
    /// account, and the one that says whether the books are balancing.
    ///
    /// A *rate*, differenced here rather than in a panel, for the reason `dissipation` is: the
    /// history samples every `n` ticks and `n` is configurable, so anything that differenced
    /// the cumulative counters itself would report an income that changed when you changed how
    /// often you looked at it. `absorbed - dissipation` is the world's net energy income, and
    /// where that crosses zero and stays is where the energy economy has found its level.
    pub absorbed: i64,
    pub energy_in: i64,
    pub energy_out: i64,
    pub energy_stored: i64,
    /// Matter that arrived from off-slide since the last sample, across every chemical.
    ///
    /// A rate, like `dissipation` and `absorbed` and for the same reason. `influx - efflux` is
    /// the world's net material income, and a flow-through slide has found its level when that
    /// crosses zero and stays — which is the material half of the same statement `absorbed -
    /// dissipation` makes about energy.
    pub influx: i64,
    /// Matter that left the slide since the last sample, across every chemical.
    pub efflux: i64,
    /// Energy that arrived latent in matter, cumulative. Part of `energy_in`.
    pub energy_imported: i64,
    /// Energy that left latent in matter, cumulative. Part of `energy_out`, and *not* part of
    /// `dissipation` — matter washing off the slide is not the world getting warmer.
    pub energy_exported: i64,
    /// Matter in, cumulative. `influx` is this differenced.
    pub matter_in: i64,
    /// Matter out, cumulative. `efflux` is this differenced.
    pub matter_out: i64,

    pub mean_age: i64,
    pub mean_energy: i64,
    pub mean_mass: i64,
    pub mean_genome_len: i64,
    /// How much interning is saving: distinct genomes against population.
    pub distinct_genomes: u64,
    /// Distinct organelle loadouts present. The cheapest measure of organisational
    /// complexity, and the one that will show differentiation when M7 arrives.
    pub distinct_loadouts: u64,
    /// Mean nucleus copy fidelity, `Q10`, over the cells that *have* a nucleus. The plottable
    /// trait that makes mutation rate evolvable rather than a constant somebody chose
    /// (SPEC §9).
    pub mean_fidelity: i64,
    /// Cells with no working nucleus.
    ///
    /// Its own column rather than folded into the mean, because it used to be folded in — a
    /// cell with no nucleus reported a fidelity of zero, so a population that had mostly
    /// abandoned its nuclei showed up as a population with poor copy fidelity. The two are
    /// very different things and only one of them is evolution.
    pub no_nucleus: u64,

    /// Trophic composition: what fraction of the world's energy income came from light, in
    /// parts per thousand (SPEC §13).
    pub trophic_light: i64,
    /// The guild census (M8): cells carrying a chloroplast, a lysosome, a spike, and cells
    /// carrying none of the machinery that would make them anything but an osmotroph.
    ///
    /// Counts rather than fractions, and overlapping rather than a partition: a cell with a
    /// chloroplast and a lysosome is both, because it is both. There is no cell-type enum
    /// here any more than anywhere else — these are inferences from organelle loadouts.
    pub producers: u64,
    pub scavengers: u64,
    pub predators: u64,
    pub osmotrophs: u64,
    /// Carrion in the fluid, `Q10`. The size of the detrital pool: a number that climbs and
    /// stays climbed means nothing is eating the dead.
    pub carrion: i64,
    /// Carrion digested back into substrate since the last sample, `Q10`.
    pub scavenged: i64,
    /// Spike damage dealt since the last sample, `Q10`.
    pub wounding: i64,
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
        // Counted apart, because averaging a cell that *has* no fidelity in as a zero is what
        // hid a nucleus-free majority behind a plausible-looking mean.
        let mut with_nucleus = 0i64;
        let mut no_nucleus = 0u64;
        let mut loadouts: Vec<u64> = Vec::with_capacity(cells.len());

        for i in cells.iter() {
            age += cells.age[i] as i64;
            energy += cells.energy[i] as i64;
            mass += cells.mass[i] as i64;
            genome_len += cells.genome[i].len() as i64;
            match crate::biology::nucleus_fidelity(cells, i) {
                Some(f) => {
                    fidelity += f as i64;
                    with_nucleus += 1;
                }
                None => no_nucleus += 1,
            }

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
        let matter_in: i64 = ledger.injected().iter().sum();
        let matter_out: i64 = ledger.drained().iter().sum();
        // Heat, and only heat. `energy_out` also carries energy that left latent in matter
        // crossing the boundary, and reporting an outflow of food as dissipation would say the
        // world was warming when it was being washed out.
        let dissipation = match previous {
            Some(p) => {
                (ledger.energy_out() - ledger.energy_exported())
                    - (p.energy_out - p.energy_exported)
            }
            None => 0,
        };
        let influx = match previous {
            Some(p) => matter_in - p.matter_in,
            None => 0,
        };
        let efflux = match previous {
            Some(p) => matter_out - p.matter_out,
            None => 0,
        };
        let absorbed = match previous {
            Some(p) => ledger.energy_in() - p.energy_in,
            None => 0,
        };

        let mix = crate::ecology::TrophicMix::of(cells);

        Sample {
            tick: world.tick_count(),
            population: cells.len() as u64,
            births: report.biology.births as u64,
            deaths: report.biology.deaths as u64,
            dissipation,
            absorbed,
            energy_in: ledger.energy_in(),
            energy_out: ledger.energy_out(),
            energy_stored: ledger.energy_stored(),
            influx,
            efflux,
            energy_imported: ledger.energy_imported(),
            energy_exported: ledger.energy_exported(),
            matter_in,
            matter_out,
            mean_age: age / n,
            mean_energy: energy / n,
            mean_mass: mass / n,
            mean_genome_len: genome_len / n,
            distinct_genomes: cells.distinct_genomes() as u64,
            distinct_loadouts: loadouts.len() as u64,
            mean_fidelity: fidelity / with_nucleus.max(1),
            no_nucleus,
            trophic_light: ledger.trophic_share(crate::TrophicSource::Light),
            producers: mix.producers as u64,
            scavengers: mix.scavengers as u64,
            predators: mix.predators as u64,
            osmotrophs: mix.osmotrophs as u64,
            carrion: chemicals[crate::ecology::CARRION],
            scavenged: report.ecology.scavenged,
            wounding: report.ecology.damage_dealt,
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
                r#""dissipation":{},"absorbed":{},"energy_in":{},"energy_out":{},"energy_stored":{},"#,
                r#""influx":{},"efflux":{},"energy_imported":{},"energy_exported":{},"#,
                r#""mean_age":{},"mean_energy":{},"mean_mass":{},"mean_genome_len":{},"#,
                r#""distinct_genomes":{},"distinct_loadouts":{},"mean_fidelity":{},"#,
                r#""no_nucleus":{},"trophic_light":{},"producers":{},"scavengers":{},"#,
                r#""predators":{},"#,
                r#""osmotrophs":{},"carrion":{},"scavenged":{},"wounding":{},"#,
                r#""total_matter":{},"chemicals":[{}]}}"#
            ),
            self.tick,
            self.population,
            self.births,
            self.deaths,
            self.dissipation,
            self.absorbed,
            self.energy_in,
            self.energy_out,
            self.energy_stored,
            self.influx,
            self.efflux,
            self.energy_imported,
            self.energy_exported,
            self.mean_age,
            self.mean_energy,
            self.mean_mass,
            self.mean_genome_len,
            self.distinct_genomes,
            self.distinct_loadouts,
            self.mean_fidelity,
            self.no_nucleus,
            self.trophic_light,
            self.producers,
            self.scavengers,
            self.predators,
            self.osmotrophs,
            self.carrion,
            self.scavenged,
            self.wounding,
            self.total_matter,
            chems.join(",")
        )
    }

    /// The terminal table's columns: heading, and how wide the column is.
    ///
    /// One list, used to build both the heading row and the data rows, because the two were
    /// two separate format strings and they had drifted. `"population"` is ten characters in
    /// an eight-wide column, so the heading had been pushing every column to its right out of
    /// line since M1 — visible in every run anybody watched, and invisible to every test,
    /// because nothing compared the two strings. Derived from one list, they cannot disagree.
    ///
    /// The widths are wide enough for the headings, and a value that overruns its column
    /// pushes the rest of *its own* row along. That is the right way round: a number too big
    /// for its column should be shown in full rather than truncated.
    const COLUMNS: [(&'static str, usize); 11] = [
        ("tick", 10),
        ("cells", 8),
        ("births", 7),
        ("deaths", 7),
        ("dissipation", 13),
        ("energy", 10),
        ("genome", 8),
        ("distinct", 9),
        ("produc", 7),
        ("scav", 6),
        ("pred", 6),
    ];

    /// A fixed-width line for a terminal, for watching a run go by.
    ///
    /// The three guild counts were added at M8. Watching a run and being unable to see whether
    /// anything is eating the dead is watching the wrong half of it — and a predator column
    /// that stays at zero for a million ticks is the single most useful thing the terminal can
    /// tell you about a scenario.
    #[must_use]
    pub fn to_row(&self) -> String {
        let values: [i64; 11] = [
            self.tick as i64,
            self.population as i64,
            self.births as i64,
            self.deaths as i64,
            self.dissipation,
            self.mean_energy,
            self.mean_genome_len,
            self.distinct_genomes as i64,
            self.producers as i64,
            self.scavengers as i64,
            self.predators as i64,
        ];
        let mut out = String::new();
        for (v, (_, width)) in values.iter().zip(Sample::COLUMNS) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&format!("{v:>width$}"));
        }
        out
    }

    /// Column headings matching [`Sample::to_row`].
    #[must_use]
    pub fn header() -> String {
        let mut out = String::new();
        for (label, width) in Sample::COLUMNS {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&format!("{label:>width$}"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scenario;

    #[test]
    fn the_row_lines_up_with_its_header() {
        let world = World::new(Scenario::stress(8, 8)).unwrap();
        let row = Sample::take(&world, None).to_row();
        let header = Sample::header();
        assert_eq!(
            row.len(),
            header.len(),
            "the row and its header are different widths:\n{header}\n{row}"
        );
        assert_eq!(
            row.split_whitespace().count(),
            Sample::COLUMNS.len(),
            "the row has a different number of columns than there are columns:\n{header}\n{row}"
        );
        // Every heading has to fit its column, or it pushes the ones after it along.
        for (label, width) in Sample::COLUMNS {
            assert!(
                label.len() <= width,
                "the heading {label:?} is {} characters in a {width}-wide column",
                label.len()
            );
        }
    }

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

        // The line is built from one long format string, so a field added to the struct and
        // forgotten here is a silent hole in every export. Naming them makes that a failure.
        for key in [
            "influx",
            "efflux",
            "energy_imported",
            "energy_exported",
            "population",
            "births",
            "deaths",
            "dissipation",
            "energy_in",
            "energy_out",
            "energy_stored",
            "mean_age",
            "mean_energy",
            "mean_mass",
            "mean_genome_len",
            "distinct_genomes",
            "distinct_loadouts",
            "mean_fidelity",
            "no_nucleus",
            "trophic_light",
            "producers",
            "scavengers",
            "predators",
            "osmotrophs",
            "carrion",
            "scavenged",
            "wounding",
            "total_matter",
            "chemicals",
        ] {
            assert_eq!(
                line.matches(&format!("\"{key}\":")).count(),
                1,
                "{key} is not in the exported line exactly once:\n{line}"
            );
        }
        assert!(line.starts_with('{') && line.ends_with('}'));
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
