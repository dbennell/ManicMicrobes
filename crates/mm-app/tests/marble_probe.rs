//! Foam or marbles, and what it costs to be one rather than the other.
//!
//! Run with `--release --ignored --nocapture`.
//!
//! # The two pictures
//!
//! A packed slide currently reads as a continuous sheet: cells flattened against one another into
//! polygons, with no gaps anywhere. That is `slide::area_swell` working exactly as designed — it
//! grows each cell until what survives its neighbours' seams encloses the area the cell actually
//! has, which is "what separates a foam from a gravel pile" in its own words, and it took real
//! effort to get.
//!
//! It is also only one of the two things a packed crowd of cells can look like. Yeast under a
//! microscope stay obstinately round: pressed together hard, still separate bodies, with visible
//! gaps between them. The difference is not how hard they are pushed, it is what they are made
//! of — a bag of fluid squeezed on one side bulges on the other, and a **walled** cell does not.
//!
//! # The measure
//!
//! `area_swell` returns the factor a cell is drawn larger by. One means "drawn at its true radius,
//! cut by its seams, gaps left where the circles do not meet" — marbles. Above one is a cell
//! inflating into the space its neighbours leave — foam. So the mean swell across a packed slide
//! *is* the foam-to-marble axis, and this reports it against what the cells are built of.
//!
//! # What it found
//!
//! ```text
//!   what                                  membrane  rigidity  joined   mean swell
//!   lean, free — the shipped ancestors        24      0.09      no        1.215
//!   lean, free, no turgor at all              24      0.00      no        1.237
//!   firm, free — a heap of marbles           255      1.00      no        1.000
//!   firm, JOINED — glued, so tissue anyway   255      1.00     yes        1.082
//!   lean, joined — tissue, as before          24      0.09     yes        1.232
//!   half firm, free                          128      0.50      no        1.118
//! ```
//!
//! **All three pictures are reachable and the default is the one that shipped.** A lean, unjoined
//! cell — which is every ancestor in the tree — is drawn at 1.215 against the 1.237 it was drawn
//! at before any of this, a difference of 1.8% and well below what a person can see or what
//! `swell_probe` already tolerates between frames.
//!
//! **Glue beats firmness, which is the whole point of doing it this way.** The fourth row is a
//! maximally rigid cell that would be a marble on its own and is drawn most of the way back to
//! tissue because it is joined to its neighbours. A lineage that decides to be one body gets one
//! body's picture whatever its cells are made of, and that is the true reason a moss leaf
//! tessellates: its cells are stuck together, not soft.
//!
//! **And the marble end is bought.** Membrane 24 to 255 is 5.1x the structural matter to build
//! and 6.8x the upkeep to carry, on top of holding the solute that pressurises the wall, which
//! `osmotic_upkeep` charges for quadratically.
//!
//! # The junction budget shows up in the picture
//!
//! The glued row reads 1.082 rather than the 1.237 of a free lean cell, and the gap is not a
//! calibration problem. `JUNCTIONS_PER_CELL` is **four**, and a cell packed into a lattice
//! touches six or more — so a sheet that is glued as thoroughly as the engine allows is still
//! only about two-thirds joined, and is drawn two-thirds of the way to tissue.
//!
//! A fully tessellated body is therefore not currently expressible: not because the picture
//! cannot draw one, but because a cell cannot be joined to everything it touches. That is the
//! same four-slot ceiling `docs/NEURONS.md` measured from the other end, where a directed nerve
//! chain of three cells already spends two slots on its middle cell.

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::{
    chem::CHEM_COUNT, LightRegime, MutationRates, Op, Organelle, OrganelleType, Scenario, World,
};
use mm_app::slide::Slide;

