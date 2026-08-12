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
    bouts, median, tournament, Arena, Contender, Layout, Report, DISCRIMINATION_FLOOR, EVEN,
    MIRROR_TOLERANCE, PAYOFF_FLOOR, SEEDS,
};
use mm_core::{MutationRates, Scenario, World};

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
            // No contest: both lineages were extinct at every seed, so there is no share to
            // report and the world is excluded from best, spread and wins. Printing the
            // sentinel here as though it were a 500 draw is exactly how it was missed.
            if !row.contested.get(a).copied().unwrap_or(true) {
                eprint!(" {:>8}-", "n/c");
                continue;
            }
            let mark = if row.alive.get(a).copied().unwrap_or(false) {
                ' '
            } else {
                // Died everywhere in this world at every seed, against a reference that did
                // not. That is a real reading — it lost — and it is kept.
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
    eprintln!("\n(permille of the two-lineage population, median of the seeds that were a \
               contest. 500 is a dead heat.\n \
               \u{2020} = the lineage was extinct at the end of every seed.\n \
               n/c = no contest: both lineages died, so that world is no reading at all.)");

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
        lane: None,
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

/// What winter thins rather than culls.
///
/// The `seasons` entry of the panel came back with nine of eleven contenders extinct, which is
/// not a season, it is a cull — and a world that kills everything discriminates between nothing.
/// `docs/ECONOMY.md` §10a asked for it to be re-cut, and this is the measurement that decides
/// where.
///
/// # Why the constant-light numbers do not carry over
///
/// §5 swept a *uniform* intensity and found the population flat from 1024 down to 512, at 32
/// cells by 256, and extinct at 128. `Seasonal` is a triangle inside a triangle: the year
/// interpolates noon from `summer_day` to `winter_day`, and the day interpolates from `night` up
/// to that noon. With `night: 0` a triangular day averages **half of noon**, so the shipped
/// `winter_day: 224` is a mean of 112 — below the level at which a constant slide is already
/// dead, and nothing in the file says so.
///
/// The mean is not the whole story either, which is why this is measured rather than divided by
/// two. A cell under a day/night cycle sees zero every night whatever the mean is, so a world can
/// be survivable on average and lethal in detail — and that gap is the whole reason to have the
/// world at all, because closing it is what storing against the dark is *for*.
///
/// Run with
/// `cargo test --release -p mm-core --test balance -- --ignored --nocapture what_winter`.
#[test]
#[ignore = "a sweep; run it when re-cutting the panel"]
fn what_winter_thins_rather_than_culls() {
    let ancestor = assemble("ancestor.mm");
    let hoarder = assemble("hoarder.mm");

    eprintln!(
        "\nsixteen founders, three compressed years (60,000 ticks), mutation off.\n\
         `mean` is the daily average a triangular day gives: half of noon, with night at zero.\n"
    );
    eprintln!(
        "{:>6} {:>6}   {:>18}   {:>18}",
        "noon", "mean", "ancestor pk/tr/end", "hoarder  pk/tr/end"
    );

    for winter_day in [224i32, 320, 448, 576, 704, 832] {
        let mut row = Vec::new();
        for genome in [&ancestor, &hoarder] {
            let mut world = World::new(Scenario {
                light: mm_core::LightRegime::Seasonal {
                    day_ticks: 240,
                    year_ticks: 20_000,
                    summer_day: mm_core::Q10_ONE * 5 / 4,
                    winter_day,
                    night: 0,
                },
                biology: mm_core::BiologyConfig {
                    mutation: MutationRates::none(),
                    ..Default::default()
                },
                ..scenario("seasons.ron")
            })
            .expect("world");
            world.place_founders(genome, 16);

            // Sampled through the run rather than read at the end: the question is the *shape* of
            // the year, and a population that recovers by midsummer looks identical at tick
            // 60,000 to one that never dipped.
            let (mut peak, mut trough) = (0usize, usize::MAX);
            // The first year is the founding race, not a season. Only the second and third count.
            for step in 0..60u32 {
                world.run(1_000);
                let n = world.cells().len();
                if step >= 20 {
                    peak = peak.max(n);
                    trough = trough.min(n);
                }
            }
            row.push((peak, trough, world.cells().len()));
        }
        eprintln!(
            "{:>6} {:>6}   {:>5} {:>5} {:>5}   {:>5} {:>5} {:>5}",
            winter_day,
            winter_day / 2,
            row[0].0,
            row[0].1,
            row[0].2,
            row[1].0,
            row[1].1,
            row[1].2,
        );
    }
    eprintln!(
        "\nwanted: a trough well below the peak, and nothing at zero. A world that empties \
         discriminates\nbetween nothing, and one that never dips is not a season."
    );
}

/// What actually kills `hoarder.mm`.
///
/// `what_winter_thins_rather_than_culls` swept the winter six ways and the hoarder was extinct at
/// every one, including the two where the ancestor barely notices — so the depth of winter is not
/// what kills it and re-cutting the world cannot save it.
///
/// This asks the engine rather than inferring: `MetabolicReport` counts `starved` and `poisoned`
/// apart, so a run can say which of the two ways out a population took. The soup is here as the
/// control, because the hoarder dies there too — and if it dies the same way in both, the dark
/// has nothing to do with it.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_actually_kills_the_hoarder() {
    for world_name in ["soup.ron", "seasons.ron"] {
        eprintln!("\n{world_name}");
        eprintln!(
            "{:>10} {:>6} {:>7} {:>8} {:>8} {:>9} {:>9}",
            "genome", "tick", "cells", "med E", "med load", "starved", "poisoned"
        );
        for name in ["ancestor.mm", "hoarder.mm"] {
            let bytes = assemble(name);
            let base = scenario(world_name);
            let mut world = World::new(Scenario {
                // The panel's compressed year, so this and the panel are about one world.
                light: if world_name == "seasons.ron" {
                    mm_core::LightRegime::Seasonal {
                        day_ticks: 240,
                        year_ticks: 20_000,
                        summer_day: mm_core::Q10_ONE * 5 / 4,
                        winter_day: 224,
                        night: 0,
                    }
                } else {
                    base.light.clone()
                },
                biology: mm_core::BiologyConfig {
                    mutation: MutationRates::none(),
                    ..Default::default()
                },
                ..base
            })
            .expect("world");
            world.place_founders(&bytes, 16);

            let (mut starved, mut poisoned) = (0u64, 0u64);
            for k in 0..40u32 {
                for _ in 0..500 {
                    world.step();
                    starved += u64::from(world.report().metabolism.starved);
                    poisoned += u64::from(world.report().metabolism.poisoned);
                }
                let cells = world.cells();
                let mut energy: Vec<i32> = cells.iter().map(|i| cells.energy[i]).collect();
                let mut load: Vec<i64> = cells
                    .iter()
                    .map(|i| mm_core::biology::osmotic_load(cells, i))
                    .collect();
                energy.sort_unstable();
                load.sort_unstable();
                // Only every fourth sample, or this is forty lines a genome and unreadable.
                if k % 4 == 3 || cells.len() == 0 {
                    eprintln!(
                        "{:>10} {:>6} {:>7} {:>8} {:>8} {:>9} {:>9}",
                        name.trim_end_matches(".mm"),
                        (k + 1) * 500,
                        cells.len(),
                        energy.get(energy.len() / 2).copied().unwrap_or(0) / mm_core::Q10_ONE,
                        load.get(load.len() / 2).copied().unwrap_or(0) / mm_core::Q10_ONE as i64,
                        starved,
                        poisoned,
                    );
                }
                if world.cells().len() == 0 {
                    break;
                }
            }
        }
    }
    eprintln!(
        "\nthe osmotic threshold is {} units; a membrane at param 24 fails past {} damage.",
        mm_core::MetabolicRates::default().osmotic_threshold / mm_core::Q10_ONE,
        24
    );
}

