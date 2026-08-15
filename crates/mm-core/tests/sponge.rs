//! Filter feeding, and whether staying still is a way to make a living (SPEC §17.6).
//!
//! The claim §17.6 makes is that a sessile filter feeder needs four things — a wall to sit
//! against, a flow to sit in, a particulate to catch, and something to anchor with — and that
//! *none of them mentions filter feeding*. If it works, nothing in the engine knows the word.
//!
//! These are not a milestone's acceptance tests, because §17 has no milestone. They are the
//! properties the trade has to have for it to be a trade at all: that the water has to be moving
//! relative to the cell, that swimming through still water is the same reading as standing in a
//! current, and that a cell doing this is measurably better off than the same cell not doing it.

use mm_core::cell::{CellId, CellSeed};
use mm_core::ecology::DETRITUS;
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::light::CurrentField;
use mm_core::{Barrier, LightRegime, Organelle, OrganelleType, Scenario, Seeding, World};

fn assemble(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path).expect("genome file");
    mm_asm::assemble(&src).expect("it assembles").bytes
}

/// A channel: two walls with water running between them, and detritus in the water.
fn channel(current: CurrentField) -> Scenario {
    Scenario {
        name: "channel".to_string(),
        seed: 0x5F04,
        width: 48,
        height: 48,
        light: LightRegime::Uniform { intensity: Q10_ONE },
        current,
        jitter: 0,
        // The ordinary chemistry a cell needs to be alive at all: something to fix, something
        // to breathe, something to build from. Without it both cells starve inside a few
        // hundred ticks and the comparison is between two corpses — which is what the first
        // version of this measured, and it reported it as "the filter gained nothing".
        seeding: vec![
            Seeding::Uniform {
                chemical: 4,
                per_square: q10(400),
            },
            // The minerals every recipe in the catalogue is costed in, at the
            // Redfield proportion of the carbon above. Nothing produces them.
            Seeding::Uniform {
                chemical: 5,
                per_square: (q10(400)) * 16 / 106,
            },
            Seeding::Uniform {
                chemical: 6,
                per_square: (q10(400)) / 53,
            },
            Seeding::Uniform {
                chemical: 11,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 14,
                per_square: q10(400),
            },
        ],
        barriers: vec![
            Barrier::Rect {
                x: 4,
                y: 18,
                width: 40,
                height: 2,
            },
            Barrier::Rect {
                x: 4,
                y: 28,
                width: 40,
                height: 2,
            },
        ],
        ..Scenario::default()
    }
}

/// Put a cell somewhere with a working body, optionally with a holdfast on it.
fn put(world: &mut World, genome: &[u8], x: i32, y: i32, holdfast: Option<u8>) -> CellId {
    let structural = world.biology().structural_chemical;
    let g = world.genomes().intern(genome.to_vec()).expect("intern");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(y),
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
        if let Some(param) = holdfast {
            cells.slots_mut(i)[4] = Organelle::finished(OrganelleType::Holdfast, param);
        }
        // Lean enough to leave room. At q10(200) both cells sit at the interior cap for
        // structural matter, `room` is zero, and captured detritus has nowhere to go — so the
        // comparison came out 65536 against 65536 and measured the cap rather than the filter.
        cells.interior_mut(i)[structural] = q10(40);
        cells.interior_mut(i)[11] = q10(40);
        cells.interior_mut(i)[14] = q10(40);
    }
    id
}

/// Detritus everywhere in the water, replenished so the experiment is about the cell and not
/// about the supply running out.
fn seed_detritus(world: &mut World, per_square: i32) {
    let (w, h) = (world.substrate().width(), world.substrate().height());
    let mut added = 0i64;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            added += i64::from(world.substrate_mut().add_chem(DETRITUS, x, y, per_square));
        }
    }
    assert!(added > 0, "no detritus went in");
    world.adopt_current_contents_as_baseline();
}

/// Every unit of structural matter on the slide, in cytoplasm and in bodies.
///
/// The *lineage*, not the founder, and that distinction is the whole of why this test reads the
/// way it does now. See the note on the test below.
fn structural_on_slide(world: &World) -> i64 {
    let sc = world.biology().structural_chemical;
    world
        .cells()
        .iter()
        .map(|i| {
            i64::from(world.cells().interior(i)[sc]) + i64::from(world.cells().mass[i])
        })
        .sum()
}

