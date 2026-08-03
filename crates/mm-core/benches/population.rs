//! The M2 and M3 performance gates.
//!
//! * **M2** — 50,000 cells at ≥ 60 ticks/second headless on 8 cores.
//! * **M3** — 50,000 cells *with sensors and cilia* at ≥ 45 ticks/second on 8 cores.
//!
//! Benchmarks are gates, not information (CLAUDE.md). The two gates measure the same slide
//! with two different populations on it, and the gap between them is the price of M3: sensing
//! reads the world around each cell, and cilia write momentum back into the fluid.
//!
//! # Reaching the population honestly
//!
//! The gate says fifty thousand cells, so the benchmark grows fifty thousand cells by letting
//! the ancestor reproduce, rather than by spawning them. A slide populated by hand would have
//! every cell at the same age, on the same genome, holding the same chemistry, in a grid — and
//! it would measure a workload the simulation never actually runs. A grown population has the
//! age spread, the genome spread and the clumping that make branch prediction and the
//! neighbour index behave the way they behave in a real run.
//!
//! That takes a while, which is why this is a benchmark and not a test. It is reported rather
//! than asserted, for the same reason `fluid.rs` reports: a gate that failed the build would
//! turn "met on eight threads, missed on two" into a red cross with no number attached.

use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::light::CurrentField;
use mm_core::neighbours::{self, NeighbourIndex};
use mm_core::{LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding, World};

const TARGET_CELLS: usize = 50_000;
const M2_GATE: f64 = 60.0;
const M3_GATE: f64 = 45.0;

fn assemble(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path).expect("genome file");
    mm_asm::assemble(&src).expect("it assembles").bytes
}

