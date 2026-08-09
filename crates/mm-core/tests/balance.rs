//! The balance gates: the shipped library against the shipped worlds.
//!
//! `mm_core::balance` is the harness; this is the panel it is pointed at and the four gates it
//! has to clear. Read that module's header first — in particular the two places where the
//! measurement is not neutral, which are stated there rather than hidden.
//!
//! Run the fast gates with `cargo test -p mm-core --test balance --release`, and the full panel
//! with
//! `cargo test --release -p mm-core --test balance -- --ignored --nocapture --test-threads=1`.
//!
//! # Why the gates are shaped the way they are
//!
//! CLAUDE.md: *"If such a test fails, that is a finding, not just a bug. Report which parameter
//! appears to be starving the result rather than tuning until it passes."* Every gate below is
//! written so that its failure message names the thing that failed and not merely the number, and
//! **none of them says which strategy should win.** They say: everything survives somewhere,
//! everything is competitive somewhere, the worlds tell strategies apart, and nothing wins
//! everywhere.
//!
//! # The panel
//!
//! Five worlds, each posing a limit none of the others does. It lives in
//! [`mm_core::balance::shipped_panel`] rather than here, so that these gates and `mm-cli balance`
//! run the same worlds; every entry carries a `poses` field saying what it asks that the others
//! do not, and an entry that cannot fill it in is a duplicate.

use std::path::Path;

use mm_core::balance::{
    median, tournament, Arena, Contender, Layout, Report, DISCRIMINATION_FLOOR, EVEN,
    MIRROR_TOLERANCE, PAYOFF_FLOOR, SEEDS,
};
use mm_core::Scenario;

fn assemble(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    mm_asm::assemble(&src)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .bytes
}

/// Load a shipped world, resolving whatever ruleset it names.
///
/// **Not `Scenario::from_ron`.** `the_thicket.ron` says `ruleset: "rival_light"`, and loading it
/// the plain way would run the default economy under the thicket's name — a panel entry that
/// poses none of the limit it is in the panel for, and no error anywhere to say so.
fn scenario(file: &str) -> Scenario {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let text = std::fs::read_to_string(root.join("scenarios").join(file))
        .unwrap_or_else(|e| panic!("{file}: {e}"));
    let mut library = mm_core::ruleset::RulesetLibrary::new();
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(root.join("rulesets"))
        .unwrap_or_else(|e| panic!("rulesets/: {e}"))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ron"))
        .collect();
    files.sort();
    for path in files {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{stem}: {e}"));
        library
            .insert(stem, &text)
            .unwrap_or_else(|e| panic!("{stem}: {e}"));
    }
    library
        .load_scenario(&text)
        .unwrap_or_else(|e| panic!("{file}: {e}"))
}

/// The reference every contender is measured against. See the module header of `mm_core::balance`.
fn reference() -> Contender {
    Contender::new("ancestor", assemble("ancestor.mm"))
}

/// Every shipped organism except the reference itself.
///
/// The five ISA demonstrations — `arithmetic`, `expression`, `scan`, `replicator`, `dormant` —
/// are not here because they build no body and are not organisms. `ancestor_sloppy` is not here
/// because it is a deliberately broken control (it emits chemical 15 instead of 13 and poisons
/// itself); measuring it would be measuring the joke.
fn contenders() -> Vec<Contender> {
    [
        "drifter.mm",
        "hoarder.mm",
        "hunter.mm",
        "marble.mm",
        "oscillator.mm",
        "parasite.mm",
        "predator.mm",
        "scavenger.mm",
        "sentinel.mm",
        "sponge.mm",
        "stalker.mm",
    ]
    .into_iter()
    .map(|n| Contender::new(n.trim_end_matches(".mm"), assemble(n)))
    .collect()
}

/// The panel, from `mm_core::balance::shipped_panel` — the same five worlds the `mm-cli balance`
/// front end runs, so the gates and the tool cannot drift apart. Each entry's `poses` field says
/// what limit it adds; a new entry that cannot fill that field in is a duplicate.
fn panel(ticks_scale: u64) -> Vec<Arena> {
    mm_core::balance::shipped_panel()
        .iter()
        .map(|e| e.arena(scenario(e.file), ticks_scale))
        .collect()
}

