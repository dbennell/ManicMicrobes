//! What a pack actually looks like, as numbers rather than as a screenshot.
//!
//! Run with `--release --ignored --nocapture`. Reports, for each scenario, the things that
//! distinguish a good pack from a bad one: how deeply cells interpenetrate, how far the worst
//! pair is through the core floor, and the spread of radii — because the packing bench holds
//! cells of one size and a growing population does not.
//!
//! Kept in the repository because describing screenshots was how this went wrong for a long
//! time: two changes were reported as visible improvements when a pixel diff later showed they
//! had altered nothing at all.
use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, pos_to_square, q10, q10_to_pos, POS_ONE};
use mm_core::{LightRegime, MutationRates, Op, Organelle, OrganelleType, Scenario, Seeding, World};

struct Stats {
    population: usize,
    pairs: usize,
    deep: usize,
    worst: f32,
    radii: Vec<f32>,
    occupancy: f32,
    press: Vec<f32>,
    /// Interior contents as a percentage of interior capacity, per cell.
    fill: Vec<f32>,
    /// How many cells carry an active vacuole, which is the only way to raise that capacity.
    vacuoles: usize,
}

fn stats(world: &World) -> Stats {
    let cells = world.cells();
    let index = world.neighbours();
    let (mut pairs, mut deep) = (0usize, 0usize);
    let mut worst = 1.0f32;
    let mut radii = Vec::new();
    for i in cells.iter() {
        let ri = q10_to_pos(mm_core::biology::radius(cells, i));
        radii.push(ri as f32 / POS_ONE as f32);
        let (sx, sy) = (pos_to_square(cells.x[i]), pos_to_square(cells.y[i]));
        for j in index.around(sx, sy) {
            if j <= i || !cells.occupied(j) {
                continue;
            }
            let rj = q10_to_pos(mm_core::biology::radius(cells, j));
            let want = (ri + rj) as f32;
            let dx = (cells.x[i] - cells.x[j]) as f32;
            let dy = (cells.y[i] - cells.y[j]) as f32;
            let d = (dx * dx + dy * dy).sqrt();
            if d >= want {
                continue;
            }
            pairs += 1;
            let frac = d / want.max(1.0);
            worst = worst.min(frac);
            // The core floor is 95% of touching. Under 80% is a pair the solver has failed.
            if frac < 0.80 {
                deep += 1;
            }
        }
    }
    radii.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // How much of the slide the cells would cover if they did not overlap. Over 100% and they
    // physically cannot fit, and no solver can help.
    let area: f32 = radii.iter().map(|r| std::f32::consts::PI * r * r).sum();
    let slide = (world.substrate().width() * world.substrate().height()) as f32;
    // And how hard the population says it is being squeezed, which is what division reads.
    let mut press: Vec<f32> = world
        .pressure()
        .iter()
        .enumerate()
        .filter(|(i, _)| cells.occupied(*i))
        .map(|(_, p)| *p as f32 / mm_core::Q10_ONE as f32)
        .collect();
    press.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // How full a cell's cytoplasm is, against the cap that actually exists.
    //
    // The capacity is *per chemical* — `BASE_INTERIOR_CAPACITY` is what a cell may hold of any
    // one of them — so the fullest chemical is the one that decides whether the cell has stopped
    // being able to eat. Summing all sixteen against a single-chemical cap reads 1600% for a
    // cell that is exactly at its limit, which is a measurement of the denominator.
    let mut fill = Vec::new();
    let mut vacuoles = 0usize;
    for i in cells.iter() {
        let held = cells.interior(i).iter().copied().max().unwrap_or(0) as i64;
        let cap = mm_core::biology::interior_capacity(cells, i).max(1) as i64;
        fill.push(100.0 * held as f32 / cap as f32);
        if cells.slots(i).iter().any(|o| o.kind == OrganelleType::Vacuole && o.is_active()) {
            vacuoles += 1;
        }
    }
    fill.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Stats {
        population: cells.len(), pairs, deep, worst, radii,
        occupancy: 100.0 * area / slide, press, fill, vacuoles,
    }
}