/// A slide big enough to hold fifty thousand cells without the population being limited by
/// the walls rather than by the chemistry.
fn slide(seed: u64) -> Scenario {
    Scenario {
        name: "gate".to_string(),
        seed,
        // Big enough for fifty thousand cells to be a density the simulation actually produces.
        //
        // At 256 it was not. `split_pressure` refuses a division to a cell with nowhere to put
        // the daughter, and a 256-square slide settles at about sixteen thousand cells — so the
        // gate could only reach its own target by having that switched off, which made it a
        // measurement of a world that cannot happen. Measured on 512: fifty thousand around tick
        // 1,400 and eighty-two thousand by tick 3,000, with every rule on.
        //
        // Four times the area for the same target is also four times the *matter*, which is the
        // point — the old slide was not short of room for the cells so much as short of room for
        // the cells to be a sensible size in.
        width: 512,
        height: 512,
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

/// Where a grown population is kept between runs.
fn cache_path(genome_file: &str, seed: u64) -> std::path::PathBuf {
    let root = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../target").to_string());
    std::path::PathBuf::from(root)
        .join("bench-cache")
        .join(format!(
            "{}-{seed}-{TARGET_CELLS}.mmslide",
            genome_file.replace('.', "_")
        ))
}

/// A grown population, from cache when there is one.
///
/// Growing fifty thousand cells takes about ten minutes, and it was the same ten minutes on
/// every invocation — which made this a benchmark you ran once a day rather than after a
/// change, and a benchmark nobody runs is not a gate.
///
/// The grown world is snapshotted instead. Hard rule 7 says a snapshot restores
/// bit-identically and there is a test holding it to that, so the cached population is not an
/// approximation of the grown one: it is the grown one. Comparing two builds against the same
/// cached world is also *better* measurement than comparing two independently grown ones,
/// because the population stops being something each side re-derived and becomes a constant.
///
/// The snapshot's own format version invalidates the cache whenever world state changes shape.
/// It cannot know about a change that alters behaviour without altering the format — after one
/// of those the cached population is from the old physics, and is still a perfectly good fifty
/// thousand cells to measure throughput on, but regrow with `MM_BENCH_REGROW=1` if what you
/// want is a population the *new* rules would have produced.
fn grown(genome_file: &str, seed: u64) -> Option<World> {
    let path = cache_path(genome_file, seed);
    if std::env::var("MM_BENCH_REGROW").is_err() {
        if let Ok(bytes) = std::fs::read(&path) {
            match mm_core::Snapshot::read(&bytes) {
                Ok(world) => {
                    eprintln!(
                        "  {genome_file}: {} cells from cache ({})",
                        world.cells().len(),
                        path.display()
                    );
                    return Some(world);
                }
                Err(e) => eprintln!("  {genome_file}: cache unusable ({e:?}); regrowing"),
            }
        }
    }
    let world = grow(genome_file, seed)?;
    match mm_core::Snapshot::write(&world) {
        Ok(bytes) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = std::fs::write(&path, bytes) {
                eprintln!("  {genome_file}: could not cache the grown world: {e}");
            }
        }
        Err(e) => eprintln!("  {genome_file}: could not snapshot the grown world: {e:?}"),
    }
    Some(world)
}

/// Grow a population from scratch, the long way.
fn grow(genome_file: &str, seed: u64) -> Option<World> {
    let bytes = assemble(genome_file);
    let mut world = World::new(slide(seed)).expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::default(),
        ..BiologyConfig::default()
    });
    for k in 0..64u32 {
        let genome = world.genomes().intern(bytes.clone()).expect("interned");
        let id = world.spawn_cell(CellSeed {
            x: pos((16 + (k % 8) * 28) as i32),
            y: pos((16 + (k / 8) * 28) as i32),
            mass: q10(30),
            energy: q10(400),
            membrane: 24,
            key: 11,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        if let Some(i) = world.cells_mut().index(id) {
            let cells = world.cells_mut();
            cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
            cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
            cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
            cells.interior_mut(i)[11] = q10(40);
            cells.interior_mut(i)[14] = q10(40);
        }
    }
    world.adopt_current_contents_as_baseline();

    // Grow, checking often enough to stop near the target rather than far past it.
    for _ in 0..4_000 {
        world.run(25);
        let n = world.cells().len();
        if n >= TARGET_CELLS {
            return Some(world);
        }
        if n == 0 {
            eprintln!("  {genome_file}: went extinct while growing");
            return None;
        }
    }
    eprintln!(
        "  {genome_file}: only reached {} cells, not {TARGET_CELLS}",
        world.cells().len()
    );
    Some(world)
}

fn gate(_c: &mut Criterion) {
    if cfg!(debug_assertions) {
        return;
    }
    let threads = rayon::current_num_threads();
    eprintln!("\nPopulation gates ({threads} threads):");

    // The same slide with nothing alive on it. The gate is about cells, so when it is missed
    // the number that matters is how much of the tick the cells were even responsible for: a
    // fluid that already eats the whole budget is a different problem from slow biology and
    // needs a different fix.
    let mut empty = World::new(slide(1)).expect("world");
    empty.run(20);
    let n = 120;
    let t = Instant::now();
    empty.run(n);
    let bare_per_tick = t.elapsed().as_secs_f64() / n as f64;
    eprintln!(
        "  fluid alone, no cells:          {:6.1} ticks/s",
        1.0 / bare_per_tick
    );

    for (milestone, file, want) in [
        ("M2", "ancestor.mm", M2_GATE),
        ("M3", "drifter.mm", M3_GATE),
    ] {
        let Some(mut world) = grown(file, 1) else {
            continue;
        };
        let population = world.cells().len();
        // A few ticks to settle any allocation the growth phase left behind, so the timed
        // window measures steady state.
        world.run(20);

        let n = 120;
        let t = Instant::now();
        world.run(n);
        let per_tick = t.elapsed().as_secs_f64() / n as f64;
        let rate = 1.0 / per_tick;
        let cells_share = (per_tick - bare_per_tick).max(0.0) / per_tick * 100.0;
        eprintln!(
            "  {milestone}: {population:>6} cells  {rate:6.1} ticks/s  (need {want:.0})  {}  \
             — {cells_share:.0}% of the tick is the cells",
            if rate >= want { "MET" } else { "MISSED" }
        );
    }
}

/// Which phase the tick is actually spent in.
///
/// Added when both gates were missed by a wide margin and the fluid turned out to account for
/// none of it. A number that says "too slow" is not actionable; a number that says which of
/// the six phases is too slow is. Every phase here is a public function taking public types,
/// so this measures them exactly as `World::step` calls them, without instrumenting `World`
/// itself — which could not carry an `Instant` anyway (hard rule 5).
fn phase_breakdown(_c: &mut Criterion) {
    if cfg!(debug_assertions) {
        return;
    }
    let Some(mut world) = grown("ancestor.mm", 1) else {
        return;
    };
    let population = world.cells().len();
    let (w, h) = (world.substrate().width(), world.substrate().height());
    let n = 60u32;

    eprintln!("\nPhase breakdown at {population} cells ({w}x{h}):");

    // The whole tick first, before anything below has touched the world.
    //
    // Every phase measured after this one is called directly and repeatedly, which advances that
    // phase without advancing the rest — sixty metabolic steps with no fluid to eat from, sixty
    // physics steps with no collisions to stop them. That is a fair way to time a phase and a
    // terrible state to leave a world in, and `whole` used to be measured at the end of it, on
    // the wreckage. Measuring it first is the only way the percentages below mean anything.
    let t = Instant::now();
    world.run(n as u64);
    let whole = t.elapsed() / n;

    let mut index = NeighbourIndex::default();
    let t = Instant::now();
    for _ in 0..n {
        index.rebuild(world.cells(), w, h);
    }
    let rebuild = t.elapsed() / n;

    let mut radii = Vec::new();
    let mut crowding = Vec::new();
    let mut pressure = Vec::new();
    let t = Instant::now();
    for _ in 0..n {
        std::hint::black_box(neighbours::resolve_collisions(
            world.cells_mut(),
            &index,
            &mut radii,
            &mut crowding,
            &mut pressure,
            &[],
        ));
    }
    let collisions = t.elapsed() / n;


    // The VM itself, measured rather than left inside the remainder. It is the phase any
    // proposal to move the simulation onto a GPU would be moving, so what it actually costs
    // decides whether that is worth discussing: Amdahl bounds the whole exercise at whatever
    // share this line reports.
    //
    // Running it repeatedly on one world advances the cells' VMs without advancing anything
    // else, which is the same liberty `resolve_collisions` is measured under above — the work
    // per call is what is being timed, not the trajectory.
    let vm = world.scenario().vm;
    let spike_damage = world.biology().ecology.spike_damage;
    let chemistry = world.biology().metabolism.catalogue.metabolism;
    // Cloned once, because `execute` wants the cells mutably and the substrate shared, and
    // both live in the same `World`. Outside the timed loop, so it costs the measurement
    // nothing.
    let substrate = world.substrate().clone();
    let capacity = world.cells().capacity();
    let mut intents = mm_core::intent::IntentBuffer::new();
    let t = Instant::now();
    for _ in 0..n {
        intents.begin_tick(capacity);
        mm_core::biology::execute(
            world.cells_mut(),
            &substrate,
            &index,
            &mut intents,
            &vm,
            0,
            1,
            spike_damage,
            chemistry,
        );
    }
    let execute = t.elapsed() / n;

    // --- what used to be "the remainder" ---
    //
    // It was 58.6% of the tick and it was a subtraction, not a measurement: whole tick minus the
    // phases that happened to be easy to call. A number that large with no name on it is not a
    // finding, it is a place to guess, so each piece is now timed the same way the phases above
    // are — called directly, repeatedly, on this world.
    //
    // The same liberty the collision and execute measurements take: running a phase repeatedly
    // advances it without advancing the rest, so what is timed is the work per call and not a
    // trajectory anyone should read anything into.
    let t = Instant::now();
    for _ in 0..n {
        index.gather_touch(world.cells());
    }
    let gather = t.elapsed() / n;

    let mut ledger = mm_core::ledger::Ledger::new();
    let mut starving = Vec::new();
    // Cloned, because `step` wants the cells mutably and both of these live in the same `World`.
    // Outside the timed loops, so it costs the measurement nothing — the same move the substrate
    // above makes for the same reason.
    let metabolism = world.biology().metabolism.clone();
    let chem = world.scenario().chemicals.clone();
    let ecology_cfg = world.biology().ecology;
    let t = Instant::now();
    for _ in 0..n {
        starving.clear();
        std::hint::black_box(metabolism.step(
            world.cells_mut(),
            &substrate,
            &chem,
            &mut ledger,
            &mut starving,
            &[],
        ));
    }
    let metabolic = t.elapsed() / n;

    let mut eco_substrate = substrate.clone();
    let t = Instant::now();
    for _ in 0..n {
        std::hint::black_box(mm_core::ecology::step(
            world.cells_mut(),
            &mut eco_substrate,
            &index,
            &crowding,
            &ecology_cfg,
            &chemistry,
            &mut ledger,
        ));
    }
    let ecology = t.elapsed() / n;

    let mut impulse_x = vec![0i32; substrate.len()];
    let mut impulse_y = vec![0i32; substrate.len()];
    let forces = mm_core::sensing::BodyForces {
        jitter: 0,
        gravity: 0,
    };
    let t = Instant::now();
    for _ in 0..n {
        std::hint::black_box(mm_core::sensing::step_physics(
            world.cells_mut(),
            &substrate,
            &mut impulse_x,
            &mut impulse_y,
            forces,
            0,
            1,
        ));
    }
    let physics = t.elapsed() / n;

    // Intent resolution, which is where the remainder went after the four above turned out to be
    // small. Fed the buffer `execute` just filled, so it applies a real tick's worth of intents
    // rather than an empty one.
    let pool = world.genomes().clone();
    let config = world.biology().clone();
    let mut pending = mm_core::intent::Pending::default();
    let mut resolve_substrate = substrate.clone();
    let pressure_snapshot = vec![0i32; capacity];
    let t = Instant::now();
    for _ in 0..n {
        pending.births.clear();
        pending.deaths.clear();
        std::hint::black_box(mm_core::biology::resolve(
            world.cells_mut(),
            &mut resolve_substrate,
            &pool,
            &intents,
            &config,
            &chem,
            &mut ledger,
            &mut pending,
            &pressure_snapshot,
            0,
            1,
        ));
    }
    let resolve = t.elapsed() / n;



    // The fluid on the *populated* world's substrate, not an empty one.
    //
    // This was measured by running an empty slide and calling the result "fluid + bookkeeping",
    // and it flattered the fluid badly. `sweep` skips chemicals that are not present anywhere,
    // and an empty slide has only the three the scenario seeded — a slide with fifty thousand
    // metabolising cells on it has most of the table, because respiration and waste put them
    // there. So the empty world was diffusing three planes and the real one is diffusing
    // twelve or more, and the difference was landing in the unnamed remainder.
    let present = substrate.present().iter().filter(|p| **p).count();
    let mut fluid_substrate = substrate.clone();
    let mut scratch = mm_core::fluid::FluidScratch::default();
    let rates = chem.diffusion_rates();
    let t = Instant::now();
    for _ in 0..n {
        mm_core::fluid::step(
            &mut fluid_substrate,
            &mm_core::fluid::FluidRates { diffusion: rates, ..Default::default() },
            &mut scratch,
        );
    }
    let fluid = t.elapsed() / n;

    let mut empty = World::new(slide(1)).expect("world");
    empty.run(10);
    let t = Instant::now();
    empty.run(n as u64);
    let empty_tick = t.elapsed() / n;

    let accounted =
        rebuild * 2 + collisions + fluid + execute + gather + metabolic + ecology + physics
            + resolve;
    let rest = whole.saturating_sub(accounted);
    let pct = |d: std::time::Duration| d.as_secs_f64() / whole.as_secs_f64() * 100.0;
    eprintln!("  whole tick            {whole:>10.2?}");
    eprintln!(
        "  neighbour rebuild x2  {:>10.2?}  {:5.1}%",
        rebuild * 2,
        pct(rebuild * 2)
    );
    eprintln!(
        "  collision separation  {collisions:>10.2?}  {:5.1}%",
        pct(collisions)
    );
    eprintln!(
        "  execute (the VM)      {execute:>10.2?}  {:5.1}%",
        pct(execute)
    );
    eprintln!(
        "  fluid ({present:>2} planes)      {fluid:>10.2?}  {:5.1}%   [empty slide: {empty_tick:.2?}]",
        pct(fluid)
    );
    eprintln!(
        "  gather touch          {gather:>10.2?}  {:5.1}%",
        pct(gather)
    );
    eprintln!(
        "  metabolism            {metabolic:>10.2?}  {:5.1}%",
        pct(metabolic)
    );
    eprintln!(
        "  ecology               {ecology:>10.2?}  {:5.1}%",
        pct(ecology)
    );
    eprintln!(
        "  physics integration   {physics:>10.2?}  {:5.1}%",
        pct(physics)
    );
    // Still a subtraction, but a much smaller one now: intent resolution, births, deaths,
    // junctions, phylogeny and the metrics. Named as what is left rather than guessed at.
    eprintln!(
        "  intent resolution     {resolve:>10.2?}  {:5.1}%",
        pct(resolve)
    );
    // Births, deaths, junction components, phylogeny and the metrics. Named as what is left.
    eprintln!(
        "  births/deaths etc     {rest:>10.2?}  {:5.1}%  (the remainder)",
        pct(rest)
    );
}

/// Per-phase throughput, for finding out *where* a regression went rather than only that one
/// happened.
fn phases(c: &mut Criterion) {
    if cfg!(debug_assertions) {
        return;
    }
    let mut group = c.benchmark_group("population");
    group.sample_size(10);
    for (name, file) in [("ancestor", "ancestor.mm"), ("drifter", "drifter.mm")] {
        if let Some(mut world) = grown(file, 1) {
            group.bench_function(name, |b| b.iter(|| world.step()));
        }
    }
    group.finish();
}

/// M5's gate: phylogeny and metrics under 5% of tick time at scale.
///
/// # Measured on two clones, not two windows
///
/// The obvious version — step the world, then turn sampling off and step it again — is wrong,
/// and wrong in a way that reported success. The population is still growing, so the second
/// window runs on a bigger world than the first: the measurement came back with the archive
/// *disabled* costing 559ms a tick against 158ms with it enabled, and `.max(0.0)` dutifully
/// turned that impossible negative into "0.00%, MET".
///
/// It was printing both absolute timings that gave it away, which is the argument for printing
/// them. So: two clones of one grown world, stepped the same number of ticks from the same
/// state, differing only in whether the census runs. Comparing like with like is the whole
/// measurement.
///
/// The difference has to be measured from outside because no timer can go inside `World` —
/// hard rule 5 forbids it carrying a clock.
fn phylogeny_gate(_c: &mut Criterion) {
    if cfg!(debug_assertions) {
        return;
    }
    let Some(world) = grown("ancestor.mm", 1) else {
        return;
    };
    let population = world.cells().len();
    let species = world.archive().len();
    let n = 200u64;

    // Sampling at its normal interval.
    let mut with = world.clone();
    with.run(10);
    let t = Instant::now();
    with.run(n);
    let with_archive = t.elapsed().as_secs_f64() / n as f64;

    // The same world, from the same state, with sampling pushed beyond the window so no
    // census falls inside it.
    let mut without = world.clone();
    without.archive_mut().sample_interval = u64::MAX / 2;
    without.run(10);
    let t = Instant::now();
    without.run(n);
    let bare = t.elapsed().as_secs_f64() / n as f64;

    // The two clones must still be describing the same world, or the difference between them
    // is not the archive.
    let drift = with.cells().len().abs_diff(without.cells().len());
    assert!(
        (drift as f64) < population.max(1) as f64 * 0.05,
        "the clones drifted apart in population ({} against {}); they are no longer comparable",
        with.cells().len(),
        without.cells().len()
    );

    let share = (with_archive - bare) / with_archive * 100.0;
    eprintln!("\nM5 phylogeny gate at {population} cells, {species} species:");
    eprintln!(
        "  with archive {:.2}ms/tick, without {:.2}ms/tick — {share:.2}% (need under 5%)  {}",
        with_archive * 1000.0,
        bare * 1000.0,
        if share < 5.0 { "MET" } else { "MISSED" }
    );
}

/// M7's gate: the junction solve under 5% of tick time at 50,000 junctions.
///
/// Measured the same way as M5's — two clones of one grown world, stepped from the same state,
/// differing only in whether the junctions are there. Comparing like with like is the whole
/// measurement; the alternative of timing two consecutive windows on a growing population
/// reported the archive as costing negative time, which is how that lesson was learned.
fn junction_gate(_c: &mut Criterion) {
    if cfg!(debug_assertions) {
        return;
    }
    let Some(world) = grown("ancestor.mm", 1) else {
        return;
    };
    let population = world.cells().len();

    // Wire genuinely adjacent cells together until the target is reached.
    //
    // By spatial neighbour, not by arena slot. The first version paired consecutive slots —
    // which follow birth order, not position — and managed fourteen junctions out of fifty
    // thousand before reporting the gate as MET. A gate measured on a thousandth of the load
    // it names has measured nothing, which is why the count is printed either way.
    let mut wired = world.clone();
    let target = 50_000usize;
    let mut made = 0usize;
    {
        let mut index = NeighbourIndex::default();
        let (w, h) = (wired.substrate().width(), wired.substrate().height());
        index.rebuild(wired.cells(), w, h);
        let slots: Vec<usize> = wired.cells().iter().collect();
        for i in slots {
            if made >= target {
                break;
            }
            let (sx, sy) = (
                mm_core::fixed::pos_to_square(wired.cells().x[i]),
                mm_core::fixed::pos_to_square(wired.cells().y[i]),
            );
            let near: Vec<usize> = index.around(sx, sy).collect();
            for j in near {
                if made >= target || j <= i || !wired.cells().occupied(j) {
                    continue;
                }
                let (Some(sa), Some(sb)) = (
                    mm_core::junction::free_slot(wired.cells(), i),
                    mm_core::junction::free_slot(wired.cells(), j),
                ) else {
                    break;
                };
                let id_i = wired.cells().id_at(i);
                let id_j = wired.cells().id_at(j);
                let rest = mm_core::junction::distance(wired.cells(), i, j)
                    .max(mm_core::fixed::POS_ONE / 2);
                wired.cells_mut().junctions_mut(i)[sa] = mm_core::junction::Junction {
                    kind: mm_core::junction::JunctionKind::Hard,
                    other: id_j,
                    rest,
                };
                wired.cells_mut().junctions_mut(j)[sb] = mm_core::junction::Junction {
                    kind: mm_core::junction::JunctionKind::Hard,
                    other: id_i,
                    rest,
                };
                made += 1;
            }
        }
    }

    let mut bare = world;
    let n = 120u64;
    bare.run(10);
    let t = Instant::now();
    bare.run(n);
    let without = t.elapsed().as_secs_f64() / n as f64;

    wired.run(10);
    let t = Instant::now();
    wired.run(n);
    let with = t.elapsed().as_secs_f64() / n as f64;

    let share = ((with - without) / with * 100.0).max(0.0);
    eprintln!("\nM7 junction gate at {population} cells, {made} junctions:");
    eprintln!(
        "  with junctions {:.2}ms/tick, without {:.2}ms/tick — {share:.2}% (need under 5%)  {}",
        with * 1000.0,
        without * 1000.0,
        if share < 5.0 { "MET" } else { "MISSED" }
    );
    if made < target {
        // Said out loud: a gate measured at a tenth of the population it names has measured
        // something else. `JUNCTIONS_PER_CELL` is four, so fifty thousand junctions needs
        // twenty-five thousand cells with every slot full and a neighbour for each.
        eprintln!("  (only {made} junctions of the {target} the gate names)");
    }
}

/// M8's gate: the ecology phase under 5% of tick time with a population that all carries the
/// machinery.
///
/// Measured on two clones like M5's and M7's. The worst case is not a world with a few
/// predators in it — it is a world where *every* cell has an extended spike and a lysosome, so
/// every cell does a neighbour scan and a substrate read every tick. That is the load the
/// phase has to be affordable under, so that is what is measured, even though no run would
/// ever look like it.
///
/// # The spikes deal no damage, on purpose
///
/// The first version of this armed every cell for real and reported the phase at **0.00%,
/// MET** — on the strength of the armed world running at 3.25ms/tick against the control's
/// 46.05ms. It was faster because everything had stabbed everything: the population fell from
/// 52,737 to 2,989 while the control grew to 129,525, and `.max(0.0)` turned an impossible
/// negative into a pass. Which is precisely the trap [`phylogeny_gate`] documents four
/// functions up, reproduced in the gate written after reading it.
///
/// So `spike_damage` is zeroed in the armed clone. Every line of the hot path still runs —
/// extension is computed, upkeep is charged, the neighbour index is scanned, reach is tested,
/// carrion is digested — and nothing dies, so the two clones stay the same size and the
/// difference between them is the phase rather than the body count. Killing the population is
/// a *behavioural* consequence of predation, not part of what the phase costs to run.
fn ecology_gate(_c: &mut Criterion) {
    if cfg!(debug_assertions) {
        return;
    }
    let Some(mut world) = grown("ancestor.mm", 1) else {
        return;
    };
    let population = world.cells().len();

    // Carrion under every cell, so the digestion half has something to do — placed *before*
    // the clone, so both worlds hold exactly the same matter. Putting it in only the armed
    // clone made it a food subsidy rather than a workload: the armed world grew to 151,218
    // against the control's 129,525, and the drift assertion below caught it.
    let slots: Vec<usize> = world.cells().iter().collect();
    for i in &slots {
        let (x, y) = (
            mm_core::fixed::pos_to_square(world.cells().x[*i]),
            mm_core::fixed::pos_to_square(world.cells().y[*i]),
        );
        world
            .substrate_mut()
            .add_chem(mm_core::ecology::CARRION, x, y, mm_core::fixed::q10(400));
    }
    world.adopt_current_contents_as_baseline();

    // Arm every cell, and arrange for the phase to do its work without changing the outcome.
    //
    // The spike is the expensive half — a neighbour scan and a reach test per cell per tick —
    // so it runs at full extension with `spike_damage` zeroed. Nothing dies, and the scan is
    // measured at full load, which is what the gate is for.
    //
    // `spike_upkeep` is zeroed too, and *that* is what took three goes to find. An extended
    // spike does nothing unless the cell can pay for it, so the first version granted every
    // armed cell `q10(50_000)` of energy to make sure the scan ran — fifty thousand units
    // against the hundred or so a cell normally holds. The armed world grew to 151,218 against
    // the control's 129,525, and the two guesses that followed (throttling digestion, then
    // zeroing `digestion_efficiency`) moved it to 151,275 and 151,281, which is how it became
    // clear the food was never the problem. It was the energy. With the upkeep at zero the
    // `paid >= cost` check passes on nothing, the scan runs, and no cell is handed anything.
    //
    // The lysosome is throttled to `param 1` at a sixty-fourth rather than opened wide, so the
    // digestion path — capacity, substrate read, `add_chem`, both ledger conversions, the
    // interior write — runs every tick for every cell while moving a negligible amount of
    // matter. That much was a good idea for the wrong reason.
    // And the catalogue upkeep of the two organelles is zeroed as well, which is the last of
    // it. `EcologyConfig::spike_upkeep` is the cost of holding a spike *out*; the catalogue's
    // `upkeep` is the cost of having one at all, charged by the metabolism phase, and a spike
    // at `param 200` is the dearest thing in the catalogue to carry — deliberately, since that
    // is what stops every lineage growing one. Zeroing only the extension cost left the armed
    // clone at 116,277 against 129,525: it had stopped being fed extra and started paying rent.
    //
    // The pattern across all four of these is one rule: neutralise every effect the organelles
    // have on the *outcome*, keep every line of the code path. What is left is the phase.
    let mut armed = world.clone();
    let mut specs = *armed.biology().metabolism.catalogue.specs();
    for kind in [
        mm_core::OrganelleType::Spike,
        mm_core::OrganelleType::Lysosome,
    ] {
        specs[kind as usize].upkeep = 0;
        specs[kind as usize].upkeep_per_param = 0;
    }
    let mut biology = armed.biology().clone();
    biology.metabolism.catalogue.set_specs(specs);
    biology.ecology.spike_damage = 0;
    biology.ecology.spike_upkeep = 0;
    armed.set_biology(biology);
    for i in &slots {
        let cells = armed.cells_mut();
        cells.slots_mut(*i)[12] = mm_core::Organelle::finished(mm_core::OrganelleType::Spike, 200);
        let mut lysosome = mm_core::Organelle::finished(mm_core::OrganelleType::Lysosome, 1);
        lysosome.control[0] = (mm_core::Q10_ONE / 64) as i16;
        cells.slots_mut(*i)[11] = lysosome;
    }
    armed.adopt_current_contents_as_baseline();

    let mut bare = world;
    let n = 120u64;
    bare.run(10);
    let t = Instant::now();
    bare.run(n);
    let without = t.elapsed().as_secs_f64() / n as f64;

    armed.run(10);
    let t = Instant::now();
    armed.run(n);
    let with = t.elapsed().as_secs_f64() / n as f64;

    // The clones have to still be describing the same world, or the difference between them is
    // not the phase. Asserted before the number is reported rather than printed beside it: the
    // first version printed a 43x population gap and called the gate MET in the line above it.
    let (armed_n, bare_n) = (armed.cells().len(), bare.cells().len());
    let drift = armed_n.abs_diff(bare_n);
    assert!(
        (drift as f64) < population.max(1) as f64 * 0.05,
        "the clones drifted apart in population ({armed_n} against {bare_n}); they are no \
         longer comparable and the difference between their tick times is not the ecology phase"
    );

    let share = ((with - without) / with * 100.0).max(0.0);
    eprintln!(
        "\nM8 ecology gate at {population} cells, every one scanning at full spike reach and \
         digesting:"
    );
    eprintln!(
        "  with ecology {:.2}ms/tick, without {:.2}ms/tick — {share:.2}% (need under 5%)  {}",
        with * 1000.0,
        without * 1000.0,
        if share < 5.0 { "MET" } else { "MISSED" }
    );
    eprintln!("  populations {armed_n} against {bare_n} ({drift} apart)");
}

criterion_group!(
    benches,
    gate,
    phylogeny_gate,
    junction_gate,
    ecology_gate,
    phase_breakdown,
    phases
);
criterion_main!(benches);
