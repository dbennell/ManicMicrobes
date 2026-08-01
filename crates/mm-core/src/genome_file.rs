//! A shareable genome file (M6).
//!
//! > Genome import/export as a shareable single file including ISA version.
//!
//! # Why this is not just the bytes
//!
//! A genome is meaningless without the instruction set it was written against. Opcodes are
//! `byte % 64` against a fixed table (SPEC §3), and hard rule 8 says that changing that table
//! is a version bump because **archived genomes must be replayed under the version they
//! evolved in**. A bare `.mm` or a bare byte string carries no such stamp, so a genome that
//! arrived from someone else's build would run — silently, wrongly, as a different organism.
//!
//! So the file carries the ISA version, and loading one from a different ISA is an error the
//! caller has to look at, not a warning it can miss. That is M6's fourth acceptance test.
//!
//! # Text, not binary
//!
//! This is the format people paste into forum posts and attach to messages. It is line-based
//! UTF-8 with a header and a hex body, so it survives copy-paste, diffs legibly, and can be
//! eyeballed to see whether it is what it claims to be. The bytes are the contract; everything
//! else in the header is provenance.
//!
//! ```text
//! manic-microbes-genome 1
//! isa 1
//! name drifter
//! bytes 332
//! hash 3f8a1c09d4e5b672
//! ---
//! 2e04102e...
//! ```

use crate::genome::Genome;
use crate::isa::ISA_VERSION;

/// The file format's own version, separate from the ISA version.
///
/// One says "this build knows how to parse the file", the other says "this build agrees about
/// what the bytes mean". They move independently: a new header field is a format bump and not
/// an ISA bump, and a new opcode is the reverse.
pub const GENOME_FILE_VERSION: u16 = 1;

const MAGIC: &str = "manic-microbes-genome";

/// What went wrong loading a genome file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GenomeFileError {
    /// Not a genome file at all.
    NotAGenomeFile,
    /// A file format this build cannot parse.
    FormatVersion { found: u16, expected: u16 },
    /// The genome was written against a different instruction set.
    ///
    /// Deliberately not a warning. Under a different ISA the same bytes are a different
    /// program, so running it anyway would produce a plausible-looking organism that is not
    /// the one in the file (hard rule 8).
    IsaMismatch { found: u16, expected: u16 },
    /// A header line this build does not understand, or one that is missing.
    BadHeader(String),
    /// The body is not hex, or is an odd number of digits.
    BadBody(String),
    /// The bytes do not hash to what the header claims.
    HashMismatch { found: u64, expected: u64 },
    /// Longer than [`crate::genome::MAX_GENOME_LEN`].
    TooLong(usize),
}

impl std::fmt::Display for GenomeFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenomeFileError::NotAGenomeFile => {
                write!(f, "not a Manic Microbes genome file")
            }
            GenomeFileError::FormatVersion { found, expected } => write!(
                f,
                "genome file format {found}, but this build reads {expected}"
            ),
            GenomeFileError::IsaMismatch { found, expected } => write!(
                f,
                "this genome was written for ISA version {found} and this build is ISA \
                 {expected}. The same bytes mean different instructions under a different \
                 instruction set, so it has not been loaded. Run it under a build of ISA \
                 {found} to see what it really does."
            ),
            GenomeFileError::BadHeader(what) => write!(f, "bad header: {what}"),
            GenomeFileError::BadBody(what) => write!(f, "bad genome body: {what}"),
            GenomeFileError::HashMismatch { found, expected } => write!(
                f,
                "the genome hashes to {found:016x} but the file says {expected:016x}; it has \
                 been altered or truncated"
            ),
            GenomeFileError::TooLong(n) => write!(f, "{n} bytes is longer than a genome may be"),
        }
    }
}

impl std::error::Error for GenomeFileError {}

/// A genome plus the provenance needed to know what it is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GenomeFile {
    pub bytes: Vec<u8>,
    pub isa: u16,
    /// A human label. Never load-bearing — two files with the same name are still two files,
    /// and the bytes decide.
    pub name: String,
    /// Free-text provenance: which species, which run, who wrote it.
    pub notes: Vec<String>,
}

