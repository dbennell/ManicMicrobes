//! Mapping between genome bytes and the `.mm` source that produced them.
//!
//! The debugger and the editor (M6) need to point at a line when a breakpoint hits, and the
//! disassembler needs to annotate a live genome with its original source where one exists.
//! Both directions are needed, so the map is stored as a sorted list of spans and searched
//! by binary search rather than kept as two hash maps.

/// One assembled item — an instruction with its template, or a label's template letters —
/// and where it came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    /// First genome byte this item produced.
    pub byte: u32,
    /// How many bytes it produced. Never zero.
    pub len: u32,
    /// 1-based source line.
    pub line: u32,
    /// 1-based column of the item's first character.
    pub col: u32,
}

impl Span {
    #[must_use]
    pub const fn contains(&self, byte: u32) -> bool {
        byte >= self.byte && byte < self.byte.saturating_add(self.len)
    }
}

/// Byte offsets to source positions, ascending and non-overlapping by construction.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct SourceMap {
    spans: Vec<Span>,
}

impl SourceMap {
    #[must_use]
    pub fn new() -> SourceMap {
        SourceMap { spans: Vec::new() }
    }

    /// Append a span. Callers emit in ascending byte order, which the assembler does
    /// naturally; the invariant is checked in debug builds.
    pub fn push(&mut self, span: Span) {
        debug_assert!(
            self.spans
                .last()
                .is_none_or(|p| p.byte.saturating_add(p.len) <= span.byte),
            "source map spans must be appended in ascending, non-overlapping byte order"
        );
        self.spans.push(span);
    }

    #[must_use]
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The source position of a genome byte, if it came from source at all.
    #[must_use]
    pub fn lookup(&self, byte: u32) -> Option<Span> {
        let i = self
            .spans
            .partition_point(|s| s.byte.saturating_add(s.len) <= byte);
        self.spans.get(i).copied().filter(|s| s.contains(byte))
    }

    /// The first genome byte produced by a source line, if any.
    #[must_use]
    pub fn byte_of_line(&self, line: u32) -> Option<u32> {
        self.spans.iter().find(|s| s.line == line).map(|s| s.byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> SourceMap {
        let mut m = SourceMap::new();
        m.push(Span {
            byte: 0,
            len: 1,
            line: 1,
            col: 9,
        });
        m.push(Span {
            byte: 1,
            len: 5,
            line: 2,
            col: 9,
        });
        m.push(Span {
            byte: 8,
            len: 1,
            line: 4,
            col: 1,
        });
        m
    }

    #[test]
    fn lookup_finds_the_containing_span() {
        let m = map();
        assert_eq!(m.lookup(0).unwrap().line, 1);
        for b in 1..6 {
            assert_eq!(m.lookup(b).unwrap().line, 2, "byte {b}");
        }
        assert_eq!(m.lookup(8).unwrap().line, 4);
    }

    #[test]
    fn lookup_reports_gaps_and_overruns() {
        let m = map();
        assert_eq!(m.lookup(6), None);
        assert_eq!(m.lookup(7), None);
        assert_eq!(m.lookup(9), None);
        assert_eq!(m.lookup(u32::MAX), None);
    }

    #[test]
    fn line_to_byte() {
        let m = map();
        assert_eq!(m.byte_of_line(2), Some(1));
        assert_eq!(m.byte_of_line(3), None);
    }
}
