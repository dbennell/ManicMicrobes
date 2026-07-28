//! The `.mm` assembler.
//!
//! # Syntax
//!
//! One instruction per line. `;` begins a comment. Mnemonics are case-insensitive and are
//! exactly the names in SPEC §5.1.
//!
//! ```text
//!         GENE    #replicate      ; named promoter, hashed to an 8-bit pattern
//!         GLEN
//!         SETLN
//! loop:                           ; a label emits its own template letters
//!         COPYB
//!         LOOPLN  loop            ; emits the complement, which the search then matches
//!         SPLIT
//! ```
//!
//! # Operands
//!
//! | Form | Meaning | Valid on |
//! |------|---------|----------|
//! | `label` | jump to a label | `JMPF` `JMPB` `JMPZ` `JMPNZ` `CALL` `LOOPLN` |
//! | `#name` | named promoter | `GENE` `EXPRESS` |
//! | `42`, `42:6` | numeric literal, optionally width-pinned | `IMM` |
//! | `%1011` | raw template letters, first letter leftmost | any template opcode |
//! | *(none)* | zero-length template | any template opcode |
//! | `ADD~2` | pick a non-canonical byte for the opcode | any opcode |
//!
//! # Labels are code, not markers
//!
//! A label does not name an address — there are no addresses. It compiles to a run of
//! template letters emitted *at the label site*, and a jump to it compiles to the
//! complementary run. The jump then finds its target by base-pairing (SPEC §4.3), which is
//! why a mutated genome relocates its own code correctly instead of turning to rubble.
//!
//! A label therefore costs bytes wherever it is defined, including when nothing references
//! it, and a genome may define at most 16 of them before the 4-bit pattern space runs out.
//! Past that, write raw `%` patterns.
//!
//! # Names hash; labels probe
//!
//! `#name` hashes to a fixed 8-bit pattern, the same in every file, so a promoter means the
//! same thing across genomes and lineages — that is what makes `EXPRESS #hunt` in one
//! genome bind sensibly to `GENE #hunt` inherited from another. Two different names that
//! hash alike in the same file are an error rather than a silent merge.
//!
//! Jump labels are only ever matched within one genome, so they get the cheaper treatment:
//! a hash-seeded start followed by linear probing over the labels in sorted name order.
//! That never collides, at the cost of a label's pattern depending on what else is defined
//! in the file.

use std::collections::{BTreeMap, BTreeSet};

use mm_core::isa::{Op, Template};
use mm_core::rng::mix64;

use crate::source_map::{SourceMap, Span};

/// Bits in a jump label's template. 16 distinct labels per genome.
pub const LABEL_BITS: u8 = 4;
/// Bits in a named promoter's template. The full 8, for separation under
/// `promoter_bind_threshold`.
pub const PROMOTER_BITS: u8 = 8;

/// An assembly failure, positioned in the source.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AsmError {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

impl std::fmt::Display for AsmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for AsmError {}

/// Errors are collected rather than reported one at a time, so the editor can underline
/// every problem in a file from one assemble.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AsmErrors(pub Vec<AsmError>);

impl std::fmt::Display for AsmErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, e) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for AsmErrors {}

/// Where a label was defined and what pattern it compiled to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LabelInfo {
    pub template: Template,
    /// Byte offset of the label's first template letter.
    pub byte: u32,
    pub line: u32,
}

/// A successfully assembled genome.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Assembled {
    pub bytes: Vec<u8>,
    pub source_map: SourceMap,
    pub labels: BTreeMap<String, LabelInfo>,
    /// `#name` to promoter pattern, for every name mentioned.
    pub promoters: BTreeMap<String, Template>,
}

/// The stable hash behind `#name` promoters and label seeding.
#[must_use]
pub fn name_hash(name: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01B3);
    }
    mix64(h)
}