/// The hoarder's vacuole is too small, and the cliff it falls off is exactly locatable.
///
/// `what_actually_kills_the_hoarder` says it starves, holding 837 units of solute against a
/// threshold of 256. `turgor_cost` is quadratic in the excess, so that load costs 2,632 `Q10` a
/// tick against a `param 50` mitochondrion's gross income of 2,400 — the tax on what it is
/// storing is larger than everything it earns, before a single organelle is paid for.
///
/// Setting the two equal gives the ceiling. A cell with this engine can hold about **810 units**
/// before turgor takes all of it, and the hoarder sits at 837. It is over the edge by three per
/// cent, which is why it dies everywhere rather than only in the dark.
///
/// A vacuole exempts `param` units and `param` is a `u8`, so one can hide at most 255 and the
/// shipped genome asks for 200. This is how many it actually needs, in the control world and in
/// the world it was written for. Variants are made by editing the source and reassembling rather
/// than by forcing the organelle — forcing one makes the genome rebuild it every tick and pay
/// `build_energy` each time, which is how `predator_probe` measured its own instrument.
#[test]
#[ignore = "a probe; run it on purpose"]
fn how_many_vacuoles_the_hoarder_needs() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/hoarder.mm"),
    )
    .expect("hoarder.mm");
    let shipped = "        IMM     200             ; and the granule, which is the whole of the difference";
    let build = |n: usize| -> String {
        let mut granules = String::new();
        for k in 0..n {
            granules.push_str(&format!(
                "        IMM     255
        IMM     4
        IMM     {}
        BUILD
",
                4 + k
            ));
        }
        // Replace the shipped granule and the three lines that follow it.
        let head = src.find(shipped).expect("the granule");
        let tail = src[head..].find("        BUILD
").expect("its BUILD") + head + "        BUILD
".len();
        format!("{}{}{}", &src[..head], granules, &src[tail..])
    };

    for world in ["soup.ron", "seasons.ron"] {
        eprintln!("
{world} — sixteen founders, 12,000 ticks, mutation off.");
        eprintln!(
            "{:>10} {:>7} {:>9} {:>9} {:>9}",
            "granules", "cells", "exempt", "med load", "starved"
        );
        for n in 1..=3usize {
            let Ok(assembled) = mm_asm::assemble(&build(n)) else {
                eprintln!("{n:>10}  did not assemble");
                continue;
            };
            let base = scenario(world);
            let mut w = World::new(Scenario {
                light: if world == "seasons.ron" {
                    mm_core::LightRegime::Seasonal {
                        day_ticks: 240,
                        year_ticks: 20_000,
                        summer_day: mm_core::Q10_ONE * 5 / 4,
                        winter_day: 224,
                        night: 0,
                    }
                } else {
                    base.light.clone()
                },
                biology: mm_core::BiologyConfig {
                    mutation: MutationRates::none(),
                    ..Default::default()
                },
                ..base
            })
            .expect("world");
            w.place_founders(&assembled.bytes, 16);
            let mut starved = 0u64;
            for _ in 0..12_000 {
                w.step();
                starved += u64::from(w.report().metabolism.starved);
            }
            let cells = w.cells();
            let mut load: Vec<i64> = cells
                .iter()
                .map(|i| mm_core::biology::osmotic_load(cells, i))
                .collect();
            load.sort_unstable();
            let exempt = cells
                .iter()
                .map(|i| mm_core::biology::sequestered(cells.slots(i)))
                .max()
                .unwrap_or(0);
            eprintln!(
                "{:>10} {:>7} {:>9} {:>9} {:>9}",
                n,
                cells.len(),
                exempt / mm_core::Q10_ONE as i64,
                load.get(load.len() / 2).copied().unwrap_or(0) / mm_core::Q10_ONE as i64,
                starved,
            );
        }
    }
    eprintln!(
        "
threshold {} units; each granule costs {} Q10 a tick to carry, against a gross of 2,400.",
        mm_core::MetabolicRates::default().osmotic_threshold / mm_core::Q10_ONE,
        16 + 255,
    );
}

/// Does storing against the dark pay, once the cell can afford to store?
///
/// This is the question `docs/ECONOMY.md` §10 is built on and has never been able to ask.
/// `hoarder.mm` is extinct in every world in the panel, so its column said nothing about
/// storage — it said the genome cannot pay its own turgor bill, which
/// `whether_a_bigger_vacuole_saves_the_hoarder` traced to a vacuole that exempts 200 units where
/// it needs 510, and a `param` that is a `u8` and so cannot reach it in one organelle.
///
/// With two vacuoles it survives. So this is the experiment §10 predicted: dark forces storage,
/// storage is matter, and matter is what a dying cell hands to whoever is standing on it. If the
/// prediction holds, the two-vacuole hoarder should do markedly better where the light comes and
/// goes than where it does not.
#[test]
#[ignore = "the panel, one contender; minutes"]
fn whether_storing_against_the_dark_pays() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/hoarder.mm"),
    )
    .expect("hoarder.mm");
    let two = src
        .replace(
            "        IMM     200             ; and the granule, which is the whole of the difference",
            "        IMM     255             ; the granule",
        )
        .replace(
            "        RET\n\n; ---------------------------------------------------------------- feed",
            "        IMM     255\n        IMM     4\n        IMM     5\n        BUILD\n        RET\n\n; ---------------------------------------------------------------- feed",
        );
    let bytes = mm_asm::assemble(&two).expect("the variant assembles").bytes;
    let hoarder = Contender::new("hoarder+2", bytes);
    let reference = Contender::new("ancestor", assemble("ancestor.mm"));

    eprintln!("\ntwo-vacuole hoarder against the ancestor, median of three seeds.");
    eprintln!("permille of the two-lineage population; 500 is a dead heat.\n");
    eprintln!("{:>10} {:>8}   {}", "world", "share", "the limit it poses");
    for entry in mm_core::balance::shipped_panel() {
        let arena = entry.arena(scenario(entry.file), 100);
        let results = bouts(&arena, &hoarder, &reference, &SEEDS[..3]).expect("bouts");
        let mut shares: Vec<u32> = results.iter().map(|b| b.share).collect();
        let alive = results.iter().any(|b| b.alive);
        eprintln!(
            "{:>10} {:>8}{}   {}",
            entry.label,
            median(&mut shares),
            if alive { " " } else { "*" },
            entry.poses,
        );
    }
    eprintln!("\n* = extinct at the end of every seed.");
}

