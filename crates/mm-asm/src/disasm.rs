//! The disassembler.
//!
//! Two jobs that pull in different directions. Reading an evolved genome in the editor wants
//! offsets and raw bytes; feeding the result back through the assembler wants nothing but
//! instructions. So the output is structured, and [`Disassembly::to_source`] renders the
//! plain form while [`Disassembly::to_listing`] adds the annotations as trailing comments —
//! which keeps the listing assemblable too.
//!
//! # Losslessness
//!
//! `assemble(disassemble(b)) == b` for *any* byte string, not only for ones a human wrote.
//! That is what lets the editor round-trip a genome found in the world. Two things are
//! needed for it:
//!
//! * a byte that is not the canonical encoding of its opcode gets a `~n` suffix (SPEC §4.2
//!   gives every opcode four encodings, and evolved genomes use all of them);
//! * a template whose letters are themselves non-canonical `NOP` bytes is *not* rendered as
//!   a `%` operand — the `%` form would reassemble to canonical letters and change the
//!   bytes. Those letters are emitted as ordinary `NOP0`/`NOP1` lines instead, which
//!   assemble back to exactly the bytes they came from and mean exactly the same thing to
//!   the VM, since it reads templates from bytes rather than from source.

use mm_core::isa::{Op, Template, MAX_TEMPLATE_LEN};

/// One rendered line of disassembly.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Line {
    /// Offset of the opcode byte.
    pub offset: u32,
    /// Bytes this line renders: the opcode, plus its template letters when they are
    /// rendered inline.
    pub len: u32,
    pub op: Op,
    /// Which of the four bytes encoding `op` was used, `0..=3`.
    pub variant: u8,
    /// The template rendered as this line's operand.
    ///
    /// Empty when the instruction takes no template *or* when its letters are being emitted
    /// as separate lines to preserve their encodings. To ask what the VM will actually read
    /// at this point, consult the genome's own template table.
    pub template: Template,
}

impl Line {
    /// The reassemblable text for this instruction, with its template as `%` letters.
    #[must_use]
    pub fn to_source(&self) -> String {
        self.render(|t| {
            let mut s = String::from("%");
            for i in 0..t.len {
                s.push(if t.letter(i) == 1 { '1' } else { '0' });
            }
            s
        })
    }

    /// The same instruction with its template written as a number.
    ///
    /// Also reassemblable, and to the identical bytes — `%000101` and `40` are two spellings of
    /// the same six letters, because letter *i* is bit *i*. The width comes back only when it
    /// is not the narrowest one that holds the value, which is the same rule the assembler
    /// applies to a bare number, so `%0001010` is `40:7` and `%000101` is just `40`.
    ///
    /// This is what a person reads. `to_source` renders the letters, which is what you want
    /// when the template is a base-pairing pattern — a jump looking for its complement, a
    /// promoter being matched by similarity — and is unreadable when it is a plain number,
    /// which is most of a real genome.
    #[must_use]
    pub fn to_readable(&self) -> String {
        self.render(|t| {
            // What the assembler assumes for a bare number. `Template::new` masks the value to
            // its length, so this is never wider than the template and the pinned form is only
            // needed for a template padded beyond its value.
            let minimal = 8u8.saturating_sub(t.value.leading_zeros() as u8);
            if t.len == minimal {
                t.value.to_string()
            } else {
                format!("{}:{}", t.value, t.len)
            }
        })
    }

    /// The mnemonic, its encoding suffix, and an operand rendered by `operand`.
    fn render(&self, operand: impl Fn(Template) -> String) -> String {
        let mut s = String::new();
        s.push_str(self.op.name());
        if self.variant != 0 {
            s.push('~');
            s.push_str(&self.variant.to_string());
        }
        if self.template.len > 0 {
            while s.len() < 8 {
                s.push(' ');
            }
            s.push(' ');
            s.push_str(&operand(self.template));
        }
        s
    }
}

/// A decoded genome.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Disassembly {
    pub lines: Vec<Line>,
}

impl Disassembly {
    /// Text that assembles back to the original bytes.
    #[must_use]
    pub fn to_source(&self) -> String {
        self.render(Line::to_source)
    }

    /// The same, with templates as numbers rather than `%` letters. See [`Line::to_readable`].
    ///
    /// This is what the editor is loaded with: a genome taken off a live cell should arrive as
    /// something a person can edit, and a page of `%00110000` is a hex editor with extra steps.
    #[must_use]
    pub fn to_readable(&self) -> String {
        self.render(Line::to_readable)
    }