/// The promoter pattern a `#name` compiles to. Stable across files and runs.
#[must_use]
pub fn promoter_pattern(name: &str) -> Template {
    Template::new(PROMOTER_BITS, (name_hash(name) >> 32) as u8)
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Operand {
    None,
    Raw(Template),
    Number { value: u8, width: u8 },
    Label(String),
    Promoter(String),
}

#[derive(Clone, Debug)]
enum Item {
    LabelDef {
        name: String,
        line: u32,
        col: u32,
    },
    Instr {
        op: Op,
        variant: u8,
        operand: Operand,
        line: u32,
        col: u32,
    },
}

/// Assemble `.mm` source into genome bytes.
///
/// # Errors
///
/// Returns every problem found, positioned, rather than stopping at the first.
pub fn assemble(src: &str) -> Result<Assembled, AsmErrors> {
    let mut errors: Vec<AsmError> = Vec::new();
    let items = parse(src, &mut errors);

    let mut defined: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    let mut promoter_names: BTreeSet<String> = BTreeSet::new();

    for item in &items {
        match item {
            Item::LabelDef { name, line, col } => {
                if defined.insert(name.clone(), (*line, *col)).is_some() {
                    errors.push(AsmError {
                        line: *line,
                        col: *col,
                        message: format!("label `{name}` is defined more than once"),
                    });
                }
            }
            Item::Instr { operand, .. } => match operand {
                Operand::Label(n) => {
                    referenced.insert(n.clone());
                }
                Operand::Promoter(n) => {
                    promoter_names.insert(n.clone());
                }
                _ => {}
            },
        }
    }

    for item in &items {
        if let Item::Instr {
            operand: Operand::Label(name),
            line,
            col,
            ..
        } = item
        {
            if !defined.contains_key(name) {
                errors.push(AsmError {
                    line: *line,
                    col: *col,
                    message: format!("undefined label `{name}`"),
                });
            }
        }
    }

    let label_patterns = allocate_labels(&defined, &mut errors);
    let promoters = allocate_promoters(&promoter_names, &items, &mut errors);

    if !errors.is_empty() {
        errors.sort_by_key(|e| (e.line, e.col));
        return Err(AsmErrors(errors));
    }

    // Pass 2: emit. Every template width is known by now, so byte offsets are exact.
    let mut bytes: Vec<u8> = Vec::new();
    let mut source_map = SourceMap::new();
    let mut labels: BTreeMap<String, LabelInfo> = BTreeMap::new();
    let mut sites: Vec<TemplateSite> = Vec::new();

    for item in &items {
        match item {
            Item::LabelDef { name, line, col } => {
                let t = label_patterns.get(name).copied().unwrap_or(Template::EMPTY);
                let start = bytes.len() as u32;
                sites.push(TemplateSite {
                    start,
                    len: t.len,
                    line: *line,
                    col: *col,
                    what: format!("label `{name}`"),
                });
                emit_template(&mut bytes, t);
                labels.insert(
                    name.clone(),
                    LabelInfo {
                        template: t,
                        byte: start,
                        line: *line,
                    },
                );
                if t.len > 0 {
                    source_map.push(Span {
                        byte: start,
                        len: t.len as u32,
                        line: *line,
                        col: *col,
                    });
                }
            }
            Item::Instr {
                op,
                variant,
                operand,
                line,
                col,
            } => {
                let start = bytes.len() as u32;
                bytes.push(
                    op.canonical_byte()
                        .wrapping_add(variant.wrapping_mul(mm_core::isa::OPCODE_COUNT)),
                );
                let t = match operand {
                    Operand::None => Template::EMPTY,
                    Operand::Raw(t) => *t,
                    Operand::Number { value, width } => Template::new(*width, *value),
                    // A jump emits the complement of its target's pattern, so that the
                    // base-pairing search finds the label site.
                    Operand::Label(n) => label_patterns
                        .get(n)
                        .copied()
                        .unwrap_or(Template::EMPTY)
                        .complement(),
                    // EXPRESS matches promoters by similarity, not by complement, so the
                    // pattern is emitted as-is on both the GENE and the EXPRESS.
                    Operand::Promoter(n) => promoters.get(n).copied().unwrap_or(Template::EMPTY),
                };
                sites.push(TemplateSite {
                    start: start.saturating_add(1),
                    len: t.len,
                    line: *line,
                    col: *col,
                    what: format!("the template on `{}`", op.name()),
                });
                emit_template(&mut bytes, t);
                source_map.push(Span {
                    byte: start,
                    len: 1u32.saturating_add(t.len as u32),
                    line: *line,
                    col: *col,
                });
            }
        }
    }

    check_templates_are_not_extended(&bytes, &sites, &mut errors);
    if !errors.is_empty() {
        errors.sort_by_key(|e| (e.line, e.col));
        return Err(AsmErrors(errors));
    }

    if bytes.len() > mm_core::MAX_GENOME_LEN {
        return Err(AsmErrors(vec![AsmError {
            line: 0,
            col: 0,
            message: format!(
                "assembled genome is {} bytes, over the {}-byte limit",
                bytes.len(),
                mm_core::MAX_GENOME_LEN
            ),
        }]));
    }

    Ok(Assembled {
        bytes,
        source_map,
        labels,
        promoters,
    })
}

/// A template the source explicitly asked for, and where.
struct TemplateSite {
    start: u32,
    len: u8,
    line: u32,
    col: u32,
    what: String,
}

/// A template is the *maximal* run of `NOP` letters at its position (SPEC §4.3), so letters
/// emitted immediately after one silently become part of it.
///
/// Writing a jump and then a label on the next line does exactly that: the four letters of
/// the jump and the four of the label fuse into one eight-letter template, the jump searches
/// for the complement of something nobody wrote, and the genome goes somewhere surprising —
/// or nowhere. It is easy to do, invisible in the source and painful to debug from a trace,
/// so the assembler refuses it.
///
/// A template already at the eight-letter cap cannot be extended, and a template the source
/// left empty is not checked: the disassembler emits exactly that when a run's letters use
/// non-canonical `NOP` encodings, and the letters that follow are meant to be read as its
/// template.
fn check_templates_are_not_extended(
    bytes: &[u8],
    sites: &[TemplateSite],
    errors: &mut Vec<AsmError>,
) {
    for site in sites {
        if site.len == 0 || site.len >= mm_core::MAX_TEMPLATE_LEN {
            continue;
        }
        let mut actual = site.len;
        while actual < mm_core::MAX_TEMPLATE_LEN {
            let at = (site.start as usize).saturating_add(actual as usize);
            match bytes.get(at) {
                Some(b) if Op::from_byte(*b).is_nop() => actual = actual.saturating_add(1),
                _ => break,
            }
        }
        if actual > site.len {
            errors.push(AsmError {
                line: site.line,
                col: site.col,
                message: format!(
                    "{} is {} letters as written, but the {} that follow extend it to {} — a \
                     template is the maximal run of NOP letters. Put an instruction between \
                     them.",
                    site.what,
                    site.len,
                    actual.saturating_sub(site.len),
                    actual
                ),
            });
        }
    }
}

fn emit_template(bytes: &mut Vec<u8>, t: Template) {
    for i in 0..t.len {
        bytes.push(if t.letter(i) == 1 {
            Op::Nop1.canonical_byte()
        } else {
            Op::Nop0.canonical_byte()
        });
    }
}

/// Hash-seeded start, then linear probing in sorted name order. Deterministic, and never
/// collides while the label count fits the pattern space.
fn allocate_labels(
    defined: &BTreeMap<String, (u32, u32)>,
    errors: &mut Vec<AsmError>,
) -> BTreeMap<String, Template> {
    let space: u16 = 1u16 << LABEL_BITS;
    let mut used: BTreeSet<u8> = BTreeSet::new();
    let mut out = BTreeMap::new();

    if defined.len() > space as usize {
        if let Some((name, (line, col))) = defined.iter().next() {
            errors.push(AsmError {
                line: *line,
                col: *col,
                message: format!(
                    "{} labels defined but only {space} {LABEL_BITS}-bit patterns exist \
                     (first: `{name}`); use raw %patterns for the rest",
                    defined.len()
                ),
            });
        }
        return out;
    }

    for name in defined.keys() {
        let start = (name_hash(name) % space as u64) as u16;
        let mut pattern = start;
        for _ in 0..space {
            if !used.contains(&(pattern as u8)) {
                break;
            }
            pattern = pattern.wrapping_add(1) % space;
        }
        used.insert(pattern as u8);
        out.insert(name.clone(), Template::new(LABEL_BITS, pattern as u8));
    }
    out
}

/// Named promoters use the raw hash, so the pattern is stable across files. Two names that
/// land on the same pattern are an error rather than a silent merge.
fn allocate_promoters(
    names: &BTreeSet<String>,
    items: &[Item],
    errors: &mut Vec<AsmError>,
) -> BTreeMap<String, Template> {
    let mut by_pattern: BTreeMap<u8, String> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for name in names {
        let t = promoter_pattern(name);
        if let Some(other) = by_pattern.get(&t.value) {
            let (line, col) = first_mention(items, name);
            errors.push(AsmError {
                line,
                col,
                message: format!(
                    "promoter `#{name}` hashes to the same pattern as `#{other}`; rename one"
                ),
            });
            continue;
        }
        by_pattern.insert(t.value, name.clone());
        out.insert(name.clone(), t);
    }
    out
}

fn first_mention(items: &[Item], name: &str) -> (u32, u32) {
    for item in items {
        if let Item::Instr {
            operand: Operand::Promoter(n),
            line,
            col,
            ..
        } = item
        {
            if n == name {
                return (*line, *col);
            }
        }
    }
    (0, 0)
}

/// Split a line into `(text, 1-based column)` tokens, stopping at a comment.
fn tokenize(line: &str) -> Vec<(&str, u32)> {
    let body = match line.find(';') {
        Some(i) => line.get(..i).unwrap_or(""),
        None => line,
    };
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in body.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                if let Some(t) = body.get(s..i) {
                    out.push((t, s as u32 + 1));
                }
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        if let Some(t) = body.get(s..) {
            out.push((t, s as u32 + 1));
        }
    }
    out
}

