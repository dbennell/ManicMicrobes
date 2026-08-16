//! What a cell shows the world, and what it keeps to itself (SPEC §8.2, ISA 4).
//!
//! There are now two marks on a cell and they are opposites, which is the whole design:
//!
//! * The **receptor key** is private. `SETKEY` sets it, `JOIN` discriminates on it, and nothing
//!   in the ISA can read another cell's — `junction.rs` explains why leaking even the *distance*
//!   to a key makes it hill-climbable in about seven probes and parasitism trivial. It is the
//!   only thing standing between a colony and anything that fancies joining it.
//! * The **badge** is public. Anything touching can read it, nothing in the engine reads it at
//!   all, and it costs nothing to wear or to forge.
//!
//! **The engine must never know what a friend is.** It reports that the thing you are touching
//! is wearing 4211 and says nothing about what that means. Whether 4211 is kin, prey or a liar
//! is a genome's problem — which is what leaves room for mimicry, and for the arms race that
//! makes recognition worth having in the first place.

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::light::CurrentField;
use mm_core::organelle::{Organelle, OrganelleType};
use mm_core::{LightRegime, Scenario, World};

fn slide() -> Scenario {
    Scenario {
        name: "badges".to_string(),
        seed: 0xBAD9,
        width: 32,
        height: 32,
        light: LightRegime::Uniform { intensity: 0 },
        current: CurrentField::Still,
        jitter: 0,
        ..Scenario::default()
    }
}

fn put(world: &mut World, src: &str, x: i32, y: i32, badge: u16) -> CellId {
    let bytes = mm_asm::assemble(src).expect("assembles").bytes;
    let g = world.genomes().intern(bytes).expect("intern");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(y),
        mass: q10(30),
        energy: q10(400),
        membrane: 24,
        key: 11,
        badge,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome: g,
    });
    if let Some(i) = world.cells_mut().index(id) {
        let cells = world.cells_mut();
        cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
        cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::TouchSensor, 40);
        cells.interior_mut(i)[4] = q10(200);
    }
    world.adopt_current_contents_as_baseline();
    id
}

/// Store the nearest neighbour's badge to register 0, every tick, and nothing else.
const WATCHER: &str = "
        EXPRESS #look
        HALT
        GENE    #look
        IMM     3               ; touch reading 3: what the neighbour is wearing
        IMM     2               ; slot 2, the touch sensor
        OGET
        ZERO
        RSTORE
        RET
";

fn register(world: &World, id: CellId, r: usize) -> i16 {
    world
        .cells()
        .index(id)
        .map_or(0, |i| world.cells().vm[i].regs[r])
}

#[test]
fn a_cell_can_read_what_the_thing_it_is_touching_is_wearing() {
    let mut world = World::new(slide()).expect("world");
    let watcher = put(&mut world, WATCHER, 16, 16, 100);
    put(&mut world, WATCHER, 16, 17, 4211);
    world.run(40);
    assert_eq!(
        register(&world, watcher, 0),
        4211,
        "it could not read its neighbour's badge"
    );
}

#[test]
fn a_cell_touching_nobody_reads_nothing() {
    // Zero rather than the last value it saw, or a cell would go on recognising a neighbour
    // that had died and drifted away.
    let mut world = World::new(slide()).expect("world");
    let lonely = put(&mut world, WATCHER, 4, 4, 100);
    world.run(40);
    assert_eq!(register(&world, lonely, 0), 0);
}

#[test]
fn a_genome_can_change_what_it_shows() {
    let mut world = World::new(slide()).expect("world");
    let dresser = put(
        &mut world,
        "
        EXPRESS #dress
        HALT
        GENE    #dress
        IMM     3               ; 777 = 3<<8 | 9. `IMM` takes a byte, so a badge past 255 has
        IMM     8               ; to be composed — which is the point rather than a nuisance:
        SHL                     ; a fifteen-bit badge is not somewhere a single byte mutation
        IMM     9               ; can land, so a lineage's colours are stickier than its
        OR                      ; immediates.
        SETBADGE
        RET
",
        16,
        16,
        1,
    );
    world.run(20);
    let i = world.cells().index(dresser).expect("alive");
    assert_eq!(world.cells().badge[i], 777);
}

#[test]
fn a_cell_can_read_its_own_badge_back() {
    // Which is what lets recognition survive the badge changing: a genome comparing a
    // neighbour's badge to a hard-coded number stops knowing its own kin the moment a mutation
    // moves it, so no lineage could ever drift its colours. Comparing neighbour to self means a
    // lineage that changes what it wears changes what it answers to in the same stroke.
    let mut world = World::new(slide()).expect("world");
    let id = put(
        &mut world,
        "
        EXPRESS #look
        HALT
        GENE    #look
        IMM     24              ; membrane reading 24: my own badge. **It has moved twice** —
                                ; 21 originally, 22 when dinitrogen landed at ISA 11, 24 when
                                ; calcium and carbonate landed at ISA 12. The membrane's scalars
                                ; sit after the chemical readings, so widening the table shifts
                                ; every one of them, and this assertion failing is the version
                                ; stamp doing its job rather than a regression
        ZERO                    ; slot 0, the membrane
        OGET
        ZERO
        RSTORE
        RET
",
        16,
        16,
        1234,
    );
    world.run(20);
    assert_eq!(register(&world, id, 0), 1234);
}

