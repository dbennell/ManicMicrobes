//! What a genome can actually see of the light, which is less than one bit.

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::light::LightRegime;
use mm_core::{MutationRates, Organelle, OrganelleType, Scenario, World};

#[test]
fn a_photosensor_cannot_tell_day_from_night() {
    // `read_sensor` reports ambient light as `sat_i16(light / Q10_ONE)`, and `the_short_night.ron`
    // — the one shipped day/night world — runs between 128 and 1024. So the whole cycle reads as
    // the integer 0 except at the instant of peak noon, and a genome has no way to know it is
    // night. This is the same defect the pH sensor's own note in `sensing.rs` describes and fixes
    // for itself: "divided down, the whole interesting range of a slide would be the integer 7".
    //
    // Recorded as a test rather than a comment because it bounds what any dormancy genome can be
    // written against — see `genomes/sleeper.mm`, which watches its inputs instead.
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
    // And the genome sees one number the whole way round. Not "0 except at noon" — the triangle
    // peaks at 1023, one short of the divisor, so the reading never reaches 1 at all.
    assert_eq!(
        distinct.len(),
        1,
        "the reading has gained resolution — good, and this test now needs rewriting: {readings:?}"
    );
    assert_eq!(readings[0], 0);
}
