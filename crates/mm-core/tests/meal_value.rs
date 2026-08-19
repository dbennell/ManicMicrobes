//! What one swallowed cell is worth to its eater, end to end.

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::ecology::CARRION;
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::{MutationRates, Organelle, OrganelleType, Scenario, World};

fn world() -> World {
    let mut w = World::new(Scenario {
        seed: 5,
        width: 16,
        height: 16,
        ..Scenario::default()
    })
    .expect("world");
    w.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    w
}

fn spawn(w: &mut World, x: i32, mass: i32, energy: i32) -> CellId {
    let genome = w.genomes().intern(vec![0x2E]).expect("genome");
    w.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(8),
        mass: q10(mass),
        energy: q10(energy),
        membrane: 24,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    })
}

#[test]
#[ignore = "a probe; run it on purpose"]
fn what_one_meal_is_worth() {
    let mut w = world();
    // A predator with the engulfer's working parts, and a prey of median weight.
    let eater = spawn(&mut w, 8, 200, 400);
    let i = w.cells().index(eater).expect("alive");
    w.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 56);
    w.cells_mut().slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
    w.cells_mut().slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 100);
    let mut vac = Organelle::finished(OrganelleType::Vacuole, 120);
    vac.control[1] = Q10_ONE as i16; // appetite open
    w.cells_mut().slots_mut(i)[4] = vac;
    w.cells_mut().slots_mut(i)[6] = Organelle::finished(OrganelleType::Lysosome, 100);

    let prey = spawn(&mut w, 8, 60, 400);
    let j = w.cells().index(prey).expect("alive");
    w.cells_mut().slots_mut(j)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
    // Give the prey a realistic cytoplasm.
    let sub = w.biology().metabolism.catalogue.metabolism.primary().substrate;
    w.cells_mut().interior_mut(j)[sub] = q10(20);
    w.adopt_current_contents_as_baseline();

    let prey_energy = w.cells().energy[j];
    let prey_mass = w.cells().mass[j];
    eprintln!(
        "prey: mass {} units, charge {} units, cytoplasm {} units of substrate",
        prey_mass / Q10_ONE,
        prey_energy / Q10_ONE,
        q10(20) / Q10_ONE
    );

    let ei = w.cells().index(eater).expect("alive");
    let before = w.cells().energy[ei];
    let sub_before = w.cells().interior(ei)[sub];
    let mass_before = w.cells().mass[ei];
    let world_matter_before: i64 = w.total_matter().iter().sum();
    eprintln!(
        "eater before: energy {} substrate {} mass {}",
        before / Q10_ONE,
        sub_before / Q10_ONE,
        mass_before / Q10_ONE
    );
    let mut ate_at = None;
    let mut peak = before;
    for tick in 0..400 {
        w.run(1);
        if ate_at.is_none() && w.cells().index(prey).is_none() {
            ate_at = Some(tick);
        }
        if let Some(i) = w.cells().index(eater) {
            peak = peak.max(w.cells().energy[i]);
            if tick % 50 == 0 || Some(tick) == ate_at {
                eprintln!(
                    "  t={tick:<3} eater energy {:<5} carrion held {:<4} substrate {:<4}{}",
                    w.cells().energy[i] / Q10_ONE,
                    w.cells().interior(i)[CARRION] / Q10_ONE,
                    w.cells().interior(i)[sub] / Q10_ONE,
                    if Some(tick) == ate_at { "  <- swallowed" } else { "" }
                );
            }
        }
    }
    let ei = w.cells().index(eater).expect("alive");
    eprintln!(
        "\neater after 400 ticks: energy {} substrate {} mass {}",
        w.cells().energy[ei] / Q10_ONE,
        w.cells().interior(ei)[sub] / Q10_ONE,
        w.cells().mass[ei] / Q10_ONE
    );
    eprintln!(
        "gains from the meal: substrate {:+}, mass {:+}",
        (w.cells().interior(ei)[sub] - sub_before) / Q10_ONE,
        (w.cells().mass[ei] - mass_before) / Q10_ONE
    );
    eprintln!(
        "prey charge {} units DISSIPATED; peak eater energy {} (started {})",
        prey_energy / Q10_ONE,
        peak / Q10_ONE,
        before / Q10_ONE
    );
    eprintln!("world matter {} -> {} (I4)", world_matter_before, w.total_matter().iter().sum::<i64>());
}