/// A fixed, tightly packed lattice of inert cells, so the picture is the only thing varying.
///
/// Inert on purpose: a growing population changes its own radii, spacing and solute from tick to
/// tick, and none of that is what this measures. The packing probe's bench, with membrane and
/// solute under control.
fn bench(membrane: u8, solute_capacities: i32, joined: bool) -> World {
    let scenario = Scenario {
        name: "marbles".into(),
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
    let mut world = World::new(scenario).expect("world");
    let mut biology = BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    };
    // Nothing may age, starve, poison itself or grow: the lattice has to sit still.
    biology.metabolism.rates.background_damage = 0;
    biology.metabolism.rates.metabolic_floor = 0;
    biology.metabolism.rates.growth_rate = 0;
    biology.metabolism.rates.osmotic_upkeep = 0;
    biology.metabolism.rates.energy_leak = 0;
    biology.ecology.crowding_damage = 0;
    world.set_biology(biology);

    let inert = world
        .genomes()
        .intern(vec![Op::Halt.canonical_byte()])
        .expect("genome");
    let threshold = world.biology().metabolism.rates.osmotic_threshold;
    for k in 0..220u32 {
        let across = 15u32;
        let span = mm_core::fixed::POS_ONE * 5 / 4;
        let start = (pos(48) - (across as i32 - 1) * span) / 2;
        let id = world.spawn_cell(CellSeed {
            x: start + (k % across) as i32 * span,
            y: start + (k / across) as i32 * span,
            mass: q10(18 + (k * 7 % 26) as i32),
            energy: q10(1_000_000),
            membrane,
            key: 11,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome: std::sync::Arc::clone(&inert),
        });
        if let Some(i) = world.cells_mut().index(id) {
            let cells = world.cells_mut();
            cells.slots_mut(i)[0] = Organelle::finished(OrganelleType::Membrane, membrane);
            // Solute, spread across the species so no single one hits its own cap. What matters
            // is the total, which is what `osmotic_load` sums and what pressurises the wall.
            //
            // Peroxide is skipped, and the first run of this probe is why: chemical 13 is toxic
            // above `toxicity_threshold` = 8 units, so loading it to a whole interior capacity
            // killed every cell on the slide and three rows of the table read zero. Turgor does
            // not care which species the particles are; the poison does.
            let usable: Vec<usize> = (0..CHEM_COUNT).filter(|c| *c != 13).collect();
            let each = (threshold as i64 * solute_capacities as i64 / usable.len() as i64) as i32;
            for c in usable {
                cells.interior_mut(i)[c] = each.max(0);
            }
        }
    }
    world.adopt_current_contents_as_baseline();

    // Glue the lattice into one body, if that is what is being asked. Four junctions a cell, so
    // each is joined right and down and receives the mirror of that from left and up — which is
    // the whole budget and exactly what a sheet of tissue needs.
    if joined {
        let ids: Vec<CellId> = world.cells().iter().map(|i| world.cells().id_at(i)).collect();
        let across = 15usize;
        for (n, a) in ids.iter().enumerate() {
            for b in [
                (n % across + 1 < across).then(|| n + 1).filter(|m| *m < ids.len()),
                Some(n + across).filter(|m| *m < ids.len()),
            ]
            .into_iter()
            .flatten()
            {
                let (Some(ia), Some(ib)) = (
                    world.cells().index(*a),
                    world.cells().index(ids[b]),
                ) else {
                    continue;
                };
                let rest =
                    mm_core::junction::distance(world.cells(), ia, ib).max(mm_core::fixed::POS_ONE);
                let (Some(sa), Some(sb)) = (
                    mm_core::junction::free_slot(world.cells(), ia),
                    mm_core::junction::free_slot(world.cells(), ib),
                ) else {
                    continue;
                };
                let other_b = ids[b];
                world.cells_mut().junctions_mut(ia)[sa] = mm_core::junction::Junction {
                    kind: mm_core::junction::JunctionKind::Hard,
                    other: other_b,
                    rest,
                };
                world.cells_mut().junctions_mut(ib)[sb] = mm_core::junction::Junction {
                    kind: mm_core::junction::JunctionKind::Hard,
                    other: *a,
                    rest,
                };
            }
        }
    }

    // Let the separation solver settle the lattice before anything is drawn.
    world.run(200);
    world
}

