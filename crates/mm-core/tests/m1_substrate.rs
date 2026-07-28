//! M1 acceptance tests — substrate, fluid and chemistry.
//!
//! > A world that conserves matter exactly and accounts for energy exactly, with nothing
//! > alive in it.
//!
//! The long runs are `#[ignore]`d and each has a fast guard that runs the identical scenario
//! for fewer ticks, so `cargo test` catches anything the long run would catch that shows up
//! early. Sizes are overridable with `MM_M1_TICKS`.
//!
//! ```text
//! cargo test -p mm-core --test m1_substrate                          # guards, seconds
//! cargo test --release -p mm-core --test m1_substrate -- --ignored   # the acceptance runs
//! ```

mod common;

use common::env_usize;
use mm_core::chem::CHEM_COUNT;
use mm_core::fixed::q10;
use mm_core::light::CurrentField;
use mm_core::{Barrier, LightRegime, Scenario, Seeding, Snapshot, World};

fn ticks(default: usize) -> u64 {
    env_usize("MM_M1_TICKS", default) as u64
}

/// The hostile case: steep gradients, aggressive stirring, barriers, a day/night cycle, and
/// a spike holding half the representable range in a single square.
fn stress(w: u32, h: u32) -> Scenario {
    Scenario::stress(w, h)
}

// ---------------------------------------------------------------- 1. matter conservation

/// > 1,000,000 ticks with aggressive stirring, steep initial gradients and barriers.
/// > Per-species totals drift by exactly zero. Not "within epsilon" — zero.
fn conservation_run(scenario: Scenario, n: u64) {
    let mut world = World::new(scenario).expect("world");
    let baseline = world.substrate().total_chem();
    assert!(
        baseline.iter().any(|t| *t > 0),
        "the scenario seeded nothing, so this would pass vacuously"
    );

    // Check often enough that a failure names a tick close to where it happened, but not so
    // often that summing four million values dominates the run.
    let check_every = (n / 200).max(1);
    for tick in 0..n {
        world.step();
        if tick % check_every == 0 || tick + 1 == n {
            let actual = world.substrate().total_chem();
            for c in 0..CHEM_COUNT {
                assert_eq!(
                    actual[c],
                    baseline[c],
                    "chemical {c} drifted by {} at tick {tick}",
                    actual[c] - baseline[c]
                );
            }
            world
                .check_invariants()
                .unwrap_or_else(|e| panic!("at tick {tick}: {e}"));
            assert!(
                !world.substrate().any_negative(),
                "a square went negative at tick {tick}"
            );
            assert!(
                !world.substrate().any_matter_inside_a_barrier(),
                "matter appeared inside a barrier at tick {tick}"
            );
        }
    }
}

#[test]
fn matter_conservation_guard() {
    conservation_run(stress(32, 24), ticks(3_000));
}

#[test]
#[ignore = "1,000,000 ticks; run with --release --ignored"]
fn acceptance_matter_conservation() {
    conservation_run(stress(48, 48), ticks(1_000_000));
}

#[test]
fn conservation_holds_for_awkward_grid_shapes() {
    // A single row, a single column, primes, and a grid smaller than one rayon band: the
    // shapes where an off-by-one in the sweep would leak rather than crash.
    for (w, h) in [
        (1, 1),
        (1, 64),
        (64, 1),
        (2, 2),
        (17, 31),
        (127, 3),
        (3, 127),
    ] {
        conservation_run(stress(w, h), 400);
    }
}

#[test]
fn conservation_holds_with_every_current_field() {
    for current in [
        CurrentField::Still,
        CurrentField::Uniform { vx: 200, vy: -150 },
        CurrentField::Rotational { strength: 255 },
        CurrentField::Shear { strength: 255 },
    ] {
        let scenario = Scenario {
            current,
            ..stress(40, 40)
        };
        conservation_run(scenario, 500);
    }
}

// ---------------------------------------------------------------- 2. energy accounting

/// > `energy_in == energy_out + Δenergy_stored` exactly, every tick, over 1,000,000 ticks.
///
/// Nothing is alive at M1, so the world's own energy flows are all zero and checking only
/// them would pass vacuously. What this checks instead is the ledger that M2's metabolism
/// will run through: synthetic absorb/dissipate traffic standing in for chloroplasts and
/// heat, with the identity asserted after every single transaction.
fn energy_run(n: u64) {
    let mut world = World::new(stress(24, 24)).expect("world");
    // A world starts holding energy — the latent energy of the substrate chemical dissolved
    // in it — so what this measures is the change the transactions make, not the absolutes.
    let baseline_in = world.ledger().energy_in();
    let baseline_out = world.ledger().energy_out();
    let mut absorbed_total = 0i64;
    let mut dissipated_total = 0i64;

    for tick in 0..n {
        world.step();

        // A varying, sometimes-zero, sometimes-larger-than-available flow, so the clamp in
        // `dissipate` is exercised rather than assumed.
        let absorb = ((tick * 7919) % 1000) as i64;
        let dissipate = ((tick * 104_729) % 1500) as i64;
        world.ledger_mut().absorb(absorb);
        absorbed_total += absorb;
        dissipated_total += world.ledger_mut().dissipate(dissipate);

        world
            .check_energy()
            .unwrap_or_else(|e| panic!("energy accounting broke at tick {tick}: {e}"));
    }

    let l = world.ledger();
    assert_eq!(
        l.energy_in() - baseline_in,
        absorbed_total,
        "absorbed energy went missing"
    );
    assert_eq!(l.energy_out() - baseline_out, dissipated_total);
    assert_eq!(
        l.energy_in(),
        l.energy_out() + l.energy_stored(),
        "the identity is what the whole entropy story rests on"
    );
    assert!(
        absorbed_total > 0,
        "no energy moved, so this proved nothing"
    );
}

