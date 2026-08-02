//! Does the pack have an inside?
//!
//! Run with `--release --ignored --nocapture`. The packing probe next door measures geometry —
//! how deeply cells interpenetrate and how much of the slide they cover. This one measures
//! *chemistry against position*: whether a cell wedged in the middle of a pack lives in a
//! different medium from one on the rind, which is the thing that makes a biofilm stratify and
//! gives motility a reason to exist.
//!
//! The control is built in. Every scenario here seeds the fluid uniformly, so the medium starts
//! with no gradient anywhere; anything the profile shows is something the cells did.
use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, pos_to_square, q10, q10_to_pos, POS_ONE, Q10_ONE};
use mm_core::{LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding, World};

/// A live slide grown from one founder. Same recipe as the packing probe's, size parameterised
/// so the pack can be an island in open medium rather than the whole slide.
fn growth_world(size: u32) -> World {
    let sc = Scenario {
        name: "growth".into(),
        seed: 1,
        width: size,
        height: size,
        light: LightRegime::Uniform { intensity: Q10_ONE },
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

/// The packing bench, as the packing probe builds it: a fixed population of inert cells that
/// tiles perfectly. The control for every pressure number.
fn bench_world() -> World {
    let scenario = Scenario {
        name: "bench".into(),
        seed: 1,
        width: 48,
        height: 48,
        light: LightRegime::Uniform { intensity: Q10_ONE },
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
    let inert = world
        .genomes()
        .intern(vec![mm_core::Op::Halt.canonical_byte()])
        .expect("genome");
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

fn percentile(sorted: &[f32], q: usize) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[(sorted.len() * q / 100).min(sorted.len() - 1)]
}

/// Pressure, sorted, in `Q10` units, for every occupied cell.
fn pressures(world: &World) -> Vec<f32> {
    let cells = world.cells();
    let mut v: Vec<f32> = world
        .pressure()
        .iter()
        .enumerate()
        .filter(|(i, _)| cells.occupied(*i))
        .map(|(_, p)| *p as f32 / Q10_ONE as f32)
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

/// How many neighbours a cell is actually touching. The rind/core coordinate: a cell in the
/// interior of a hexagonal pack has six, one on the edge has three or fewer.
fn degrees(world: &World) -> Vec<usize> {
    let cells = world.cells();
    let index = world.neighbours();
    let mut deg = vec![0usize; cells.capacity()];
    for i in cells.iter() {
        let ri = q10_to_pos(mm_core::biology::radius(cells, i)) as f32;
        let (sx, sy) = (pos_to_square(cells.x[i]), pos_to_square(cells.y[i]));
        let mut n = 0;
        for j in index.around(sx, sy) {
            if j == i || !cells.occupied(j) {
                continue;
            }
            let rj = q10_to_pos(mm_core::biology::radius(cells, j)) as f32;
            let dx = (cells.x[i] - cells.x[j]) as f32;
            let dy = (cells.y[i] - cells.y[j]) as f32;
            // Touching, with a little slack: contact is maintained a hair short of tangency.
            if (dx * dx + dy * dy).sqrt() < (ri + rj) * 1.05 {
                n += 1;
            }
        }
        deg[i] = n;
    }
    deg
}

/// Which chemicals are worth printing: the ones that are actually in the world.
fn present(world: &World) -> Vec<usize> {
    let total = world.substrate().total_chem();
    (0..16).filter(|&c| total[c] > 0).collect()
}

/// The measured pressure band, for both worlds, at every percentile rather than the median
/// alone. §17.7 records a band a sink must bite inside: above what a healthy pack runs at and below what an
/// overfilled one does, which is a claim about two distributions and not about two numbers.
#[test]
#[ignore = "diagnostic; run with --release --ignored --nocapture"]
fn pressure_band() {
    println!("BAND  world                    n     p10    p25    p50    p75    p90    p99    max");
    let mut bench = bench_world();
    bench.run(900);
    let mut rows: Vec<(String, Vec<f32>)> = vec![("bench, settled".into(), pressures(&bench))];

    let mut growth = growth_world(16);
    growth.run(1600);
    rows.push(("growth 16, dividing".into(), pressures(&growth)));
    let mut b = growth.biology().clone();
    b.division_energy = i32::MAX / 2;
    growth.set_biology(b);
    growth.run(20_000);
    rows.push(("growth 16, converged".into(), pressures(&growth)));

    let mut island = growth_world(32);
    island.run(1600);
    rows.push(("growth 32 @1600".into(), pressures(&island)));
    island.run(2400);
    rows.push(("growth 32 @4000".into(), pressures(&island)));

    for (label, p) in rows {
        println!(
            "BAND  {label:<22} {:>4}  {:>6.2} {:>6.2} {:>6.2} {:>6.2} {:>6.2} {:>6.2} {:>6.2}",
            p.len(),
            percentile(&p, 10),
            percentile(&p, 25),
            percentile(&p, 50),
            percentile(&p, 75),
            percentile(&p, 90),
            percentile(&p, 99),
            p.last().copied().unwrap_or(0.0),
        );
    }
}

/// Is there a gradient from the rind of a pack to its core, and does an interior cell live in a
/// different medium from a surface one?
///
/// Two views of the same slide. The radial profile reads the fluid alone, ring by ring out from
/// the pack's centroid, which says whether the *medium* is stratified. The degree buckets read
/// the cells, grouped by how many neighbours each is wedged against, which says whether being
/// buried changes what a cell has access to. A pack that is diffusion-limited shows both. A pack
/// swimming in a well-stirred bath shows neither.
#[test]
#[ignore = "diagnostic; run with --release --ignored --nocapture"]
fn rind_to_core() {
    for (size, ticks) in [(32u32, 4000u64), (16, 1600)] {
        let mut world = growth_world(size);
        world.run(ticks);
        let cells = world.cells();
        if cells.len() == 0 {
            println!("GRAD  size {size}: extinct");
            continue;
        }

        // Centroid of the pack, in square coordinates.
        let (mut cx, mut cy) = (0f64, 0f64);
        for i in cells.iter() {
            cx += pos_to_square(cells.x[i]) as f64;
            cy += pos_to_square(cells.y[i]) as f64;
        }
        cx /= cells.len() as f64;
        cy /= cells.len() as f64;

        let chems = present(&world);
        let sub = world.substrate();
        let (w, h) = (sub.width() as i32, sub.height() as i32);
        let max_ring = ((w.max(h) as f64) * 0.75) as usize;

        // Ring bins over the fluid: squares, cells centred there, and the mean of each chemical.
        let mut sq_count = vec![0f64; max_ring + 1];
        let mut chem_sum = vec![vec![0f64; max_ring + 1]; 16];
        for y in 0..h {
            for x in 0..w {
                let d = (((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt()) as usize;
                let r = d.min(max_ring);
                sq_count[r] += 1.0;
                let idx = sub.index(x, y);
                for &c in &chems {
                    chem_sum[c][r] += sub.chem_plane(c)[idx] as f64;
                }
            }
        }
        let mut cell_count = vec![0f64; max_ring + 1];
        for i in cells.iter() {
            let d = (((pos_to_square(cells.x[i]) as f64 - cx).powi(2)
                + (pos_to_square(cells.y[i]) as f64 - cy).powi(2))
            .sqrt()) as usize;
            cell_count[d.min(max_ring)] += 1.0;
        }

        let names: Vec<String> = chems
            .iter()
            .map(|&c| {
                let n = &world.scenario().chemicals.get(c).name;
                format!("{}:{}", c, &n[..n.len().min(6)])
            })
            .collect();
        println!("\nGRAD  size {size}, {ticks} ticks, pop {}, centroid ({cx:.1},{cy:.1})", cells.len());
        print!("GRAD  ring  cells  squares");
        for n in &names {
            print!(" {n:>12}");
        }
        println!("   (mean units per square, Q10)");
        for r in 0..=max_ring {
            if sq_count[r] == 0.0 {
                continue;
            }
            print!("GRAD  {r:>4} {:>6.0} {:>8.0}", cell_count[r], sq_count[r]);
            for &c in &chems {
                print!(" {:>12.0}", chem_sum[c][r] / sq_count[r]);
            }
            println!();
        }

        // The same slide read through the cells instead: what does being buried change?
        let deg = degrees(&world);
        let buckets: [(&str, usize, usize); 4] =
            [("edge 0-2", 0, 2), ("3-4", 3, 4), ("5-6", 5, 6), ("core 7+", 7, 99)];
        println!(
            "GRAD  bucket      n   mass   energy   fill%   press   {}",
            names.join(" ")
        );
        for (label, lo, hi) in buckets {
            let members: Vec<usize> =
                world.cells().iter().filter(|&i| deg[i] >= lo && deg[i] <= hi).collect();
            if members.is_empty() {
                continue;
            }
            let n = members.len() as f64;
            let cells = world.cells();
            let mass: f64 =
                members.iter().map(|&i| cells.mass[i] as f64).sum::<f64>() / n / Q10_ONE as f64;
            let energy: f64 =
                members.iter().map(|&i| cells.energy[i] as f64).sum::<f64>() / n / Q10_ONE as f64;
            let fill: f64 = members
                .iter()
                .map(|&i| {
                    let held = cells.interior(i).iter().copied().max().unwrap_or(0) as f64;
                    100.0 * held / mm_core::biology::interior_capacity(cells, i).max(1) as f64
                })
                .sum::<f64>()
                / n;
            let press: f64 = members
                .iter()
                .map(|&i| world.pressure()[i] as f64 / Q10_ONE as f64)
                .sum::<f64>()
                / n;
            print!("GRAD  {label:<8} {:>5} {mass:>6.1} {energy:>8.1} {fill:>7.0} {press:>7.2}  ", members.len());
            let sub = world.substrate();
            for &c in &chems {
                let v: f64 = members
                    .iter()
                    .map(|&i| {
                        sub.chem_at(c, pos_to_square(cells.x[i]), pos_to_square(cells.y[i])) as f64
                    })
                    .sum::<f64>()
                    / n;
                print!(" {v:>12.0}");
            }
            println!();
        }
    }
}

/// Where the energy goes.
///
/// §17.7 says stored energy has no ceiling and there is no sink. Half of that is true and half
/// is not: `Ledger::dissipate` is called on every conversion — photosynthesis banks half of what
/// it absorbs and respiration three quarters of what it releases — so free energy *is* leaving
/// the world as heat already. The question this asks is whether that outflow keeps up. It prints
/// income, outflow and holdings side by side, plus what the median cell is sitting on, so the
/// difference between "there is no sink" and "the sink is proportional to throughput and
/// therefore cannot bound holdings" is visible rather than assumed.
#[test]
#[ignore = "diagnostic; run with --release --nocapture"]
fn energy_budget() {
    for size in [16u32, 32] {
        let mut world = growth_world(size);
        println!(
            "\nENRG  size {size}   tick    pop   energy_in  energy_out    stored   in-out/tick  \
             energy50   ---- upkeep-ticks in hand ----    fill50\n\
             ENRG                                                                              \
                          min      p10      p50      p90"
        );
        let (mut last_in, mut last_out) = (0i64, 0i64);
        for step in 0..20 {
            world.run(200);
            let l = world.ledger();
            let cells = world.cells();
            let mut e: Vec<i64> = cells.iter().map(|i| cells.energy[i] as i64).collect();
            e.sort_unstable();
            let pick = |q: usize| -> i64 {
                if e.is_empty() { 0 } else { e[(e.len() * q / 100).min(e.len() - 1)] }
            };
            // How many ticks of upkeep the median cell could pay for out of what it holds: the
            // holding expressed in the only unit that says whether it is a buffer or a hoard.
            let upkeep: Vec<i64> = cells
                .iter()
                .map(|i| {
                    let floor = world.biology().metabolism.rates.metabolic_floor.max(0) as i64;
                    let organelles: i64 = cells
                        .slots(i)
                        .iter()
                        .filter(|o| o.is_present())
                        .map(|o| {
                            let s = world.biology().metabolism.catalogue.spec(o.kind);
                            s.upkeep as i64 + s.upkeep_per_param as i64 * o.param as i64
                        })
                        .sum();
                    (floor + organelles).max(1)
                })
                .collect();
            let mut ratio: Vec<i64> = cells
                .iter()
                .zip(&upkeep)
                .map(|(i, u)| cells.energy[i] as i64 / u)
                .collect();
            ratio.sort_unstable();
            let r = |q: usize| -> i64 {
                if ratio.is_empty() { 0 } else { ratio[(ratio.len() * q / 100).min(ratio.len() - 1)] }
            };
            // How full the median cytoplasm is against its own cap, so the matter side of the
            // same question is in the same table.
            let mut fill: Vec<i64> = cells
                .iter()
                .map(|i| {
                    let held = cells.interior(i).iter().copied().max().unwrap_or(0) as i64;
                    100 * held / mm_core::biology::interior_capacity(cells, i).max(1) as i64
                })
                .collect();
            fill.sort_unstable();
            println!(
                "ENRG  {:>12} {:>6} {:>11} {:>11} {:>9} {:>13} {:>9} {:>8} {:>8} {:>8} {:>8} {:>8}",
                (step + 1) * 200,
                cells.len(),
                l.energy_in(),
                l.energy_out(),
                l.energy_stored(),
                ((l.energy_in() - last_in) - (l.energy_out() - last_out)) / 200,
                pick(50) / Q10_ONE as i64,
                ratio.first().copied().unwrap_or(0),
                r(10),
                r(50),
                r(90),
                fill.get(fill.len() / 2).copied().unwrap_or(0),
            );
            last_in = l.energy_in();
            last_out = l.energy_out();
        }
    }
}

/// Does a pack slow anything down?
///
/// The same slide, twice, differing in one thing. Grow a pack, then release an identical tracer
/// spike into the square at the middle of it — once with the pack in place, once with every cell
/// removed a tick earlier — and read the spread from the same origin over the same ticks.
///
/// The first attempt at this compared the pack against open medium on the far side of the slide,
/// which measured the closed box: a spike near a wall cannot spread through it and piles up,
/// which reads exactly like being retarded by cells. Same square, cells or no cells, is the only
/// comparison with one variable in it.
#[test]
#[ignore = "diagnostic; run with --release --ignored --nocapture"]
fn tracer_through_the_pack() {
    // Chemical 0 is a signal: inert, no energy yield, not structural, nothing metabolises it.
    const TRACER: usize = 0;
    const DOSE: i32 = 1_000_000;

    let mut grown = growth_world(32);
    grown.run(4000);
    let cells = grown.cells();
    let (mut cx, mut cy) = (0i64, 0i64);
    for i in cells.iter() {
        cx += pos_to_square(cells.x[i]) as i64;
        cy += pos_to_square(cells.y[i]) as i64;
    }
    let n = cells.len().max(1) as i64;
    let (cx, cy) = ((cx / n) as i32, (cy / n) as i32);
    let deg = degrees(&grown);
    let buried = grown.cells().iter().filter(|&i| deg[i] >= 5).count();
    println!(
        "TRACER  origin ({cx},{cy}); pop {}, of which {buried} have five or more contacts",
        grown.cells().len()
    );

    let mut empty = grown.clone();
    let ids: Vec<_> = {
        let c = empty.cells();
        c.iter().map(|i| c.id_at(i)).collect()
    };
    for id in ids {
        empty.kill_cell(id);
    }
    empty.run(1);
    println!("TRACER  control population after clearing: {}", empty.cells().len());

    for w in [&mut grown, &mut empty] {
        w.substrate_mut().add_chem(TRACER, cx, cy, DOSE);
    }

    let profile = |w: &World| -> Vec<i64> {
        let sub = w.substrate();
        let (width, height) = (sub.width() as i32, sub.height() as i32);
        let (mut sums, mut counts) = (vec![0i64; 8], vec![0i64; 8]);
        for y in 0..height {
            for x in 0..width {
                let d = ((((x - cx) as f32).powi(2) + ((y - cy) as f32).powi(2)).sqrt()) as usize;
                if d < 8 {
                    sums[d] += sub.chem_plane(TRACER)[sub.index(x, y)] as i64;
                    counts[d] += 1;
                }
            }
        }
        sums.iter().zip(counts).map(|(s, c)| s / c.max(1)).collect()
    };

    println!("TRACER  ticks  where     ring0  ring1  ring2  ring3  ring4  ring5  ring6  ring7");
    for step in 0..6 {
        grown.run(20);
        empty.run(20);
        for (label, w) in [("pack   ", &grown), ("no cells", &empty)] {
            println!(
                "TRACER {:>6}  {label}  {}",
                (step + 1) * 20,
                profile(w).iter().map(|v| format!("{v:>7}")).collect::<Vec<_>>().join(""),
            );
        }
    }
}

/// The two numbers a storage rule and a light rule would have to be calibrated against.
///
/// `interior_capacity` is a cap on *one* chemical, so the quantity an osmotic rule would read —
/// the total free solute in the cytoplasm — is not bounded by anything at all today, and is not
/// what any existing probe reports. And photosynthesis reads the light on a cell's centre square
/// without consuming it, so whether making light a rival good would bite at all depends on how
/// often two cells share a square, which is also unmeasured.
#[test]
#[ignore = "diagnostic; run with --release --ignored --nocapture"]
fn solute_and_shade() {
    for (size, marks) in [(16u32, [400u64, 1200, 4000]), (32, [400, 1200, 4000])] {
        let mut world = growth_world(size);
        let mut at = 0u64;
        for mark in marks {
            world.run(mark - at);
            at = mark;
            let cells = world.cells();
            if cells.len() == 0 {
                continue;
            }
            // Total free solute, as a multiple of the per-chemical cap that is the only cap there
            // is. A cell exactly at the cap on all sixteen would read 16.
            let mut solute: Vec<f32> = cells
                .iter()
                .map(|i| {
                    let held: i64 = cells.interior(i).iter().map(|&v| v as i64).sum();
                    held as f32 / mm_core::biology::interior_capacity(cells, i).max(1) as f32
                })
                .collect();
            solute.sort_by(|a, b| a.partial_cmp(b).unwrap());

            // How many cells sit on one square, which is what a rival light rule would divide by.
            let sub = world.substrate();
            let mut per_square = vec![0u32; sub.len()];
            for i in cells.iter() {
                let idx = sub.index(pos_to_square(cells.x[i]), pos_to_square(cells.y[i]));
                per_square[idx] += 1;
            }
            let shared: usize = per_square.iter().filter(|&&n| n > 1).count();
            let lit: usize = per_square.iter().filter(|&&n| n > 0).count();
            let worst = per_square.iter().copied().max().unwrap_or(0);
            // And the same question asked over a cell's whole footprint rather than its centre:
            // area covered against squares occupied, which is what shading would actually divide.
            let area: f32 = cells
                .iter()
                .map(|i| {
                    let r = q10_to_pos(mm_core::biology::radius(cells, i)) as f32 / POS_ONE as f32;
                    std::f32::consts::PI * r * r
                })
                .sum();
            println!(
                "SOLUTE size {size} @{mark:<5} pop {:>4}  free solute x cap: p10 {:>5.1} p50 {:>5.1} p90 {:>5.1} max {:>6.1}   \
                 squares with a centre {lit:>4}, with two or more {shared:>3}, worst {worst}, \
                 footprint/occupied square {:.2}",
                cells.len(),
                percentile(&solute, 10),
                percentile(&solute, 50),
                percentile(&solute, 90),
                solute.last().copied().unwrap_or(0.0),
                area / lit.max(1) as f32,
            );
        }
    }
}

/// The safety check a storage charge stands or falls on.
///
/// The margin numbers in §17.7 are marginals: during the expansion the poorest cell alive has one
/// tick of upkeep in hand, and somewhere on the same slide a cell is holding seven times its cap
/// in solute. Whether a charge on solute kills the founding race depends entirely on whether
/// those are the *same cell*, and a marginal cannot say. This crosses them.
///
/// Reported as the solute load of the cells in the bottom decile of energy buffer, against the
/// load of the population as a whole. A charge is safe exactly when the poor are lean.
#[test]
#[ignore = "diagnostic; run with --release --ignored --nocapture"]
fn poor_and_full() {
    println!(
        "JOINT  tick   pop  |  poorest decile by buffer: buffer p50   solute p50   solute max  \
         |  whole population: solute p50   solute max"
    );
    let mut world = growth_world(16);
    for step in 0..20 {
        world.run(200);
        let cells = world.cells();
        if cells.len() == 0 {
            continue;
        }
        // (upkeep-ticks in hand, solute as a multiple of the cell's own per-chemical cap).
        let mut rows: Vec<(i64, f32)> = cells
            .iter()
            .map(|i| {
                let floor = world.biology().metabolism.rates.metabolic_floor.max(0) as i64;
                let organelles: i64 = cells
                    .slots(i)
                    .iter()
                    .filter(|o| o.is_present())
                    .map(|o| {
                        let s = world.biology().metabolism.catalogue.spec(o.kind);
                        s.upkeep as i64 + s.upkeep_per_param as i64 * o.param as i64
                    })
                    .sum();
                let buffer = cells.energy[i] as i64 / (floor + organelles).max(1);
                let held: i64 = cells.interior(i).iter().map(|&v| v as i64).sum();
                let load = held as f32 / mm_core::biology::interior_capacity(cells, i).max(1) as f32;
                (buffer, load)
            })
            .collect();
        rows.sort_by_key(|r| r.0);
        let poor = &rows[..(rows.len() / 10).max(1)];
        let mut poor_load: Vec<f32> = poor.iter().map(|r| r.1).collect();
        poor_load.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut all_load: Vec<f32> = rows.iter().map(|r| r.1).collect();
        all_load.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "JOINT {:>6} {:>5}  | {:>26} {:>12.1} {:>12.1}  | {:>28.1} {:>12.1}",
            (step + 1) * 200,
            cells.len(),
            poor[poor.len() / 2].0,
            percentile(&poor_load, 50),
            poor_load.last().copied().unwrap_or(0.0),
            percentile(&all_load, 50),
            all_load.last().copied().unwrap_or(0.0),
        );
    }
}