/// The same question with the search taken out of it: is a *richer body* viable if it is cheaper?
///
/// `economy_probe::do_organelles_cost_too_much_to_be_worth_building` scaled the whole catalogue
/// under mutation and found the loadout unmoved at 4.00 organelles across an eightfold range of
/// price. That result has a hole in it, and it is worth stating plainly: **mutation has to find a
/// genome that builds a fifth organelle before the price of one can matter.** If no such variant
/// is ever produced, the sweep measured mutation's search and not the economy, and concluded
/// nothing.
///
/// This removes the search. `genomes/` is built as matched pairs — `scavenger` is the ancestor
/// plus a lysosome, `sponge` plus a holdfast, `oscillator` plus a clock, `hoarder` plus granules
/// — so racing each against the reference at several prices asks the question directly: given a
/// body that already carries the thing, does making it cheaper to carry make it pay?
///
/// What to watch is **where the shares stop**. §1 predicts they climb towards a dead heat and
/// never past it: cheaper upkeep can refund what the machinery costs, and nothing can pay for
/// what it does, because no organelle but the mitochondrion is in the income expression. A
/// contender that goes *above* 500 has an organelle that earns — and that would be the first one.
#[test]
#[ignore = "the panel, four prices; minutes"]
fn whether_a_richer_body_pays_when_it_is_cheaper() {
    let reference = Contender::new("ancestor", assemble("ancestor.mm"));
    let contenders: Vec<Contender> = ["scavenger", "sponge", "oscillator", "hoarder"]
        .into_iter()
        .map(|n| Contender::new(n, assemble(&format!("{n}.mm"))))
        .collect();

    eprintln!("\non the soup, three seeds, mutation off. permille; 500 is a dead heat.\n");
    eprint!("{:>8}", "upkeep");
    for c in &contenders {
        eprint!(" {:>14}", c.name);
    }
    eprintln!();
    eprintln!("{:>8}{}", "", "   share  them/ancestor".repeat(1));

    for percent in [25i64, 50, 100, 200] {
        let base = scenario("soup.ron");
        let mut bio = base.biology.clone();
        bio.mutation = MutationRates::none();
        let mut specs = *bio.metabolism.catalogue.specs();
        for spec in specs.iter_mut() {
            spec.upkeep = ((spec.upkeep as i64 * percent) / 100) as i32;
            spec.upkeep_per_param = ((spec.upkeep_per_param as i64 * percent) / 100) as i32;
        }
        bio.metabolism.catalogue.set_specs(specs);

        let arena = Arena {
            label: "soup".into(),
            scenario: Scenario { biology: bio, ..base },
            layout: Layout::Vertical,
            ticks: 12_000,
            founders: 8,
            lane: None,
        };
        eprint!("{percent:>7}%");
        for c in &contenders {
            let results = bouts(&arena, c, &reference, &SEEDS[..3]).expect("bouts");
            let mut shares: Vec<u32> = results.iter().map(|b| b.share).collect();
            // The absolute counts as well as the share, because a share is a ratio and a ratio
            // cannot tell a contender thriving from a reference collapsing — which is exactly
            // the ambiguity the first run of this left behind.
            let mut mine: Vec<u32> = results.iter().map(|b| b.challenger).collect();
            let mut theirs: Vec<u32> = results.iter().map(|b| b.reference).collect();
            mine.sort_unstable();
            theirs.sort_unstable();
            eprint!(
                " {:>4} {:>4}/{:<4}",
                median(&mut shares),
                mine[mine.len() / 2],
                theirs[theirs.len() / 2],
            );
        }
        eprintln!();
    }
    eprintln!(
        "\n* = extinct at every seed. A share that climbs to {EVEN} and stops is machinery whose \
         cost\ncan be refunded and whose benefit cannot. One that climbs past it earns something."
    );
}