fn drawn(world: World) -> (f32, f32, f32) {
    let mut slide = Slide::new(Scenario::stress(8, 8)).expect("slide");
    slide.set_world(world);
    slide.set_camera(24.0, 24.0, 40.0, 40.0);
    slide.set_zoom(64.0);
    let frame = slide.frame();
    let swells: Vec<f32> = frame
        .cells
        .iter()
        .filter(|d| !d.squash.is_empty())
        .map(|d| d.area_swell)
        .collect();
    if swells.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let n = swells.len() as f32;
    let mean = swells.iter().sum::<f32>() / n;
    let max = swells.iter().cloned().fold(f32::MIN, f32::max);
    (mean, max, n)
}

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn what_it_costs_to_stay_round() {
    println!(
        "\nMARBLE  220 inert cells on a fixed lattice, settled 200 ticks.\n\
         `swell` is the factor a cell is drawn larger by so its clipped outline keeps its area.\n\
         1.00 is a marble — the true circle, cut by its seams, gaps left. Above 1 is foam."
    );
    println!("  what                                  membrane  rigidity  joined   mean swell");
    for (label, membrane, solute, joined) in [
        ("lean, free — the shipped ancestors", 24u8, 1i32, false),
        ("lean, free, no turgor at all", 24, 0, false),
        ("firm, free — a heap of marbles", 255, 1, false),
        ("firm, JOINED — glued, so tissue anyway", 255, 1, true),
        ("lean, joined — tissue, as before", 24, 1, true),
        ("half firm, free", 128, 1, false),
    ] {
        let world = bench(membrane, solute, joined);
        let rates = world.biology().metabolism.rates;
        let rigidity = world
            .cells()
            .iter()
            .next()
            .map(|i| mm_core::biology::rigidity(world.cells(), i, &rates))
            .unwrap_or(0) as f32
            / Q10_ONE as f32;
        let (mean, _max, _n) = drawn(world);
        println!(
            "  {label:<38}{membrane:>6}  {rigidity:>8.2}  {:>6}   {mean:>10.3}",
            if joined { "yes" } else { "no" }
        );
    }
    println!(
        "\n  1.000 is a marble — the true circle, cut by its seams, gaps left.\n           Above 1 is foam: inflated until the clipped outline keeps the cell's area."
    );
}

// ---------------------------------------------------------------------------------------------
// The same question asked of a live slide rather than a lattice.

