//! M7 acceptance tests — junctions, structure and multicellularity.
//!
//! > Organisms rather than cells.
//!
//! The two that matter most are 2 and 3, and they matter because of what they are *not*
//! allowed to be. Colony locomotion and muscle both have to fall out of one mechanism —
//! distance constraints between cells that each move on their own — with **no code in the
//! engine that moves a cluster as a unit**. If either needed a special case, junctions would
//! have failed at the thing they exist for.

mod common;

use std::path::Path;

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, pos_to_square, q10, POS_ONE};
use mm_core::junction::{self, Junction, JunctionKind};
use mm_core::light::CurrentField;
use mm_core::{
    LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding, Snapshot, World,
};

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

fn petri(seed: u64, size: u32) -> Scenario {
    Scenario {
        name: "petri".to_string(),
        seed,
        width: size,
        height: size,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        current: CurrentField::Still,
        seeding: vec![
            Seeding::Uniform {
                chemical: 11,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 14,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 4,
                per_square: q10(400),
            },
        ],
        ..Scenario::default()
    }
}

/// A world with no biology running: cells sit where they are put, junctions do what they do.
///
/// Used by the physics tests so that a result is about the constraint solver rather than about
/// whether the ancestor happened to divide.
fn still_world(size: u32) -> World {
    let mut world = World::new(petri(1, size)).expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    world
}

fn place(world: &mut World, x: i32, y: i32, key: u8, genome: &[u8]) -> CellId {
    let g = world.genomes().intern(genome.to_vec()).expect("genome");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(y),
        mass: q10(30),
        energy: q10(2_000),
        membrane: 24,
        key,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome: g,
    });
    if let Some(i) = world.cells_mut().index(id) {
        let cells = world.cells_mut();
        cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
        cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
        cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
        cells.interior_mut(i)[11] = q10(40);
        cells.interior_mut(i)[14] = q10(40);
    }
    // Filling a cytoplasm by hand creates matter, which is what scenario setup is for — but
    // `spawn_cell` set the baseline *before* it was filled, so it has to be re-adopted or the
    // conservation check is comparing against a total that never existed.
    world.adopt_current_contents_as_baseline();
    id
}

/// A cell with a membrane and nothing else, for the physics tests.
///
/// No mitochondrion, so it does not respire; no respiration, so it makes no peroxide; no
/// peroxide, so it does not poison itself and die halfway through a six-hundred-tick run. The
/// first version of the locomotion tests used fully equipped cells whose genome was four
/// `HALT`s — they respired, could not excrete, and died, and the "cluster" that drifted
/// thirty-two squares was an empty list of dead cells averaging to the origin.
///
/// Physics tests should test physics.
fn place_inert(world: &mut World, x: i32, y: i32) -> CellId {
    let g = world.genomes().intern(vec![0x2E; 4]).expect("genome");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(y),
        mass: q10(30),
        energy: q10(100_000),
        membrane: 24,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome: g,
    });
    world.adopt_current_contents_as_baseline();
    id
}

/// Join two cells directly, bypassing the genome. For building a hand-written cluster.
fn wire(world: &mut World, a: CellId, b: CellId, kind: JunctionKind) {
    let (ia, ib) = (
        world.cells().index(a).expect("a alive"),
        world.cells().index(b).expect("b alive"),
    );
    let rest = junction::distance(world.cells(), ia, ib).max(POS_ONE);
    let sa = junction::free_slot(world.cells(), ia).expect("a slot on a");
    world.cells_mut().junctions_mut(ia)[sa] = Junction {
        kind,
        other: b,
        rest,
    };
    let sb = junction::free_slot(world.cells(), ib).expect("a slot on b");
    world.cells_mut().junctions_mut(ib)[sb] = Junction {
        kind,
        other: a,
        rest,
    };
}

fn centre_of(world: &World, ids: &[CellId]) -> (i64, i64) {
    let mut sx = 0i64;
    let mut sy = 0i64;
    let mut n = 0i64;
    for id in ids {
        if let Some(i) = world.cells().index(*id) {
            sx += world.cells().x[i] as i64;
            sy += world.cells().y[i] as i64;
            n += 1;
        }
    }
    if n == 0 {
        return (0, 0);
    }
    (sx / n, sy / n)
}

