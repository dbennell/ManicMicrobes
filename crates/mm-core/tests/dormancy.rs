//! Sleeping through the dark, and what it costs.
//!
//! SPEC §5 has promised dormancy as an evolvable strategy since the beginning, and until now the
//! engine implemented the cheap half of it. `HALT` yields the rest of the instruction budget, so
//! *thinking* was free — but thinking is not what a cell dies of. The bill that kills is
//! `metabolic_floor + organelle upkeep + turgor + leak`, measured at **96% organelle upkeep** for
//! an idle cell, and nothing a genome could do touched it. A cell in the dark paid full price for
//! a chloroplast that could not photosynthesise and a mitochondrion with nothing to burn, and
//! `events::Occurrence::Dormancy` carried the note "requires a dormant state, which nothing
//! implements".
//!
//! `OrganelleSpec::upkeep_throttled` is that state. Three quarters of a throttleable organelle's
//! upkeep now follows `control[0]`, so `OSET`ting an engine shut buys a lower bill as well as a
//! lower rate.
//!
//! # What this is not
//!
//! Not free, and not a spore. The quarter that stays basal, plus the nucleus and the membrane —
//! whose `control[0]` is copy fidelity and permeability, not a throttle, and whose services a
//! cell cannot surrender and live — mean the reachable saving is about a third of the whole bill.
//! That funds a night. It does not fund a season, and the reason it cannot is `leak_cost`: energy
//! above `energy_reserve` bleeds at `Q10_ONE/64` a tick, so a cell cannot bank its way to a long
//! sleep either. Dormancy in this engine is a lower burn rate, never a stopped clock.

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::events::Occurrence;
use mm_core::light::LightRegime;
use mm_core::{MutationRates, Organelle, OrganelleType, Scenario, World};

/// A dark, still slide. Nothing to photosynthesise by, so a chloroplast is pure cost — which is
/// the whole situation dormancy is for.
fn night() -> World {
    let mut world = World::new(Scenario {
        seed: 42,
        width: 16,
        height: 16,
        light: LightRegime::Uniform { intensity: 0 },
        ..Scenario::default()
    })
    .expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    world
}

/// One cell carrying the loadout of a working autotroph, with `awake` deciding whether its two
/// engines are running or shut. `0x2E` is a lone `HALT`, so the genome does nothing at all and
/// the controls stand exactly as set — this measures the physics, not a genome's cleverness.
fn sleeper(world: &mut World, x: i32, awake: bool) -> CellId {
    let genome = world.genomes().intern(vec![0x2E]).expect("genome");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(8),
        mass: q10(30),
        energy: q10(400),
        membrane: 24,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    let i = world.cells().index(id).expect("alive");
    let control = if awake { Q10_ONE as i16 } else { 0 };
    world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 56);
    for (slot, kind) in [
        (2, OrganelleType::Chloroplast),
        (3, OrganelleType::Mitochondrion),
    ] {
        let mut o = Organelle::finished(kind, if slot == 2 { 100 } else { 50 });
        o.control[0] = control;
        world.cells_mut().slots_mut(i)[slot] = o;
    }
    id
}

/// How many ticks until this cell is gone, giving up after `limit`.
fn lifespan(world: &mut World, id: CellId, limit: u64) -> u64 {
    for tick in 0..limit {
        world.step();
        if world.cells().index(id).is_none() {
            return tick + 1;
        }
    }
    limit
}

#[test]
fn a_cell_that_idles_in_the_dark_outlives_one_that_does_not() {
    // The acceptance test for the whole mechanism, and deliberately the crudest possible form of
    // it: two identical bodies, one with its engines shut, nothing else different. Separate
    // worlds so that neither can shade, crowd or eat the other.
    let mut awake_world = night();
    let awake = sleeper(&mut awake_world, 4, true);
    let mut asleep_world = night();
    let asleep = sleeper(&mut asleep_world, 4, false);

    let awake_ticks = lifespan(&mut awake_world, awake, 20_000);
    let asleep_ticks = lifespan(&mut asleep_world, asleep, 20_000);

    eprintln!("awake {awake_ticks} ticks, asleep {asleep_ticks} ticks");
    assert!(
        awake_ticks < 20_000,
        "the awake cell never starved; the test is measuring nothing"
    );
    assert!(
        asleep_ticks > awake_ticks,
        "shutting the engines bought nothing: {awake_ticks} awake, {asleep_ticks} asleep"
    );
    // A third longer is the number the catalogue predicts — a chloroplast at 100 and a
    // mitochondrion at 50 are 0.127 of a 0.437 bill, three quarters of which is reachable. Held
    // loosely as a floor, because the point is that it is worth a genome's while and not that it
    // is any particular figure.
    assert!(
        asleep_ticks >= awake_ticks + awake_ticks / 5,
        "the discount is not worth evolving for: {awake_ticks} -> {asleep_ticks}"
    );
}