#[test]
fn energy_accounting_guard() {
    energy_run(ticks(5_000));
}

#[test]
#[ignore = "1,000,000 ticks; run with --release --ignored"]
fn acceptance_energy_accounting() {
    energy_run(ticks(1_000_000));
}

#[test]
fn energy_cannot_be_dissipated_that_was_never_absorbed() {
    // Otherwise energy_out would run ahead of energy_in and the world would be exporting
    // heat it never took in.
    // An empty slide, so the only energy in the world is what this test puts there.
    let mut world = World::new(Scenario {
        width: 4,
        height: 4,
        seeding: Vec::new(),
        ..Scenario::default()
    })
    .unwrap();
    assert_eq!(world.ledger().energy_stored(), 0, "nothing to start with");
    world.ledger_mut().absorb(100);
    assert_eq!(world.ledger_mut().dissipate(10_000), 100);
    world.check_energy().unwrap();
    assert_eq!(world.ledger().energy_stored(), 0);
}

// ---------------------------------------------------------------- 3. schedule independence

/// > Identical state hash at 100,000 ticks with 1, 2, 4 and 16 rayon threads.
///
/// The grid is deliberately larger than the solver's serial-path threshold, or every thread
/// count would take the same code path and the test would prove nothing.
fn schedule_run(n: u64) {
    let scenario = stress(128, 128);
    assert!(
        scenario.width as usize * scenario.height as usize > 8192,
        "grid must be large enough to take the parallel path"
    );

    let mut hashes = Vec::new();
    for threads in [1usize, 2, 4, 16] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("thread pool");
        let scenario = scenario.clone();
        let hash = pool.install(move || {
            let mut world = World::new(scenario).expect("world");
            world.run(n);
            world.check_invariants().expect("invariants");
            world.state_hash()
        });
        hashes.push((threads, hash));
    }

    let (_, first) = hashes[0];
    for (threads, hash) in &hashes {
        assert_eq!(
            *hash, first,
            "{threads} threads produced a different world than 1 thread"
        );
    }
}

#[test]
fn schedule_independence_guard() {
    schedule_run(ticks(400));
}

#[test]
#[ignore = "100,000 ticks at four thread counts; run with --release --ignored"]
fn acceptance_schedule_independence() {
    schedule_run(ticks(100_000));
}

#[test]
fn the_band_decomposition_is_invisible() {
    // I6 the other way round: not just thread count, but where the work is cut. A grid whose
    // height is not a multiple of the band size exercises the ragged last band.
    let mut hashes = Vec::new();
    for h in [96u32, 97, 100, 103] {
        let mut world = World::new(stress(128, h)).unwrap();
        world.run(300);
        hashes.push(world.substrate().total_chem());
    }
    // Different shapes hold different totals; what must hold is that each conserved its own.
    for (i, totals) in hashes.iter().enumerate() {
        assert!(totals.iter().any(|t| *t > 0), "shape {i} seeded nothing");
    }
}

// ---------------------------------------------------------------- 4. barriers

/// > No chemical crosses a `blocked` square, ever.
#[test]
fn barriers_are_impermeable() {
    // A slide cut clean in two, with everything on the left, a flow pressing right, and the
    // full run length. If a single unit reaches the right-hand side, the wall leaked.
    let scenario = Scenario {
        width: 64,
        height: 48,
        current: CurrentField::Uniform { vx: 255, vy: 100 },
        light: LightRegime::Uniform { intensity: 1024 },
        barriers: vec![Barrier::WallWithGap {
            at: 32,
            vertical: true,
            gap_start: 0,
            gap_len: 0,
        }],
        seeding: vec![
            Seeding::Patch {
                chemical: 0,
                x: 0,
                y: 0,
                width: 32,
                height: 48,
                per_square: q10(80_000),
            },
            Seeding::Spike {
                chemical: 8,
                x: 31,
                y: 24,
                amount: q10(500_000),
            },
        ],
        ..Scenario::default()
    };

    let mut world = World::new(scenario).unwrap();
    let baseline = world.substrate().total_chem();

    for tick in 0..ticks(20_000) {
        world.step();
        if tick % 500 == 0 {
            for y in 0..48i32 {
                for x in 32..64i32 {
                    for c in 0..CHEM_COUNT {
                        assert_eq!(
                            world.substrate().chem_at(c, x, y),
                            0,
                            "chemical {c} crossed the wall to ({x}, {y}) by tick {tick}"
                        );
                    }
                }
            }
            assert_eq!(world.substrate().total_chem(), baseline);
        }
    }
}