// ---------------------------------------------------------------------------------------
// Acceptance 1 — cheap clonal assembly.
//
// > Clonal cells sharing a receptor key form clusters at `join_base_cost`; non-clonal join
// > attempts cost the full forced penalty. Verified directly against the energy ledger.

/// What one `JOIN` costs the joiner, isolated from everything else the tick does.
///
/// Measured by difference: the same cell, the same genome, the same tick, run once with a
/// target in reach and once without. Metabolism, upkeep and the cost of running the
/// instructions are identical in both, so what is left is the junction.
///
/// The first version of this measured the energy delta across one tick and reported a
/// *negative* cost — the cell had photosynthesised more than the join cost it. A number that
/// comes out the wrong sign is a measurement of the wrong thing.
fn cost_of_join(joiner_key: u8, target_key: u8) -> i32 {
    // `IMM key; IMM kind; IMM handle; JOIN; DROP; HALT` — the handle is the target's arena
    // slot, which is what a touch sensor would have given it.
    let source = format!(
        "        IMM     {joiner_key}\n        IMM     1\n        IMM     1\n        JOIN\n        DROP\n        HALT\n"
    );
    let genome = mm_asm::assemble(&source).expect("assembles").bytes;

    let run = |with_target: bool| -> i32 {
        let mut world = still_world(24);
        // The energy leak is proportional to what a cell is holding, so a cell that has just
        // paid for a join leaks less than one that has not — by a sixty-fourth of the very cost
        // this is isolating. The two runs below would then differ by `cost - cost/64` and the
        // difference of differences would stop cancelling: measured, a 512 join reads as 504.
        // This is a claim about what a junction costs, not about metabolism, so the leak comes
        // off rather than being corrected for.
        let mut biology = world.biology().clone();
        biology.metabolism.rates.energy_leak = 0;
        world.set_biology(biology);
        let a = place(&mut world, 10, 10, joiner_key, &genome);
        if with_target {
            place(&mut world, 11, 10, target_key, &[0x2E; 4]);
        } else {
            // Present but far out of reach, so the tick does the same work and the join
            // simply has nothing to attach to.
            place(&mut world, 22, 22, target_key, &[0x2E; 4]);
        }
        let before = {
            let i = world.cells().index(a).expect("alive");
            world.cells().energy[i]
        };
        world.step();
        let after = {
            let i = world.cells().index(a).expect("alive");
            world.cells().energy[i]
        };
        before - after
    };
    run(true) - run(false)
}

#[test]
fn acceptance_a_matching_key_is_cheap_and_a_mismatch_costs_the_penalty() {
    let config = mm_core::junction::JunctionConfig::default();

    let consensual = cost_of_join(11, 11);
    let forced = cost_of_join(11, 77);

    eprintln!("consensual join cost {consensual}, forced {forced}");
    assert_eq!(
        consensual, config.join_base_cost,
        "a matching key should cost exactly join_base_cost"
    );
    assert_eq!(
        forced,
        junction::join_cost(&config, false, 24),
        "a mismatched key should cost the full forced penalty against the target's membrane"
    );
    assert!(
        forced > consensual * 50,
        "forced {forced} against consensual {consensual} is not a deterrent"
    );
}

#[test]
fn a_clonal_colony_assembles_almost_for_free() {
    // SPEC §8.2's first claim: the bootstrap problem for multicellularity dissolves because
    // clonal cells cooperate by default. Eight clones joining costs eight base costs.
    let config = mm_core::junction::JunctionConfig::default();
    let mut world = still_world(32);
    let ids: Vec<CellId> = (0..8)
        .map(|k| place(&mut world, 6 + k, 10, 11, &[0x2E; 4]))
        .collect();
    let before: i32 = ids
        .iter()
        .filter_map(|id| world.cells().index(*id))
        .map(|i| world.cells().energy[i])
        .sum();

    for pair in ids.windows(2) {
        wire(&mut world, pair[0], pair[1], JunctionKind::Hard);
    }
    // Wiring bypasses the cost, so charge it the way `JOIN` would to make the comparison.
    let would_cost = junction::join_cost(&config, true, 24) * 7;
    assert!(
        would_cost < q10(10),
        "assembling an eight-cell colony costs {would_cost}; that is not 'nearly free'"
    );
    let _ = before;

    let mut components = world.components().clone();
    let i = world.cells().index(ids[0]).expect("alive");
    assert_eq!(components.size_of(i), 8, "the colony is not one organism");
}

