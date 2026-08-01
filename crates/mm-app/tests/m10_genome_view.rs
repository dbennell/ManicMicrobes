//! M10.3b acceptance 3 — the reading view is not the source view.
//!
//! > The text the editor hands back still reassembles to the original bytes, for every genome
//! > in `genomes/` and for random byte strings. M0's round-trip test, extended to whatever the
//! > genome pane emits.
//!
//! The pane holds two documents of the same instructions. One is byte-exact and reassembles;
//! the other resolves every template to what it means and reassembles to nothing. Both can be
//! wrong in their own way, and the two failures need different tests:
//!
//! * the **source** form fails by no longer round-tripping, which would silently corrupt a
//!   genome the moment somebody pressed `edit`. That is M0's test, re-run against the text the
//!   pane actually emits rather than against the disassembler directly;
//! * the **reading** form fails by being *plausible and wrong* — naming a jump target the VM
//!   does not go to. Nothing about such a listing looks broken, which is exactly why it is
//!   worth a test that asks the VM where it really went.
//!
//! The second is the one that justifies the feature existing. A listing that says `→ 47` while
//! the cell goes to 12 is worse than the `%101` it replaced, because the bits made no claim.

use mm_app::inspector::{Ink, Listing};
use mm_core::config::VmConfig;
use mm_core::genome::Genome;
use mm_core::host::NullHost;
use mm_core::rng::RandCtx;
use mm_core::vm::Vm;

/// The pane's source document, as one assemblable string.
fn source_text(genome: &Genome, cfg: VmConfig) -> String {
    let mut listing = Listing::default();
    let mut out = String::new();
    for line in listing.of(genome, genome.hash(), cfg) {
        out.push_str("        ");
        out.push_str(&line.text);
        out.push('\n');
    }
    out
}

/// The pane's reading document, line by line.
fn readings(genome: &Genome, cfg: VmConfig) -> Vec<(u32, String)> {
    let mut listing = Listing::default();
    listing
        .of(genome, genome.hash(), cfg)
        .iter()
        .map(|l| (l.offset, l.reading.clone()))
        .collect()
}

/// A deterministic byte string. No `rand` here for the same reason `mm-core` has none.
fn pseudo_random(seed: u64, len: usize) -> Vec<u8> {
    let mut s = seed | 1;
    (0..len)
        .map(|_| {
            // xorshift64*, which is plenty for "bytes nobody chose".
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 24) as u8
        })
        .collect()
}

/// Every `.mm` under `genomes/`, assembled.
fn shipped_genomes() -> Vec<(String, Vec<u8>)> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../genomes");
    let mut out = Vec::new();
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("genomes/ is part of the repository")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "mm"))
        .collect();
    // Sorted: a test that reports "the third genome failed" has to mean the same third genome
    // on every machine. Hard rule 6 applies to test suites too.
    paths.sort();
    for path in paths {
        let src = std::fs::read_to_string(&path).expect("readable");
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into();
        match mm_asm::assemble(&src) {
            Ok(a) => out.push((name, a.bytes)),
            Err(e) => panic!("{name} does not assemble: {e:?}"),
        }
    }
    assert!(!out.is_empty(), "found no genomes to test");
    out
}

#[test]
fn the_source_form_still_reassembles_to_the_original_bytes() {
    let cfg = VmConfig::DEFAULT;
    let mut cases: Vec<(String, Vec<u8>)> = shipped_genomes();

    // Byte strings nobody wrote, which is what an evolved genome is. These use every opcode
    // encoding including the non-canonical ones, and it is those that make the round trip
    // hard: a template whose letters are non-canonical `NOP` bytes must not be rendered as a
    // `%` operand, or reassembling it changes the genome.
    for seed in 0..64u64 {
        for len in [1usize, 2, 7, 64, 300] {
            cases.push((
                format!("random seed {seed} len {len}"),
                pseudo_random(seed, len),
            ));
        }
    }
    cases.push(("every byte".into(), (0u8..=255).collect()));
    cases.push(("every byte, reversed".into(), (0u8..=255).rev().collect()));

    for (name, bytes) in cases {
        let genome = Genome::new(bytes.clone()).expect("within the addressing limit");
        let text = source_text(&genome, cfg);
        let back = mm_asm::assemble(&text)
            .unwrap_or_else(|e| panic!("{name}: the pane's source did not assemble: {e:?}"))
            .bytes;
        assert_eq!(back, bytes, "{name}: the pane's source changed the genome");
    }
}