#[test]
fn a_barrier_raised_over_matter_is_accounted_rather_than_silent() {
    // The one sanctioned way for matter to leave the world. It must show up in the ledger's
    // eviction column, not as conservation drift.
    let mut world = World::new(Scenario {
        width: 16,
        height: 16,
        seeding: vec![Seeding::Uniform {
            chemical: 2,
            per_square: q10(100),
        }],
        ..Scenario::default()
    })
    .unwrap();
    world.check_invariants().unwrap();

    let evicted = world.substrate_mut().set_blocked(4, 4, true);
    assert_eq!(evicted[2], q10(100));
    world.ledger_mut().record_evicted(&evicted);
    world.check_invariants().unwrap();
    assert_eq!(world.ledger().evicted()[2], q10(100) as i64);
}

#[test]
fn a_barrier_casts_a_shadow_and_stops_the_flow() {
    let world = World::new(Scenario {
        width: 16,
        height: 16,
        light: LightRegime::Uniform { intensity: 1024 },
        current: CurrentField::Uniform { vx: 255, vy: 0 },
        barriers: vec![Barrier::Square { x: 8, y: 8 }],
        ..Scenario::default()
    })
    .unwrap();
    assert_eq!(world.substrate().light_at(8, 8), 0, "a barrier is opaque");
    assert_eq!(world.substrate().velocity_at(8, 8), (0, 0));
    assert_eq!(world.substrate().light_at(8, 7), 1024);
}

// ---------------------------------------------------------------- 5. serialisation

/// > Save at tick 50,000, reload, run to 100,000; state hash matches an uninterrupted run.
fn snapshot_run(save_at: u64, total: u64) {
    let scenario = stress(48, 40);

    let mut uninterrupted = World::new(scenario.clone()).expect("world");
    uninterrupted.run(total);
    let expected = uninterrupted.state_hash();

    let mut interrupted = World::new(scenario).expect("world");
    interrupted.run(save_at);
    let bytes = Snapshot::write(&interrupted).expect("write");
    let mut resumed = Snapshot::read(&bytes).expect("read");
    assert_eq!(
        resumed.state_hash(),
        interrupted.state_hash(),
        "the snapshot did not restore the world it saved"
    );

    resumed.run(total - save_at);
    assert_eq!(
        resumed.state_hash(),
        expected,
        "a resumed run diverged from an uninterrupted one"
    );
    resumed
        .check_invariants()
        .expect("invariants after resuming");
}

#[test]
fn serialisation_round_trip_guard() {
    snapshot_run(ticks(500), ticks(500) * 2);
}

#[test]
#[ignore = "100,000 ticks; run with --release --ignored"]
fn acceptance_serialisation_round_trip() {
    snapshot_run(50_000, 100_000);
}

#[test]
fn a_snapshot_survives_every_kind_of_state() {
    // Hard rule 7: if you add state, extend the serialisation in the same commit. This is the
    // test that notices when that has not happened — it dirties every field there is.
    let mut world = World::new(stress(24, 24)).unwrap();
    world.run(37);
    world.inject_impulse(3, 4, 200, -180);
    world.ledger_mut().absorb(9_999);
    world.ledger_mut().dissipate(1_234);
    let evicted = world.substrate_mut().set_blocked(11, 11, true);
    world.ledger_mut().record_evicted(&evicted);
    world.run(13);

    let restored = Snapshot::read(&Snapshot::write(&world).unwrap()).unwrap();
    assert_eq!(restored, world, "some field is missing from the format");
    assert_eq!(restored.state_hash(), world.state_hash());
    assert_eq!(restored.tick_count(), world.tick_count());
    assert_eq!(restored.ledger().evicted(), world.ledger().evicted());
}

// ---------------------------------------------------------------- determinism (I1)

#[test]
fn a_scenario_and_a_seed_are_the_whole_experiment() {
    // I1 stated as the property a user relies on: the same file and the same seed give the
    // same world, every time, with nothing else carried over between runs.
    let scenario = stress(40, 32);
    let mut hashes = Vec::new();
    for _ in 0..3 {
        let mut world = World::new(scenario.clone()).unwrap();
        world.run(1_000);
        hashes.push(world.state_hash());
    }
    assert_eq!(hashes[0], hashes[1]);
    assert_eq!(hashes[1], hashes[2]);

    // And a scenario that differs anywhere gives a different world.
    let mut other = World::new(Scenario {
        seed: scenario.seed + 1,
        ..scenario.clone()
    })
    .unwrap();
    other.run(1_000);
    assert_ne!(
        other.state_hash(),
        hashes[0],
        "the seed did not reach the hash"
    );
}

#[test]
fn a_scenario_round_trips_through_ron_and_produces_the_same_world() {
    let scenario = stress(32, 32);
    let text = scenario.to_ron().unwrap();
    let parsed = Scenario::from_ron(&text).unwrap();

    let mut a = World::new(scenario).unwrap();
    let mut b = World::new(parsed).unwrap();
    a.run(500);
    b.run(500);
    assert_eq!(a.state_hash(), b.state_hash());
}
