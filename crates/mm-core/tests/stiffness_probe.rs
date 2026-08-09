//! Where firmness stops being a switch and becomes a choice.
//!
//! `MetabolicRates::rigidity_gain` is zero in every scenario but two, and its own doc says why —
//! *"zero switches it off exactly, which is how every test written before it runs"*, which is a
//! compatibility default rather than a considered one. At zero, `neighbours::firmness` returns 0
//! for every cell whatever its membrane and whatever its turgor, so the two currencies a cell
//! spends on being firm buy nothing at all.
//!
//! That much is not in doubt. The question this asks is the one that decides what the default
//! should be: **over what range of the gain is firmness a decision rather than a fact?** Too low
//! and nobody can afford it; too high and everybody has it for nothing. A parameter worth having
//! is one with a band in the middle, and this measures whether there is one and where.
//!
//! Run with
//! `cargo test --release -p mm-core --test stiffness_probe -- --ignored --nocapture --test-threads=1`.
//!
//! `#[ignore]`, like every other probe in the tree.

use std::path::Path;

use mm_core::balance::{bouts, median, Arena, Contender, Layout};
use mm_core::fixed::{q10, Q10_ONE};
use mm_core::{BiologyConfig, CellId, MutationRates, Scenario, World};

/// The gains the sweep visits. `Q10_ONE` is a gain of one — firmness *is* wall × turgor — and
/// the two scenarios that set this today use 1024 (`the_marbles`) and 16384 (`the_thicket`).
const GAINS: [i32; 9] = [0, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768];

/// Three seeds rather than `balance::SEEDS`' five. Nine gains against five seeds is forty-five
/// bouts of twenty thousand ticks; the shape of a sweep is legible at three and the gates that
/// have to be certain of a single number still use five.
const SEEDS: [u64; 3] = [0x0BA1, 0x1CE5, 0x2D07];

fn assemble(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    mm_asm::assemble(&src)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .bytes
}

/// `ancestor.mm` with its membrane built to `param` and nothing else changed.
///
/// The clean contender. `marble.mm` is the shipped thick-walled body and it also carries a
/// vacuole and eats more carbon, so a bout against it measures three things at once. This
/// measures the wall.
fn walled(param: u8) -> Vec<u8> {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm"),
    )
    .expect("ancestor");
    // Straight after the mitochondrion, which is the last thing `#build` builds.
    let anchor = "        IMM     50\n        IMM     2               ; mitochondrion\n        IMM     2\n        BUILD\n";
    assert!(src.contains(anchor), "ancestor's #build changed shape");
    let wall = format!(
        "{anchor}        IMM     {param}\n        ZERO\n        ZERO\n        BUILD\n"
    );
    mm_asm::assemble(&src.replace(anchor, &wall, ))
        .expect("walled variant assembles")
        .bytes
}

/// The control condition, with one parameter overridden.
fn soup(gain: i32) -> Scenario {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/soup.ron"),
    )
    .expect("soup");
    let mut scenario = Scenario::from_ron(&src).expect("parse");
    scenario.biology.metabolism.rates.rigidity_gain = gain;
    scenario
}

fn arena(gain: i32) -> Arena {
    Arena {
        label: format!("soup, gain {gain}"),
        scenario: soup(gain),
        layout: Layout::Vertical,
        ticks: 20_000,
        founders: 16,
    }
}

/// What one grown founder of `genome` actually achieves, at this gain.
fn achieved(genome: &[u8], gain: i32, ticks: u64) -> Option<(i32, i32, i32, i64)> {
    let mut scenario = soup(gain);
    scenario.biology.mutation = MutationRates::none();
    let mut world = World::new(scenario).expect("world");
    world.place_founders_at(genome, 1, Some((32, 32)));
    world.run(ticks);
    let i = world.cells().iter().next()?;
    let rates = &world.biology().metabolism.rates;
    Some((
        world.cells().slots(i)[0].param as i32,
        mm_core::biology::rigidity(world.cells(), i, rates),
        mm_core::neighbours::firmness(world.cells(), i, rates),
        mm_core::biology::osmotic_load(world.cells(), i),
    ))
}

// ---------------------------------------------------------------------------------------------

/// The spectrum, before any question of whether it pays.
///
/// `firmness = clamp(wall × turgor × gain, 0, 1)` where the wall is `membrane.param / 255` and
/// the turgor is `osmotic_load / osmotic_threshold`. Both terms are bought — the wall in
/// structural matter and upkeep, the turgor in the quadratic osmotic bill — so this table is
/// what a cell gets for what it has already paid.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_each_body_can_actually_buy() {
    eprintln!("\nfirmness achieved by one grown founder on the soup, 2000 ticks, per mille.");
    eprintln!("1000 is a marble; 0 is a bag. The wall is the membrane's param over 255, so the");
    eprintln!("default founder's is 96/1024 — it cannot be firm at a gain of one whatever else");
    eprintln!("it does.\n");
    let bodies: Vec<(String, Vec<u8>)> = [24u8, 60, 128, 200, 255]
        .iter()
        .map(|p| (format!("membrane {p}"), walled(*p)))
        .chain(std::iter::once((
            "marble.mm".to_string(),
            assemble("marble.mm"),
        )))
        .collect();

    eprint!("{:>14} {:>7} {:>8}", "body", "wall", "turgor");
    for g in GAINS {
        eprint!(" {g:>6}");
    }
    eprintln!();
    for (label, genome) in &bodies {
        // Wall and turgor do not depend on the gain, so read them once.
        let Some((param, rigidity, _, load)) = achieved(genome, 0, 2000) else {
            eprintln!("{label:>14}   died before it was measured");
            continue;
        };
        eprint!(
            "{:>14} {:>7} {:>8}",
            label,
            param * Q10_ONE / 255,
            // rigidity is wall × turgor, so turgor is what is left when the wall is divided out.
            if param > 0 {
                rigidity * 255 / param.max(1)
            } else {
                0
            }
        );
        for g in GAINS {
            let f = achieved(genome, g, 2000).map_or(-1, |(_, _, f, _)| f);
            eprint!(" {:>6}", f * 1000 / Q10_ONE);
        }
        eprintln!("    load {load}");
    }
    eprintln!("\nsaturated at 1000 means the gain is doing the work rather than the body: every");
    eprintln!("cell is a marble and nobody paid for it. Zero everywhere means nobody can be one.");
}

