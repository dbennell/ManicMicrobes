//! M10.3 acceptance tests — several ways to make a living.
//!
//! > A world offers several metabolic pathways, not one. An organelle chooses which reaction it
//! > runs by its `control[1]`, so a mitochondrion can only burn what it is set to burn — and a
//! > lineage must either make that substrate itself or eat something that does.
//!
//! The reasoning is in `docs/CHEMISTRY.md`; the normative statement is SPEC §7.2. What is
//! asserted here is that the mechanism works *and does not cost the invariants anything*, which
//! is the part most likely to be subtly wrong: every pathway is a fresh set of ledger
//! conversions, and a reaction that fails to report itself is indistinguishable from a
//! conservation bug.

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::light::CurrentField;
use mm_core::organelle::{MetabolicChemistry, PATHWAY_COUNT};
use mm_core::{LightRegime, Organelle, OrganelleType, Scenario, Seeding, World};

/// A lit slide holding some of everything the four default pathways run on.
fn petri(seed: u64) -> Scenario {
    Scenario {
        name: "pathways".to_string(),
        seed,
        width: 48,
        height: 48,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        current: CurrentField::Still,
        seeding: vec![
            // Waste and oxidant, which every pathway shares.
            Seeding::Uniform {
                chemical: 11,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 14,
                per_square: q10(400),
            },
            // Structural.
            Seeding::Uniform {
                chemical: 4,
                per_square: q10(400),
            },
            // The three substrates the default set names: sugar, lipid, sulphide.
            Seeding::Uniform {
                chemical: 8,
                per_square: q10(200),
            },
            Seeding::Uniform {
                chemical: 9,
                per_square: q10(200),
            },
            Seeding::Uniform {
                chemical: 10,
                per_square: q10(200),
            },
        ],
        ..Scenario::default()
    }
}