#[test]
fn an_immediate_reads_as_the_value_it_pushes() {
    // IMM with the letters 0,0,1,1,1,1 — bit i is set by letter i, so this is 0b111100 = 60.
    let genome = Genome::new(vec![0x02, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01]).unwrap();
    let reading = &readings(&genome, VmConfig::DEFAULT)[0].1;
    assert!(
        reading.contains("60"),
        "an immediate should read as its value, got {reading:?}"
    );

    // And the VM agrees about what it pushes, which is the point of showing it.
    let mut vm = Vm::default();
    step_once(&mut vm, &genome, VmConfig::DEFAULT);
    assert_eq!(vm.peek(), 60);
}

#[test]
fn a_template_spelled_out_for_the_round_trip_still_reads_as_its_value() {
    // 0x41/0x80/0xC1/0x41/0x41/0x41 are NOP1/NOP0/NOP1/NOP1/NOP1/NOP1 in non-canonical
    // encodings. `disasm` refuses to render them as a `%` operand — doing so would reassemble
    // to different bytes — so the source form spells them out as separate NOP lines and the
    // line's own `template` is empty.
    //
    // The VM does not read source. It reads bytes, and at this offset it sees an ordinary
    // six-letter template. So the reading has to resolve against the genome's table rather
    // than against the disassembly, or it reports `IMM 0` for an instruction that pushes 61.
    let bytes = vec![0x02, 0x41, 0x80, 0xC1, 0x41, 0x41, 0x41];
    let genome = Genome::new(bytes).unwrap();
    let cfg = VmConfig::DEFAULT;

    let source = source_text(&genome, cfg);
    assert!(
        source.contains("NOP1~1"),
        "the source form must still spell these out:\n{source}"
    );

    let mut vm = Vm::default();
    step_once(&mut vm, &genome, cfg);
    let pushed = vm.peek();
    assert_eq!(pushed, 61, "sanity: these six letters are 0b111101");
    let reading = &readings(&genome, cfg)[0].1;
    assert!(
        reading.contains(&pushed.to_string()),
        "the reading says {reading:?} but the VM pushed {pushed}"
    );
}

#[test]
fn a_jump_reads_as_the_offset_the_vm_actually_reaches() {
    let cfg = VmConfig::DEFAULT;
    let mut checked = 0usize;

    let mut cases: Vec<(String, Vec<u8>)> = shipped_genomes();
    for seed in 0..96u64 {
        cases.push((format!("random {seed}"), pseudo_random(seed, 120)));
    }
    // One of each searching opcode with its match deliberately in place. Random bytes produce
    // plenty of jumps but hardly any that *hit*, and a jump that misses exercises none of the
    // resolution this test is about. These four guarantee at least one hit per search
    // direction whatever else the corpus does.
    cases.extend([
        (
            "JMPF with a match ahead".into(),
            vec![0x20, 0x01, 0x50, 0x00, 0x50],
        ),
        (
            "JMPB with a match behind".into(),
            vec![0x00, 0x50, 0x21, 0x01, 0x50],
        ),
        (
            "JMPZ with a match ahead".into(),
            vec![0x22, 0x01, 0x50, 0x00, 0x50],
        ),
        (
            "CALL with a match ahead".into(),
            vec![0x24, 0x01, 0x50, 0x00, 0x50],
        ),
    ]);

    for (name, bytes) in cases {
        let genome = Genome::new(bytes).unwrap();
        for (offset, reading) in readings(&genome, cfg) {
            // Only the lines that name a destination. `→ 47`, `→ 47 (gene b)` and `↺ 12` all
            // claim the VM ends up at that offset; anything else claims nothing.
            let Some(target) = claimed_target(&reading) else {
                continue;
            };

            // Start the VM on this instruction and run exactly one. An empty data stack pops
            // zero, so JMPZ takes its branch and JMPNZ does not; LN starts at zero, so LOOPLN
            // falls through. Those three are excluded below rather than being coaxed into
            // branching, because they reach the same two search functions as the rest.
            if skip_conditional(&genome, offset) {
                continue;
            }
            let mut vm = Vm {
                ip: u16::try_from(offset).unwrap(),
                ..Vm::default()
            };
            step_once(&mut vm, &genome, cfg);

            assert_eq!(
                vm.ip, target,
                "{name}: the pane reads offset {offset} as {reading:?}, \
                 but the VM went to {}",
                vm.ip
            );
            checked += 1;
        }
    }

    // A test that silently checked nothing would pass forever. This is a floor on "did
    // anything resolve at all", not a coverage target: the four built cases above account for
    // 4 of it and the shipped genomes for the rest, and the number moves whenever `genomes/`
    // does. If it ever reads near zero, the reading has stopped naming targets.
    assert!(
        checked >= 20,
        "only checked {checked} jumps — is anything resolving?"
    );
}