// ---------------------------------------------------------------------------------------
// Acceptance 2 — colony locomotion is emergent.
//
// > A hand-written 8-cell cluster with cilia on one member translates coherently, with no
// > code in the engine that moves clusters as a unit.

#[test]
fn acceptance_a_cluster_translates_when_one_member_swims() {
    let mut world = still_world(64);
    // A chain of eight, joined hard. Only the first has cilia.
    let ids: Vec<CellId> = (0..8)
        .map(|k| place_inert(&mut world, 20 + k, 32))
        .collect();
    for pair in ids.windows(2) {
        wire(&mut world, pair[0], pair[1], JunctionKind::Hard);
    }
    {
        let i = world.cells().index(ids[0]).expect("alive");
        let cells = world.cells_mut();
        let mut cilium = Organelle::finished(OrganelleType::Cilium, 200);
        // Full power, pointing along +x.
        cilium.control[0] = mm_core::Q10_ONE as i16;
        cilium.control[1] = 0;
        cells.slots_mut(i)[4] = cilium;
    }

    let start = centre_of(&world, &ids);
    let tail_start = {
        let i = world.cells().index(*ids.last().unwrap()).expect("alive");
        world.cells().x[i]
    };
    world.run(600);
    let end = centre_of(&world, &ids);
    let tail_end = {
        let i = world.cells().index(*ids.last().unwrap()).expect("alive");
        world.cells().x[i]
    };

    let moved = end.0 - start.0;
    eprintln!(
        "cluster centre moved {moved} POS units; the far tail moved {}",
        tail_end - tail_start
    );
    assert!(
        moved.abs() > POS_ONE as i64,
        "the cluster barely moved ({moved} POS units); one cilium is not dragging it"
    );
    // Coherently: the far end came too, rather than the swimmer tearing away.
    assert!(
        (tail_end - tail_start).abs() > POS_ONE / 2,
        "the swimmer moved but the tail did not follow; the cluster came apart"
    );
    // And it is still one organism.
    let i = world.cells().index(ids[0]).expect("alive");
    let mut components = world.components().clone();
    assert!(
        components.size_of(i) >= 6,
        "the cluster fell apart into pieces of {}",
        components.size_of(i)
    );
}

#[test]
fn nothing_in_the_engine_moves_a_cluster_as_a_unit() {
    // The other half of acceptance 2, stated as the property it depends on: joining cells
    // together must not, by itself, make them go anywhere.
    //
    // Not "a joined cluster does not move" — it does, because Brownian jitter moves every cell
    // and the centre of eight of them takes a random walk. The first version of this asserted
    // zero drift and failed on 300 POS units of jitter, which was the simulation working.
    //
    // What has to hold is that being joined does not *add* motion: the same eight cells,
    // joined and unjoined, drift about the same. A solver injecting momentum would show up as
    // the joined cluster travelling further.
    let drift_of = |joined: bool| -> i64 {
        let mut world = still_world(64);
        let ids: Vec<CellId> = (0..8)
            .map(|k| place_inert(&mut world, 20 + k, 32))
            .collect();
        if joined {
            for pair in ids.windows(2) {
                wire(&mut world, pair[0], pair[1], JunctionKind::Hard);
            }
        }
        let start = centre_of(&world, &ids);
        world.run(600);
        let end = centre_of(&world, &ids);
        ((end.0 - start.0).abs()).max((end.1 - start.1).abs())
    };
    let joined = drift_of(true);
    let loose = drift_of(false);
    eprintln!("joined cluster drifted {joined} POS units, the same cells unjoined {loose}");
    assert!(
        joined <= loose.max(POS_ONE as i64) * 3,
        "joining eight cells made them travel {joined} POS units against {loose} loose; the \
         constraint solver is injecting momentum"
    );
}

