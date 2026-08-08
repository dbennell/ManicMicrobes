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
//! A clean, monotone axis from one picture to the other, bought with wall and pressure:
//!
//! ```text
//!   membrane   rigidity   mean swell
//!       —        0.00       1.237      no turgor at all: today's foam, exactly
//!       24       0.09       1.215      what the ancestors build — 1.8% off, imperceptible
//!       64       0.25       1.178
//!      128       0.50       1.118
//!      200       0.78       1.051
//!      255       1.00       1.000      marbles: the true circle, cut by its seams, gaps left
//! ```
//!
//! **The default picture is preserved.** A cell that holds no solute has no turgor and therefore
//! no rigidity, and is drawn exactly as it always was. The ancestors, at a membrane of 24 out of
//! a possible 255, come out 1.8% less inflated than before — which is below anything a person can
//! see and well inside what `swell_probe` already tolerates frame to frame.
//!
//! **And it is bought rather than switched on.** Going from membrane 24 to 255 costs 5.1x the
//! structural matter to build (14 units against 71.75) and 6.8x the upkeep to carry (0.078
//! against 0.53 energy a tick), on top of holding enough solute to pressurise the wall, which
//! `osmotic_upkeep` charges for quadratically. A lineage that wants to be a heap of marbles pays
//! in both currencies, every tick, forever; one that wants to be a sheet of tissue pays nothing
//! and is what a cell is by default.
//!
//! Turgor saturates at one whole `osmotic_threshold`, which is why the 1x and 4x rows are
//! identical: past the threshold the wall is as pressurised as it is going to get and the only
//! remaining term is what it is built of.

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
fn bench(membrane: u8, solute_capacities: i32) -> World {
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
    println!("  membrane  solute (x threshold)  rigidity   mean swell   max swell   cells");
    for membrane in [24u8, 64, 128, 200, 255] {
        for capacities in [0i32, 1, 4] {
            let world = bench(membrane, capacities);
            let rates = world.biology().metabolism.rates;
            let rigidity = world
                .cells()
                .iter()
                .next()
                .map(|i| mm_core::biology::rigidity(world.cells(), i, &rates))
                .unwrap_or(0) as f32
                / Q10_ONE as f32;
            let (mean, max, n) = drawn(world);
            println!(
                "  {membrane:>8}  {capacities:>19}   {rigidity:>8.2}   {mean:>10.3}   \
                 {max:>9.3}   {n:>5.0}"
            );
        }
    }
    println!(
        "\n  The ancestors build a membrane of 24, so the top-left corner of that table is the\n  \
         picture as it has always been, and it is unchanged."
    );
}
