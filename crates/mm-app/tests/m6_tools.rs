//! M6 acceptance 3 — portability — and the laboratory tools end to end.
//!
//! > A genome exported on one machine loads and behaves identically on another.
//!
//! "Another machine" cannot be reached from a test suite that runs on one. What *can* be
//! tested is everything that would make a genome behave differently somewhere else, and the
//! list is short because the hard rules already close most of it:
//!
//! * the file survives the round trip byte for byte, including every byte value;
//! * it survives the things a transport does to text — line endings, trailing whitespace,
//!   a stray blank line, being pasted into something that re-wraps it;
//! * a genome loaded from a file behaves identically to the one it was exported from, checked
//!   by running both and comparing state hashes rather than by comparing bytes;
//! * a genome from a different ISA is refused rather than quietly meaning something else.
//!
//! What is left for a second machine is float behaviour, endianness and iteration order, and
//! hard rules 2, 6 and the fixed-point arithmetic remove all three from `mm-core` by
//! construction. A second machine is a CI job, not a test.

use mm_app::editor::Editor;
use mm_app::tools;
use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::genome_file::{GenomeFile, GenomeFileError};
use mm_core::light::CurrentField;
use mm_core::{LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding, World};

fn ancestor() -> Vec<u8> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../genomes/ancestor.mm"
    ))
    .expect("the ancestor is in the repository");
    mm_asm::assemble(&src).expect("assembles").bytes
}