/// Print a report as a table, because a matrix of numbers is the artefact here.
fn show(report: &Report) {
    eprint!("{:>12}", "");
    for a in &report.arenas {
        eprint!(" {a:>9}");
    }
    eprintln!("   {:>6} {:>7} {:>5}", "best", "spread", "wins");
    eprint!("{:>12}", "mirror");
    for m in &report.mirror {
        eprint!(" {m:>9}");
    }
    eprintln!();
    for row in &report.rows {
        eprint!("{:>12}", row.name);
        for (a, s) in row.share.iter().enumerate() {
            let mark = if row.alive.get(a).copied().unwrap_or(false) {
                ' '
            } else {
                // Died everywhere in this world at every seed. The share is still reported,
                // because "extinct against a lineage that also died" is not the same as "lost".
                '\u{2020}'
            };
            eprint!(" {s:>8}{mark}");
        }
        eprintln!(
            "   {:>6} {:>7} {:>5}",
            row.best(),
            row.spread(),
            row.wins()
        );
    }
    eprintln!("\n(permille of the two-lineage population, median of {} seeds. 500 is a dead heat.\n \
               \u{2020} = the lineage was extinct at the end of every seed.)",
        3);

    eprintln!("\nbest share reached by any contender carrying each organelle:");
    for (kind, best) in report.by_organelle() {
        let flag = if best < PAYOFF_FLOOR { "  <-- pays nowhere" } else { "" };
        eprintln!("  {:>14}  {best:>4}{flag}", kind.name());
    }
}

/// The control that has to pass before any other number here means anything, on one world.
///
/// Two identical lineages, mirrored, must finish level. A world that does not return a dead heat
/// has a better half, and every number taken from it is measuring the terrain rather than the
/// genomes. This is the cheap version, on `soup.ron` alone, so that the default test run has a
/// real guard on the harness's fairness; [`the_whole_panel_is_fair`] is the same check on all
/// five worlds and is where a new panel entry gets vetted.
#[test]
fn the_reference_slide_does_not_pick_a_winner() {
    let me = reference();
    let arena = Arena {
        label: "soup".into(),
        scenario: scenario("soup.ron"),
        layout: Layout::Vertical,
        ticks: 2_500,
        founders: 4,
    };
    let report = tournament(&[arena], &[], &me, &SEEDS[..3]).expect("tournament");
    let m = report.mirror[0];
    assert!(
        report.unfair().is_empty(),
        "two identical lineages on `soup.ron` finished {m} to {}, not level. \
         The harness cannot measure anything until that is level — do not widen \
         MIRROR_TOLERANCE ({MIRROR_TOLERANCE}).",
        1000 - m
    );
}

/// The same control on every world in the panel.
///
/// Separate from the fast one because it is a minute a world and the default test run should not
/// pay it on every commit. **Run this whenever a world is added to the panel or its `Layout` is
/// changed** — it is the check that caught `the_drift.ron`, where a left-to-right current makes a
/// vertical mirror hand the bout to whoever starts upstream.
#[test]
#[ignore = "a minute a world; run it when the panel changes"]
fn the_whole_panel_is_fair() {
    let me = reference();
    // Short bouts: asymmetry in a slide shows up immediately and does not need a converged
    // population to be visible.
    let panel = panel(15);
    let report = tournament(&panel, &[], &me, &SEEDS[..3]).expect("tournament");
    eprintln!("\nmirror bouts, 3 seeds:");
    for (a, m) in report.arenas.iter().zip(report.mirror.iter()) {
        let verdict = if m.abs_diff(EVEN) <= MIRROR_TOLERANCE { "level" } else { "TILTED" };
        eprintln!("  {a:>10}  {m:>4}  {verdict}");
    }
    let unfair = report.unfair();
    assert!(
        unfair.is_empty(),
        "these worlds do not give two identical lineages the same chance: {unfair:?}. \
         Every number taken from them is measuring the slide. Fix the `Layout` for the world, \
         or take it out of the panel — do not widen MIRROR_TOLERANCE ({MIRROR_TOLERANCE})."
    );
}

