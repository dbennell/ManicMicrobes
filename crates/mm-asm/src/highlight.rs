//! Syntax highlighting for `.mm` assembly (M6).
//!
//! # Why this lives in the assembler
//!
//! The editor needs to know which word is an opcode, which is a label and which is a comment.
//! The assembler already knows, because it has to. Putting a second, approximate answer in the
//! front-end would mean two definitions of the language that could disagree — and the one that
//! disagreed would be the one people are looking at while they write.
//!
//! So this classifies against [`mm_core::isa::Op::from_name`] and the same operand rules the
//! assembler applies, and a test asserts that every canonical opcode name highlights as an
//! opcode. It cannot drift from the real language without failing.
//!
//! # What it does not do
//!
//! It does not report errors. Diagnostics come from actually assembling — `assemble` returns
//! every problem with a line and column, which is what the editor underlines. A highlighter
//! that also guessed at errors would guess differently from the assembler and be wrong in a
//! way that looks authoritative.

use mm_core::isa::Op;

/// What a run of characters is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    /// `; anything to end of line`.
    Comment,
    /// A canonical opcode name.
    Opcode,
    /// A `#name` promoter reference.
    Promoter,
    /// A `%pattern` raw template.
    Pattern,
    /// A numeric literal, with or without a `:width` suffix.
    Number,
    /// A label definition, `name:`.
    LabelDef,
    /// A bare identifier used as an operand: a jump target.
    LabelRef,
    /// Anything the assembler would reject, or does not recognise.
    Unknown,
    /// Spaces and tabs between tokens.
    Space,
}

/// A classified run of the source, in byte offsets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Token {
    pub kind: TokenKind,
    /// Byte offsets into the line, not into the file.
    pub start: usize,
    pub end: usize,
}

/// Classify one line of `.mm` source.
///
/// Per line rather than per file, because that is how a text editor repaints: it knows which
/// line changed and would rather not re-lex a thousand others. There is no cross-line state in
/// the language — no block comments, no continuations — so a line is genuinely enough.
#[must_use]
pub fn line(src: &str) -> Vec<Token> {
    let bytes = src.as_bytes();
    let mut out: Vec<Token> = Vec::new();
    let mut at = 0usize;
    // The first non-comment word decides how the rest of the line reads: an opcode takes an
    // operand, a `name:` is a definition on its own.
    let mut seen_opcode = false;

    while at < bytes.len() {
        let c = bytes[at];
        if c == b';' {
            out.push(Token {
                kind: TokenKind::Comment,
                start: at,
                end: bytes.len(),
            });
            break;
        }
        if c.is_ascii_whitespace() {
            let start = at;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            out.push(Token {
                kind: TokenKind::Space,
                start,
                end: at,
            });
            continue;
        }
        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b';' {
            at += 1;
        }
        let word = &src[start..at];
        let kind = classify(word, seen_opcode);
        if kind == TokenKind::Opcode {
            seen_opcode = true;
        }
        out.push(Token {
            kind,
            start,
            end: at,
        });
    }
    out
}