impl GenomeFile {
    #[must_use]
    pub fn new(bytes: Vec<u8>, name: impl Into<String>) -> GenomeFile {
        GenomeFile {
            bytes,
            isa: ISA_VERSION,
            name: name.into(),
            notes: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> GenomeFile {
        self.notes.push(sanitise(&note.into()));
        self
    }

    /// Render to the shareable text form.
    #[must_use]
    pub fn to_text(&self) -> String {
        let hash = crate::genome::content_hash(&self.bytes);
        let mut out = format!(
            "{MAGIC} {GENOME_FILE_VERSION}\nisa {}\nname {}\nbytes {}\nhash {hash:016x}\n",
            self.isa,
            sanitise(&self.name),
            self.bytes.len(),
        );
        for note in &self.notes {
            out.push_str(&format!("note {note}\n"));
        }
        out.push_str("---\n");
        // Wrapped at 64 bytes a line, so a genome stays readable in a message and diffs
        // line-by-line rather than as one enormous changed line.
        for chunk in self.bytes.chunks(32) {
            for b in chunk {
                out.push_str(&format!("{b:02x}"));
            }
            out.push('\n');
        }
        out
    }

    /// Parse the shareable text form.
    ///
    /// # Errors
    ///
    /// Not a genome file, a format or ISA version this build will not honour, a malformed
    /// header or body, or a body that does not match the hash in the header.
    pub fn from_text(text: &str) -> Result<GenomeFile, GenomeFileError> {
        let mut lines = text.lines();
        let header = lines.next().ok_or(GenomeFileError::NotAGenomeFile)?;
        let Some(version) = header.strip_prefix(MAGIC).map(str::trim) else {
            return Err(GenomeFileError::NotAGenomeFile);
        };
        let format: u16 = version
            .parse()
            .map_err(|_| GenomeFileError::BadHeader(format!("format version `{version}`")))?;
        if format != GENOME_FILE_VERSION {
            return Err(GenomeFileError::FormatVersion {
                found: format,
                expected: GENOME_FILE_VERSION,
            });
        }

        let mut isa = None;
        let mut name = String::new();
        let mut declared_len = None;
        let mut declared_hash = None;
        let mut notes = Vec::new();
        let mut body = String::new();
        let mut in_body = false;

        for line in lines {
            if in_body {
                body.push_str(line.trim());
                continue;
            }
            let line = line.trim();
            if line == "---" {
                in_body = true;
                continue;
            }
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "isa" => {
                    isa = Some(value.parse::<u16>().map_err(|_| {
                        GenomeFileError::BadHeader(format!("isa version `{value}`"))
                    })?)
                }
                "name" => name = value.to_string(),
                "bytes" => {
                    declared_len =
                        Some(value.parse::<usize>().map_err(|_| {
                            GenomeFileError::BadHeader(format!("byte count `{value}`"))
                        })?)
                }
                "hash" => {
                    declared_hash = Some(
                        u64::from_str_radix(value, 16)
                            .map_err(|_| GenomeFileError::BadHeader(format!("hash `{value}`")))?,
                    )
                }
                "note" => notes.push(value.to_string()),
                // Unknown keys are ignored rather than refused: a file written by a later
                // build that added a provenance field should still load, because none of
                // those fields change what the genome *is*. Anything that could change
                // meaning goes in the version numbers, which are checked.
                _ => {}
            }
        }
        if !in_body {
            return Err(GenomeFileError::BadHeader(
                "no `---` separating header from genome".to_string(),
            ));
        }

        let isa = isa.ok_or_else(|| GenomeFileError::BadHeader("no isa version".to_string()))?;
        // Checked before parsing the body: under a different ISA the bytes are a different
        // program, and there is nothing to be gained by decoding them first.
        if isa != ISA_VERSION {
            return Err(GenomeFileError::IsaMismatch {
                found: isa,
                expected: ISA_VERSION,
            });
        }

        if !body.len().is_multiple_of(2) {
            return Err(GenomeFileError::BadBody(format!(
                "{} hex digits is an odd number",
                body.len()
            )));
        }
        let mut bytes = Vec::with_capacity(body.len() / 2);
        let raw = body.as_bytes();
        for pair in raw.chunks(2) {
            let s = std::str::from_utf8(pair)
                .map_err(|_| GenomeFileError::BadBody("not text".to_string()))?;
            bytes.push(
                u8::from_str_radix(s, 16)
                    .map_err(|_| GenomeFileError::BadBody(format!("`{s}` is not a hex byte")))?,
            );
        }
        if bytes.len() > crate::genome::MAX_GENOME_LEN {
            return Err(GenomeFileError::TooLong(bytes.len()));
        }
        if let Some(want) = declared_len {
            if want != bytes.len() {
                return Err(GenomeFileError::BadBody(format!(
                    "header says {want} bytes, body has {}",
                    bytes.len()
                )));
            }
        }
        // The hash is checked last, so a file that is wrong in a more specific way says so
        // rather than reporting a mismatch the reader would have to work backwards from.
        if let Some(want) = declared_hash {
            let found = crate::genome::content_hash(&bytes);
            if found != want {
                return Err(GenomeFileError::HashMismatch {
                    found,
                    expected: want,
                });
            }
        }

        Ok(GenomeFile {
            bytes,
            isa,
            name,
            notes,
        })
    }

    /// Turn the loaded bytes into a genome.
    ///
    /// # Errors
    ///
    /// Only if the bytes exceed the maximum length, which `from_text` already refuses.
    pub fn to_genome(&self) -> Result<Genome, crate::genome::GenomeError> {
        Genome::new(self.bytes.clone())
    }
}

/// Strip anything that would break the line-based format out of a header value.
///
/// Newlines would inject header lines; control characters would make the file unreadable.
/// Quietly rather than as an error, because a name is decoration and refusing to export a
/// genome because someone put a tab in its name would be absurd.
fn sanitise(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        (0..200u16).map(|i| (i * 7 % 251) as u8).collect()
    }