// ---------------------------------------------------------------------------------------
// Acceptance 3 — muscle works.
//
// > A hand-written cluster modulating `JLEN` periodically produces measurable shape change
// > and net displacement.

#[test]
fn acceptance_modulating_rest_length_changes_a_cluster_s_shape() {
    let mut world = still_world(64);
    let ids: Vec<CellId> = (0..4)
        .map(|k| place_inert(&mut world, 20 + k * 2, 32))
        .collect();
    for pair in ids.windows(2) {
        wire(&mut world, pair[0], pair[1], JunctionKind::Hard);
    }

    // Measured as the length of the body along its own chain, not as the distance between its
    // ends. A contracting chain buckles — the cells pile up and then expand into whatever
    // shape they like — so an end-to-end measurement reports a body that folded as a body that
    // shrank, and one that unfolded sideways as one that barely grew. Summing the links is
    // what "shape change" means for a chain, whichever way it is pointing.
    let span = |world: &World| -> i32 {
        ids.windows(2)
            .filter_map(|pair| {
                let a = world.cells().index(pair[0])?;
                let b = world.cells().index(pair[1])?;
                Some(junction::distance(world.cells(), a, b))
            })
            .sum()
    };

    world.run(60);
    let relaxed = span(&world);

    // Contract every junction to its minimum.
    for id in &ids {
        let i = world.cells().index(*id).expect("alive");
        for slot in 0..mm_core::junction::JUNCTIONS_PER_CELL {
            if world.cells().junctions(i)[slot].kind == JunctionKind::Hard {
                world.cells_mut().junctions_mut(i)[slot].rest = POS_ONE / 2;
            }
        }
    }
    world.run(120);
    let contracted = span(&world);

    // And extend them.
    for id in &ids {
        let i = world.cells().index(*id).expect("alive");
        for slot in 0..mm_core::junction::JUNCTIONS_PER_CELL {
            if world.cells().junctions(i)[slot].kind == JunctionKind::Hard {
                world.cells_mut().junctions_mut(i)[slot].rest = POS_ONE * 2;
            }
        }
    }
    world.run(120);
    let extended = span(&world);

    eprintln!("span relaxed {relaxed}, contracted {contracted}, extended {extended}");
    assert!(
        contracted < relaxed,
        "contracting the junctions did not shorten the body: {contracted} against {relaxed}"
    );
    assert!(
        extended > contracted,
        "extending did not lengthen the body: {extended} against {contracted}"
    );
    assert!(
        extended - contracted > POS_ONE,
        "the shape change is {} POS units, which is not measurable movement",
        extended - contracted
    );
}

#[test]
fn jlen_from_a_genome_reaches_the_junction() {
    // The opcode path, as opposed to writing rest lengths by hand. `JLEN ( v jidx -- )`.
    let source = "        IMM     100\n        ZERO\n        JLEN\n        HALT\n";
    let genome = mm_asm::assemble(source).expect("assembles").bytes;
    let mut world = still_world(24);
    let a = place(&mut world, 10, 10, 11, &genome);
    let b = place(&mut world, 12, 10, 11, &[0x2E; 4]);
    wire(&mut world, a, b, JunctionKind::Hard);

    let before = {
        let i = world.cells().index(a).expect("alive");
        world.cells().junctions(i)[0].rest
    };
    world.step();
    let after = {
        let i = world.cells().index(a).expect("alive");
        world.cells().junctions(i)[0].rest
    };
    assert_ne!(before, after, "JLEN did not move the rest length");
    // And both ends agree, or the solver would be pulling against itself.
    let j = world.cells().index(b).expect("alive");
    assert_eq!(
        world.cells().junctions(j)[0].rest,
        after,
        "the two ends of one junction disagree about its rest length"
    );
}

// ---------------------------------------------------------------------------------------
// Acceptance 4 — parasitism is possible and costly.
//
// > A hand-written parasite with the correct key infects successfully; the same parasite with
// > a wrong key succeeds only after paying the penalty, and dies if under-resourced.

