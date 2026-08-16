//! The calcite test and the pH sensor: catalogue slots 31 and 23 (`docs/CHEMISTRY.md` §11).
//!
//! Both fill slots the upper half had already reserved for them. `Shell = 15` pairs with 31 and
//! `Chemosensor = 7` pairs with 23, and the layout's whole argument is that bit 4 of a type
//! operand means "the same job done a different way" — so a single copy error moves a lineage
//! between siblings, and evolution can hill-climb between them instead of having to find each
//! from nothing.
//!
//! **The claim worth testing is the pair, not either half.** A calcite test that merely worked
//! would be a re-skin of the silica one; what makes it worth a slot is that the two fail in
//! opposite conditions, so neither dominates and which is better is a property of the water.

use mm_core::cell::{CellId, CellSeed};
use mm_core::chem::{CALCIUM, CARBONATE, CARBON_DIOXIDE, PH_NEUTRAL};
use mm_core::fixed::{pos, q10};
use mm_core::organelle::shell_cover;
use mm_core::{LightRegime, Organelle, OrganelleType, Scenario, Seeding, World, Q10_ONE};

fn slide() -> Scenario {
    Scenario {
        name: "calcite".to_string(),
        seed: 4,
        width: 32,
        height: 32,
        light: LightRegime::Uniform {
            intensity: Q10_ONE,
        },
        seeding: vec![
            Seeding::Uniform {
                chemical: CARBON_DIOXIDE,
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
            Seeding::Uniform {
                chemical: 5,
                per_square: q10(400) * 16 / 106,
            },
            Seeding::Uniform {
                chemical: 6,
                per_square: q10(400) / 53,
            },
            Seeding::Uniform {
                chemical: CARBONATE,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: CALCIUM,
                per_square: q10(200),
            },
        ],
        ..Scenario::default()
    }
}

/// A cell holding `stock` of each listed chemical, running `src`.
fn cell_running(world: &mut World, src: &str, stock: &[(usize, i32)]) -> usize {
    let genome = world
        .genomes()
        .intern(mm_asm::assemble(src).expect("assembles").bytes)
        .expect("interned");
    let id = world.spawn_cell(CellSeed {
        x: pos(16),
        y: pos(16),
        mass: q10(40),
        energy: q10(100_000),
        membrane: 48,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    let i = world.cells_mut().index(id).expect("spawned");
    let cells = world.cells_mut();
    cells.interior_mut(i)[4] = q10(400);
    for (c, v) in stock {
        cells.interior_mut(i)[*c] = *v;
    }
    i
}

/// **A calcite test needs both halves, and one alone will not do.**
///
/// `build_trace` is an AND: every non-zero entry is required, charged and refunded together. That
/// is exactly why this is a sibling organelle rather than an option on the shell's recipe —
/// calcium beside silicon in one trace would make a shell needing *both* minerals, which is not
/// "either" and is not what anybody wanted.
#[test]
fn a_calcite_test_needs_calcium_and_carbonate_together() {
    // param 100 into slot 5, building type 31.
    let src = "IMM 100\nIMM 31\nIMM 5\nBUILD\nHALT\n";
    for (calcium, carbonate, expected) in [
        (0, 0, false),
        (q10(200), 0, false),
        (0, q10(200), false),
        (q10(200), q10(200), true),
    ] {
        let mut world = World::new(slide()).expect("world");
        // Bare water, so what the cell holds is the only supply — the affordability gate reads
        // the interior *and* the square it is standing on.
        for y in 0..32i32 {
            for x in 0..32i32 {
                world.substrate_mut().set_chem(CALCIUM, x, y, 0);
                world.substrate_mut().set_chem(CARBONATE, x, y, 0);
            }
        }
        let i = cell_running(
            &mut world,
            src,
            &[(CALCIUM, calcium), (CARBONATE, carbonate)],
        );
        // The fixture stocked a cell by hand, which is matter arriving from outside the world.
        // Taking the baseline afterwards is how a test says so; without it `check_matter` is
        // measuring the fixture rather than the mechanism.
        world.adopt_current_contents_as_baseline();
        world.run(4);
        let built = world.cells().slots(i)[5].kind == OrganelleType::CalciteShell;
        assert_eq!(
            built, expected,
            "with {calcium} calcium and {carbonate} carbonate the test was {}built",
            if built { "" } else { "not " }
        );
        if !expected {
            // And a refused build spends nothing. A half-charged build is matter destroyed.
            let cost = world
                .biology()
                .metabolism
                .catalogue
                .spec(OrganelleType::CalciteShell)
                .matter_cost(100);
            let spent = q10(400) - world.cells().interior(i)[4];
            assert!(
                spent < cost,
                "a build that could not afford its recipe still spent {spent} carbon against a \
                 structural cost of {cost}"
            );
        }
        world.check_matter().expect("building must conserve");
    }
}

/// Both kinds of test are armour, and the coverage law does not care which.
#[test]
fn a_calcite_test_is_armour_like_its_sibling() {
    let mut world = World::new(slide()).expect("world");
    let i = cell_running(&mut world, "HALT\n", &[]);
    for (slot, kind) in [(1, OrganelleType::Shell), (2, OrganelleType::CalciteShell)] {
        let mut o = Organelle::finished(kind, 255);
        o.control[0] = Q10_ONE as i16;
        world.cells_mut().slots_mut(i)[slot] = o;
    }
    let both = shell_cover(world.cells(), i);
    world.cells_mut().slots_mut(i)[2] = Organelle::default();
    let one = shell_cover(world.cells(), i);
    assert!(one > 0, "a silica test covers nothing");
    assert!(
        both > one,
        "a calcite test added no cover: {one} alone against {both} for the pair"
    );
}

/// **The pair, on opposite terms.** The claim that makes the sibling worth a slot.
///
/// A calcite test standing in sour water gives its mineral back; a silica one in the same water
/// does not notice. Neither is better — which is better is a property of the neighbourhood, and
/// that is the pressure a second kind of armour is for.
#[test]
fn calcite_wears_in_sour_water_and_silica_does_not() {
    let held = |w: &World, c: usize| -> i64 {
        w.substrate()
            .chem_plane(c)
            .iter()
            .map(|v| i64::from(*v))
            .sum()
    };

    // Reefs of each, standing in the same acid.
    let mut world = World::new(slide()).expect("world");
    let dose = world.rock_dose();
    world.set_rock(&[(8, 8)], CALCIUM, dose / 2);
    world.set_rock(&[(8, 8)], CARBONATE, dose / 2);
    world.set_rock(&[(24, 24)], 7, dose);
    for y in 0..32i32 {
        for x in 0..32i32 {
            world.substrate_mut().set_chem(CARBON_DIOXIDE, x, y, q10(900));
            world.substrate_mut().set_chem(CARBONATE, x, y, 0);
            world.substrate_mut().set_chem(CALCIUM, x, y, 0);
            // Silica's own water held *at* its saturation, so the only thing that could dissolve
            // it is the acid — which is the comparison this test is making.
            let sat = world.scenario().chemicals.get(7).saturation;
            world.substrate_mut().set_chem(7, x, y, sat);
        }
    }
    world.adopt_current_contents_as_baseline();
    // Beside the reef, not inside it: a blocked square holds nothing and reads neutral, which
    // is what `rock_has_no_ph` asserts. The water attacking it is the water next to it.
    assert!(
        world.substrate().ph_at(9, 8) < PH_NEUTRAL,
        "the fixture water is not sour"
    );

    let ksi = mm_core::chem::solid_slot(7).unwrap();
    let silica_before = world.substrate().solid_at(ksi, 24, 24);
    let calcite_before = world.substrate().solid_total_at(8, 8);
    world.run(4_000);
    let silica_after = world.substrate().solid_at(ksi, 24, 24);
    let calcite_after = world.substrate().solid_total_at(8, 8);

    assert!(
        calcite_after < calcite_before,
        "calcite stood in acid unchanged: {calcite_before} -> {calcite_after}"
    );
    assert_eq!(
        silica_after, silica_before,
        "silica dissolved in water that is saturated in it; the only difference here is the pH, \
         and silica is supposed to be indifferent to it"
    );
    let _ = held(&world, CARBONATE);
    world.check_matter().expect("the comparison must conserve");
}

/// **A genome can read the water's acidity, and which way it sours.**
///
/// Without a sensor the carbonate swing selects on lineages that cannot act on it, which is a
/// pressure with no strategy behind it. The reading is raw `Q10` rather than divided down to whole
/// units, following the photosensor's glow readings — pH runs nought to fourteen, and rounded to
/// integers a whole slide would read `7` and every gradient would be nothing.
#[test]
fn a_ph_sensor_reads_the_water_and_its_gradient() {
    let mut world = World::new(slide()).expect("world");
    // Sour on the left, sweet on the right.
    for y in 0..32i32 {
        for x in 0..32i32 {
            let co2 = if x < 16 { q10(1200) } else { q10(100) };
            world.substrate_mut().set_chem(CARBON_DIOXIDE, x, y, co2);
        }
    }
    let i = cell_running(&mut world, "HALT\n", &[]);
    world.cells_mut().slots_mut(i)[3] =
        Organelle::finished(OrganelleType::PhSensor, 255);
    world.run(2);

    // Read through the same path a genome does.
    let ph_here = world.substrate().ph_at(16, 16);
    assert!(
        ph_here != PH_NEUTRAL,
        "the fixture put the cell in neutral water, so there is nothing to read"
    );
    let r = mm_core::sensing::sense_ph(world.substrate(), 16, 16);
    assert_eq!(r.concentration, ph_here);
    assert!(
        r.gradient_x > 0,
        "the water is sour to the left and sweet to the right, and the gradient does not say so: \
         {}",
        r.gradient_x
    );
    assert_eq!(r.gradient_y, 0, "a uniform column reported a vertical gradient");

    // Off the slide reads neutral rather than nothing: pH has no zero, and treating the edge as
    // maximally acid would put a permanent gradient round the rim for every cell to follow.
    let edge = mm_core::sensing::sense_ph(world.substrate(), 0, 0);
    assert!(
        edge.concentration != 0 || ph_here == 0,
        "the edge read as pH zero rather than as the water actually there"
    );
}