/// What specialising is worth: the returns curve on carrying more of one thing.
///
/// The design intuition this measures against is a *shape* rather than a number. Sixteen slots is
/// the hard cap; a soft cap somewhere around twelve, past which a body should have to be
/// delivering something extraordinary; and a specialist putting six or eight of its slots into
/// the one thing it is good at — chloroplasts, granules, cilia, spikes, whichever niche it has
/// found. Against that, every evolved cell in this engine carries **four**, and
/// `economy_probe::do_organelles_cost_too_much_to_be_worth_building` could not move it with an
/// eightfold change in price.
///
/// The arithmetic says specialising ought to pay, which is what makes the absence interesting.
/// `capacity_by_pathway` *sums* over every organelle of a type, so four chloroplasts fix four
/// times as much and four mitochondria burn four times as much — a matched quadruple should earn
/// four times an ancestor's income for well under four times its upkeep, because the metabolic
/// floor and the membrane and the nucleus are paid once either way.
///
/// So this builds the matched pairs by hand and races each against the plain ancestor. The column
/// to read is `built`: a variant that asks for seven pairs and carries four could not afford them,
/// and that is a different finding from one that carries seven and still loses.
#[test]
#[ignore = "seven variants; minutes"]
fn what_specialising_is_worth() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm"),
    )
    .expect("ancestor.mm");
    let reference = Contender::new("ancestor", assemble("ancestor.mm"));

    // The ancestor builds nucleus@1, mitochondrion@2, chloroplast@3. Extra pairs go into the free
    // slots from 4 up, and the nucleus is enlarged because the variants are longer and SPEC §4.1
    // truncates a daughter at the nucleus it built for itself.
    let variant = |pairs: usize, catalase: bool| -> String {
        let mut extra = String::new();
        if catalase {
            // One lysosome, in a slot the extra pairs do not reach. See `docs/ECONOMY.md` §12.1:
            // the reactive share of the exhaust scales with throughput, so an engine without an
            // enzyme to match it poisons the cell.
            extra.push_str("        IMM     70\n        IMM     11\n        IMM     15\n        BUILD\n");
        }
        for k in 1..pairs {
            let (m, c) = (2 + 2 * k, 3 + 2 * k);
            extra.push_str(&format!(
                "        IMM     50\n        IMM     2\n        IMM     {m}\n        BUILD\n\
                 \x20       IMM     60\n        IMM     3\n        IMM     {c}\n        BUILD\n"
            ));
        }
        src.replace(
            "        IMM     40              ; param",
            "        IMM     96              ; param — room for the extra builds",
        )
        .replace("        IMM     50\n        IMM     2               ; mitochondrion",
                 &format!("{extra}        IMM     50\n        IMM     2               ; mitochondrion"))
    };

    eprintln!("\nmatched chloroplast/mitochondrion pairs against the plain ancestor.");
    eprintln!("soup, three seeds, mutation off. permille; 500 is a dead heat.\n");
    eprintln!(
        "{:>10} {:>6} {:>6} {:>7} {:>8} {:>8} {:>14} {:>7}",
        "pairs", "asked", "built", "alone", "starved", "poisond", "them / ancestor", "share"
    );
    for (pairs, catalase) in [
        (1usize, false), (2, false), (3, false), (4, false),
        (3, true), (4, true), (6, true), (7, true),
    ] {
        let source = variant(pairs, catalase);
        let Ok(assembled) = mm_asm::assemble(&source) else {
            eprintln!("{pairs:>7}  did not assemble");
            continue;
        };
        let contender = Contender::new(format!("x{pairs}"), assembled.bytes.clone());
        let base = scenario("soup.ron");
        let arena = Arena {
            label: "soup".into(),
            scenario: Scenario {
                biology: mm_core::BiologyConfig {
                    mutation: MutationRates::none(),
                    ..base.biology.clone()
                },
                ..base
            },
            layout: Layout::Vertical,
            ticks: 12_000,
            founders: 8,
            lane: None,
        };

        // What it actually managed to build, and what became of it — **alone**, not in the
        // bout. `balance::setup` places both lineages, so reading a loadout out of that world
        // reports the *reference's* four the moment the variant dies, which is exactly the
        // mistake the first version of this made.
        let (built, alone, starved, poisoned) = {
            let mut w = World::new(Scenario {
                biology: mm_core::BiologyConfig {
                    mutation: MutationRates::none(),
                    ..Default::default()
                },
                ..scenario("soup.ron")
            })
            .expect("world");
            w.place_founders(&assembled.bytes, 16);
            let (mut s, mut p, mut peak) = (0u64, 0u64, 0usize);
            for _ in 0..12_000 {
                w.step();
                s += u64::from(w.report().metabolism.starved);
                p += u64::from(w.report().metabolism.poisoned);
                peak = peak.max(
                    w.cells()
                        .iter()
                        .map(|i| w.cells().slots(i).iter().filter(|o| o.is_active()).count())
                        .max()
                        .unwrap_or(0),
                );
            }
            (peak, w.cells().len(), s, p)
        };

        let results = bouts(&arena, &contender, &reference, &SEEDS[..3]).expect("bouts");
        let mut shares: Vec<u32> = results.iter().map(|b| b.share).collect();
        let mut mine: Vec<u32> = results.iter().map(|b| b.challenger).collect();
        let mut theirs: Vec<u32> = results.iter().map(|b| b.reference).collect();
        mine.sort_unstable();
        theirs.sort_unstable();
        eprintln!(
            "{:>6}{} {:>6} {:>6} {:>7} {:>8} {:>8} {:>6} / {:<5} {:>7}",
            pairs,
            if catalase { "+cat" } else { "    " },
            2 + 2 * pairs + usize::from(catalase),
            built,
            alone,
            starved,
            poisoned,
            mine[mine.len() / 2],
            theirs[theirs.len() / 2],
            median(&mut shares),
        );
    }
    eprintln!(
        "\n`asked` counts the membrane and nucleus too, so a cell asking for eight pairs wants \
         eighteen\nslots and there are sixteen. `built` is what a grown cell was actually \
         carrying."
    );
}