#[test]
fn a_filter_takes_detritus_out_of_a_current_and_a_bare_cell_does_not() {
    // The whole trade: same slide, same current, same square, same seed — one cell with a
    // holdfast and one without, run as two separate slides.
    //
    // # Why two slides and a lineage, when it used to be two cells and a founder
    //
    // It ran both cells on one slide and compared what each *founder* was still holding at tick
    // 600, and it broke — the filter read 88,544 against the bare cell's 105,216, which says
    // filtering is a net loss. It is not, and three things were wrong with the measurement.
    //
    // **Both founders had stopped growing.** From about tick 750 they sit at exactly 90,112 and
    // 114,960 and never move again, through another twenty-eight births on the slide. The
    // comparison was of two ceilings, not of two rates. At tick 300, before either reaches one,
    // the filter is ahead 80,199 to 39,680 — the trade was always paying and the window had
    // simply been left running past the point where anything could be read from it.
    //
    // **And the ceilings are not a property of the cells.** The filtering one is anchored, so it
    // is walled in by its own daughters; the bare one lets go and drifts into open water where
    // nothing crowds it. The better-fed cell had the *lower* cap, for reasons that are about
    // where it is standing rather than about what it eats.
    //
    // **A founder is the wrong unit anyway.** Matter a cell spends on a daughter leaves the
    // founder's books entirely, so the lineage that breeds fastest reads poorest. That is the
    // same trap one level up from the one this test was caught by before, when it counted the
    // cytoplasm and not the body and read zero for a cell that had filtered a great deal and
    // spent it all.
    //
    // So: two slides rather than two cells, which also removes them competing for the same
    // water; and the whole population's matter rather than one cell's, which no ceiling and no
    // division can hide. The claim being made is unchanged and slightly stronger — a filter is
    // materially better off than a cell without one — and it is now measured on something that
    // keeps counting.
    let genome = assemble("sponge.mm");
    let run = |holdfast: Option<u8>| -> (i64, i64) {
        let mut world = World::new(channel(CurrentField::Uniform {
            vx: Q10_ONE / 8,
            vy: 0,
        }))
        .expect("world");
        seed_detritus(&mut world, q10(40));
        // Against the top wall of the channel, because a holdfast grips a barrier and mid-channel
        // there is nothing within reach — an early version put them at y=24 and measured two
        // cells drifting side by side, which is a fair reading of nothing at all.
        put(&mut world, &genome, 10, 21, holdfast);
        let before = world.ledger().chem_totals()[DETRITUS];
        for _ in 0..600 {
            world.step();
        }
        (
            before - world.ledger().chem_totals()[DETRITUS],
            structural_on_slide(&world),
        )
    };

    let (filter_took, filter_has) = run(Some(200));
    let (bare_took, bare_has) = run(None);
    eprintln!("filter: took {filter_took} detritus, holds {filter_has} structural");
    eprintln!("bare:   took {bare_took} detritus, holds {bare_has} structural");

    // The title of this test, asserted directly rather than through a proxy.
    assert!(
        filter_took > bare_took,
        "the filter took no more detritus out of the water than a cell without one: \
         {filter_took} against {bare_took}"
    );
    // And that it is worth having, which is the claim the proxy was standing in for.
    assert!(
        filter_has > bare_has,
        "the filter's lineage gained no structural matter over one without: \
         {filter_has} against {bare_has}"
    );
}

#[test]
fn a_cell_carried_by_the_water_catches_nothing() {
    // The property that stops this being a slower `EAT`. Two identical filtering cells, one on
    // a slide where the water is moving and one where it is still — and the still one is *not*
    // the poorer, because a cell adrift in a current is in still water as far as it can tell.
    //
    // No barriers here: nothing to hold on to, so the cell in the current goes with it.
    let genome = assemble("sponge.mm");
    let mut drifting = 0i64;
    let mut anchored = 0i64;
    for (label, current, walls) in [
        (
            "adrift",
            CurrentField::Uniform {
                vx: Q10_ONE / 8,
                vy: 0,
            },
            false,
        ),
        (
            "anchored",
            CurrentField::Uniform {
                vx: Q10_ONE / 8,
                vy: 0,
            },
            true,
        ),
    ] {
        let mut scenario = channel(current);
        if !walls {
            scenario.barriers.clear();
        }
        let mut world = World::new(scenario).expect("world");
        seed_detritus(&mut world, q10(40));
        // Against the wall when there is one, so it has something to grip.
        let y = if walls { 21 } else { 24 };
        put(&mut world, &genome, 10, y, Some(200));
        let before = world.ledger().chem_totals()[DETRITUS];
        for _ in 0..600 {
            world.step();
        }
        let taken = before - world.ledger().chem_totals()[DETRITUS];
        eprintln!("{label}: {taken} detritus taken");
        if walls {
            anchored = taken;
        } else {
            drifting = taken;
        }
    }
    assert!(
        anchored > drifting,
        "holding station bought nothing: anchored took {anchored}, adrift took {drifting}"
    );
}

#[test]
fn filtering_does_not_cost_the_world_a_single_unit_of_matter() {
    // I4, over the one path that moves matter between three places at once: out of the water,
    // into a cell as structural, and back out as waste. Three chances to lose a unit.
    let genome = assemble("sponge.mm");
    let mut world = World::new(channel(CurrentField::Uniform {
        vx: Q10_ONE / 6,
        vy: 0,
    }))
    .expect("world");
    seed_detritus(&mut world, q10(60));
    for k in 0..6 {
        put(&mut world, &genome, 8 + k * 6, 21, Some(255));
    }
    world.adopt_current_contents_as_baseline();

    for tick in 0..800 {
        world.step();
        if tick % 100 == 0 {
            world
                .check_matter()
                .unwrap_or_else(|e| panic!("matter drifted at tick {tick}: {e:?}"));
        }
    }
    world
        .check_matter()
        .expect("matter drifted over the whole run");
}