#[test]
fn a_jump_that_matches_nothing_says_where_it_falls_through_to() {
    // JMPF with a one-letter template and nothing complementary anywhere: the VM falls through
    // to the byte after the template. Saying so is the useful reading — a jump that never
    // fires is usually the reason a lineage went quiet.
    let genome = Genome::new(vec![0x20, 0x01, 0x50, 0x50]).unwrap();
    let cfg = VmConfig::DEFAULT;
    let reading = &readings(&genome, cfg)[0].1;
    assert!(
        reading.contains("falls through"),
        "a jump with no match should say so, got {reading:?}"
    );

    let mut vm = Vm::default();
    step_once(&mut vm, &genome, cfg);
    assert!(
        reading.contains(&vm.ip.to_string()),
        "the reading says {reading:?} but the VM fell through to {}",
        vm.ip
    );
}

#[test]
fn the_reading_follows_the_world_that_is_running() {
    // The search range is editable while the world runs (M10.2). A `CALL` whose match sits
    // outside a narrowed range stops reaching it, and the pane has to say so — a listing
    // resolved against `VmConfig::DEFAULT` would go on naming a target the cell no longer
    // reaches, which is the failure this whole file is about.
    let mut bytes = vec![0x24, 0x01]; // CALL %1
    bytes.extend(std::iter::repeat_n(0x50u8, 40)); // a long stretch of ADD~1
    bytes.push(0x00); // NOP0 — the complement of %1, and the match
    bytes.push(0x50); // somewhere for the call to land that is not offset 0
    let genome = Genome::new(bytes).unwrap();

    let wide = VmConfig {
        template_search_range: 512,
        ..VmConfig::DEFAULT
    };
    let narrow = VmConfig {
        template_search_range: 4,
        ..VmConfig::DEFAULT
    };

    let far = readings(&genome, wide)[0].1.clone();
    let near = readings(&genome, narrow)[0].1.clone();
    assert!(
        far.contains('→'),
        "with room to search, the call binds: {far:?}"
    );
    assert!(
        near.contains("falls through"),
        "with the range cut to 4 the call cannot reach its match: {near:?}"
    );

    // And the VM does what each listing said it would.
    for (cfg, expected) in [(wide, far), (narrow, near)] {
        let mut vm = Vm::default();
        step_once(&mut vm, &genome, cfg);
        assert!(
            expected.contains(&vm.ip.to_string()),
            "reading {expected:?} disagrees with the VM's {}",
            vm.ip
        );
    }
}

/// Run exactly one instruction against a sandbox host, the way the debugger does.
fn step_once(vm: &mut Vm, genome: &Genome, cfg: VmConfig) {
    let ctx = RandCtx::new(0, 0, 0);
    let mut host = NullHost;
    vm.run(genome, &cfg, &ctx, &mut host, 1);
}

/// The offset a reading claims the VM ends up at, if it claims one.
///
/// Only a bare number counts. `EXPRESS` reads as `→ gene d (drift 1)`, which names a gene
/// rather than an offset — scanning for the first digit anywhere would read that as a claim to
/// jump to 1 and this test would fail on a listing that is perfectly correct.
fn claimed_target(reading: &str) -> Option<u16> {
    let marker = reading.find(['→', '↺'])?;
    let after: String = reading.get(marker..)?.chars().skip(1).collect();
    after.split_whitespace().next()?.parse().ok()
}