/// And whether it pays: the same wall, raced against the ancestor, at every gain.
///
/// `balance`'s instrument, on `soup.ron` with one parameter changed, so the number here is
/// comparable with every other share in `ECONOMY.md`. `PAYOFF_FLOOR` is 400.
#[test]
#[ignore = "a probe; run it on purpose"]
fn where_a_wall_starts_paying_for_itself() {
    eprintln!("\nchallenger's share of the two lineages, per mille, median of three seeds.");
    eprintln!("500 is a dead heat; 400 is `balance::PAYOFF_FLOOR`, the bar for 'competitive'.");
    eprintln!("20,000 ticks on soup.ron, mutation as shipped, 16 founders a side.\n");

    let reference = Contender::new("ancestor", assemble("ancestor.mm"));
    let challengers = [
        ("membrane 128", walled(128)),
        ("membrane 255", walled(255)),
        ("marble.mm", assemble("marble.mm")),
    ];

    eprint!("{:>14}", "gain");
    for (name, _) in &challengers {
        eprint!(" {name:>14}");
    }
    eprintln!("  {:>8}", "mirror");
    for gain in GAINS {
        let a = arena(gain);
        eprint!("{gain:>14}");
        for (_, genome) in &challengers {
            let c = Contender::new("challenger", genome.clone());
            let share = bouts(&a, &c, &reference, &SEEDS)
                .map(|mut b| {
                    let mut shares: Vec<u32> = b.drain(..).map(|x| x.share).collect();
                    median(&mut shares)
                })
                .unwrap_or(0);
            eprint!(" {share:>14}");
        }
        // The fairness control: the reference against itself. A gain that made the slide
        // lopsided would show up here before it showed up anywhere else.
        let mirror = bouts(&a, &reference, &reference, &SEEDS)
            .map(|mut b| {
                let mut shares: Vec<u32> = b.drain(..).map(|x| x.share).collect();
                median(&mut shares)
            })
            .unwrap_or(0);
        eprintln!("  {mirror:>8}");
    }
    eprintln!("\nthe mirror column is the reference against itself and should stay near 500 at");
    eprintln!("every gain. If it wanders, the gain is changing the slide rather than the cell.");
}

/// What the world costs when everything in it is firm.
///
/// The other half of a trade. A firm cell keeps a larger incompressible core, so it takes more
/// room; and `pressure` is normalised against the band between touching and the core, so a firm
/// cell reads near-maximum pressure the moment it touches anything — which `split_pressure` and
/// `growth_pressure` both read. Being a marble should cost a cell something in a crowd, and this
/// is where to see whether it does.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_a_slide_of_marbles_holds() {
    eprintln!("\none lineage alone on the soup to saturation, 20,000 ticks, mutation off.\n");
    eprintln!(
        "{:>14} {:>7} {:>8} {:>9} {:>9} {:>10}",
        "body", "gain", "cells", "refused", "med mass", "firmness"
    );
    for (label, genome) in [
        ("ancestor.mm", assemble("ancestor.mm")),
        ("membrane 255", walled(255)),
    ] {
        for gain in [0i32, 1024, 4096, 16384] {
            let mut scenario = soup(gain);
            scenario.biology.mutation = MutationRates::none();
            let mut world = World::new(scenario).expect("world");
            world.place_founders(&genome, 16);
            let mut refused = 0u64;
            for _ in 0..20_000 {
                world.step();
                refused += world.report().biology.failed_splits as u64;
            }
            let rates = &world.biology().metabolism.rates;
            let mut masses: Vec<i32> = world.cells().iter().map(|i| world.cells().mass[i]).collect();
            masses.sort_unstable();
            let mut firm: Vec<i32> = world
                .cells()
                .iter()
                .map(|i| mm_core::neighbours::firmness(world.cells(), i, rates))
                .collect();
            firm.sort_unstable();
            eprintln!(
                "{:>14} {:>7} {:>8} {:>9} {:>9} {:>10}",
                label,
                gain,
                world.cells().len(),
                refused,
                masses.get(masses.len() / 2).copied().unwrap_or(0) / Q10_ONE,
                firm.get(firm.len() / 2).copied().unwrap_or(0) * 1000 / Q10_ONE,
            );
        }
    }
    eprintln!("\n`refused` is divisions `split_pressure` turned down over the whole run. If");
    eprintln!("firmness costs anything, it costs it here.");
}

/// A sanity check that the walled variants are the ancestor plus a wall and nothing else.
#[test]
#[ignore = "a probe; run it on purpose"]
fn the_walled_variants_are_the_ancestor_plus_a_wall() {
    for param in [24u8, 128, 255] {
        let Some((built, _, _, _)) = achieved(&walled(param), 0, 2000) else {
            panic!("membrane {param} died");
        };
        eprintln!("asked for membrane {param}, built {built}");
        assert_eq!(built, param as i32, "the variant did not build its wall");
    }
    let _ = (q10(1), CellId::NONE, BiologyConfig::default());
}