/// The four gates, on one run of the panel.
///
/// One test rather than four, and the reason is not tidiness: each of these needs the whole
/// matrix, and four tests would each build it — four panel runs to answer four questions about
/// one measurement. It also reads better as a failure. A balance pass that broke three gates
/// should say so in one report rather than stopping at whichever ran first, so every gate is
/// evaluated and the assertion comes at the end.
///
/// **None of these says which strategy should win.** They say: everything survives somewhere,
/// everything is competitive somewhere, the worlds tell strategies apart, and nothing wins
/// everywhere. See `mm_core::balance`'s header for why those four and not others.
///
/// Roughly an hour. `mm-cli balance --scale 25` is the quick look; this is the suite.
#[test]
#[ignore = "the whole panel; about an hour. `mm-cli balance` is the interactive form"]
fn the_balance_panel() {
    let report = run_panel(100);
    show(&report);

    // Nothing below means anything if the panel is not fair, so this is checked first and hard.
    let unfair = report.unfair();
    assert!(
        unfair.is_empty(),
        "these worlds do not give two identical lineages the same chance: {unfair:?}. \
         Every number in the matrix above is measuring the slide."
    );

    let mut broken: Vec<String> = Vec::new();

    // 1. Viability. A genome in `genomes/` that cannot live in any world in `scenarios/` is a
    //    broken example, and the library is a teaching artefact.
    let extinct = report.extinct();
    if !extinct.is_empty() {
        broken.push(format!(
            "VIABILITY: extinct in every world at every seed: {extinct:?}"
        ));
    }

    // 2. Payoff. Not "wins somewhere" — see `PAYOFF_FLOOR`, which is 400 of 1000 deliberately.
    //    A feature nothing can make pay in any world is upkeep a cell gets nothing for.
    let stranded = report.stranded();
    if !stranded.is_empty() {
        broken.push(format!(
            "PAYOFF: no world in the panel makes these worth their upkeep (floor {PAYOFF_FLOOR} \
             of 1000): {stranded:?}. Either the mechanism pays nowhere, or the panel has no world \
             that asks for it — and which of those it is, is the finding."
        ));
    }

    // 3. Discrimination. If every world ranks the contenders the same way, the worlds are
    //    decoration and the economy has one axis. `docs/ECONOMY.md` §2 is this gate failing.
    let d = report.discrimination();
    if d < DISCRIMINATION_FLOOR {
        broken.push(format!(
            "DISCRIMINATION: the median contender's fortunes move by only {d} of 1000 across the \
             whole panel (floor {DISCRIMINATION_FLOOR}). The worlds pose one question in five \
             costumes."
        ));
    }
    let winners = report.distinct_winners();
    if winners < 2 {
        broken.push(format!(
            "DISCRIMINATION: the same genome is best in all {} worlds, so nothing in the panel \
             selects for anything else",
            report.arenas.len()
        ));
    }

    // 4. No sweep. A library in which one entry beats the reference everywhere has no trade-offs
    //    in it, and open-ended evolution needs somewhere for every strategy to lose.
    let sweepers = report.sweepers();
    if !sweepers.is_empty() {
        broken.push(format!(
            "SWEEP: {sweepers:?} beat the reference in every world. Either they are strictly \
             better bodies — in which case the reference is the thing to fix — or the panel has \
             no world that costs them anything."
        ));
    }

    assert!(
        broken.is_empty(),
        "the economy is out of balance in {} of four ways:\n  {}\n\nCLAUDE.md: this is a \
         finding, not a number to tune. Name the parameter that starves the result.",
        broken.len(),
        broken.join("\n  ")
    );
}

fn run_panel(scale: u64) -> Report {
    tournament(&panel(scale), &contenders(), &reference(), &SEEDS[..3]).expect("tournament")
}

/// The median is the median, including on the even-length runs the panel actually uses.
#[test]
fn the_median_ties_low() {
    assert_eq!(median(&mut [1, 2, 3]), 2);
    assert_eq!(median(&mut [1, 2, 3, 4]), 2);
    assert_eq!(median(&mut []), 0);
    assert_eq!(median(&mut [EVEN]), EVEN);
}
