//! The genome editor (M6).
//!
//! > `.mm` syntax highlighting, assembler diagnostics with source positions, disassembly of
//! > any live genome with source map where available.
//!
//! # What this is
//!
//! A text buffer that knows it is `.mm`. It holds the source, re-assembles when asked, and
//! keeps whatever came back: bytes and a source map on success, a list of positioned errors on
//! failure. The panel that draws it needs no opinion about the language — highlighting comes
//! from [`mm_asm::highlight`], which classifies against the real opcode table, and diagnostics
//! come from actually assembling.
//!
//! # Why it does not assemble on every keystroke
//!
//! Because half-typed source is *usually* broken, and a wall of errors that appears the moment
//! you start a line and clears when you finish it is noise, not feedback. The buffer tracks
//! whether it has changed since the last assemble and the front-end decides when to ask —
//! after a pause, or on a keypress. That decision belongs to the front-end, so it is not made
//! here.
//!
//! # Disassembly
//!
//! Any genome can be disassembled, because every byte sequence is a legal program (hard rule
//! 3). A genome that came from *this* buffer also has a source map, so the disassembly can be
//! lined up against the source it was built from. One that was picked off a living cell has no
//! source map and never will — it was never written by anybody — so it disassembles to
//! canonical form and says so.

use mm_asm::asm::{AsmError, Assembled};
use mm_asm::highlight::{self, Token};
use mm_core::genome_file::{GenomeFile, GenomeFileError};

/// What the last assemble produced.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Build {
    /// Nothing has been assembled yet.
    #[default]
    Untouched,
    Ok(Box<Assembled>),
    Failed(Vec<AsmError>),
}

impl Build {
    #[must_use]
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Build::Ok(a) => Some(&a.bytes),
            _ => None,
        }
    }

    #[must_use]
    pub fn errors(&self) -> &[AsmError] {
        match self {
            Build::Failed(e) => e,
            _ => &[],
        }
    }

    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self, Build::Ok(_))
    }
}

/// A `.mm` source buffer and the last thing the assembler said about it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Editor {
    source: String,
    build: Build,
    /// Whether the source has changed since the last assemble.
    dirty: bool,
    /// A name for the buffer, carried into any genome exported from it.
    pub name: String,
}

impl Editor {
    #[must_use]
    pub fn new() -> Editor {
        Editor {
            name: "untitled".to_string(),
            ..Editor::default()
        }
    }

    /// Open some source, unassembled.
    #[must_use]
    pub fn with_source(source: impl Into<String>, name: impl Into<String>) -> Editor {
        Editor {
            source: source.into(),
            build: Build::Untouched,
            dirty: true,
            name: name.into(),
        }
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn set_source(&mut self, source: impl Into<String>) {
        let source = source.into();
        if source != self.source {
            self.source = source;
            self.dirty = true;
        }
    }

    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[must_use]
    pub fn build(&self) -> &Build {
        &self.build
    }

    /// Assemble, keeping whatever comes back.
    pub fn assemble(&mut self) -> &Build {
        self.build = match mm_asm::assemble(&self.source) {
            Ok(a) => Build::Ok(Box::new(a)),
            Err(e) => Build::Failed(e.0),
        };
        self.dirty = false;
        &self.build
    }

    /// Highlighting for one line, by line number from zero.
    #[must_use]
    pub fn highlight(&self, line: usize) -> Vec<Token> {
        self.source
            .lines()
            .nth(line)
            .map_or_else(Vec::new, highlight::line)
    }

    /// Every error on a given line, for underlining it.
    #[must_use]
    pub fn errors_on(&self, line: usize) -> Vec<&AsmError> {
        // Assembler lines are one-based; editor lines here are zero-based, which is the one
        // place the two conventions meet and so the one place to convert.
        let want = line as u32 + 1;
        self.build
            .errors()
            .iter()
            .filter(|e| e.line == want)
            .collect()
    }

    /// A one-line summary for the status bar.
    #[must_use]
    pub fn status(&self) -> String {
        match &self.build {
            Build::Untouched => "not assembled".to_string(),
            Build::Ok(a) => format!(
                "{} bytes, {} labels, {} promoters{}",
                a.bytes.len(),
                a.labels.len(),
                a.promoters.len(),
                if self.dirty { " (stale)" } else { "" }
            ),
            Build::Failed(e) => format!("{} error{}", e.len(), if e.len() == 1 { "" } else { "s" }),
        }
    }

    /// Export what was assembled as a shareable genome file.
    ///
    /// `None` if the buffer has not assembled cleanly — there is no genome to export, and
    /// exporting the last one that worked would hand somebody a file that does not match the
    /// source it claims to be.
    #[must_use]
    pub fn export(&self) -> Option<GenomeFile> {
        let bytes = self.build.bytes()?;
        Some(GenomeFile::new(bytes.to_vec(), self.name.clone()).with_note("written in the editor"))
    }

    /// Load a shareable genome file, replacing the buffer with its disassembly.
    ///
    /// The bytes are the genome; the source is reconstructed. A file carries no source, so
    /// what appears is canonical assembly rather than whatever anyone originally wrote — and
    /// re-assembling it gives back the same bytes, which the round-trip tests in `mm-asm`
    /// guarantee.
    ///
    /// # Errors
    ///
    /// Anything [`GenomeFile::from_text`] refuses, including a genome from another ISA.
    pub fn load_genome_file(&mut self, text: &str) -> Result<(), GenomeFileError> {
        let file = GenomeFile::from_text(text)?;
        self.name = if file.name.is_empty() {
            "imported".to_string()
        } else {
            file.name.clone()
        };
        self.set_source(mm_asm::disassemble(&file.bytes).to_source());
        self.assemble();
        Ok(())
    }

    /// Replace the buffer with the disassembly of a genome taken off a live cell.
    ///
    /// The readable rendering, so templates arrive as `IMM 40` rather than `IMM %000101`. It
    /// reassembles to the identical bytes — see [`mm_asm::Line::to_readable`] — so nothing
    /// about the round trip changes, only what you have to look at while editing.
    pub fn load_bytes(&mut self, bytes: &[u8], name: impl Into<String>) {
        self.name = name.into();
        self.set_source(mm_asm::disassemble(bytes).to_readable());
        self.assemble();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ancestor_source() -> String {
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../genomes/ancestor.mm"
        ))
        .expect("the ancestor is in the repository")
    }

