//! The census by descent, against a world with real cells in it (M8, SPEC §10.1 and §13).
//!
//! `census`'s own unit tests check the verdicts on populations written in by hand. What they
//! cannot check is the attribution, because that needs cells to attribute — a slide where cells
//! are actually born, drift, fork into new species and die while the cohorts they descend from
//! stay fixed.
//!
//! The property that matters is **totality**: every living cell belongs to exactly one founding
//! cohort, for as long as the run lasts. If that fails, every share and every fate computed from
//! it is quietly wrong by an unknown amount, which is the failure mode the old guild-column tests
//! had and the reason this file exists.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use mm_core::biology::BiologyConfig;
use mm_core::census::{Census, CensusLog, Cohort};
use mm_core::ecology::TrophicMix;
use mm_core::{MutationRates, Placement, Scenario, World};

fn assemble(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    mm_asm::assemble(&src)
        .unwrap_or_else(|e| panic!("{name} does not assemble:\n{e}"))
        .bytes
}

fn scenario(name: &str) -> Scenario {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scenarios")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    ron::from_str(&text).unwrap_or_else(|e| panic!("{name} does not parse: {e}"))
}

/// A small, well-lit slide with a mixed community on it, seeded through the one path.
fn community(seed: u64, mutation: MutationRates) -> (World, Vec<Cohort>) {
    let mut world = World::new(Scenario {
        seed,
        width: 64,
        height: 64,
        ..scenario("soup.ron")
    })
    .expect("world");
    world.set_biology(BiologyConfig {
        mutation,
        ..BiologyConfig::default()
    });
    let ancestor = assemble("ancestor.mm");
    let scavenger = assemble("scavenger.mm");
    let predator = assemble("predator.mm");
    let cohorts = world.place_community(
        &[
            ("ancestor", &ancestor, 12),
            ("scavenger", &scavenger, 4),
            ("predator", &predator, 4),
        ],
        Placement::Spread,
    );
    (world, cohorts)
}

#[test]
fn every_founder_lands_in_a_cohort_of_its_own() {
    let (_world, cohorts) = community(1, MutationRates::none());
    assert_eq!(cohorts.len(), 3);
    for c in &cohorts {
        assert!(
            c.founded > 0,
            "{} placed none of its founders; the census would be of nothing",
            c.label
        );
        assert_ne!(
            c.root,
            u32::MAX,
            "{} landed {} founders but resolved to no root species",
            c.label,
            c.founded
        );
    }
    // Three distinct genomes are three distinct roots. If two collapsed into one, every share
    // computed against them would be the sum of two lineages reported as one.
    let roots: BTreeMap<u32, &str> = cohorts.iter().map(|c| (c.root, c.label.as_str())).collect();
    assert_eq!(
        roots.len(),
        3,
        "three genomes should found three roots, got {roots:?}"
    );
}

#[test]
fn the_same_genome_twice_is_one_root_and_says_so() {
    // `Phylogeny::found` merges by fingerprint deliberately — twelve founders of one ancestor are
    // one species. Two *cohorts* of one genome therefore cannot be told apart, and the census must
    // resolve them consistently rather than by iteration order: the first entry takes the cells.
    let mut world = World::new(Scenario {
        seed: 7,
        width: 48,
        height: 48,
        ..scenario("soup.ron")
    })
    .expect("world");
    let ancestor = assemble("ancestor.mm");
    let cohorts = world.place_community(
        &[("left", &ancestor, 4), ("right", &ancestor, 4)],
        Placement::Spread,
    );
    assert_eq!(
        cohorts[0].root, cohorts[1].root,
        "one genome is one root; this is `found` merging, not a bug"
    );
    world.run(200);
    let census = world.census(&cohorts);
    assert_eq!(
        census.unattributed, 0,
        "a shared root must still attribute every cell"
    );
    assert_eq!(
        census.cohorts[1].cells, 0,
        "the second cohort of a shared root holds nothing; the first takes them all"
    );
    assert_eq!(
        census.cohorts[0].cells, census.total,
        "and the first holds the whole slide"
    );
}

#[test]
fn every_living_cell_is_attributed_for_the_whole_run() {
    // The load-bearing property. Mutation on, so genomes drift, species fork, and the archive
    // grows underneath the attribution.
    let (mut world, cohorts) = community(3, MutationRates::default());
    let mut log = CensusLog::new();
    let step = if cfg!(debug_assertions) { 200 } else { 1_000 };
    for _ in 0..10 {
        world.run(step);
        log.sample(world.tick_count(), world.cells(), world.archive(), &cohorts);
        let census = log.last().expect("just sampled");
        assert_eq!(
            census.unattributed,
            0,
            "tick {}: {} cells belong to no cohort. Every cell chains to a root and every root \
             is founded by a seeding, so this means the cohort list is missing one.\n{}",
            census.tick,
            census.unattributed,
            census.report()
        );
        let summed: u32 = census.cohorts.iter().map(|c| c.cells).sum();
        assert_eq!(
            summed, census.total,
            "tick {}: cohorts sum to {summed} but the slide holds {}",
            census.tick, census.total
        );
        if world.cells().is_empty() {
            break;
        }
    }
    world.check_invariants().expect("invariants hold");
}

