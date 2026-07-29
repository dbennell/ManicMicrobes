//! M6 acceptance tests that live in the simulation: arena reproducibility, and the ISA guard.
//!
//! The other two — debugger non-interference and genome portability — are tested where the
//! code is, in `mm-app/src/debugger.rs` and `mm-app/tests/m6_tools.rs`.

mod common;

use std::path::Path;

use mm_core::arena::{play, setup, Entry, MatchRules, Outcome, Side};
use mm_core::genome_file::{GenomeFile, GenomeFileError};
use mm_core::isa::ISA_VERSION;
use mm_core::Snapshot;

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

fn contenders() -> (Entry, Entry) {
    (
        Entry::new("tidy", assemble("ancestor.mm")),
        Entry::new("sloppy", assemble("ancestor_sloppy.mm")),
    )
}

fn rules(ticks: u64) -> MatchRules {
    MatchRules {
        tick_limit: ticks,
        sample_every: (ticks / 8).max(1),
        cells_per_side: 6,
        width: 48,
        height: 48,
        ..MatchRules::default()
    }
}

// ---------------------------------------------------------------------------------------
// Acceptance 1 — match reproducibility.
//
// > An arena match replays identically from its saved scenario and seed, 100 times, on 2
// > different machines.
//
// The 100 replays are here. The two machines are not, and cannot be from inside a test suite
// that only runs on one — what stands in for it is that everything a machine could vary is
// already forbidden: no floats in `mm-core` (hard rule 2), no wall clock or global RNG (rule
// 5), no iteration-order dependence (rule 6), and `thread_count_is_not_an_input` in `m2_life`
// shows the answer does not move with the thread count. A second machine is a CI job, not a
// test.

fn replays_identically(ticks: u64, times: usize) {
    let (left, right) = contenders();
    let rules = rules(ticks);
    let first = play(&rules, &left, &right).expect("match");

    for n in 1..times {
        let again = play(&rules, &left, &right).expect("match");
        assert_eq!(
            again.final_hash, first.final_hash,
            "replay {n} of {times} ended on a different world"
        );
        assert_eq!(
            again.outcome, first.outcome,
            "replay {n} had a different outcome"
        );
        assert_eq!(
            again.standings, first.standings,
            "replay {n} took a different path"
        );
        assert_eq!(again.ended_at, first.ended_at);
        assert_eq!(again.copy_damaged, first.copy_damaged);
    }
    eprintln!(
        "{times} replays of {ticks} ticks agreed: {}",
        first.summary().lines().next().unwrap_or_default()
    );
}

#[test]
fn a_match_replays_identically_guard() {
    replays_identically(if cfg!(debug_assertions) { 200 } else { 1_500 }, 12);
}

#[test]
#[ignore = "100 replays of a full-length match; run with --release --ignored"]
fn acceptance_match_reproducibility() {
    replays_identically(
        common::env_usize("MM_M6_TICKS", 20_000) as u64,
        common::env_usize("MM_M6_REPLAYS", 100),
    );
}

#[test]
fn a_match_replays_from_its_saved_report_alone() {
    // The stronger form of the same claim: the report is a complete record, not a summary.
    // Someone handed only the report can reproduce the match exactly.
    let (left, right) = contenders();
    let report = play(&rules(800), &left, &right).expect("match");

    let replay = play(&report.rules, &report.left, &report.right).expect("match");
    assert_eq!(replay.final_hash, report.final_hash);
    assert_eq!(replay.outcome, report.outcome);
    assert_eq!(replay.standings, report.standings);
}

#[test]
fn a_match_replays_from_a_snapshot_taken_mid_way() {
    // Arena matches must be pausable and resumable, or a tournament could not be run in
    // pieces. The state hash after resuming has to match the one from running straight
    // through, which is hard rule 7 applied to a match.
    let (left, right) = contenders();
    let rules = rules(600);
    let mut straight = setup(&rules, &left, &right).expect("setup");
    straight.run(600);

    let mut halted = setup(&rules, &left, &right).expect("setup");
    halted.run(250);
    let bytes = Snapshot::write(&halted).expect("write");
    let mut resumed = Snapshot::read(&bytes).expect("read");
    resumed.run(350);

    assert_eq!(
        resumed.state_hash(),
        straight.state_hash(),
        "a match resumed from a snapshot diverged from one run straight through"
    );
}

#[test]
fn swapping_the_two_entries_mirrors_the_match_rather_than_changing_it() {
    // Neither side has the better half of the slide. If the same two genomes give a
    // systematically different answer depending on which is entered first, the arena is not
    // fair and no result from it means anything.
    let (a, b) = contenders();
    let rules = rules(if cfg!(debug_assertions) { 300 } else { 2_000 });
    let forward = play(&rules, &a, &b).expect("match");
    let reversed = play(&rules, &b, &a).expect("match");

    let winner_name = |r: &mm_core::arena::MatchReport| {
        r.outcome.winner().map(|s| match s {
            Side::Left => r.left.name.clone(),
            Side::Right => r.right.name.clone(),
        })
    };
    eprintln!(
        "forward: {:?}, reversed: {:?}",
        winner_name(&forward),
        winner_name(&reversed)
    );
    assert_eq!(
        winner_name(&forward),
        winner_name(&reversed),
        "the same two genomes gave different winners depending on seating; the slide favours \
         one side"
    );
}