    fn render(&self, line: impl Fn(&Line) -> String) -> String {
        let mut out = String::new();
        for l in &self.lines {
            out.push_str("        ");
            out.push_str(&line(l));
            out.push('\n');
        }
        out
    }

    /// The same, with each line's offset and raw bytes appended as a comment. Assembles back
    /// to the original bytes just as [`Self::to_source`] does.
    #[must_use]
    pub fn to_listing(&self, bytes: &[u8]) -> String {
        let mut out = String::new();
        for line in &self.lines {
            let start = line.offset as usize;
            let end = start.saturating_add(line.len as usize).min(bytes.len());
            let raw: Vec<String> = bytes
                .get(start..end)
                .unwrap_or(&[])
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            out.push_str(&format!(
                "        {:<24} ; {:>5}: {}\n",
                line.to_source(),
                line.offset,
                raw.join(" ")
            ));
        }
        out
    }

    /// The line containing a byte offset.
    #[must_use]
    pub fn line_at(&self, offset: u32) -> Option<&Line> {
        let i = self
            .lines
            .partition_point(|l| l.offset.saturating_add(l.len) <= offset);
        self.lines
            .get(i)
            .filter(|l| offset >= l.offset && offset < l.offset.saturating_add(l.len))
    }
}

/// Decode a genome.
///
/// Total: every byte string is a legal program (I3), so this cannot fail.
#[must_use]
pub fn disassemble(bytes: &[u8]) -> Disassembly {
    let mut lines = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(b) => *b,
            None => break,
        };
        let op = Op::from_byte(b);
        let variant = b / mm_core::isa::OPCODE_COUNT;
        let mut template = Template::EMPTY;
        let mut len = 1u32;

        if op.takes_template() {
            // The maximal run of template letters following, capped at 8 — exactly what the
            // VM's precomputed table reports at this offset.
            let mut value = 0u8;
            let mut n = 0u8;
            let mut canonical = true;
            while n < MAX_TEMPLATE_LEN {
                let Some(next) = bytes.get(i.saturating_add(1).saturating_add(n as usize)) else {
                    break;
                };
                match Op::from_byte(*next) {
                    Op::Nop1 => value |= 1u8 << n,
                    Op::Nop0 => {}
                    _ => break,
                }
                // `%` letters reassemble to 0x00/0x01. Anything else has to be spelled out.
                if *next > 1 {
                    canonical = false;
                }
                n = n.saturating_add(1);
            }
            if canonical {
                template = Template::new(n, value);
                len = len.saturating_add(n as u32);
            }
        }

        lines.push(Line {
            offset: i as u32,
            len,
            op,
            variant,
            template,
        });
        i = i.saturating_add(len as usize);
    }
    Disassembly { lines }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::assemble;

    fn round_trips(bytes: &[u8]) {
        // Both renderings, every time. They are two spellings of one document and the
        // guarantee is the same for each: whatever the editor is handed, it hands back these
        // bytes. A readable form that quietly lost a template's width would corrupt a genome
        // the first time somebody opened it and pressed assemble.
        for (which, src) in [
            ("source", disassemble(bytes).to_source()),
            ("readable", disassemble(bytes).to_readable()),
        ] {
            let back = assemble(&src).map(|a| a.bytes).unwrap_or_else(|e| {
                panic!("{which} did not assemble for {bytes:02x?}: {e:?}\n{src}")
            });
            assert_eq!(
                back, bytes,
                "{which} round trip failed for {bytes:02x?}\n{src}"
            );
        }
    }

    #[test]
    fn templates_are_consumed_by_their_host() {
        let d = disassemble(&[0x02, 0x01, 0x00, 0x01, 0x10]);
        assert_eq!(d.lines.len(), 2);
        assert_eq!(d.lines[0].op, Op::Imm);
        assert_eq!(d.lines[0].template, Template::new(3, 0b101));
        assert_eq!(d.lines[0].len, 4);
        assert_eq!(d.lines[1].op, Op::Add);
        assert_eq!(d.lines[0].to_source(), "IMM      %101");
    }

    #[test]
    fn loose_template_letters_are_ordinary_instructions() {
        let d = disassemble(&[0x00, 0x01, 0x10]);
        assert_eq!(d.lines.len(), 3);
        assert_eq!(d.lines[0].op, Op::Nop0);
        assert_eq!(d.lines[1].op, Op::Nop1);
        assert_eq!(d.lines[0].to_source(), "NOP0");
    }

    #[test]
    fn template_runs_stop_at_eight() {
        let mut bytes = vec![0x02];
        bytes.extend(std::iter::repeat_n(0x01u8, 10));
        let d = disassemble(&bytes);
        assert_eq!(d.lines[0].template.len, 8);
        assert_eq!(d.lines[0].len, 9);
        assert_eq!(d.lines.len(), 3); // IMM + 8 letters, then two loose NOP1
        round_trips(&bytes);
    }

    #[test]
    fn degenerate_bytes_keep_their_variant() {
        let d = disassemble(&[0x90]);
        assert_eq!(d.lines[0].op, Op::Add);
        assert_eq!(d.lines[0].variant, 2);
        assert_eq!(d.lines[0].to_source(), "ADD~2");
    }

    #[test]
    fn degenerate_template_letters_are_spelled_out() {
        // 0x41/0x80/0xC1 are NOP1/NOP0/NOP1 in non-canonical encodings. Rendering them as
        // `%101` would reassemble to 0x01/0x00/0x01 and change the genome.
        let bytes = [0x02, 0x41, 0x80, 0xC1, 0x50];
        let d = disassemble(&bytes);
        assert_eq!(d.lines[0].op, Op::Imm);
        assert_eq!(d.lines[0].template, Template::EMPTY);
        assert_eq!(d.lines[0].len, 1);
        assert_eq!(d.lines.len(), 5);
        assert_eq!(d.lines[1].to_source(), "NOP1~1");
        round_trips(&bytes);
    }

    #[test]
    fn any_byte_string_round_trips() {
        let cases: Vec<Vec<u8>> = vec![
            vec![],
            vec![0x00],
            vec![0x20],
            vec![0x20, 0x01],
            vec![0xFF; 9],
            vec![0x02, 0x41, 0x80, 0xC1, 0x50],
            (0u8..=255).collect(),
            (0u8..=255).rev().collect(),
        ];
        for bytes in cases {
            round_trips(&bytes);
        }
    }

    #[test]
    fn every_byte_pair_round_trips() {
        // Exhaustive over two-byte genomes: catches any opcode whose template handling
        // disagrees between the two directions.
        for a in 0..=255u8 {
            for b in 0..=255u8 {
                round_trips(&[a, b]);
            }
        }
    }

    #[test]
    fn listings_are_still_assemblable() {
        let bytes: Vec<u8> = (0u8..=255).collect();
        let listing = disassemble(&bytes).to_listing(&bytes);
        assert_eq!(assemble(&listing).map(|a| a.bytes).unwrap(), bytes);
    }

    #[test]
    fn a_template_reads_as_a_decimal_number() {
        // Letter i is bit i, so `%000101` is bits 3 and 5 — 40. This is the line the whole
        // readable rendering exists for: it is most of a real genome.
        let d = disassemble(&[0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01]);
        assert_eq!(d.lines[0].to_source(), "IMM      %000101");
        assert_eq!(d.lines[0].to_readable(), "IMM      40");
    }

    #[test]
    fn a_template_padded_beyond_its_value_keeps_its_width() {
        // `%1` and `%1000` are both the value 1 and are *not* the same template: the second
        // matches four letters and the first one. A bare number takes the narrowest width, so
        // the padded one has to say so or the round trip loses the padding.
        let narrow = disassemble(&[0x02, 0x01]);
        assert_eq!(narrow.lines[0].to_readable(), "IMM      1");
        let padded = disassemble(&[0x02, 0x01, 0x00, 0x00, 0x00]);
        assert_eq!(padded.lines[0].to_readable(), "IMM      1:4");
        round_trips(&[0x02, 0x01, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn every_template_opcode_takes_a_number() {
        // Not just IMM. A jump's template is a pattern to match rather than a value to push,
        // but it is still eight bits and still unreadable as `%00110000`.
        for byte in 0..64u8 {
            let op = Op::from_byte(byte);
            if !op.takes_template() {
                continue;
            }
            let bytes = [byte, 0x01, 0x00, 0x01];
            let readable = disassemble(&bytes).to_readable();
            assert!(
                !readable.contains('%'),
                "{} still renders letters: {readable}",
                op.name()
            );
            round_trips(&bytes);
        }
    }

    #[test]
    fn line_at_finds_bytes_inside_a_template() {
        let d = disassemble(&[0x02, 0x01, 0x00, 0x01, 0x10]);
        for b in 0..4 {
            assert_eq!(d.line_at(b).unwrap().op, Op::Imm, "byte {b}");
        }
        assert_eq!(d.line_at(4).unwrap().op, Op::Add);
        assert_eq!(d.line_at(5), None);
    }
}