fn live(genome: &str, ticks: u64) -> World {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(genome);
    let bytes = mm_asm::assemble(&std::fs::read_to_string(src).expect("genome"))
        .expect("assembles")
        .bytes;
    // `soup.ron`'s recipe, written out rather than parsed: `ron` is not a dev-dependency of
    // this crate and adding one to read a file whose contents are six lines would be the wrong
    // trade. If the shipped soup changes, `m8_ecology` is where that is noticed.
    let scenario = Scenario {
        name: "primordial soup".into(),
        seed: 20250728,
        width: 64,
        height: 64,
        light: LightRegime::Uniform { intensity: Q10_ONE },
        current: mm_core::light::CurrentField::Still,
        fluid_interval: 1,
        seeding: vec![
            mm_core::Seeding::Uniform { chemical: 11, per_square: q10(400) },
            mm_core::Seeding::Uniform { chemical: 14, per_square: q10(400) },
            mm_core::Seeding::Uniform { chemical: 4, per_square: q10(400) },
            // The minerals every recipe is costed in, at the Redfield proportion of
            // the carbon above. Nothing in the engine produces them.
            mm_core::Seeding::Uniform { chemical: 5, per_square: (q10(400)) * 16 / 106 },
            mm_core::Seeding::Uniform { chemical: 6, per_square: (q10(400)) / 53 },
        ],
        ..Scenario::default()
    };
    let mut world = World::new(scenario).expect("world");
    world.place_founders(&bytes, 16);
    world.run(ticks);
    world
}

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn a_marble_on_a_real_slide() {
    // `marble.mm` against `ancestor.mm`, same scenario, same seeding, same everything else. The
    // lattice above proves the picture responds to what a cell is made of; this proves a lineage
    // can get there and stay there while feeding itself.
    println!("\nLIVE  soup.ron, 16 founders, 20 000 ticks");
    println!(
        "  genome        pop   membrane p50   rigidity p50   mean swell   mean radius   coverage"
    );
    for genome in ["ancestor.mm", "marble.mm"] {
        let world = live(genome, 20_000);
        let rates = world.biology().metabolism.rates;
        let mut membranes: Vec<i32> = Vec::new();
        let mut rigidities: Vec<f32> = Vec::new();
        let mut radii: Vec<f32> = Vec::new();
        for i in world.cells().iter() {
            membranes.push(world.cells().slots(i)[0].param as i32);
            rigidities
                .push(mm_core::biology::rigidity(world.cells(), i, &rates) as f32 / Q10_ONE as f32);
            radii.push(
                mm_core::biology::radius(world.cells(), i) as f32 / Q10_ONE as f32,
            );
        }
        membranes.sort_unstable();
        rigidities.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        radii.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        let pop = membranes.len();
        if pop == 0 {
            println!("  {genome:<12}  extinct");
            continue;
        }
        // How much of the slide the cells actually cover, which is the difference between a
        // packed sheet and a scatter — and the thing a median radius cannot tell you.
        let area: f32 = radii.iter().map(|r| std::f32::consts::PI * r * r).sum();
        let slide = (world.substrate().width() * world.substrate().height()) as f32;
        let (mean, _max, _n) = drawn(world);
        println!(
            "  {genome:<12} {pop:>4}   {:>12}   {:>12.2}   {mean:>10.3}   {:>11.2}   {:>8.0}%",
            membranes[pop / 2],
            rigidities[pop / 2],
            radii[pop / 2],
            area * 100.0 / slide,
        );
    }
    println!(
        "\n  1.000 is a marble — the true circle, cut by its seams, gaps left.\n  \
         Above 1 is foam: inflated until the clipped outline keeps the cell's area."
    );
}

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn what_makes_a_mat_of_marbles_rather_than_a_scatter() {
    // `marble.mm` is round but sparse: 55% coverage against the ancestor's 105%, because it pays
    // seven times the membrane upkeep and its equilibrium population is lower for it. A picture
    // of separate round cells with acres between them is not the picture — a smear of yeast is
    // wall to wall *and* round.
    //
    // Coverage cannot be fixed by shrinking the slide: light arrives per square, so carrying
    // capacity scales with area and the fraction is unchanged. What raises it is income.
    println!("\nMAT  marble.mm on soup.ron, 16 founders, 20 000 ticks, light varied");
    println!("  light   pop   coverage   mean swell");
    for intensity in [1024i32, 2048, 4096, 8192] {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../genomes/marble.mm");
        let bytes = mm_asm::assemble(&std::fs::read_to_string(src).expect("genome"))
            .expect("assembles")
            .bytes;
        let scenario = Scenario {
            name: "mat".into(),
            seed: 20250728,
            width: 64,
            height: 64,
            light: LightRegime::Uniform { intensity },
            current: mm_core::light::CurrentField::Still,
            fluid_interval: 1,
            seeding: vec![
                mm_core::Seeding::Uniform { chemical: 11, per_square: q10(400) },
                mm_core::Seeding::Uniform { chemical: 14, per_square: q10(400) },
                mm_core::Seeding::Uniform { chemical: 4, per_square: q10(400) },
                // The minerals every recipe is costed in, at the Redfield proportion of
                // the carbon above. Nothing in the engine produces them.
                mm_core::Seeding::Uniform { chemical: 5, per_square: (q10(400)) * 16 / 106 },
                mm_core::Seeding::Uniform { chemical: 6, per_square: (q10(400)) / 53 },
            ],
            ..Scenario::default()
        };
        let mut world = World::new(scenario).expect("world");
        world.place_founders(&bytes, 16);
        world.run(20_000);
        let pop = world.cells().len();
        let area: f32 = world
            .cells()
            .iter()
            .map(|i| {
                let r = mm_core::biology::radius(world.cells(), i) as f32 / Q10_ONE as f32;
                std::f32::consts::PI * r * r
            })
            .sum();
        let slide = (world.substrate().width() * world.substrate().height()) as f32;
        let cover = area * 100.0 / slide;
        let (mean, _, _) = drawn(world);
        println!("  {intensity:>5}  {pop:>4}   {cover:>7.0}%   {mean:>10.3}");
    }
}

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn who_is_actually_on_the_marbles_slide() {
    // Reported from the microscope: the marbles scenario looks squashed. The suspicion is that
    // something softer overtook `marble.mm`, and the point of this is that the suspicion is
    // checkable — a cell's firmness is wall times turgor, and both are readable off the arena.
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/marble.mm");
    let bytes = mm_asm::assemble(&std::fs::read_to_string(src).expect("genome"))
        .expect("assembles")
        .bytes;
    let scenario = Scenario {
        name: "the marbles".into(),
        seed: 20250728,
        width: 64,
        height: 64,
        light: LightRegime::Uniform { intensity: Q10_ONE },
        current: mm_core::light::CurrentField::Still,
        fluid_interval: 1,
        seeding: vec![
            mm_core::Seeding::Uniform { chemical: 11, per_square: q10(400) },
            mm_core::Seeding::Uniform { chemical: 14, per_square: q10(400) },
            mm_core::Seeding::Uniform { chemical: 4, per_square: q10(400) },
            // The minerals every recipe is costed in, at the Redfield proportion of
            // the carbon above. Nothing in the engine produces them.
            mm_core::Seeding::Uniform { chemical: 5, per_square: (q10(400)) * 16 / 106 },
            mm_core::Seeding::Uniform { chemical: 6, per_square: (q10(400)) / 53 },
        ],
        ..Scenario::default()
    };
    let mut world = World::new(scenario).expect("world");
    let mut biology = BiologyConfig::default();
    biology.separation_relax = Q10_ONE / 8;
    biology.metabolism.rates.rigidity_gain = Q10_ONE * 16;
    world.set_biology(biology);
    world.place_founders(&bytes, 16);

    println!("\nWHO  the marbles, 16 founders of marble.mm, mutation on");
    println!("  tick    pop   membrane p10/p50/p90   rigidity p10/p50/p90   mean swell");
    for step in 0..=6 {
        if step > 0 {
            world.run(2_000);
        }
        let rates = world.biology().metabolism.rates;
        let mut mem: Vec<i32> = Vec::new();
        let mut rig: Vec<f32> = Vec::new();
        for i in world.cells().iter() {
            mem.push(world.cells().slots(i)[0].param as i32);
            rig.push(
                mm_core::biology::rigidity(world.cells(), i, &rates) as f32 / Q10_ONE as f32,
            );
        }
        mem.sort_unstable();
        rig.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        let n = mem.len();
        if n == 0 {
            println!("  {:>5}   extinct", step * 2_000);
            break;
        }
        let pick = |v: &[f32], q: usize| v[(v.len() - 1) * q / 100];
        let pickm = |v: &[i32], q: usize| v[(v.len() - 1) * q / 100];
        // Draw it, on a copy, so the frame is the one the microscope would build.
        let mut slide = Slide::new(Scenario::stress(8, 8)).expect("slide");
        slide.set_world(world.clone());
        slide.set_camera(32.0, 32.0, 40.0, 40.0);
        slide.set_zoom(64.0);
        let frame = slide.frame();
        let swells: Vec<f32> = frame
            .cells
            .iter()
            .filter(|d| !d.squash.is_empty())
            .map(|d| d.area_swell)
            .collect();
        let swell = if swells.is_empty() {
            0.0
        } else {
            swells.iter().sum::<f32>() / swells.len() as f32
        };
        println!(
            "  {:>5}   {n:>4}   {:>3}/{:>3}/{:>3}              {:.2}/{:.2}/{:.2}           {swell:.3}",
            step * 2_000,
            pickm(&mem, 10),
            pickm(&mem, 50),
            pickm(&mem, 90),
            pick(&rig, 10),
            pick(&rig, 50),
            pick(&rig, 90),
        );
        world.run(0);
    }
}