fn parse(src: &str, errors: &mut Vec<AsmError>) -> Vec<Item> {
    let mut items = Vec::new();
    for (li, raw) in src.lines().enumerate() {
        let line = li as u32 + 1;
        let tokens = tokenize(raw);
        let mut i = 0usize;

        // Leading label definitions.
        while let Some((tok, col)) = tokens.get(i).copied() {
            let Some(name) = tok.strip_suffix(':') else {
                break;
            };
            i = i.saturating_add(1);
            if name.is_empty() {
                errors.push(AsmError {
                    line,
                    col,
                    message: "empty label name".to_string(),
                });
                continue;
            }
            if !is_ident(name) {
                errors.push(AsmError {
                    line,
                    col,
                    message: format!(
                        "`{name}` is not a valid label name (letters, digits, `_`, not \
                         starting with a digit)"
                    ),
                });
                continue;
            }
            items.push(Item::LabelDef {
                name: name.to_string(),
                line,
                col,
            });
        }

        let Some((mnemonic, col)) = tokens.get(i).copied() else {
            continue;
        };
        i = i.saturating_add(1);

        let (base, variant) = match split_variant(mnemonic) {
            Ok(v) => v,
            Err(message) => {
                errors.push(AsmError { line, col, message });
                continue;
            }
        };
        let Some(op) = Op::from_name(&base.to_ascii_uppercase()) else {
            errors.push(AsmError {
                line,
                col,
                message: format!("unknown instruction `{base}`"),
            });
            continue;
        };

        let operand_token = tokens.get(i).copied();
        if operand_token.is_some() {
            i = i.saturating_add(1);
        }
        if let Some((extra, ecol)) = tokens.get(i).copied() {
            errors.push(AsmError {
                line,
                col: ecol,
                message: format!("unexpected `{extra}`: one operand at most"),
            });
        }

        let operand = match parse_operand(op, operand_token, line, errors) {
            Some(o) => o,
            None => continue,
        };

        items.push(Item::Instr {
            op,
            variant,
            operand,
            line,
            col,
        });
    }
    items
}