fn report(label: &str, s: &Stats) {
    let p = |q: usize| -> f32 {
        if s.radii.is_empty() { 0.0 } else { s.radii[(s.radii.len() * q / 100).min(s.radii.len() - 1)] }
    };
    println!(
        "PROBE {label:<28} pop {:>5}  pairs {:>5}  deep {:>5} ({:>5.1}%)  worst {:>5.1}%  \
         r p50 {:.2}  area {:>5.0}%  pressure p50 {:.1}  fill p50 {:>5.1}% p90 {:>5.1}% max {:>5.1}%  vacuoles {}",
        s.population, s.pairs, s.deep,
        100.0 * s.deep as f32 / s.pairs.max(1) as f32,
        100.0 * s.worst,
        p(50),
        s.occupancy,
        if s.press.is_empty() { 0.0 } else { s.press[s.press.len() / 2] },
        if s.fill.is_empty() { 0.0 } else { s.fill[s.fill.len() / 2] },
        if s.fill.is_empty() { 0.0 } else { s.fill[s.fill.len() * 9 / 10] },
        s.fill.last().copied().unwrap_or(0.0),
        s.vacuoles,
    );
}

/// The packing bench, as `SlideRes::bench` builds it: a fixed population settling.
fn bench_world() -> World {
    let scenario = Scenario {
        name: "bench".into(),
        seed: 1,
        width: 48,
        height: 48,
        light: LightRegime::Uniform { intensity: mm_core::Q10_ONE },
        current: mm_core::light::CurrentField::Still,
        gravity: 2,
        jitter: 0,
        seeding: vec![],
        ..Scenario::default()
    };
    let mut world = World::new(scenario).expect("bench");
    let mut biology = BiologyConfig { mutation: MutationRates::none(), ..BiologyConfig::default() };
    biology.metabolism.rates.background_damage = 0;
    biology.metabolism.rates.metabolic_floor = 0;
    biology.metabolism.rates.growth_rate = 0;
    biology.ecology.crowding_damage = 0;
    biology.ecology.spike_damage = 0;
    world.set_biology(biology);
    let inert = world.genomes().intern(vec![Op::Halt.canonical_byte()]).expect("genome");
    for k in 0..220u32 {
        let across = 15u32;
        let span = POS_ONE * 5 / 4;
        let start = (pos(48) - (across as i32 - 1) * span) / 2;
        let id = world.spawn_cell(CellSeed {
            x: start + (k % across) as i32 * span,
            y: start + (k / across) as i32 * span,
            mass: q10(18 + (k * 7 % 26) as i32),
            energy: q10(1_000_000),
            membrane: 24,
            key: 11,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome: std::sync::Arc::clone(&inert),
        });
        if let Some(i) = world.cells_mut().index(id) {
            world.cells_mut().slots_mut(i)[0] =
                Organelle::finished(OrganelleType::Membrane, 24 + (k % 5) as u8 * 40);
        }
    }
    world.adopt_current_contents_as_baseline();
    world
}

