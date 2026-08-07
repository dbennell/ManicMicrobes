//! Manic Microbes — assembler, disassembler and source maps for `.mm` assembly.
//!
//! Genomes are byte strings; this crate is how a human writes and reads them. It is not part
//! of the simulation — `mm-core` never depends on it — but the round-trip property it
//! guarantees is what makes an evolved genome inspectable and editable at M6.
//!
//! ```
//! let a = mm_asm::assemble("        GENE    #replicate\n        GLEN\n        SETLN\n").unwrap();
//! let src = mm_asm::disassemble(&a.bytes).to_source();
//! assert_eq!(mm_asm::assemble(&src).unwrap().bytes, a.bytes);
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![warn(missing_debug_implementations)]

pub mod asm;
pub mod disasm;
pub mod highlight;
/// Where the shipped `genomes/` and `scenarios/` are at run time — asked of the filesystem
/// rather than baked in at compile time, which is what a released binary needs.
pub mod locate;
pub mod source_map;

pub use asm::{
    assemble, name_hash, promoter_pattern, AsmError, AsmErrors, Assembled, LabelInfo, LABEL_BITS,
    PROMOTER_BITS,
};
pub use disasm::{disassemble, Disassembly, Line};
pub use source_map::{SourceMap, Span};