#[test]
fn a_match_between_identical_genomes_is_symmetric() {
    // The sharpest fairness test available: the same genome on both sides. Any asymmetry in
    // the starting layout, the tick order or the tie-breaking shows up as one side winning a
    // match it has no way to deserve.
    let bytes = assemble("ancestor.mm");
    let left = Entry::new("a", bytes.clone());
    let right = Entry::new("b", bytes);
    let report = play(
        &rules(if cfg!(debug_assertions) { 300 } else { 2_500 }),
        &left,
        &right,
    )
    .expect("match");
    let last = report.standings.last().expect("standings");
    eprintln!(
        "identical genomes finished {} to {} ({:?})",
        last.left, last.right, report.outcome
    );

    // Not a demand for an exact draw: the two sides occupy different squares, so their cells
    // meet different neighbours and diverge honestly. What must not happen is a rout.
    let (l, r) = (last.left.max(1) as f64, last.right.max(1) as f64);
    let ratio = l.max(r) / l.min(r);
    assert!(
        ratio < 3.0,
        "identical genomes finished {} to {}, a {ratio:.1}x difference; the arena is not \
         symmetric",
        last.left,
        last.right
    );
}

// ---------------------------------------------------------------------------------------
// Acceptance 4 — the ISA guard.
//
// > Loading a genome stamped with a different ISA version produces a clear warning and
// > refuses to run it silently.

#[test]
fn acceptance_a_foreign_isa_genome_is_refused_with_a_clear_message() {
    let file = GenomeFile::new(assemble("ancestor.mm"), "ancestor");
    let text = file.to_text();
    assert!(
        text.contains(&format!("isa {ISA_VERSION}")),
        "an exported genome does not carry its ISA version"
    );

    for foreign in [0u16, 2, 99, u16::MAX] {
        let altered = text.replace(&format!("isa {ISA_VERSION}"), &format!("isa {foreign}"));
        match GenomeFile::from_text(&altered) {
            Err(GenomeFileError::IsaMismatch { found, expected }) => {
                assert_eq!(found, foreign);
                assert_eq!(expected, ISA_VERSION);
                let message = GenomeFileError::IsaMismatch { found, expected }.to_string();
                // "Clear" means it says what happened and what to do, not just that something
                // is wrong.
                assert!(message.contains("has not been loaded"), "{message}");
                assert!(message.contains(&foreign.to_string()), "{message}");
            }
            Ok(_) => panic!("a genome stamped ISA {foreign} loaded silently"),
            Err(other) => panic!("expected an ISA mismatch for {foreign}, got {other}"),
        }
    }
}

#[test]
fn a_genome_of_the_right_isa_still_loads() {
    // The guard must not be so eager that nothing works.
    let file = GenomeFile::new(assemble("ancestor.mm"), "ancestor");
    let back = GenomeFile::from_text(&file.to_text()).expect("loads");
    assert_eq!(back.bytes, file.bytes);
    assert_eq!(back.isa, ISA_VERSION);
}

#[test]
fn an_arena_entry_cannot_be_a_genome_from_another_isa() {
    // The path that matters: a match is exactly where a foreign genome would silently behave
    // as a different organism, and where nobody would notice because the numbers would still
    // look like a match.
    let text = GenomeFile::new(assemble("ancestor.mm"), "ancestor")
        .to_text()
        .replace(&format!("isa {ISA_VERSION}"), "isa 42");
    assert!(
        GenomeFile::from_text(&text).is_err(),
        "a foreign genome reached the point where it could have been entered"
    );
}

#[test]
fn a_side_with_a_genome_that_cannot_live_loses_rather_than_erroring() {
    // A legal but useless entry. Every byte sequence is a legal program (hard rule 3), so the
    // arena must handle an entry that simply does nothing — losing is the right answer, not a
    // crash and not a refusal.
    let left = Entry::new("real", assemble("ancestor.mm"));
    let right = Entry::new("inert", vec![0x2E; 4]);
    let report = play(
        &rules(if cfg!(debug_assertions) {
            2_000
        } else {
            20_000
        }),
        &left,
        &right,
    )
    .expect("a match with an inert entry still runs");
    assert_eq!(
        report.outcome.winner(),
        Some(Side::Left),
        "the inert entry did not lose: {:?}",
        report.outcome
    );
    if let Outcome::Elimination { at_tick, .. } = report.outcome {
        assert!(at_tick > 0);
    }
}

#[test]
#[ignore = "diagnostic: finds the first tick at which a snapshot resume diverges"]
fn bisect_the_snapshot_divergence() {
    let (left, right) = contenders();
    let rules = rules(600);
    for at in [1u64, 2, 5, 10, 25, 50, 100, 200, 250] {
        let mut straight = setup(&rules, &left, &right).expect("setup");
        straight.run(at + 20);
        let mut halted = setup(&rules, &left, &right).expect("setup");
        halted.run(at);
        let bytes = Snapshot::write(&halted).expect("write");
        let restored = Snapshot::read(&bytes).expect("read");
        let immediate = restored.state_hash() == halted.state_hash();
        let mut resumed = restored;
        resumed.run(20);
        let after = resumed.state_hash() == straight.state_hash();
        eprintln!(
            "snapshot at {at:>4}: identical on restore {immediate}, matches after 20 more {after}"
        );
        if !immediate || !after {
            eprintln!("  first divergence at {at}");
            break;
        }
    }
}