/// A body carrying four of one organelle must at least be able to live.
///
/// The fifth gate, and the one this document's §12 exists to explain. `SPECIALIST_DEPTH` carries
/// the reasoning; the short version is that the engine offers sixteen slots and every evolved cell
/// carries four, which is the loadout it was seeded with, and no price moves it (§9a).
///
/// **It asks for viability, not victory.** A specialist should lose in a world that does not
/// reward its speciality, and a gate demanding otherwise would be the fitness function this
/// project must not have. What it refuses to accept is a depth that is simply fatal.
///
/// The specialist is metabolic because that is the only speciality the engine currently rewards at
/// all — `capacity_by_pathway` sums over organelles of a type, so more mitochondria really do burn
/// more. If a depth of four is unreachable *there*, it is unreachable everywhere, and the failure
/// is about the engine rather than about which organelle was chosen.
#[test]
#[ignore = "part of the balance suite; a minute"]
fn a_specialist_can_carry_four_of_its_speciality() {
    use mm_core::balance::SPECIALIST_DEPTH;

    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm"),
    )
    .expect("ancestor.mm");

    // The ancestor builds nucleus@1, mitochondrion@2, chloroplast@3. The extra pairs go into the
    // free slots from 4 up, and the nucleus is enlarged because the variant is longer and SPEC
    // §4.1 truncates a daughter at the nucleus it built for itself.
    let mut extra = String::new();
    for k in 1..SPECIALIST_DEPTH {
        let (m, c) = (2 + 2 * k, 3 + 2 * k);
        extra.push_str(&format!(
            "        IMM     50\n        IMM     2\n        IMM     {m}\n        BUILD\n\
             \x20       IMM     60\n        IMM     3\n        IMM     {c}\n        BUILD\n"
        ));
    }
    // And a catalase. Stacking respiration is a *coupled* investment: the reactive share of the
    // exhaust scales with throughput, so an engine without an enzyme to match it poisons the cell
    // (`docs/ECONOMY.md` §12.1). A specialist that carries four mitochondria and no lysosome is
    // not a specialist, it is a cell that has not finished the trade — so the body under test
    // pays for both, in slots and in upkeep.
    extra.push_str(
        "        IMM     70\n        IMM     11\n        IMM     10\n        BUILD\n",
    );
    let source = src
        .replace(
            "        IMM     40              ; param",
            "        IMM     96              ; param — room for the extra builds",
        )
        .replace(
            "        IMM     50\n        IMM     2               ; mitochondrion",
            &format!("{extra}        IMM     50\n        IMM     2               ; mitochondrion"),
        );
    let bytes = mm_asm::assemble(&source)
        .expect("the specialist assembles")
        .bytes;

    let mut world = World::new(Scenario {
        biology: mm_core::BiologyConfig {
            mutation: MutationRates::none(),
            ..Default::default()
        },
        ..scenario("soup.ron")
    })
    .expect("world");
    world.place_founders(&bytes, 16);

    let (mut starved, mut poisoned, mut deepest) = (0u64, 0u64, 0usize);
    for _ in 0..12_000 {
        world.step();
        starved += u64::from(world.report().metabolism.starved);
        poisoned += u64::from(world.report().metabolism.poisoned);
        deepest = deepest.max(
            world
                .cells()
                .iter()
                .map(|i| {
                    world.cells().slots(i)
                        .iter()
                        .filter(|o| o.is_active() && o.kind == mm_core::OrganelleType::Mitochondrion)
                        .count()
                })
                .max()
                .unwrap_or(0),
        );
    }

    let alive = world.cells().len();
    eprintln!(
        "\ndepth {SPECIALIST_DEPTH}: {alive} alive after 12,000 ticks, deepest loadout carried \
         {deepest} mitochondria,\n{starved} starved and {poisoned} poisoned along the way."
    );

    // Built at all: separates "the engine would not let it carry them" from "it carried them and
    // died", which are different findings and want different work.
    assert!(
        deepest >= SPECIALIST_DEPTH,
        "a body asking for {SPECIALIST_DEPTH} mitochondria never carried more than {deepest}. \
         That is a build cost, a slot budget or a mass ceiling rather than an economy, and \
         `docs/ECONOMY.md` §12.1 is the account of which."
    );
    assert!(
        alive > 0,
        "a body carrying {SPECIALIST_DEPTH} of one organelle cannot live on the control slide: \
         {starved} starved and {poisoned} poisoned. Four of one thing is a modest specialist and \
         the engine offers sixteen slots.\n\
         This is a finding rather than a number to tune — see `docs/ECONOMY.md` §12.1, which \
         traces it to respiration's exhaust scaling with respiration while excretion does not."
    );
}
