//! The carbonate system: a buffer that is matter, a pH that is a reading (`docs/CHEMISTRY.md` §11).
//!
//! The claim the whole section rests on is that **biology moves the chemical balance and the
//! balance is legible**, and it rests on something that was already there: photosynthesis is
//! `2 CO₂ + light -> sugar + O₂` with respiration running it back, so every respiring cell makes
//! acid and every photosynthesising one takes it away. Nothing had to be built to drive this; what
//! was missing was anything that read it.
//!
//! pH is not matter and is not stored. The buffer is, and conserves like everything else.

use mm_core::chem::{ph_of, CALCIUM, CARBONATE, CARBON_DIOXIDE, PH_MAX, PH_NEUTRAL};
use mm_core::fixed::{q10, Q10_ONE};
use mm_core::{LightRegime, Scenario, Seeding, World};

/// A lit, still slide with both pools and the CO₂ they are read against.
fn slide() -> Scenario {
    Scenario {
        name: "carbonate".to_string(),
        seed: 11,
        width: 24,
        height: 24,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        seeding: vec![
            Seeding::Uniform {
                chemical: CARBON_DIOXIDE,
                per_square: q10(400),
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

/// The reading itself: total, bounded, monotone, and neutral where there is nothing to read.
#[test]
fn the_ph_reading_is_total_bounded_and_monotone() {
    // Equal pools are neutral by construction, which is what makes a seeded slide start at seven.
    assert_eq!(ph_of(q10(400), q10(400)), PH_NEUTRAL);
    assert_eq!(ph_of(1, 1), PH_NEUTRAL);
    // Nothing at all reads neutral rather than dividing by zero. Water with no carbon chemistry
    // in it has no acidity to report, and a blocked square holds nothing.
    assert_eq!(ph_of(0, 0), PH_NEUTRAL);

    // All acid, all base, and neither escapes the scale.
    assert_eq!(ph_of(0, q10(400)), 0);
    assert_eq!(ph_of(q10(400), 0), PH_MAX);
    for (b, a) in [
        (0, 0),
        (i32::MAX, 0),
        (0, i32::MAX),
        (i32::MAX, i32::MAX),
        (-1, -1),
        (-5, q10(9)),
    ] {
        let ph = ph_of(b, a);
        assert!(
            (0..=PH_MAX).contains(&ph),
            "ph_of({b}, {a}) = {ph} is off the scale"
        );
    }

    // Monotone in the thing it is a reading of: more acid is never a higher pH.
    let mut last = PH_MAX + 1;
    for co2 in (0..=q10(800)).step_by(q10(20) as usize) {
        let ph = ph_of(q10(400), co2);
        assert!(ph <= last, "pH rose as CO2 rose, at {co2}");
        last = ph;
    }
}

/// **Carbonate buffers the swing.** The test this section exists for.
///
/// The same insult of CO₂ moves pH less in a well-buffered square than in a poorly buffered one,
/// monotonically in how much buffer there is. Nothing implements that — it falls out of pH being
/// a *ratio*: at the operating point the two pools are comparable, and the swing for an insult
/// `d` goes as `K·d / 2P` where `P` is the size of the pools. Double the buffer, halve the swing.
///
/// # Which knob is "more buffer", and the way to get this wrong
///
/// **Both pools are scaled together, and that is the point rather than a convenience.** The first
/// version of this test held CO₂ fixed and raised carbonate alone, found the swing getting
/// *larger*, and looked like a refutation. It was measuring a different thing: raising one pool
/// against a fixed other walks the *operating point* up the curve, and the curve is deliberately
/// least sensitive at its ends and most sensitive in the middle — a world pinned at pH 2 does not
/// move much because there is nowhere for it to go, which is saturation and not buffering.
///
/// Carbonate hardness in a tank is a capacity measured with the system *at* its pH, and the
/// analogue here is the total carbon chemistry at a fixed ratio. That is what this varies.
#[test]
fn carbonate_buffers_the_swing() {
    let insult = q10(200);
    let mut swings = Vec::new();
    for pool in [q10(50), q10(100), q10(200), q10(400), q10(800)] {
        // At the operating point: equal pools, which reads neutral.
        assert_eq!(ph_of(pool, pool), PH_NEUTRAL);
        swings.push((pool, PH_NEUTRAL - ph_of(pool, pool + insult)));
    }
    for pair in swings.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        assert!(
            b.1 < a.1,
            "the same CO2 moved pH by {} in water carrying {} of buffer and by {} in water \
             carrying {} — more buffer must mean less swing, or this is not a buffer",
            a.1,
            a.0,
            b.1,
            b.0
        );
    }
    assert!(
        swings[0].1 > 4 * swings[4].1,
        "sixteen times the buffer barely changed the swing: {swings:?}"
    );
}

/// And the other reading of "more carbonate", which is a different question with a different and
/// also correct answer: it makes the water *more alkaline*, not more stable.
///
/// Kept as a test because the two are easy to confuse and confusing them is how the buffer test
/// above came to be written backwards the first time.
#[test]
fn carbonate_against_fixed_acid_raises_the_ph() {
    let mut last = -1;
    for buffer in [q10(50), q10(100), q10(200), q10(400), q10(800)] {
        let ph = ph_of(buffer, q10(400));
        assert!(ph > last, "more base did not mean a higher pH, at {buffer}");
        last = ph;
    }
    assert!(ph_of(q10(50), q10(400)) < PH_NEUTRAL, "acid water read as neutral");
    assert!(ph_of(q10(800), q10(400)) > PH_NEUTRAL, "alkaline water read as neutral");
}

/// **A crowd acidifies its own water and a lit mat sweetens it**, from the existing pathways alone.
///
/// The two halves of `2 CO₂ + light <-> sugar + O₂`, read as pH. Nothing in the engine was changed
/// to make this happen; §7 measured the CO₂ pool nearly emptied by a population and refilled as
/// respiration caught up, and this is that measurement with a different instrument on it.
#[test]
fn photosynthesis_sweetens_the_water_and_respiration_sours_it() {
    let mean_ph = |world: &World| -> i64 {
        let s = world.substrate();
        let mut total = 0i64;
        for y in 0..s.height() as i32 {
            for x in 0..s.width() as i32 {
                total += i64::from(s.ph_at(x, y));
            }
        }
        total / i64::from(s.len() as i64)
    };

    // The same slide twice, differing only in whether the CO₂ pool is being drawn down or filled.
    // Done on the substrate directly rather than by growing two populations, because what is
    // under test is that the *reading* follows the chemistry — the pathways moving that chemistry
    // are `tests/m2_metabolism.rs`'s business and are not re-tested here.
    let mut world = World::new(slide()).expect("world");
    let start = mean_ph(&world);
    assert_eq!(start, i64::from(PH_NEUTRAL), "a matched slide is not neutral");

    let mut respiring = World::new(slide()).expect("world");
    for y in 0..24i32 {
        for x in 0..24i32 {
            let here = respiring.substrate().chem_at(CARBON_DIOXIDE, x, y);
            respiring
                .substrate_mut()
                .set_chem(CARBON_DIOXIDE, x, y, here + q10(200));
        }
    }
    assert!(
        mean_ph(&respiring) < start,
        "respiration's waste did not acidify the water"
    );

    for y in 0..24i32 {
        for x in 0..24i32 {
            let here = world.substrate().chem_at(CARBON_DIOXIDE, x, y);
            world
                .substrate_mut()
                .set_chem(CARBON_DIOXIDE, x, y, here - q10(200));
        }
    }
    assert!(
        mean_ph(&world) > start,
        "photosynthesis eating the CO2 did not sweeten the water"
    );
}

/// A blocked square reads neutral, because rock has no pH.
#[test]
fn rock_has_no_ph() {
    let mut world = World::new(slide()).expect("world");
    world.set_barrier(6, 6, true);
    assert_eq!(world.substrate().ph_at(6, 6), PH_NEUTRAL);
}

/// Both pools are matter and conserve like everything else, over a running world.
#[test]
fn the_buffer_is_matter_and_conserves() {
    let mut world = World::new(slide()).expect("world");
    let before = world.total_matter();
    world.run(2_000);
    let after = world.total_matter();
    for c in [CARBONATE, CALCIUM] {
        assert_eq!(
            after[c], before[c],
            "chemical {c} went from {} to {} in two thousand ticks",
            before[c], after[c]
        );
    }
    world.check_matter().expect("the carbonate system must conserve");
}

/// The buffer moves with the water, which is what makes it reach the square being acidified.
///
/// A pool that cannot get to where the insult is buffers nothing there — so unlike phosphate,
/// which is deliberately immobile, carbonate is the most mobile of the four minerals on both axes.
#[test]
fn the_buffer_is_well_mixed() {
    let mut scenario = slide();
    scenario.seeding.retain(|s| !matches!(s, Seeding::Uniform { chemical, .. } if *chemical == CARBONATE));
    scenario.seeding.push(Seeding::Spike {
        chemical: CARBONATE,
        x: 12,
        y: 12,
        amount: q10(4000),
    });
    let mut world = World::new(scenario).expect("world");
    let reach = |w: &World| {
        w.substrate()
            .chem_plane(CARBONATE)
            .iter()
            .filter(|v| **v > Q10_ONE)
            .count()
    };
    assert!(reach(&world) <= 1, "the spike was not a spike");
    world.run(400);
    assert!(
        reach(&world) > 100,
        "the buffer reached only {} squares in four hundred ticks; a buffer that cannot travel \
         to the acid is not buffering it",
        reach(&world)
    );
}

/// **Calcite forms in sweet water and dissolves in sour**, and conserves both ways.
///
/// The pair is not governed by the per-mineral solubility law — their `saturation` entries are
/// zero so that loop skips them — but by a product and a pH together. A function of three
/// quantities at a square is a mechanism, not a table edit (§8's precedent, from denitrification).
#[test]
fn calcite_precipitates_above_the_line_and_dissolves_below_it() {
    let solid_at = |w: &World, x: i32, y: i32| {
        let s = w.substrate();
        s.solid_at(mm_core::chem::solid_slot(CALCIUM).unwrap(), x, y)
            + s.solid_at(mm_core::chem::solid_slot(CARBONATE).unwrap(), x, y)
    };

    // Alkaline: carbonate over CO₂, and both pools well over the line.
    let mut sweet = World::new(slide()).expect("world");
    for y in 0..24i32 {
        for x in 0..24i32 {
            sweet.substrate_mut().set_chem(CARBONATE, x, y, q10(600));
            sweet.substrate_mut().set_chem(CALCIUM, x, y, q10(600));
            sweet.substrate_mut().set_chem(CARBON_DIOXIDE, x, y, q10(100));
        }
    }
    sweet.adopt_current_contents_as_baseline();
    assert!(
        sweet.substrate().ph_at(12, 12) > i32::from(7) * Q10_ONE,
        "the sweet fixture is not alkaline"
    );
    sweet.run(400);
    assert!(
        solid_at(&sweet, 12, 12) > 0,
        "alkaline water well over the line laid down no reef"
    );
    sweet.check_matter().expect("precipitation must conserve");

    // Sour: the same pools, with the CO₂ of a crowd respiring into them.
    let mut sour = World::new(slide()).expect("world");
    let k = mm_core::chem::solid_slot(CALCIUM).unwrap();
    let kc = mm_core::chem::solid_slot(CARBONATE).unwrap();
    for y in 0..24i32 {
        for x in 0..24i32 {
            sour.substrate_mut().add_solid(k, x, y, q10(60));
            sour.substrate_mut().add_solid(kc, x, y, q10(60));
            sour.substrate_mut().set_chem(CARBON_DIOXIDE, x, y, q10(900));
            sour.substrate_mut().set_chem(CARBONATE, x, y, q10(10));
        }
    }
    sour.adopt_current_contents_as_baseline();
    let before = solid_at(&sour, 12, 12);
    sour.run(400);
    let after = solid_at(&sour, 12, 12);
    assert!(
        after < before,
        "a reef standing in sour water did not dissolve: {before} -> {after}"
    );
    sour.check_matter().expect("dissolution must conserve");
}

/// **A reef dissolving raises the buffer.** The negative feedback, end to end.
///
/// This is the behaviour the whole section is for and the reason the reef is worth having: acid
/// dissolves the substrate, the substrate returns its carbonate to the water, the buffer rises,
/// and the water resists further acidification. Nobody wrote it as a feedback — it falls out of
/// the reef being made of the same stuff the buffer is.
#[test]
fn a_reef_dissolving_raises_the_buffer() {
    let mut world = World::new(slide()).expect("world");
    let k = mm_core::chem::solid_slot(CALCIUM).unwrap();
    let kc = mm_core::chem::solid_slot(CARBONATE).unwrap();
    for y in 0..24i32 {
        for x in 0..24i32 {
            world.substrate_mut().add_solid(k, x, y, q10(80));
            world.substrate_mut().add_solid(kc, x, y, q10(80));
            // Sour water with almost no buffer left in it, which is the state a crowd creates.
            world.substrate_mut().set_chem(CARBON_DIOXIDE, x, y, q10(900));
            world.substrate_mut().set_chem(CARBONATE, x, y, q10(5));
        }
    }
    world.adopt_current_contents_as_baseline();

    let buffer = |w: &World| -> i64 {
        w.substrate()
            .chem_plane(CARBONATE)
            .iter()
            .map(|v| i64::from(*v))
            .sum()
    };
    let ph = |w: &World| w.substrate().ph_at(12, 12);
    let (buffer_before, ph_before) = (buffer(&world), ph(&world));

    world.run(1_000);

    assert!(
        buffer(&world) > buffer_before,
        "the reef stood in acid and gave nothing back: buffer {buffer_before} -> {}",
        buffer(&world)
    );
    assert!(
        ph(&world) > ph_before,
        "the buffer rose and the water did not sweeten: pH {ph_before} -> {}",
        ph(&world)
    );
    world.check_matter().expect("the feedback must conserve");
}

/// A reef thick enough is a wall, and one worn past the line opens again — the same derived law
/// every other mineral obeys, reached through the calcite arm rather than the per-plane one.
#[test]
fn a_calcite_reef_is_a_wall_and_wears_back_to_water() {
    let mut world = World::new(slide()).expect("world");
    let dose = world.rock_dose();
    world.set_rock(&[(12, 12)], CALCIUM, dose / 2);
    world.set_rock(&[(12, 12)], CARBONATE, dose / 2);
    assert!(
        world.substrate().blocked()[world.substrate().index(12, 12)],
        "a reef over the wall line is not a wall"
    );

    // Sour water all round it, and nothing to hold the mineral in solution.
    for y in 0..24i32 {
        for x in 0..24i32 {
            world.substrate_mut().set_chem(CARBON_DIOXIDE, x, y, q10(900));
            world.substrate_mut().set_chem(CARBONATE, x, y, 0);
            world.substrate_mut().set_chem(CALCIUM, x, y, 0);
        }
    }
    world.adopt_current_contents_as_baseline();
    world.run(20_000);

    assert!(
        !world.substrate().blocked()[world.substrate().index(12, 12)],
        "twenty thousand ticks in acid and the reef is still a wall"
    );
    world.check_matter().expect("wearing away must conserve");
}

/// **A reef wears because of the acid, and not only because the water is thirsty.**
///
/// The two dissolution terms add, and that made it possible for the acid half to be inert without
/// anything noticing: a wall in bare water dissolves on thirst alone, so a test that put a reef in
/// acid *and* bare water passes whether or not the pH is being read. It did, for one commit — the
/// arm read `ph_at` of the blocked square itself, which holds nothing and therefore reads neutral,
/// so a wall could never see the acid eating it.
///
/// This holds the water saturated in both minerals, so thirst is zero and the acid is the only
/// thing left that can move the reef. The pair, at the same saturation, differing only in pH.
#[test]
fn a_reef_in_saturated_but_sour_water_still_wears() {
    let build = |co2: i32| {
        let mut world = World::new(slide()).expect("world");
        let dose = world.rock_dose();
        world.set_rock(&[(12, 12)], CALCIUM, dose / 2);
        world.set_rock(&[(12, 12)], CARBONATE, dose / 2);
        // At the calcite line in both minerals, so there is no undersaturation anywhere and the
        // thirst term is nought. Only the pH differs between the two worlds.
        let line = world.biology().minerals.calcite_saturation;
        for y in 0..24i32 {
            for x in 0..24i32 {
                world.substrate_mut().set_chem(CALCIUM, x, y, line);
                world.substrate_mut().set_chem(CARBONATE, x, y, line);
                world.substrate_mut().set_chem(CARBON_DIOXIDE, x, y, co2);
            }
        }
        world.adopt_current_contents_as_baseline();
        world
    };

    // Sour enough that the water is well under the calcite line, and sweet enough to be over it.
    let mut sour = build(q10(4000));
    let mut sweet = build(q10(1));
    assert!(
        sour.substrate().ph_at(11, 12) < sweet.substrate().ph_at(11, 12),
        "the two fixtures are not at different pH"
    );

    let held = |w: &World| w.substrate().solid_total_at(12, 12);
    let (sour_before, sweet_before) = (held(&sour), held(&sweet));
    sour.run(4_000);
    sweet.run(4_000);

    assert!(
        held(&sour) < sour_before,
        "a reef in saturated but sour water did not wear: {sour_before} -> {}. The acid term is \
         inert and the thirst term was covering for it",
        held(&sour)
    );
    assert!(
        held(&sweet) >= sweet_before,
        "a reef in saturated, sweet water wore away anyway: {sweet_before} -> {}",
        held(&sweet)
    );
    sour.check_matter().expect("wearing must conserve");
    sweet.check_matter().expect("standing must conserve");
}
