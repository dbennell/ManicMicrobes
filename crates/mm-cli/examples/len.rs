fn main() {
    for f in [
        "genomes/ancestor.mm",
        "genomes/ancestor_sloppy.mm",
        "genomes/drifter.mm",
        "genomes/drifter_blind.mm",
    ] {
        let s = std::fs::read_to_string(f).unwrap();
        match mm_asm::assemble(&s) {
            Ok(a) => println!("{f}: {} bytes", a.bytes.len()),
            Err(e) => println!("{f}: FAILED\n{e}"),
        }
    }
}
