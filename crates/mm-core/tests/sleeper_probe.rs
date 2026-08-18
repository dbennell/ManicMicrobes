//! Does the sleeper ever sleep? Ignored — a probe, run it on purpose.

use mm_core::events::Occurrence;
use mm_core::Scenario;

fn root(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn probe(file: &str, genome: &str) {
    let scenario =
        Scenario::from_ron(&std::fs::read_to_string(root(file)).expect("scenario")).expect("parse");
    let source = std::fs::read_to_string(root(genome)).expect("genome");
    let bytes = mm_asm::assemble(&source).expect("assemble").bytes;
    eprintln!("{genome}: {} bytes", bytes.len());
    let mut world = mm_core::World::new(scenario).expect("world");
    world.place_founders(&bytes, 16);
    world.run(20_000);
    eprintln!(
        "  {file}: population {}, first dormancy {:?}",
        world.cells().len(),
        world.events().first(Occurrence::Dormancy)
    );
}

#[test]
#[ignore = "a probe; run it on purpose"]
fn does_the_sleeper_ever_sleep() {
    for file in ["scenarios/the_thicket.ron", "scenarios/the_lean_water.ron"] {
        for genome in ["genomes/ancestor.mm", "genomes/sleeper.mm"] {
            probe(file, genome);
        }
    }
}
