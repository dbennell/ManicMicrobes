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
