//! What a genome can see of the light. It was less than one bit; ISA 15 made it a signal.

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::light::LightRegime;
use mm_core::{MutationRates, Organelle, OrganelleType, Scenario, World};

#[test]
fn a_photosensor_can_tell_day_from_night() {
    // This test was written to record the *defect*: `read_sensor` divided ambient light by
    // `Q10_ONE`, and `the_short_night.ron` runs between 128 and 1024, so a full cycle gave twenty
    // distinct values of the field and one distinct reading of it — zero, never once reaching 1,
    // because the triangle peaks at 1023. A genome could not tell day from night.
    //
    // ISA 15 put the three ambient readings on `SENSE_GAIN`, the scale the gradient readings have
    // used since M3 for the same reason. It is kept pointing the other way: the reading has to
    // track the field, or a dormancy genome has nothing to sleep on.
    let mut world = World::new(Scenario {
        seed: 42,
        width: 16,
        height: 16,
        light: LightRegime::DayNight {
            period_ticks: 2000,
            day: 1024,
            night: 128,
        },
        ..Scenario::default()
    })
    .expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    let genome = world.genomes().intern(vec![0x2E]).expect("genome");
    let id = world.spawn_cell(CellSeed {
        x: pos(8),
        y: pos(8),
        mass: q10(30),
        energy: q10(4000),
        membrane: 24,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    let i = world.cells().index(id).expect("alive");
    world.cells_mut().slots_mut(i)[4] = Organelle::finished(OrganelleType::Photosensor, 200);
    let eye = world.cells().slots(i)[4];

    let mut readings = Vec::new();
    let mut raw = Vec::new();
    for step in 0..20u64 {
        world.run(100);
        raw.push(world.substrate().light_at(8, 8));
        // The real path, not a re-derivation of it: `read_sensor` index 0 is ambient light, and
        // touch, glow and shell cover are not consulted for it.
        readings.push(
            mm_core::sensing::read_sensor(
                &eye,
                0,
                mm_core::sensing::SensorContext {
                    substrate: world.substrate(),
                    x: 8,
                    y: 8,
                    tick: step * 100,
                    cell_key: 1,
                    touch: Default::default(),
                    glow: Default::default(),
                    shell_cover: 0,
                },
            )
            .expect("a photosensor reads something"),
        );
    }
    eprintln!("raw light          : {raw:?}");
    eprintln!("as a genome sees it: {readings:?}");
    let distinct: std::collections::BTreeSet<_> = readings.iter().collect();
    let distinct_raw: std::collections::BTreeSet<_> = raw.iter().collect();
    eprintln!(
        "{} distinct raw values, {} distinct readings",
        distinct_raw.len(),
        distinct.len()
    );

    // The field really does move: twenty different values between 128 and 1023.
    assert!(distinct_raw.len() > 10, "the light did not change; {raw:?}");
    // And the genome now sees it move. Not all twenty: `DayNight` is a symmetric triangle, so its
    // rising and falling legs report the same levels and any faithful reading of it has about
    // half as many distinct values as samples. Ten is that, and it was one.
    assert!(
        distinct.len() >= 10,
        "the reading is coarser than the field it reports: {} raw, {} seen — {readings:?}",
        distinct_raw.len(),
        distinct.len()
    );
    // Night is distinguishable from day by a plain comparison, which is the whole point, and
    // both ends fit in a range a genome can write with `IMM` (0..255) plus one shift.
    let (low, high) = (
        *readings.iter().min().expect("readings"),
        *readings.iter().max().expect("readings"),
    );
    assert!(low < high / 4, "night {low} is not clearly darker than day {high}");
    assert!(high < 1024, "daylight at {high} leaves a genome little room under saturation");
}