    #[test]
    fn a_good_source_assembles_and_reports_what_it_built() {
        let mut ed = Editor::with_source(ancestor_source(), "ancestor");
        assert!(ed.is_dirty());
        assert!(ed.assemble().is_ok());
        assert!(!ed.is_dirty());
        let status = ed.status();
        assert!(status.contains("bytes"), "{status}");
        assert!(!status.contains("stale"), "{status}");
        assert!(ed.build().bytes().is_some_and(|b| b.len() > 100));
    }

    #[test]
    fn a_broken_source_reports_every_error_with_a_position() {
        let mut ed = Editor::with_source("        NOTANOP\n        IMM     999\n", "broken");
        let build = ed.assemble();
        assert!(!build.is_ok());
        let errors = build.errors();
        assert!(errors.len() >= 2, "expected several errors, got {errors:?}");
        for e in errors {
            assert!(e.line >= 1, "an error with no line: {e:?}");
            assert!(!e.message.is_empty());
        }
        // And they land on the right lines for underlining.
        assert!(!ed.errors_on(0).is_empty(), "nothing reported on line 1");
        assert!(!ed.errors_on(1).is_empty(), "nothing reported on line 2");
        assert!(ed.errors_on(50).is_empty());
    }

    #[test]
    fn editing_marks_the_build_stale_rather_than_silently_lying() {
        let mut ed = Editor::with_source(ancestor_source(), "ancestor");
        ed.assemble();
        assert!(!ed.status().contains("stale"));
        ed.set_source(format!("{}\n        HALT\n", ed.source()));
        assert!(ed.is_dirty());
        assert!(
            ed.status().contains("stale"),
            "an edited buffer still claims its old build is current: {}",
            ed.status()
        );
    }

    #[test]
    fn setting_the_same_source_is_not_an_edit() {
        let mut ed = Editor::with_source(ancestor_source(), "x");
        ed.assemble();
        ed.set_source(ancestor_source());
        assert!(!ed.is_dirty(), "a no-op edit marked the build stale");
    }

    #[test]
    fn a_genome_survives_export_and_import() {
        // M6 acceptance 3, in the editor's terms: what goes out comes back the same.
        let mut ed = Editor::with_source(ancestor_source(), "ancestor");
        ed.assemble();
        let original = ed.build().bytes().expect("assembled").to_vec();
        let text = ed.export().expect("exports").to_text();

        let mut other = Editor::new();
        other.load_genome_file(&text).expect("loads");
        assert_eq!(
            other.build().bytes(),
            Some(original.as_slice()),
            "a genome changed passing through a file"
        );
        assert_eq!(other.name, "ancestor");
    }

    #[test]
    fn a_genome_from_another_isa_is_refused_by_the_editor_too() {
        let mut ed = Editor::with_source(ancestor_source(), "ancestor");
        ed.assemble();
        let text = ed
            .export()
            .expect("exports")
            .to_text()
            .replace(&format!("isa {}", mm_core::isa::ISA_VERSION), "isa 77");
        let mut other = Editor::with_source("        HALT\n", "mine");
        other.assemble();
        let before = other.source().to_string();
        let err = other.load_genome_file(&text).expect_err("must refuse");
        assert!(matches!(err, GenomeFileError::IsaMismatch { .. }));
        assert_eq!(
            other.source(),
            before,
            "a refused import overwrote the buffer anyway"
        );
    }

    #[test]
    fn a_genome_from_a_live_cell_disassembles_and_reassembles_to_itself() {
        // No source map — nobody wrote it — so it comes back as canonical assembly. What
        // matters is that assembling that assembly gives the same bytes.
        let evolved: Vec<u8> = (0..180u16)
            .map(|i| (i.wrapping_mul(53) % 256) as u8)
            .collect();
        let mut ed = Editor::new();
        ed.load_bytes(&evolved, "Cilius rapidus");
        assert!(
            ed.build().is_ok(),
            "a genome off a cell did not reassemble: {:?}",
            ed.build().errors()
        );
        assert_eq!(ed.build().bytes(), Some(evolved.as_slice()));
        assert_eq!(ed.name, "Cilius rapidus");
    }

    #[test]
    fn highlighting_lines_up_with_the_source() {
        let ed = Editor::with_source("        IMM     40      ; param\n", "x");
        let tokens = ed.highlight(0);
        assert!(!tokens.is_empty());
        // Past the end is empty rather than a panic: an editor scrolled past the last line
        // must not take the process with it.
        assert!(ed.highlight(9_999).is_empty());
    }

    #[test]
    fn an_unassembled_buffer_exports_nothing() {
        let ed = Editor::with_source(ancestor_source(), "x");
        assert!(
            ed.export().is_none(),
            "exported a genome from a buffer that was never assembled"
        );
        let mut broken = Editor::with_source("NOTANOP\n", "x");
        broken.assemble();
        assert!(
            broken.export().is_none(),
            "exported a genome from a buffer that does not assemble"
        );
    }
}