/// A parasite: join softly, then write a byte into whatever it joined.
fn parasite_genome(key: u8) -> Vec<u8> {
    let source = format!(
        "        IMM     {key}\n        ZERO\n        IMM     1\n        JOIN\n        DROP\n\
         \n        ZERO\n        SETPA\n        ZERO\n        SETPB\n        ZERO\n        INJECT\n\
         \n        DROP\n        HALT\n"
    );
    mm_asm::assemble(&source)
        .expect("the parasite assembles")
        .bytes
}

/// Run a parasite against a host and report what happened.
///
/// `spent` is the joiner's energy delta over the run, which includes whatever else it did.
/// The assertions below compare it against a *lower bound*, so metabolism working in the
/// parasite's favour can only make the test harder to pass, never easier.
fn infect(parasite_key: u8, host_key: u8, energy: i32) -> (bool, i32, bool) {
    let mut world = still_world(24);
    let genome = parasite_genome(parasite_key);
    let parasite = place(&mut world, 10, 10, parasite_key, &genome);
    let host = place(&mut world, 11, 10, host_key, &[0x2E; 40]);
    if let Some(i) = world.cells_mut().index(parasite) {
        world.cells_mut().energy[i] = energy;
    }
    let host_genome_before = {
        let i = world.cells().index(host).expect("alive");
        world.cells().genome[i].bytes().to_vec()
    };
    let energy_before = {
        let i = world.cells().index(parasite).expect("alive");
        world.cells().energy[i]
    };

    world.run(6);

    let joined = world
        .cells()
        .index(parasite)
        .is_some_and(|i| world.cells().junctions(i).iter().any(|j| j.is_some()));
    let spent = world
        .cells()
        .index(parasite)
        .map_or(energy_before, |i| energy_before - world.cells().energy[i]);
    let infected = world
        .cells()
        .index(host)
        .is_some_and(|i| world.cells().genome[i].bytes() != host_genome_before.as_slice());
    (joined, spent, infected)
}

#[test]
fn acceptance_a_parasite_with_the_right_key_infects_cheaply() {
    let (joined, spent, infected) = infect(33, 33, q10(2_000));
    eprintln!("matching key: joined {joined}, spent {spent}, host genome rewritten {infected}");
    assert!(joined, "the parasite did not form a junction");
    assert!(
        infected,
        "the parasite joined but never wrote into its host; INJECT is not reaching"
    );
    let config = mm_core::junction::JunctionConfig::default();
    assert!(
        spent < config.join_base_cost * 8,
        "a consensual infection cost {spent}, which is not cheap"
    );
}

#[test]
fn acceptance_a_parasite_with_the_wrong_key_pays_the_penalty() {
    let (joined, spent, infected) = infect(33, 99, q10(4_000));
    let config = mm_core::junction::JunctionConfig::default();
    eprintln!("wrong key: joined {joined}, spent {spent}, host rewritten {infected}");
    assert!(
        joined,
        "a well-resourced parasite could not force a junction"
    );
    // Compared against the consensual cost rather than the exact penalty, because `spent` is
    // a whole-run delta and the parasite is photosynthesising throughout. What has to hold is
    // that forcing is dramatically dearer than consenting, which is the mechanic.
    let consensual = infect(33, 33, q10(4_000)).1;
    eprintln!("forced {spent} against consensual {consensual}");
    assert!(
        spent > consensual + junction::join_cost(&config, false, 24) / 2,
        "forcing cost {spent} against {consensual} consensual; the penalty is not being \
         charged"
    );
    assert!(infected, "it forced the junction but did not use it");
}

#[test]
fn acceptance_an_under_resourced_parasite_fails() {
    // The other half: consent is economic, so a parasite that cannot pay does not get in.
    let (joined, _spent, infected) = infect(33, 99, q10(5));
    assert!(
        !joined,
        "a parasite with almost no energy forced a junction anyway; the penalty is not \
         being charged"
    );
    assert!(!infected, "it got in without paying");
}