/// `ADD~2` selects the third of the four bytes that decode to `ADD` (SPEC §4.2). The
/// disassembler emits this so that any byte string round-trips, evolved genomes included.
fn split_variant(mnemonic: &str) -> Result<(&str, u8), String> {
    let Some((base, suffix)) = mnemonic.split_once('~') else {
        return Ok((mnemonic, 0));
    };
    match suffix.parse::<u8>() {
        Ok(v) if v < 4 => Ok((base, v)),
        _ => Err(format!(
            "`~{suffix}` is not a degenerate-encoding variant; expected ~1, ~2 or ~3"
        )),
    }
}

fn parse_operand(
    op: Op,
    token: Option<(&str, u32)>,
    line: u32,
    errors: &mut Vec<AsmError>,
) -> Option<Operand> {
    let Some((tok, col)) = token else {
        if !op.takes_template() {
            return Some(Operand::None);
        }
        // A template opcode with nothing after it has a zero-length template, which makes
        // it a no-op (and IMM a push of 0). Legal, and what the disassembler emits.
        return Some(Operand::None);
    };

    if !op.takes_template() {
        errors.push(AsmError {
            line,
            col,
            message: format!("`{}` takes no operand", op.name()),
        });
        return None;
    }

    // Raw template letters work on every template opcode.
    if let Some(bits) = tok.strip_prefix('%') {
        return match parse_raw(bits) {
            Ok(t) => Some(Operand::Raw(t)),
            Err(message) => {
                errors.push(AsmError { line, col, message });
                None
            }
        };
    }

    if let Some(name) = tok.strip_prefix('#') {
        if !matches!(op, Op::Gene | Op::Express) {
            errors.push(AsmError {
                line,
                col,
                message: format!(
                    "`{}` searches for a complementary template, so it takes a label or a \
                     raw %pattern; `#name` promoters belong on GENE and EXPRESS",
                    op.name()
                ),
            });
            return None;
        }
        if !is_ident(name) {
            errors.push(AsmError {
                line,
                col,
                message: format!("`#{name}` is not a valid promoter name"),
            });
            return None;
        }
        return Some(Operand::Promoter(name.to_string()));
    }

    if op == Op::Imm {
        return match parse_number(tok) {
            Ok((value, width)) => Some(Operand::Number { value, width }),
            Err(message) => {
                errors.push(AsmError { line, col, message });
                None
            }
        };
    }

    if matches!(op, Op::Gene | Op::Express) {
        errors.push(AsmError {
            line,
            col,
            message: format!(
                "`{}` binds promoters by similarity, not by complement; write `#{tok}` or a \
                 raw %pattern",
                op.name()
            ),
        });
        return None;
    }

    if !is_ident(tok) {
        errors.push(AsmError {
            line,
            col,
            message: format!("`{tok}` is not a valid label name"),
        });
        return None;
    }
    Some(Operand::Label(tok.to_string()))
}