    #[test]
    fn a_genome_survives_the_round_trip() {
        let file = GenomeFile::new(sample(), "drifter").with_note("from Cilius rapidus");
        let text = file.to_text();
        let back = GenomeFile::from_text(&text).expect("parses");
        assert_eq!(back.bytes, file.bytes);
        assert_eq!(back.name, "drifter");
        assert_eq!(back.notes, vec!["from Cilius rapidus".to_string()]);
        assert_eq!(back.isa, ISA_VERSION);
    }

    #[test]
    fn the_empty_genome_round_trips() {
        let text = GenomeFile::new(Vec::new(), "nothing").to_text();
        assert_eq!(
            GenomeFile::from_text(&text).expect("parses").bytes,
            Vec::new()
        );
    }

    #[test]
    fn every_byte_value_survives() {
        // The hex body has to carry all 256 values, including the ones that look like
        // whitespace or line endings if anyone were tempted to write it as raw bytes.
        let all: Vec<u8> = (0..=255u8).collect();
        let text = GenomeFile::new(all.clone(), "all").to_text();
        assert_eq!(GenomeFile::from_text(&text).expect("parses").bytes, all);
    }

    #[test]
    fn a_genome_from_another_isa_is_refused_not_warned_about() {
        // M6 acceptance 4. Under a different ISA the same bytes are a different program.
        let mut text = GenomeFile::new(sample(), "alien").to_text();
        text = text.replace(&format!("isa {ISA_VERSION}"), "isa 99");
        let err = GenomeFile::from_text(&text).expect_err("must not load");
        assert_eq!(
            err,
            GenomeFileError::IsaMismatch {
                found: 99,
                expected: ISA_VERSION
            }
        );
        // And the message says what to do about it, not merely that something is wrong.
        let text = err.to_string();
        assert!(text.contains("has not been loaded"), "{text}");
        assert!(text.contains("99"), "{text}");
    }

    #[test]
    fn a_tampered_genome_is_caught_by_its_hash() {
        let text = GenomeFile::new(sample(), "x").to_text();
        // Flip a nibble in the body.
        let at = text.find("---\n").expect("separator") + 4;
        let mut broken: Vec<char> = text.chars().collect();
        broken[at] = if broken[at] == 'a' { 'b' } else { 'a' };
        let broken: String = broken.into_iter().collect();
        assert!(matches!(
            GenomeFile::from_text(&broken),
            Err(GenomeFileError::HashMismatch { .. })
        ));
    }

    #[test]
    fn a_truncated_body_is_caught_by_its_length() {
        let text = GenomeFile::new(sample(), "x").to_text();
        let cut = &text[..text.len() - 40];
        let err = GenomeFile::from_text(cut).expect_err("must not load");
        assert!(
            matches!(err, GenomeFileError::BadBody(_)),
            "expected a body error, got {err}"
        );
    }

    #[test]
    fn a_foreign_file_is_refused() {
        assert_eq!(
            GenomeFile::from_text("hello world").unwrap_err(),
            GenomeFileError::NotAGenomeFile
        );
        assert_eq!(
            GenomeFile::from_text("").unwrap_err(),
            GenomeFileError::NotAGenomeFile
        );
        // A file that says it is one but is from the future.
        let err = GenomeFile::from_text(&format!("{MAGIC} 999\nisa 1\n---\n")).unwrap_err();
        assert!(matches!(err, GenomeFileError::FormatVersion { .. }));
    }

    #[test]
    fn a_header_with_no_body_separator_says_so() {
        let err = GenomeFile::from_text(&format!("{MAGIC} 1\nisa 1\nname x\n")).unwrap_err();
        assert!(matches!(err, GenomeFileError::BadHeader(_)), "{err}");
    }

    #[test]
    fn an_unknown_header_field_is_ignored_rather_than_refused() {
        // Forward compatibility for provenance. Anything that could change what the genome
        // *means* lives in the two version numbers, and those are checked.
        let text = GenomeFile::new(sample(), "x").to_text();
        let with_extra = text.replace("---\n", "author somebody\nrating 5\n---\n");
        assert_eq!(
            GenomeFile::from_text(&with_extra).expect("parses").bytes,
            sample()
        );
    }

    #[test]
    fn a_name_cannot_inject_a_header_line() {
        let file = GenomeFile::new(sample(), "evil\nisa 99");
        let back = GenomeFile::from_text(&file.to_text()).expect("parses");
        assert_eq!(back.isa, ISA_VERSION, "a name smuggled in a header line");
        assert!(!back.name.contains('\n'));
    }
}