#[test]
fn sleeping_costs_the_income_as_well_as_saving_the_bill() {
    // Why this is not exploitable, and why it needed no anti-cheat: the same word that cuts a
    // mitochondrion's upkeep cuts its respiration. A cell cannot idle and go on earning, so
    // sleeping is only ever correct when there was nothing to earn — which is exactly when it
    // should be. `THROTTLEABLE` is the list of types where that is true, and it is the whole
    // reason the dial is per-type data rather than one line in the upkeep block.
    let mut world = night();
    let id = sleeper(&mut world, 4, false);
    let i = world.cells().index(id).expect("alive");
    let (substrate, oxidant) = {
        let m = world.biology().metabolism.catalogue.metabolism.primary();
        (m.substrate, m.oxidant)
    };
    world.cells_mut().interior_mut(i)[substrate] = q10(50);
    world.cells_mut().interior_mut(i)[oxidant] = q10(50);
    world.adopt_current_contents_as_baseline();

    let before = world.cells().interior(i)[substrate];
    world.run(50);
    let i = world.cells().index(id).expect("still alive");
    let after = world.cells().interior(i)[substrate];
    assert_eq!(
        after,
        before,
        "a shut mitochondrion burned {} of substrate",
        before - after
    );
}

#[test]
fn an_open_organelle_is_priced_exactly_as_it_was_before_any_of_this() {
    // The property that lets this land without re-running a single balance measurement. Every
    // genome in `genomes/` leaves its metabolic controls wide open, so if a full throttle is an
    // identity then no shipped strategy is repriced. Checked here at the level a cell actually
    // experiences — the whole loadout summed — rather than only per-spec in the unit tests.
    let world = night();
    let catalogue = &world.biology().metabolism.catalogue;
    let mut slots = [Organelle::empty(); mm_core::organelle::SLOT_COUNT];
    slots[0] = Organelle::finished(OrganelleType::Membrane, 24);
    slots[1] = Organelle::finished(OrganelleType::Nucleus, 56);
    slots[2] = Organelle::finished(OrganelleType::Chloroplast, 100);
    slots[3] = Organelle::finished(OrganelleType::Mitochondrion, 50);
    slots[4] = Organelle::finished(OrganelleType::Lysosome, 100);

    let hand_summed: i32 = slots
        .iter()
        .filter(|o| o.is_present())
        .map(|o| {
            let spec = catalogue.spec(o.kind);
            (spec.upkeep + spec.upkeep_per_param * o.param as i32) / 16
        })
        .sum();
    assert_eq!(
        catalogue.upkeep(&slots),
        hand_summed,
        "a loadout at full throttle is not priced as the catalogue's own columns say"
    );
}

#[test]
fn the_timeline_reports_a_cell_that_has_shut_something_down() {
    // `Occurrence::Dormancy` was declared at M5 and carried the note "requires a dormant state,
    // which nothing implements" through six milestones, because SPEC §5 describes dormancy as
    // `HALT` yielding the instruction budget and that was never what a cell died of. It is the
    // last of the sixteen to get a mechanism, which is why the two tests that guarded the
    // *absence* of one — `events::all_lists_every_kind_of_occurrence`'s trailing assert and
    // `m5_phylogeny::a_detector_for_a_mechanism_that_does_not_exist_stays_silent` — are gone.
    // Both said in as many words to delete them rather than let them pass vacuously. This is
    // what replaces them.
    let mut world = night();
    let _ = sleeper(&mut world, 4, false);
    assert_eq!(
        world.events().first(Occurrence::Dormancy),
        None,
        "before any tick"
    );
    world.run(4);
    assert!(
        world.events().first(Occurrence::Dormancy).is_some(),
        "a cell carrying two shut engines was not reported as dormant"
    );
}

#[test]
fn a_cell_running_flat_out_is_not_reported_as_resting() {
    // The other half, and the one that would make the newspaper a liar: every organelle in every
    // shipped genome is wide open, so if this fired the timeline would announce dormancy on tick
    // one of every run ever made.
    let mut world = night();
    let _ = sleeper(&mut world, 4, true);
    world.run(200);
    assert_eq!(
        world.events().first(Occurrence::Dormancy),
        None,
        "an awake cell was reported as dormant"
    );
}