#[test]
fn a_failed_join_leaks_nothing_about_the_key() {
    // SPEC §8.2's probe semantics. A failed `JOIN` must return one bit, or the key is
    // hill-climbable in about seven probes and parasitism is trivial.
    let config = mm_core::junction::JunctionConfig::default();
    assert!(
        !config.probe_leaks_distance,
        "probe_leaks_distance is on by default; the key space is hill-climbable"
    );

    // And the observable outcome of a refused join carries no gradient: two wrong keys at
    // very different Hamming distances from the truth are indistinguishable.
    let near = infect(0b0111_1110, 0b0111_1111, q10(5));
    let far = infect(0b0000_0000, 0b0111_1111, q10(5));
    assert_eq!(
        (near.0, near.2),
        (far.0, far.2),
        "a near-miss key behaved differently from a far one; the key leaks its distance"
    );
}

// ---------------------------------------------------------------------------------------
// Acceptance 5 — constraint cost. Measured in `benches/population.rs`, which is where the
// other performance gates live; what is checked here is that the solve is bounded work.

#[test]
fn the_solver_does_a_bounded_amount_of_work() {
    let mut world = still_world(64);
    let ids: Vec<CellId> = (0..64)
        .map(|k| place(&mut world, 2 + (k % 32), 2 + (k / 32) * 2, 11, &[0x2E; 4]))
        .collect();
    let mut wired = 0;
    for pair in ids.windows(2) {
        let (ia, ib) = (
            world.cells().index(pair[0]).expect("alive"),
            world.cells().index(pair[1]).expect("alive"),
        );
        if junction::free_slot(world.cells(), ia).is_some()
            && junction::free_slot(world.cells(), ib).is_some()
        {
            wire(&mut world, pair[0], pair[1], JunctionKind::Hard);
            wired += 1;
        }
    }
    assert!(wired > 32, "only wired {wired} junctions");
    world.step();
    let report = world.report();
    // Each pair is solved once per iteration, by its lower slot — not twice.
    assert!(
        report.physics.constraints <= wired as u32 * 2,
        "{} constraints solved for {wired} junctions; pairs are being solved from both ends",
        report.physics.constraints
    );
    assert!(report.physics.constraints > 0, "nothing was solved at all");
}

// ---------------------------------------------------------------------------------------
// Acceptance 6 — differentiation emerges. Long, stochastic, and diagnostic rather than
// blocking by the milestone's own words.

#[test]
#[ignore = "long open-ended run across 10 seeds; --release --ignored"]
fn acceptance_differentiation_emerges() {
    let ticks = common::env_usize("MM_M7_TICKS", 200_000) as u64;
    let seeds = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let bytes = assemble("ancestor.mm");
    let mut found = 0;

    for seed in seeds {
        let mut world = World::new(petri(seed, 96)).expect("world");
        world.set_biology(BiologyConfig {
            mutation: MutationRates::default(),
            ..BiologyConfig::default()
        });
        for k in 0..16u32 {
            let g = world.genomes().intern(bytes.clone()).expect("genome");
            let id = world.spawn_cell(CellSeed {
                x: pos((8 + (k % 4) * 20) as i32),
                y: pos((8 + (k / 4) * 20) as i32),
                mass: q10(30),
                energy: q10(400),
                membrane: 24,
                key: 11,
                badge: 0,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome: g,
            });
            if let Some(i) = world.cells_mut().index(id) {
                let cells = world.cells_mut();
                cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
                cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
                cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
                cells.interior_mut(i)[11] = q10(40);
                cells.interior_mut(i)[14] = q10(40);
            }
        }
        world.adopt_current_contents_as_baseline();

        let mut best_cluster = 0u32;
        let mut differentiated = false;
        for _ in 0..(ticks / 1_000).max(1) {
            world.run(1_000);
            if world.cells().is_empty() {
                break;
            }
            let clusters: Vec<(usize, u32)> = {
                let components = world.components();
                components.clusters(8)
            };
            for (root, size) in clusters {
                best_cluster = best_cluster.max(size);
                let kinds = {
                    let cells_ptr = world.cells() as *const _;
                    // Safe: `distinct_loadouts` reads the arena and mutates only the
                    // components' own path-compression scratch.
                    let cells: &mm_core::CellArena = unsafe { &*cells_ptr };
                    junction::distinct_loadouts(cells, world.components(), root)
                };
                if kinds >= 2 {
                    differentiated = true;
                    break;
                }
            }
            if differentiated {
                break;
            }
        }
        eprintln!("seed {seed}: largest cluster {best_cluster}, differentiated {differentiated}");
        if differentiated {
            found += 1;
        }
    }

    // The milestone says a failure here is diagnostic, not a blocker — so the number is
    // reported whatever it is, and the assertion says which parameter to look at.
    eprintln!("differentiation appeared in {found} of 10 seeds");
    assert!(
        found >= 3,
        "differentiated clusters appeared in only {found} of 10 seeds. The ancestor has no \
         `JOIN` in it, so every junction in this run had to be invented by mutation before \
         differentiation could even begin — look at whether clusters form at all before \
         looking at whether they differentiate."
    );
}