fn classify(word: &str, after_opcode: bool) -> TokenKind {
    if word.is_empty() {
        return TokenKind::Unknown;
    }
    if let Some(rest) = word.strip_prefix('#') {
        return if rest.is_empty() {
            TokenKind::Unknown
        } else {
            TokenKind::Promoter
        };
    }
    if let Some(rest) = word.strip_prefix('%') {
        return if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_alphanumeric()) {
            TokenKind::Unknown
        } else {
            TokenKind::Pattern
        };
    }
    if word.ends_with(':') && !after_opcode {
        let name = &word[..word.len() - 1];
        return if name.is_empty() {
            TokenKind::Unknown
        } else {
            TokenKind::LabelDef
        };
    }
    // A number, possibly with the `:width` suffix the assembler accepts.
    let numeric = word.split_once(':').map_or(word, |(n, _)| n);
    let is_number = if let Some(hex) = numeric
        .strip_prefix("0x")
        .or_else(|| numeric.strip_prefix("0X"))
    {
        !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit())
    } else {
        !numeric.is_empty() && numeric.bytes().all(|b| b.is_ascii_digit())
    };
    if is_number {
        return TokenKind::Number;
    }
    // Opcode names are matched by the ISA itself, so this cannot drift from the language.
    if Op::from_name(word).is_some() {
        return TokenKind::Opcode;
    }
    if after_opcode && word.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return TokenKind::LabelRef;
    }
    TokenKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(TokenKind, String)> {
        line(src)
            .into_iter()
            .filter(|t| t.kind != TokenKind::Space)
            .map(|t| (t.kind, src[t.start..t.end].to_string()))
            .collect()
    }

    #[test]
    fn every_opcode_name_highlights_as_an_opcode() {
        // The property that stops this drifting from the language. If an opcode is renamed or
        // added and the highlighter is not updated, this fails.
        for byte in 0..64u8 {
            let op = Op::from_byte(byte);
            let name = op.name();
            assert_eq!(
                classify(name, false),
                TokenKind::Opcode,
                "`{name}` is a real opcode but does not highlight as one"
            );
        }
    }

    #[test]
    fn a_comment_runs_to_the_end_of_the_line() {
        let src = "        IMM     40      ; the nucleus param";
        let toks = kinds(src);
        assert_eq!(toks[0], (TokenKind::Opcode, "IMM".to_string()));
        assert_eq!(toks[1], (TokenKind::Number, "40".to_string()));
        assert_eq!(
            toks[2],
            (TokenKind::Comment, "; the nucleus param".to_string())
        );
    }

    #[test]
    fn a_semicolon_inside_a_line_still_starts_a_comment() {
        // There are no strings in the language, so nothing can contain a semicolon.
        let toks = kinds("HALT;done");
        assert_eq!(toks[0].0, TokenKind::Opcode);
        assert_eq!(toks[1], (TokenKind::Comment, ";done".to_string()));
    }

    #[test]
    fn labels_and_their_uses_are_told_apart() {
        assert_eq!(
            kinds("enough:"),
            vec![(TokenKind::LabelDef, "enough:".to_string())]
        );
        assert_eq!(
            kinds("        JMPNZ   enough"),
            vec![
                (TokenKind::Opcode, "JMPNZ".to_string()),
                (TokenKind::LabelRef, "enough".to_string()),
            ]
        );
    }

    #[test]
    fn promoters_and_patterns_are_their_own_kinds() {
        assert_eq!(
            kinds("        EXPRESS #build"),
            vec![
                (TokenKind::Opcode, "EXPRESS".to_string()),
                (TokenKind::Promoter, "#build".to_string()),
            ]
        );
        assert_eq!(
            kinds("        GENE    %aabb"),
            vec![
                (TokenKind::Opcode, "GENE".to_string()),
                (TokenKind::Pattern, "%aabb".to_string()),
            ]
        );
    }

    #[test]
    fn numbers_in_every_form_the_assembler_takes() {
        for word in ["0", "40", "255", "0xFF", "0x0a", "13:4"] {
            assert_eq!(
                classify(word, true),
                TokenKind::Number,
                "`{word}` should be a number"
            );
        }
        for word in ["0x", "12x", "", "#"] {
            assert_ne!(
                classify(word, true),
                TokenKind::Number,
                "`{word}` should not be a number"
            );
        }
    }

    #[test]
    fn spans_cover_the_line_exactly_and_do_not_overlap() {
        // The editor paints by span. A gap would leave characters unstyled and an overlap
        // would paint one twice.
        let src = "        IMM     40      ; a comment";
        let toks = line(src);
        assert_eq!(toks.first().map(|t| t.start), Some(0));
        assert_eq!(toks.last().map(|t| t.end), Some(src.len()));
        for pair in toks.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "gap or overlap in {toks:?}");
        }
    }

    #[test]
    fn an_empty_line_produces_nothing_to_paint() {
        assert!(line("").is_empty());
        assert_eq!(kinds("   "), vec![]);
    }

    #[test]
    fn the_real_ancestor_highlights_without_unknowns() {
        // The strongest check available: a genome that is known to assemble should contain no
        // word the highlighter cannot name.
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm");
        let src = std::fs::read_to_string(path).expect("the ancestor is in the repository");
        for (n, text) in src.lines().enumerate() {
            for token in line(text) {
                assert_ne!(
                    token.kind,
                    TokenKind::Unknown,
                    "line {}: `{}` was not recognised",
                    n + 1,
                    &text[token.start..token.end]
                );
            }
        }
    }
}