/// The badge sits after the sixteen chemical readings, so adding it renumbered nothing.
#[test]
fn adding_a_reading_did_not_move_the_chemical_ones() {
    let mut world = World::new(slide()).expect("world");
    let id = put(
        &mut world,
        "
        EXPRESS #look
        HALT
        GENE    #look
        IMM     9               ; membrane reading 9: internal chemical 4
        ZERO
        OGET
        ZERO
        RSTORE
        RET
",
        16,
        16,
        1,
    );
    world.run(20);
    assert_eq!(
        register(&world, id, 0),
        200,
        "chemical 4 was seeded at 200 units and reading 9 should still be it"
    );
}

#[test]
fn a_daughter_is_born_already_wearing_her_mothers_colours() {
    // The property the whole mechanism rests on. A newborn is at her most vulnerable in the
    // window before her own first expression cycle has run — so if she had to *set* her badge
    // she would be unrecognisable during exactly the ticks recognition is for.
    let genome = {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm");
        let src = std::fs::read_to_string(path).expect("genome");
        mm_asm::assemble(&src).expect("assembles").bytes
    };
    let mut world = World::new(Scenario {
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        seeding: vec![
            mm_core::Seeding::Uniform {
                chemical: 11,
                per_square: q10(400),
            },
            mm_core::Seeding::Uniform {
                chemical: 14,
                per_square: q10(400),
            },
            mm_core::Seeding::Uniform {
                chemical: 4,
                per_square: q10(400),
            },
            // The minerals every recipe in the catalogue is costed in, at the
            // Redfield proportion of the carbon above. Nothing produces them.
            mm_core::Seeding::Uniform {
                chemical: 5,
                per_square: (q10(400)) * 16 / 106,
            },
            mm_core::Seeding::Uniform {
                chemical: 6,
                per_square: (q10(400)) / 53,
            },
        ],
        ..slide()
    })
    .expect("world");

    let g = world.genomes().intern(genome).expect("intern");
    let mother = world.spawn_cell(CellSeed {
        x: pos(16),
        y: pos(16),
        mass: q10(30),
        energy: q10(400),
        membrane: 24,
        key: 11,
        badge: 9001,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome: g,
    });
    if let Some(i) = world.cells_mut().index(mother) {
        let cells = world.cells_mut();
        cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
        cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
        cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
        cells.interior_mut(i)[4] = q10(200);
        cells.interior_mut(i)[11] = q10(40);
        cells.interior_mut(i)[14] = q10(40);
    }
    world.adopt_current_contents_as_baseline();

    world.run(1200);
    assert!(
        world.cells().len() > 1,
        "she never divided, so this proves nothing"
    );
    for i in world.cells().iter() {
        assert_eq!(
            world.cells().badge[i],
            9001,
            "a descendant was born bare-faced"
        );
    }
}

#[test]
fn nothing_in_the_engine_reads_a_badge() {
    // Inert, and it has to stay that way. Two identical worlds differing only in what everybody
    // is wearing must run to the same state — the moment any mechanism branches on a badge, the
    // engine has started deciding what a friend is.
    let genome = {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm");
        let src = std::fs::read_to_string(path).expect("genome");
        mm_asm::assemble(&src).expect("assembles").bytes
    };
    let hashes: Vec<_> = [7u16, 30000]
        .into_iter()
        .map(|badge| {
            let mut world = World::new(Scenario {
                light: LightRegime::Uniform {
                    intensity: mm_core::Q10_ONE,
                },
                seeding: vec![
                    mm_core::Seeding::Uniform {
                        chemical: 11,
                        per_square: q10(400),
                    },
                    mm_core::Seeding::Uniform {
                        chemical: 14,
                        per_square: q10(400),
                    },
                    mm_core::Seeding::Uniform {
                        chemical: 4,
                        per_square: q10(400),
                    },
                    // The minerals every recipe in the catalogue is costed in, at the
                    // Redfield proportion of the carbon above. Nothing produces them.
                    mm_core::Seeding::Uniform {
                        chemical: 5,
                        per_square: (q10(400)) * 16 / 106,
                    },
                    mm_core::Seeding::Uniform {
                        chemical: 6,
                        per_square: (q10(400)) / 53,
                    },
                ],
                ..slide()
            })
            .expect("world");
            for k in 0..4 {
                let g = world.genomes().intern(genome.clone()).expect("intern");
                let id = world.spawn_cell(CellSeed {
                    x: pos(8 + (k % 2) * 12),
                    y: pos(8 + (k / 2) * 12),
                    mass: q10(30),
                    energy: q10(400),
                    membrane: 24,
                    key: 11,
                    badge,
                    species: 0,
                    parent: CellId::NONE,
                    birth_tick: 0,
                    genome: g,
                });
                if let Some(i) = world.cells_mut().index(id) {
                    let cells = world.cells_mut();
                    cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
                    cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
                    cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
                    cells.interior_mut(i)[4] = q10(200);
                    cells.interior_mut(i)[11] = q10(40);
                    cells.interior_mut(i)[14] = q10(40);
                }
            }
            world.adopt_current_contents_as_baseline();
            world.run(800);
            (world.cells().len(), world.total_matter())
        })
        .collect();
    assert_eq!(
        hashes[0], hashes[1],
        "two worlds differing only in what everyone is wearing came out different, so \
         something in the engine is reading the badge"
    );
}

#[test]
fn a_badge_survives_being_put_away_and_taken_out_again() {
    let mut world = World::new(slide()).expect("world");
    let id = put(&mut world, WATCHER, 16, 16, 21000);
    world.run(30);

    let bytes = mm_core::snapshot::Snapshot::write(&world).expect("snapshot");
    let back = mm_core::snapshot::Snapshot::read(&bytes).expect("restore");
    let i = back.cells().index(id).expect("still there");
    assert_eq!(back.cells().badge[i], 21000);
    assert_eq!(back.state_hash(), world.state_hash());
}