/// Whether this instruction's branch depends on state this test does not set up.
///
/// A fresh VM has an empty data stack and `LN` at zero, so `JMPZ` takes its branch (popping
/// empty yields 0) while `JMPNZ` and `LOOPLN` do not. Rather than coax those two into
/// branching, they are left out: all four forward jumps reach the same `search_forward` and
/// both backward ones the same `search_backward`, so the paths are covered either way.
fn skip_conditional(genome: &Genome, offset: u32) -> bool {
    use mm_core::Op;
    let Ok(i) = usize::try_from(offset) else {
        return true;
    };
    matches!(Op::from_byte(genome.byte(i)), Op::JmpNz | Op::LoopLn)
}

/// The ink of each non-blank word of a line, for the highlighting tests.
fn inked(text: &str) -> Vec<(Ink, String)> {
    mm_app::inspector::spans(text)
        .into_iter()
        .map(|s| (s.ink, text[s.start..s.end].to_string()))
        .filter(|(_, w)| !w.trim().is_empty())
        .collect()
}

#[test]
fn spans_cover_a_listing_line_exactly() {
    // The pane paints straight through the spans to build one laid-out line. A gap would drop
    // characters on the floor and an overlap would draw one twice — and because the listing is
    // monospace and column-aligned, either shows up as the whole column below it shifting.
    for text in [
        "IMM      60",
        "JMPF     → 47 (gene b)",
        "EXPRESS  → gene d (drift 1)",
        "LOOPLN   ↺ 12",
        "CALL     ✗ falls through to 9",
        "IMM      %001111",
        "NOP1~1",
        "HALT",
        "",
    ] {
        let spans = mm_app::inspector::spans(text);
        if text.is_empty() {
            assert!(spans.is_empty());
            continue;
        }
        assert_eq!(spans.first().map(|s| s.start), Some(0), "{text:?}");
        assert_eq!(spans.last().map(|s| s.end), Some(text.len()), "{text:?}");
        for pair in spans.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "gap or overlap in {text:?}");
        }
    }
}

#[test]
fn the_values_a_reading_resolves_are_inked_as_numbers() {
    // The point of the reading form. If these are not numbers, the highlighting has stopped
    // doing the one thing it was added for.
    assert!(inked("IMM      60").contains(&(Ink::Number, "60".into())));
    assert!(inked("JMPF     → 47").contains(&(Ink::Number, "47".into())));
    assert!(inked("CALL     ✗ falls through to 9").contains(&(Ink::Number, "9".into())));

    // And the source form's bits are not numbers, because they are not the value they encode.
    let source = inked("IMM      %001111");
    assert!(source.contains(&(Ink::Pattern, "%001111".into())));
    assert!(!source.iter().any(|(ink, _)| *ink == Ink::Number));
}

#[test]
fn a_gene_name_is_not_read_as_a_jump_target() {
    // `gene b` is two words and only the first is recognisable on its own. Colouring `b` as
    // stray punctuation would break the name in half on the line.
    let express = inked("EXPRESS  → gene d (drift 1)");
    assert!(express.contains(&(Ink::Gene, "gene".into())));
    assert!(express.contains(&(Ink::Gene, "d".into())));
    assert!(express.contains(&(Ink::Marker, "→".into())));

    // A call that lands on a gene names it *and* gives the offset, and the two are told apart.
    let call = inked("CALL     → 47 (gene b)");
    assert!(call.contains(&(Ink::Number, "47".into())));
    assert!(call.contains(&(Ink::Gene, "(gene".into())));
    assert!(call.contains(&(Ink::Gene, "b)".into())));
}

#[test]
fn a_jump_that_misses_is_inked_differently_from_one_that_lands() {
    let lands = inked("JMPF     → 47");
    let misses = inked("JMPF     ✗ falls through to 9");
    assert!(lands.contains(&(Ink::Marker, "→".into())));
    assert!(misses.contains(&(Ink::Miss, "✗".into())));
    assert!(
        !misses.iter().any(|(ink, _)| *ink == Ink::Marker),
        "a jump that never fires should not look like one that does"
    );
}

#[test]
fn an_opcode_is_an_opcode_in_any_encoding() {
    // `ADD~2` is a real opcode in one of its non-canonical encodings, and evolved genomes are
    // full of them. The assembler's lexer does not know the `~` form — it is disassembly
    // output rather than anything anyone writes — so it is named here instead.
    assert!(inked("ADD~2").contains(&(Ink::Opcode, "ADD~2".into())));
    assert!(inked("NOP1~1").contains(&(Ink::Opcode, "NOP1~1".into())));
    assert!(inked("HALT").contains(&(Ink::Opcode, "HALT".into())));
}