// ---------------------------------------------------------------------------------------
// Junctions are state, so they round-trip. Hard rule 7.

#[test]
fn junctions_survive_a_snapshot() {
    let mut world = still_world(32);
    let ids: Vec<CellId> = (0..6)
        .map(|k| place(&mut world, 6 + k * 2, 10, 11, &[0x2E; 4]))
        .collect();
    for pair in ids.windows(2) {
        wire(&mut world, pair[0], pair[1], JunctionKind::Hard);
    }
    wire(&mut world, ids[0], ids[5], JunctionKind::Soft);
    world.run(20);

    let bytes = Snapshot::write(&world).expect("write");
    let mut restored = Snapshot::read(&bytes).expect("read");
    assert_eq!(
        restored.state_hash(),
        world.state_hash(),
        "junctions are missing from the snapshot format"
    );
    assert_eq!(restored, world, "some field is missing from the format");

    // And the components come back the same, which is what makes an organism survive a save.
    let a = world.components().largest();
    let b = restored.components().largest();
    assert_eq!(a, b, "the restored world has a different largest organism");

    // Running on from a snapshot keeps the constraints identical.
    let mut straight = world;
    straight.run(200);
    restored.run(200);
    assert_eq!(
        restored.state_hash(),
        straight.state_hash(),
        "a resumed world's constraints diverged"
    );
}

#[test]
fn a_dead_cell_takes_its_junctions_with_it() {
    let mut world = still_world(32);
    let a = place(&mut world, 10, 10, 11, &[0x2E; 4]);
    let b = place(&mut world, 11, 10, 11, &[0x2E; 4]);
    wire(&mut world, a, b, JunctionKind::Hard);
    world.kill_cell(b);
    world.step();

    let i = world.cells().index(a).expect("a is alive");
    assert!(
        world.cells().junctions(i).iter().all(|j| !j.is_some()),
        "a junction to a dead cell survived it"
    );
    world.check_matter().expect("books balance");
}

#[test]
fn junctions_never_break_conservation() {
    // The tools test for M6 did this for the laboratory tools; this is the same claim for the
    // mechanism that moves chemicals between cells.
    let source = "        IMM     20\n        IMM     11\n        ZERO\n        JXFER\n        DROP\n        HALT\n";
    let genome = mm_asm::assemble(source).expect("assembles").bytes;
    let mut world = still_world(32);
    let a = place(&mut world, 10, 10, 11, &genome);
    let b = place(&mut world, 11, 10, 11, &[0x2E; 4]);
    wire(&mut world, a, b, JunctionKind::Soft);
    world.run(200);
    // Checked through the ledger rather than by comparing raw per-chemical totals. Metabolism
    // transmutes one species into another every tick — that is what `Ledger::convert` is for —
    // so the totals legitimately move, and only the ledger knows by how much. Comparing them
    // directly would be asserting that photosynthesis does not happen.
    world.check_matter().expect("books balance");
    world.check_energy().expect("energy balances");
}

#[test]
fn a_transfer_actually_moves_something() {
    let source = "        IMM     20\n        IMM     11\n        ZERO\n        JXFER\n        DROP\n        HALT\n";
    let genome = mm_asm::assemble(source).expect("assembles").bytes;
    let mut world = still_world(32);
    let a = place(&mut world, 10, 10, 11, &genome);
    let b = place(&mut world, 11, 10, 11, &[0x2E; 4]);
    wire(&mut world, a, b, JunctionKind::Soft);

    let held = |world: &World, id: CellId| -> i32 {
        world
            .cells()
            .index(id)
            .map_or(0, |i| world.cells().interior(i)[11])
    };
    let before = held(&world, b);
    world.run(5);
    let after = held(&world, b);
    assert!(
        after > before,
        "nothing crossed the junction: {before} then {after}"
    );
}