/// A live slide grown from one founder: overlap injected continuously by division.
fn growth_world(size: u32) -> World {
    let sc = Scenario {
        name: "growth".into(),
        seed: 1,
        width: size,
        height: size,
        light: LightRegime::Uniform { intensity: mm_core::Q10_ONE },
        seeding: vec![
            Seeding::Uniform { chemical: 11, per_square: q10(400) },
            Seeding::Uniform { chemical: 14, per_square: q10(400) },
            Seeding::Uniform { chemical: 4, per_square: q10(400) },
        ],
        ..Scenario::default()
    };
    let mut world = World::new(sc).expect("growth");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::default(),
        ..BiologyConfig::default()
    });
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm"),
    )
    .expect("ancestor");
    let bytes = mm_asm::assemble(&src).expect("assemble").bytes;
    let genome = world.genomes().intern(bytes).expect("intern");
    let id = world.spawn_cell(CellSeed {
        x: pos(size as i32 / 2),
        y: pos(size as i32 / 2),
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
    if let Some(i) = world.cells_mut().index(id) {
        let c = world.cells_mut();
        c.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
        c.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
    }
    world.adopt_current_contents_as_baseline();
    world
}

/// Stop cells dividing without touching the solver: make a division unaffordable.
fn stop_division(world: &mut World) {
    let mut b = world.biology().clone();
    b.division_energy = i32::MAX / 2;
    world.set_biology(b);
}

/// What `split_pressure` has to be for the slide not to overfill.
///
/// The gate is per cell and the overshoot is collective: a cell at the edge of the colony reads
/// low pressure and is right to, but its daughter goes inward, and the colony as a whole sails
/// past what the slide can hold. One variable, the bench numbers as the target.
#[test]
#[ignore = "diagnostic; run with --release --ignored --nocapture"]
fn split_pressure_sweep() {
    println!("SWEEP  threshold   pop   area%   deep%   worst");
    for tenths in [30u32, 20, 15, 10, 6] {
        let mut w = growth_world(16);
        let mut b = w.biology().clone();
        b.split_pressure = (mm_core::Q10_ONE as i64 * tenths as i64 / 10) as i32;
        w.set_biology(b);
        w.run(2400);
        let s = stats(&w);
        println!(
            "SWEEP {:>10.1} {:>5} {:>7.0}% {:>7.1}% {:>7.1}%",
            tenths as f32 / 10.0,
            s.population,
            s.occupancy,
            100.0 * s.deep as f32 / s.pairs.max(1) as f32,
            100.0 * s.worst,
        );
    }
    println!("SWEEP  (bench control: 220 cells, 24% area, 0.0% deep, worst 94.3%)");
}

/// When did the slide go over 100%, and what was happening at the time?
///
/// Occupancy starts near zero with one cell, so something drove it to 237% — and the endpoint
/// cannot say whether that was division multiplying cells or metabolism enlarging them. This
/// walks the trajectory and reports both, with the births in the interval beside them.
#[test]
#[ignore = "diagnostic; run with --release --ignored --nocapture"]
fn occupancy_trajectory() {
    let mut w = growth_world(16);
    let mut last_births = 0u64;
    println!("TRAJ  tick   pop  births/interval   area%   r p50   deep%   pressure p50");
    for step in 0..24 {
        w.run(100);
        let s = stats(&w);
        let births = w.births_total();
        let p50 = if s.radii.is_empty() { 0.0 } else { s.radii[s.radii.len() / 2] };
        let press = if s.press.is_empty() { 0.0 } else { s.press[s.press.len() / 2] };
        println!(
            "TRAJ {:>5} {:>5} {:>16} {:>7.0}% {:>7.2} {:>7.1}% {:>10.1}",
            (step + 1) * 100,
            s.population,
            births - last_births,
            s.occupancy,
            p50,
            100.0 * s.deep as f32 / s.pairs.max(1) as f32,
            press,
        );
        last_births = births;
    }
}

#[test]
#[ignore = "diagnostic; run with --release --ignored --nocapture"]
fn packing_probe() {
    let mut bench = bench_world();
    bench.run(900);
    report("bench, settled", &stats(&bench));

    let mut growth = growth_world(16);
    growth.run(1600);
    let before = stats(&growth);
    report("growth, dividing", &before);

    // Step 2: stop the births and let the same pack settle. If it relaxes to bench quality,
    // births are the whole story and the solver is adequate given time.
    for extra in [200u64, 800, 3000, 10000, 20000] {
        stop_division(&mut growth);
        growth.run(extra);
        report(&format!("growth, +{extra} no births"), &stats(&growth));
    }
}