#[test]
fn the_per_cohort_guilds_sum_to_the_global_guild_census() {
    // The per-lineage mix is the same reading as `TrophicMix::of`, partitioned. If the two
    // disagree, one of them is classifying cells differently and the food-web numbers would
    // depend on which was consulted.
    let (mut world, cohorts) = community(5, MutationRates::default());
    world.run(if cfg!(debug_assertions) { 400 } else { 2_000 });
    let census = world.census(&cohorts);
    let global = TrophicMix::of(world.cells());
    let mut summed = TrophicMix::default();
    for c in &census.cohorts {
        summed.producers += c.mix.producers;
        summed.predators += c.mix.predators;
        summed.scavengers += c.mix.scavengers;
        summed.osmotrophs += c.mix.osmotrophs;
        summed.total += c.mix.total;
    }
    assert_eq!(
        summed, global,
        "the per-lineage guilds must partition the global one exactly.\n{}",
        census.report()
    );
}

#[test]
fn a_cell_never_moves_between_cohorts() {
    // M5 already asserts that a cell never changes species once born. This is the consequence the
    // census depends on: since a cohort is a root and a species' parent never changes, the cohort
    // a cell belongs to is fixed for its whole life. Checked directly, because the census would
    // still return plausible numbers if it were not.
    let (mut world, cohorts) = community(11, MutationRates::default());
    let by_root: BTreeMap<u32, &str> = cohorts.iter().map(|c| (c.root, c.label.as_str())).collect();
    let mut assigned: BTreeMap<u64, &str> = BTreeMap::new();
    let step = if cfg!(debug_assertions) { 100 } else { 500 };
    for _ in 0..8 {
        world.run(step);
        for i in world.cells().iter() {
            let id = world.cells().id_at(i);
            // Slot and generation together, so a reused slot is a different cell.
            let key = (u64::from(id.slot()) << 32) | u64::from(id.generation());
            let root = mm_core::census::root_of(world.archive(), world.cells().species[i]);
            let Some(label) = by_root.get(&root) else {
                panic!("tick {}: cell in no cohort", world.tick_count());
            };
            match assigned.get(&key) {
                Some(was) => assert_eq!(
                    was, label,
                    "tick {}: a cell moved from the {was} cohort to the {label} one",
                    world.tick_count()
                ),
                None => {
                    assigned.insert(key, label);
                }
            }
        }
        if world.cells().is_empty() {
            break;
        }
    }
    assert!(
        assigned.len() > 20,
        "only {} cells were ever seen; this test needs a living population to mean anything",
        assigned.len()
    );
}

#[test]
fn a_cohort_list_missing_a_seeding_is_reported_and_not_hidden() {
    // The one case where `unattributed` should be nonzero: a caller that spawned something the
    // cohort list does not name. It is reported rather than folded into a cohort, because the
    // alternative is every share being quietly wrong.
    let (mut world, cohorts) = community(13, MutationRates::none());
    let sponge = assemble("sponge.mm");
    let placed = world.place_inhabitants(&sponge, 4, Placement::Spread);
    assert!(placed > 0, "the sponge should have landed");
    world.run(100);
    let census = world.census(&cohorts);
    assert!(
        census.unattributed >= placed,
        "{placed} sponge founders were placed outside the cohort list and the census reported \
         {} unattributed",
        census.unattributed
    );
    assert!(
        census.report().contains("UNATTRIBUTED"),
        "and the report says so out loud:\n{}",
        census.report()
    );
}

#[test]
fn a_census_of_an_empty_slide_is_empty_rather_than_absent() {
    let mut world = World::new(Scenario {
        seed: 17,
        width: 32,
        height: 32,
        ..scenario("soup.ron")
    })
    .expect("world");
    world.run(10);
    let cohorts = vec![Cohort::new("nobody", 0, 0)];
    let census = Census::take(
        world.tick_count(),
        world.cells(),
        world.archive(),
        &cohorts,
    );
    assert_eq!(census.total, 0);
    assert_eq!(census.unattributed, 0);
    assert_eq!(census.cohorts.len(), 1);
    assert_eq!(census.cohorts[0].cells, 0);
    assert_eq!(census.cohorts[0].share(census.total), 0, "no division by zero");
    assert!(!census.is_monoculture(1), "an empty slide is not a monoculture");
    let _ = common::env_usize("MM_UNUSED", 0);
}