fn parse_raw(bits: &str) -> Result<Template, String> {
    if bits.len() > mm_core::MAX_TEMPLATE_LEN as usize {
        return Err(format!(
            "template `%{bits}` is {} letters; the maximum is {}",
            bits.len(),
            mm_core::MAX_TEMPLATE_LEN
        ));
    }
    let mut value = 0u8;
    for (i, c) in bits.chars().enumerate() {
        match c {
            '0' => {}
            '1' => value |= 1u8 << i,
            _ => return Err(format!("`{c}` in template `%{bits}`: expected 0 or 1")),
        }
    }
    Ok(Template::new(bits.len() as u8, value))
}

/// `42`, `0x2A`, or either with `:width` to pin the number of letters.
fn parse_number(tok: &str) -> Result<(u8, u8), String> {
    let (num, width) = match tok.split_once(':') {
        Some((n, w)) => {
            let w: u8 = w
                .parse()
                .map_err(|_| format!("`{w}` is not a template width"))?;
            if w > mm_core::MAX_TEMPLATE_LEN {
                return Err(format!(
                    "width {w} exceeds the {}-letter maximum",
                    mm_core::MAX_TEMPLATE_LEN
                ));
            }
            (n, Some(w))
        }
        None => (tok, None),
    };

    let value: u8 = if let Some(hex) = num.strip_prefix("0x").or_else(|| num.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16).map_err(|_| format!("`{num}` is not a byte value"))?
    } else {
        num.parse()
            .map_err(|_| format!("`{num}` is not a byte value (0-255)"))?
    };

    let minimal = 8u8.saturating_sub(value.leading_zeros() as u8);
    match width {
        Some(w) if w < minimal => Err(format!(
            "{value} needs {minimal} letters but the width is pinned to {w}"
        )),
        Some(w) => Ok((value, w)),
        None => Ok((value, minimal)),
    }
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(src: &str) -> Vec<u8> {
        assemble(src).map(|a| a.bytes).unwrap()
    }

    fn err(src: &str) -> String {
        assemble(src).unwrap_err().to_string()
    }

    #[test]
    fn plain_instructions_use_canonical_bytes() {
        assert_eq!(bytes("ADD\nSUB\nHALT"), vec![0x10, 0x11, 0x2E]);
    }

    #[test]
    fn mnemonics_are_case_insensitive() {
        assert_eq!(bytes("add"), bytes("ADD"));
        assert_eq!(bytes("Add"), bytes("ADD"));
    }

    #[test]
    fn comments_and_blank_lines_emit_nothing() {
        assert_eq!(bytes("; nothing\n\n   \nADD ; trailing"), vec![0x10]);
    }

    #[test]
    fn degenerate_variants_select_the_other_bytes() {
        assert_eq!(bytes("ADD~1"), vec![0x50]);
        assert_eq!(bytes("ADD~2"), vec![0x90]);
        assert_eq!(bytes("ADD~3"), vec![0xD0]);
        assert!(err("ADD~4").contains("degenerate-encoding variant"));
    }

    #[test]
    fn raw_templates_are_first_letter_leftmost() {
        // %101 -> NOP1 NOP0 NOP1
        assert_eq!(bytes("IMM %101"), vec![0x02, 0x01, 0x00, 0x01]);
        assert_eq!(bytes("JMPF %"), vec![0x20]);
        assert_eq!(bytes("JMPF"), vec![0x20]);
    }

    #[test]
    fn numeric_literals_use_minimal_width_unless_pinned() {
        // 5 = 0b101, three letters, LSB first -> NOP1 NOP0 NOP1
        assert_eq!(bytes("IMM 5"), vec![0x02, 0x01, 0x00, 0x01]);
        assert_eq!(bytes("IMM 5:5"), vec![0x02, 0x01, 0x00, 0x01, 0x00, 0x00]);
        assert_eq!(bytes("IMM 0"), vec![0x02]);
        assert_eq!(bytes("IMM 0x2A"), bytes("IMM 42"));
        assert!(err("IMM 5:2").contains("pinned"));
        assert!(err("IMM 256").contains("not a byte value"));
    }

    #[test]
    fn a_jump_emits_the_complement_of_its_label() {
        let a = assemble("start:\nJMPB start").unwrap();
        let label = a.labels.get("start").unwrap().template;
        assert_eq!(label.len, LABEL_BITS);
        // label letters, then JMPB, then the complementary letters
        assert_eq!(a.bytes.len(), 1 + 2 * LABEL_BITS as usize);
        let emitted_at_label = &a.bytes[..LABEL_BITS as usize];
        let emitted_at_jump = &a.bytes[LABEL_BITS as usize + 1..];
        for i in 0..LABEL_BITS as usize {
            assert_ne!(
                emitted_at_label[i], emitted_at_jump[i],
                "letter {i} should be base-paired"
            );
        }
    }

    #[test]
    fn label_patterns_are_distinct_and_deterministic() {
        let src = "a:\nADD\nb:\nADD\nc:\nADD\nd:\nADD\ne:\nADD\nJMPF a\nADD\nJMPF b\nADD\n\
                   JMPF c\nADD\nJMPF d\nADD\nJMPF e\nADD";
        let one = assemble(src).unwrap();
        let two = assemble(src).unwrap();
        assert_eq!(one.bytes, two.bytes);
        let mut seen = BTreeSet::new();
        for info in one.labels.values() {
            assert!(seen.insert(info.template.value), "label patterns collided");
        }
    }

    #[test]
    fn too_many_labels_is_an_error_not_a_collision() {
        let mut src = String::new();
        for i in 0..17 {
            src.push_str(&format!("l{i}:\n"));
        }
        assert!(err(&src).contains("patterns exist"));
    }

    #[test]
    fn promoter_names_hash_the_same_everywhere() {
        let a = assemble("GENE #hunt").unwrap();
        let b = assemble("EXPRESS #hunt").unwrap();
        assert_eq!(&a.bytes[1..], &b.bytes[1..]);
        assert_eq!(a.promoters["hunt"], promoter_pattern("hunt"));
        assert_eq!(a.promoters["hunt"].len, PROMOTER_BITS);
    }

    #[test]
    fn operand_kinds_are_checked_against_the_opcode() {
        assert!(err("JMPF #hunt").contains("complementary template"));
        assert!(err("EXPRESS loop").contains("by similarity"));
        assert!(err("ADD 5").contains("takes no operand"));
        assert!(err("JMPF nowhere").contains("undefined label"));
        assert!(err("BOGUS").contains("unknown instruction"));
        assert!(err("a:\na:\nJMPF a").contains("defined more than once"));
    }

    #[test]
    fn errors_are_collected_and_sorted() {
        let e = assemble("BOGUS\nADD 5\nJMPF nowhere").unwrap_err();
        assert_eq!(e.0.len(), 3);
        assert_eq!(e.0[0].line, 1);
        assert_eq!(e.0[1].line, 2);
        assert_eq!(e.0[2].line, 3);
    }

    #[test]
    fn source_map_covers_every_emitted_byte() {
        let a = assemble("start:\n        IMM 5\n        JMPB start").unwrap();
        for b in 0..a.bytes.len() as u32 {
            assert!(a.source_map.lookup(b).is_some(), "byte {b} unmapped");
        }
        let imm = a.source_map.lookup(LABEL_BITS as u32).unwrap();
        assert_eq!(imm.line, 2);
        assert_eq!(imm.col, 9);
        assert_eq!(imm.len, 4);
    }

    #[test]
    fn a_template_that_a_label_would_extend_is_refused() {
        // The four letters of the jump and the four of the label would fuse into one
        // eight-letter run, and the jump would search for something nobody wrote.
        let e = err("top:\n        ADD\n        JMPB    top\ndone:\n        HALT");
        assert!(e.contains("extend it to 8"), "{e}");
        // An instruction between them separates the runs.
        assert!(assemble(
            "top:\n        ADD\n        JMPB    top\n        HALT\ndone:\n        HALT"
        )
        .is_ok());
    }

    #[test]
    fn adjacent_labels_are_refused() {
        assert!(err("a:\nb:\nJMPF a\nJMPF b").contains("extend it to 8"));
    }

    #[test]
    fn a_full_length_template_cannot_be_extended() {
        // Eight letters is the cap, so nothing following changes what the VM reads.
        assert!(assemble("GENE #g\nNOP1\nNOP1\nEXPRESS #g").is_ok());
    }

    #[test]
    fn an_empty_template_is_not_checked() {
        // What the disassembler emits when a run's letters are non-canonical NOP bytes.
        assert!(assemble("IMM\nNOP1~1\nNOP0~2").is_ok());
    }

    #[test]
    fn a_label_may_share_a_line_with_an_instruction() {
        let a = assemble("loop: COPYB\nLOOPLN loop").unwrap();
        assert_eq!(a.labels["loop"].byte, 0);
        assert_eq!(a.bytes[LABEL_BITS as usize], Op::CopyB.canonical_byte());
    }
}