/// Put a cell on the slide with a mitochondrion — and optionally a chloroplast — set to
/// `pathway`, and a cytoplasm stocked with what that pathway runs on.
///
/// The genome is one `NOP`. These cells never `EAT`, so they live on what they are given and
/// then die, which is fine and deliberate: what is under test is the metabolic loop, and a real
/// genome would put its own behaviour between the mechanism and the measurement.
fn seed_on_pathway(
    world: &mut World,
    at: (i32, i32),
    pathway: i16,
    with_chloroplast: bool,
) -> CellId {
    let genome = world
        .genomes()
        .intern(vec![0x00])
        .expect("a one-byte genome interns");
    let id = world.spawn_cell(CellSeed {
        x: pos(at.0),
        y: pos(at.1),
        mass: q10(30),
        energy: q10(400),
        membrane: 24,
        key: 11,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    let chemistry = world.biology().metabolism.catalogue.metabolism;
    let p = *chemistry.pathway(pathway);
    let structural = chemistry.structural;
    if let Some(i) = world.cells_mut().index(id) {
        let cells = world.cells_mut();
        cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);

        let mut mito = Organelle::finished(OrganelleType::Mitochondrion, 50);
        mito.control[1] = pathway;
        cells.slots_mut(i)[2] = mito;

        if with_chloroplast {
            let mut chloro = Organelle::finished(OrganelleType::Chloroplast, 60);
            chloro.control[1] = pathway;
            cells.slots_mut(i)[3] = chloro;
        }

        cells.interior_mut(i)[structural] = q10(200);
        cells.interior_mut(i)[p.substrate] = q10(60);
        cells.interior_mut(i)[p.oxidant] = q10(60);
        cells.interior_mut(i)[p.waste] = q10(60);
    }
    id
}

#[test]
fn matter_stays_exact_with_every_pathway_running_at_once() {
    // I4, under the new mechanism. Each pathway is its own set of ledger conversions, and a
    // reaction that moves matter between species without saying so is indistinguishable from a
    // leak — which is the whole reason `check_matter` exists rather than a total-only check.
    let mut world = World::new(petri(3)).unwrap();
    world.adopt_current_contents_as_baseline();
    for n in 0..PATHWAY_COUNT {
        for k in 0..4i32 {
            seed_on_pathway(&mut world, (6 + n as i32 * 10, 6 + k * 10), n as i16, true);
        }
    }
    world.adopt_current_contents_as_baseline();

    let grand_before: i64 = world.total_matter().iter().sum();
    let (mut fixed, mut burned) = (0i64, 0i64);
    for tick in 0..1_500u64 {
        world.step();
        fixed += world.report().metabolism.fixed;
        burned += world.report().metabolism.burned;
        if tick % 50 == 0 {
            world
                .check_matter()
                .unwrap_or_else(|e| panic!("at tick {tick}: {e}"));
            assert_eq!(
                world.total_matter().iter().sum::<i64>(),
                grand_before,
                "total matter moved by tick {tick}"
            );
            assert!(!world.substrate().any_negative());
        }
    }
    // These cells cannot `EAT`, so they run out and die; that is not what is under test and
    // corpses conserve matter as well as cells do. What has to be true is that the reactions
    // actually ran, or the conservation above was checked over a world where nothing happened.
    assert!(fixed > 0, "no pathway photosynthesised");
    assert!(burned > 0, "no pathway respired");
}

#[test]
fn energy_is_accounted_exactly_with_several_substrates_in_play() {
    // I5, and the piece most likely to be wrong: `recompute_stored` used to read *the*
    // substrate and now has to sum over every distinct one, counting a shared substrate once.
    // Off by one substrate and the ledger and the world disagree by a constant, forever.
    let mut world = World::new(petri(9)).unwrap();
    world.adopt_current_contents_as_baseline();
    for n in 0..PATHWAY_COUNT {
        seed_on_pathway(&mut world, (8 + n as i32 * 10, 20), n as i16, true);
    }
    world.adopt_current_contents_as_baseline();

    for tick in 0..800u64 {
        world.step();
        if tick % 25 == 0 {
            world
                .check_energy()
                .unwrap_or_else(|e| panic!("at tick {tick}: {e}"));
        }
    }
}

#[test]
fn a_lineage_can_only_burn_what_it_is_set_to_burn() {
    // The mechanism, at world level: cells identical in every way except which reaction their
    // mitochondrion runs, each given a cytoplasm full of sugar and nothing else.
    //
    // Measured as *substrate burned*, not as survival. Survival is the wrong instrument here
    // and measuring it was a mistake worth recording: a cell that respires with no genome to
    // excrete peroxide poisons itself dead inside two hundred ticks, so the one that could eat
    // died and the one that could not sat there intact — a result that looks exactly like the
    // mechanism working backwards. What is being claimed is about the reaction, so the reaction
    // is what to count.
    //
    // No chloroplast, deliberately. A chloroplast set to lipid would make its own lipid and
    // the cell would eventually burn that — the mechanism working, and exactly what would stop
    // this from measuring the mitochondrion.
    let burned_on = |pathway: i16| -> i64 {
        let mut world = World::new(petri(11)).unwrap();
        let id = seed_on_pathway(&mut world, (24, 24), pathway, false);
        // Wipe the stock it was given and hand it sugar instead, whatever it is set to burn.
        // Pathway 0 can use that; a pathway naming a different substrate cannot.
        let chemistry = world.biology().metabolism.catalogue.metabolism;
        let sugar = chemistry.primary().substrate;
        let oxidant = chemistry.primary().oxidant;
        if let Some(i) = world.cells_mut().index(id) {
            world.cells_mut().interior_mut(i).fill(0);
            world.cells_mut().interior_mut(i)[chemistry.structural] = q10(200);
            world.cells_mut().interior_mut(i)[sugar] = q10(400);
            world.cells_mut().interior_mut(i)[oxidant] = q10(400);
        }
        world.adopt_current_contents_as_baseline();
        let mut burned = 0i64;
        for _ in 0..200 {
            world.step();
            burned += world.report().metabolism.burned;
        }
        burned
    };

    let on_sugar = burned_on(0);
    assert!(on_sugar > 0, "the primary pathway burned nothing at all");
    for other in 1..PATHWAY_COUNT as i16 {
        // Pathway 3 is sugar again by design, so it burns sugar too — that is the duplicate
        // slot doing its job, and asserting otherwise would be asserting a coincidence.
        let chemistry = MetabolicChemistry::default();
        if chemistry.pathway(other).substrate == chemistry.primary().substrate {
            continue;
        }
        assert_eq!(
            burned_on(other),
            0,
            "a mitochondrion set to pathway {other} burned sugar, which is not its substrate"
        );
    }
}

#[test]
fn a_world_that_says_nothing_about_pathways_behaves_as_it_always_did() {
    // Backwards compatibility as an acceptance test rather than a hope. Pathway 0 is the
    // reaction the engine ran from M2 to M9, a fresh organelle's `control[1]` is zero, and
    // every genome written before M10.3 leaves it there.
    let chemistry = MetabolicChemistry::default();
    assert_eq!(chemistry.pathway(0), chemistry.primary());
    assert_eq!(chemistry.primary().substrate, 8, "sugar");
    assert_eq!(chemistry.primary().oxidant, 14, "oxygen");
    assert_eq!(chemistry.primary().waste, 11, "carbon dioxide");
    assert_eq!(chemistry.primary().reactive, 13, "peroxide");
    assert_eq!(chemistry.structural, 4, "carbon");

    let mut world = World::new(petri(5)).unwrap();
    seed_on_pathway(&mut world, (24, 24), 0, true);
    world.adopt_current_contents_as_baseline();
    let (mut fixed, mut burned) = (0i64, 0i64);
    for _ in 0..200 {
        world.step();
        fixed += world.report().metabolism.fixed;
        burned += world.report().metabolism.burned;
    }
    assert!(
        fixed > 0,
        "an unconfigured chloroplast photosynthesised nothing"
    );
    assert!(burned > 0, "an unconfigured mitochondrion respired nothing");
}

#[test]
fn which_pathway_an_organelle_runs_survives_a_snapshot() {
    // Hard rule 7. The selector is an organelle's `control[1]`, which the snapshot has always
    // carried — but "has always carried" is exactly the kind of claim that turns out to be
    // false about the field nobody was using yet.
    let mut world = World::new(petri(13)).unwrap();
    for n in 0..PATHWAY_COUNT {
        seed_on_pathway(&mut world, (8 + n as i32 * 10, 20), n as i16, true);
    }
    world.adopt_current_contents_as_baseline();
    world.run(100);

    let bytes = mm_core::snapshot::Snapshot::write(&world).expect("writes");
    let restored = mm_core::snapshot::Snapshot::read(&bytes).expect("reads");
    assert_eq!(restored.state_hash(), world.state_hash());

    let pathways = |w: &World| -> Vec<i16> {
        let mut out = Vec::new();
        for i in 0..w.cells().capacity() {
            if !w.cells().occupied(i) {
                continue;
            }
            for o in w.cells().slots(i) {
                if o.kind == OrganelleType::Mitochondrion {
                    out.push(o.control[1]);
                }
            }
        }
        out.sort_unstable();
        out
    };
    let before = pathways(&world);
    assert!(
        before
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 1,
        "the world under test only used one pathway, so this proves nothing"
    );
    assert_eq!(before, pathways(&restored), "pathway choices were lost");
}

#[test]
fn a_world_offering_an_unclosed_pathway_is_refused_before_it_runs() {
    // A pathway whose mitochondrion burns what its chloroplast cannot rebuild is a world that
    // dies of arithmetic, however good the cells are. Worth refusing at construction rather
    // than discovering after a million ticks — and worth refusing in *any* slot, since an
    // unclosed reaction three pathways down is exactly as fatal and much harder to notice.
    for slot in 0..PATHWAY_COUNT {
        let mut chemistry = MetabolicChemistry::default();
        chemistry.pathways[slot].waste = chemistry.pathways[slot].substrate;
        assert!(
            !chemistry.closes(),
            "an unclosed loop in pathway {slot} was accepted"
        );
    }
    assert!(MetabolicChemistry::default().closes());
}