fn petri(size: u32) -> Scenario {
    Scenario {
        name: "petri".to_string(),
        seed: 1,
        width: size,
        height: size,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        current: CurrentField::Still,
        seeding: vec![
            Seeding::Uniform {
                chemical: 11,
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
            // The minerals every recipe in the catalogue is costed in, at the
            // Redfield proportion of the carbon above. Nothing produces them.
            Seeding::Uniform {
                chemical: 5,
                per_square: (q10(400)) * 16 / 106,
            },
            Seeding::Uniform {
                chemical: 6,
                per_square: (q10(400)) / 53,
            },
        ],
        ..Scenario::default()
    }
}

/// A world seeded with one genome, set up identically every time.
fn world_running(genome: &[u8]) -> World {
    let mut world = World::new(petri(48)).expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    for k in 0..6u32 {
        let g = world.genomes().intern(genome.to_vec()).expect("genome");
        let id = world.spawn_cell(CellSeed {
            x: pos((6 + (k % 3) * 14) as i32),
            y: pos((6 + (k / 3) * 14) as i32),
            mass: q10(30),
            energy: q10(400),
            membrane: 24,
            key: 11,
            badge: 0,
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
            cells.interior_mut(i)[11] = q10(40);
            cells.interior_mut(i)[14] = q10(40);
        }
    }
    world.adopt_current_contents_as_baseline();
    world
}

#[test]
fn acceptance_an_exported_genome_behaves_identically_when_loaded_back() {
    // The claim in the form that matters: not that the bytes match, but that the *organism*
    // does. Two worlds, one seeded from the original genome and one from the genome after a
    // round trip through a file, run side by side and compared on state hash.
    let original = ancestor();
    let text = GenomeFile::new(original.clone(), "ancestor").to_text();
    let loaded = GenomeFile::from_text(&text).expect("loads").bytes;

    let mut a = world_running(&original);
    let mut b = world_running(&loaded);
    for _ in 0..2_000 {
        a.step();
        b.step();
    }
    assert!(
        !a.cells().is_empty(),
        "the population died; nothing was compared"
    );
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "a genome behaved differently after a round trip through a file"
    );
    assert_eq!(a.cells().len(), b.cells().len());
}

#[test]
fn a_genome_survives_what_a_transport_does_to_text() {
    // The realistic failure: nobody hands over a byte-exact file. It goes through a chat
    // client, an email, a text box that strips trailing spaces and rewrites line endings.
    let original = ancestor();
    let text = GenomeFile::new(original.clone(), "ancestor").to_text();

    let mangled = [
        ("windows line endings", text.replace('\n', "\r\n")),
        ("trailing whitespace", text.replace('\n', "   \n")),
        ("a blank line in the header", text.replace("isa", "\nisa")),
        ("leading indentation", text.replace('\n', "\n  ")),
        ("a trailing newline lost", text.trim_end().to_string()),
    ];
    for (what, altered) in mangled {
        let back = GenomeFile::from_text(&altered)
            .unwrap_or_else(|e| panic!("{what} broke the file: {e}"));
        assert_eq!(back.bytes, original, "{what} changed the genome");
    }
}

#[test]
fn every_byte_value_survives_a_round_trip() {
    // A genome is arbitrary bytes — every byte sequence is a legal program (hard rule 3) — so
    // the file has to carry all 256 values, including the ones that are whitespace or control
    // characters in text.
    let all: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    let text = GenomeFile::new(all.clone(), "every-byte").to_text();
    assert_eq!(GenomeFile::from_text(&text).expect("loads").bytes, all);
}

#[test]
fn a_genome_picked_off_a_cell_can_be_shared_and_replanted() {
    // The whole M6 loop: tweezers to editor to file to a fresh slide.
    let mut source = world_running(&ancestor());
    source.run(200);
    let cell = source
        .cells()
        .iter()
        .next()
        .map(|i| source.cells().id_at(i))
        .expect("a living cell");

    // Pick it up.
    let file = tools::copy_genome(&source, cell).expect("a genome");
    let text = file.to_text();

    // Hand it to someone else's editor.
    let mut editor = Editor::new();
    editor.load_genome_file(&text).expect("loads");
    assert!(
        editor.build().is_ok(),
        "a genome from a live cell did not reassemble: {:?}",
        editor.build().errors()
    );
    let bytes = editor.build().bytes().expect("bytes").to_vec();

    // And it is the genome the cell was running.
    let i = source.cells().index(cell).expect("still alive");
    assert_eq!(bytes, source.cells().genome[i].bytes());

    // Plant it on a fresh slide and it lives.
    let mut fresh = world_running(&bytes);
    fresh.run(1_000);
    assert!(
        !fresh.cells().is_empty(),
        "the shared genome died on a fresh slide"
    );
    fresh.check_matter().expect("books balance");
}

#[test]
fn a_transplanted_cell_and_a_reseeded_genome_are_the_same_organism() {
    // Two routes onto a fresh slide — carrying the whole cell with the tweezers, or exporting
    // its genome and seeding from that — must produce a cell running the same program. They
    // are not the same *world*, because a transplant brings the cell's chemistry and organelles
    // with it, so what is compared is the genome each ends up running.
    let mut source = world_running(&ancestor());
    source.run(150);
    let cell = source
        .cells()
        .iter()
        .next()
        .map(|i| source.cells().id_at(i))
        .expect("a cell");

    let mut transplanted = World::new(petri(32)).expect("world");
    let event = tools::transplant(&source, cell, &mut transplanted, 16, 16);
    let tools::ToolEvent::Transplanted { to, .. } = event else {
        panic!("expected a transplant, got {event:?}");
    };

    let exported = tools::copy_genome(&source, cell).expect("genome").bytes;
    let j = transplanted.cells().index(to).expect("alive");
    assert_eq!(
        transplanted.cells().genome[j].bytes(),
        exported.as_slice(),
        "the transplanted cell is running a different genome from the one exported"
    );
}

#[test]
fn a_foreign_isa_genome_is_refused_at_every_door() {
    // Acceptance 4's other half: it is not enough for the parser to refuse it, every route
    // into the simulation has to.
    let text = GenomeFile::new(ancestor(), "alien")
        .to_text()
        .replace(&format!("isa {}", mm_core::isa::ISA_VERSION), "isa 123");

    // The file parser.
    assert!(matches!(
        GenomeFile::from_text(&text),
        Err(GenomeFileError::IsaMismatch { .. })
    ));

    // The editor, which must also leave its buffer alone.
    let mut editor = Editor::with_source("        HALT\n", "mine");
    editor.assemble();
    let before = editor.source().to_string();
    assert!(editor.load_genome_file(&text).is_err());
    assert_eq!(editor.source(), before);
}

#[test]
fn the_tools_never_break_conservation_however_they_are_used() {
    // Every tool, applied in sequence to a running world, with the books checked after each.
    // The tools are the one part of `mm-app` allowed to write to the simulation, so this is
    // where a matter leak would come from.
    let mut world = world_running(&ancestor());
    world.run(120);
    // Summed across chemicals rather than compared per chemical: since M8 a death converts
    // part of the body to carrion, a balanced conversion the ledger accounts for, so the
    // per-species totals move by design where the total cannot. `check_matter` is the strict
    // check and is called after every step below.
    let before: i64 = world.total_matter().iter().sum();
    let total = |w: &mm_core::World| -> i64 { w.total_matter().iter().sum() };

    let ids: Vec<CellId> = world
        .cells()
        .iter()
        .map(|i| world.cells().id_at(i))
        .collect();
    assert!(ids.len() >= 4, "not enough cells to exercise the tools");

    tools::relocate(&mut world, ids[0], 5, 5);
    world.check_matter().expect("after relocate");
    assert_eq!(total(&world), before, "relocate moved matter");

    tools::set_barrier(&mut world, 20, 20, true);
    world.check_matter().expect("after drawing a barrier");
    tools::set_barrier(&mut world, 20, 20, false);
    world.check_matter().expect("after erasing a barrier");

    tools::remove(&mut world, ids[1]);
    world.check_matter().expect("after remove");
    assert_eq!(total(&world), before, "removing a cell lost matter");

    let _ = tools::cells_in(&world, 0, 0, 47, 47);
    let _ = tools::copy_genome(&world, ids[2]);
    world.check_matter().expect("after reading");

    tools::isolate(&mut world, ids[2]);
    world.check_matter().expect("after isolate");
    assert_eq!(total(&world), before, "isolating lost matter");
    assert_eq!(world.cells().len(), 1);

    // And the world still runs afterwards.
    world.run(200);
    world.check_matter().expect("after running on");
}

#[test]
fn barrier_drawing_over_a_whole_region_conserves_matter() {
    // The stress case: a wall long enough that the squares it evicts have nowhere obvious to
    // go, so anything that cannot be placed has to be written off through the ledger rather
    // than dropped.
    let mut world = world_running(&ancestor());
    world.run(60);
    let before = world.total_matter();
    for x in 10..30u32 {
        for y in 10..14u32 {
            tools::set_barrier(&mut world, x, y, true);
        }
    }
    world.check_matter().expect("a wall broke conservation");
    let after = world.total_matter();
    assert_eq!(
        after, before,
        "drawing a wall changed how much matter the slide holds"
    );
    world.run(300);
    world.check_matter().expect("running on after a wall");
}
