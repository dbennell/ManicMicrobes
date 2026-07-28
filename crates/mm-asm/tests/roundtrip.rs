//! M0 acceptance test 3 — round-trip.
//!
//! > Assemble → disassemble → reassemble is byte-identical for every genome in `genomes/`.
//!
//! The stronger property, that this holds for *arbitrary* byte strings and not only for
//! assembler output, is covered exhaustively over one- and two-byte genomes in
//! `disasm`'s unit tests and statistically over random genomes here.

use std::path::{Path, PathBuf};

use mm_asm::{assemble, disassemble};
use mm_core::rng::{Purpose, RandCtx};

fn genomes_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("genomes")
}

fn sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let dir = genomes_dir();
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("mm") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        out.push((name, text));
    }
    out.sort();
    out
}

#[test]
fn genomes_directory_is_not_empty() {
    // A round-trip test over zero genomes passes vacuously, which would be worse than
    // failing.
    assert!(
        sources().len() >= 5,
        "expected the genome library to be populated; found {}",
        sources().len()
    );
}

#[test]
fn every_genome_assembles() {
    for (name, src) in sources() {
        if let Err(e) = assemble(&src) {
            panic!("{name} does not assemble:\n{e}");
        }
    }
}

#[test]
fn assemble_disassemble_reassemble_is_byte_identical() {
    for (name, src) in sources() {
        let first = assemble(&src)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .bytes;
        let listing = disassemble(&first).to_source();
        let second = assemble(&listing)
            .unwrap_or_else(|e| panic!("{name} disassembly does not reassemble:\n{e}\n{listing}"))
            .bytes;
        assert_eq!(
            second, first,
            "{name} is not byte-identical after a round trip\n{listing}"
        );

        // And it is a fixed point: a second trip changes nothing either.
        let third = assemble(&disassemble(&second).to_source())
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .bytes;
        assert_eq!(
            third, first,
            "{name} is not stable under repeated round trips"
        );
    }
}

#[test]
fn listings_of_real_genomes_reassemble() {
    for (name, src) in sources() {
        let bytes = assemble(&src)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .bytes;
        let listing = disassemble(&bytes).to_listing(&bytes);
        let back = assemble(&listing)
            .unwrap_or_else(|e| panic!("{name} listing does not reassemble:\n{e}"))
            .bytes;
        assert_eq!(back, bytes, "{name} listing round trip");
    }
}

#[test]
fn source_maps_cover_every_byte_of_every_genome() {
    for (name, src) in sources() {
        let a = assemble(&src).unwrap_or_else(|e| panic!("{name}: {e}"));
        for b in 0..a.bytes.len() as u32 {
            let span = a
                .source_map
                .lookup(b)
                .unwrap_or_else(|| panic!("{name}: byte {b} has no source position"));
            assert!(span.line >= 1, "{name}: byte {b} maps to line 0");
            let line = src.lines().nth(span.line as usize - 1).unwrap_or("");
            assert!(
                !line.trim().is_empty(),
                "{name}: byte {b} maps to blank line {}",
                span.line
            );
        }
    }
}

#[test]
fn random_byte_strings_round_trip() {
    // Arbitrary genomes, not only ones that came from source. This is the property the
    // editor depends on when it disassembles something it found in the world.
    let ctx = RandCtx::new(0x6D_69_63_72_6F_62_65_73, 0, 0);
    for case in 0..2_000u64 {
        let len = 1 + ctx.draw_below(Purpose::Harness, case, 300) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|i| (ctx.draw(Purpose::Harness, case << 20 | i as u64) >> 32) as u8)
            .collect();
        let src = disassemble(&bytes).to_source();
        let back = assemble(&src)
            .unwrap_or_else(|e| panic!("case {case} does not reassemble:\n{e}\n{src}"))
            .bytes;
        assert_eq!(back, bytes, "case {case} round trip\n{src}");
    }
}