#[test]
fn the_detectors_notice_junctions_now_that_they_exist() {
    use mm_core::events::Occurrence;
    let mut world = still_world(32);
    let ids: Vec<CellId> = (0..5)
        .map(|k| place(&mut world, 6 + k * 2, 10, 11, &[0x2E; 4]))
        .collect();
    world.archive_mut().sample_interval = 5;
    for pair in ids.windows(2) {
        wire(&mut world, pair[0], pair[1], JunctionKind::Hard);
    }
    // Between adjacent cells: a soft junction breaks beyond `soft_max_range`, and the first
    // version of this wired one across the whole chain and then wondered why it vanished.
    wire(&mut world, ids[0], ids[1], JunctionKind::Soft);
    world.run(30);

    assert!(
        world.events().first(Occurrence::HardJunction).is_some(),
        "a hard junction formed and the newspaper did not notice"
    );
    assert!(world.events().first(Occurrence::SoftJunction).is_some());
    assert!(
        world.events().first(Occurrence::Cluster(4)).is_some(),
        "a five-cell cluster did not register as a cluster of four"
    );
    // And the ones whose mechanisms are still M8's stay silent.
    assert_eq!(world.events().first(Occurrence::Predation), None);
    assert_eq!(world.events().first(Occurrence::Dormancy), None);
}

#[test]
fn a_cluster_larger_than_four_junctions_wide_cannot_form() {
    // The slot limit, stated as behaviour: a cell holds four junctions, so it cannot become a
    // hub the whole slide hangs off. Worth pinning because it bounds the per-cell budget.
    let mut world = still_world(32);
    let hub = place(&mut world, 16, 16, 11, &[0x2E; 4]);
    let spokes: Vec<CellId> = (0..6)
        .map(|k| place(&mut world, 14 + k, 17, 11, &[0x2E; 4]))
        .collect();
    let mut wired = 0;
    for spoke in &spokes {
        let ih = world.cells().index(hub).expect("alive");
        if junction::free_slot(world.cells(), ih).is_some() {
            wire(&mut world, hub, *spoke, JunctionKind::Hard);
            wired += 1;
        }
    }
    assert_eq!(
        wired,
        mm_core::junction::JUNCTIONS_PER_CELL,
        "the hub took more junctions than it has slots"
    );
    let _ = pos_to_square(0);
}

#[test]
#[ignore = "diagnostic: watches a two-cell junction settle"]
fn diagnose_rest_length_tracking() {
    let mut world = still_world(64);
    let a = place_inert(&mut world, 20, 32);
    let b = place_inert(&mut world, 22, 32);
    wire(&mut world, a, b, JunctionKind::Hard);
    let d = |w: &World| -> i32 {
        let (ia, ib) = (w.cells().index(a).unwrap(), w.cells().index(b).unwrap());
        junction::distance(w.cells(), ia, ib)
    };
    let rest_of = |w: &World| -> i32 {
        let ia = w.cells().index(a).unwrap();
        w.cells().junctions(ia)[0].rest
    };
    eprintln!("start: distance {} rest {}", d(&world), rest_of(&world));
    for target in [POS_ONE / 2, POS_ONE * 2, POS_ONE * 4] {
        let ia = world.cells().index(a).unwrap();
        let ib = world.cells().index(b).unwrap();
        world.cells_mut().junctions_mut(ia)[0].rest = target;
        world.cells_mut().junctions_mut(ib)[0].rest = target;
        for step in [1, 5, 20, 100] {
            world.run(step);
            eprintln!(
                "  rest {target}: after {step} more ticks distance {} (junction still there: {})",
                d(&world),
                world.cells().junctions(world.cells().index(a).unwrap())[0].is_some()
            );
        }
    }
    eprintln!(
        "radius in Q10 {}",
        mm_core::biology::radius(world.cells(), world.cells().index(a).unwrap())
    );
}
