//! The instruction set (SPEC §5.1) and the template encoding (SPEC §4.3).
//!
//! ISA version 1. Any change to this file is an ISA version bump (hard rule 8).

/// ISA version stamped into save files, scenarios and archived genomes (SPEC §16).
pub const ISA_VERSION: u16 = 1;

/// Number of opcodes. The opcode of a byte is `byte % OPCODE_COUNT` (SPEC §4.2), so four
/// distinct byte values map to each opcode and most point mutations are synonymous.
pub const OPCODE_COUNT: u8 = 64;

/// A template is at most 8 `NOP` letters (SPEC §4.3), so its value fits in a `u8`.
pub const MAX_TEMPLATE_LEN: u8 = 8;

/// One opcode. The discriminants are the canonical byte values and are part of the ISA;
/// they must never be renumbered.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum Op {
    // 0x00-0x0F — templates, literals, stack, memory
    Nop0 = 0x00,
    Nop1 = 0x01,
    Imm = 0x02,
    Zero = 0x03,
    One = 0x04,
    Dup = 0x05,
    Drop = 0x06,
    Swap = 0x07,
    Over = 0x08,
    Rot = 0x09,
    Load = 0x0A,
    Store = 0x0B,
    RLoad = 0x0C,
    RStore = 0x0D,
    Rand = 0x0E,
    Reserved0 = 0x0F,

    // 0x10-0x1F — arithmetic and logic, all saturating
    Add = 0x10,
    Sub = 0x11,
    Mul = 0x12,
    Div = 0x13,
    Mod = 0x14,
    Neg = 0x15,
    Abs = 0x16,
    Min = 0x17,
    Max = 0x18,
    Shl = 0x19,
    Shr = 0x1A,
    And = 0x1B,
    Or = 0x1C,
    Xor = 0x1D,
    Not = 0x1E,
    Cmp = 0x1F,

    // 0x20-0x2F — control flow and replication machinery
    JmpF = 0x20,
    JmpB = 0x21,
    JmpZ = 0x22,
    JmpNz = 0x23,
    Call = 0x24,
    Ret = 0x25,
    Gene = 0x26,
    Express = 0x27,
    SkipZ = 0x28,
    SetPa = 0x29,
    SetPb = 0x2A,
    SetLn = 0x2B,
    GLen = 0x2C,
    LoopLn = 0x2D,
    Halt = 0x2E,
    Reserved1 = 0x2F,

    // 0x30-0x3F — body and world
    Build = 0x30,
    Tear = 0x31,
    OSet = 0x32,
    OGet = 0x33,
    OType = 0x34,
    Eat = 0x35,
    Emit = 0x36,
    Bud = 0x37,
    CopyB = 0x38,
    Split = 0x39,
    Join = 0x3A,
    Leave = 0x3B,
    JXfer = 0x3C,
    JLen = 0x3D,
    SetKey = 0x3E,
    Inject = 0x3F,
}

/// Dispatch table. Indexed by `byte % 64`; see [`Op::from_byte`].
const OPS: [Op; 64] = [
    Op::Nop0,
    Op::Nop1,
    Op::Imm,
    Op::Zero,
    Op::One,
    Op::Dup,
    Op::Drop,
    Op::Swap,
    Op::Over,
    Op::Rot,
    Op::Load,
    Op::Store,
    Op::RLoad,
    Op::RStore,
    Op::Rand,
    Op::Reserved0,
    Op::Add,
    Op::Sub,
    Op::Mul,
    Op::Div,
    Op::Mod,
    Op::Neg,
    Op::Abs,
    Op::Min,
    Op::Max,
    Op::Shl,
    Op::Shr,
    Op::And,
    Op::Or,
    Op::Xor,
    Op::Not,
    Op::Cmp,
    Op::JmpF,
    Op::JmpB,
    Op::JmpZ,
    Op::JmpNz,
    Op::Call,
    Op::Ret,
    Op::Gene,
    Op::Express,
    Op::SkipZ,
    Op::SetPa,
    Op::SetPb,
    Op::SetLn,
    Op::GLen,
    Op::LoopLn,
    Op::Halt,
    Op::Reserved1,
    Op::Build,
    Op::Tear,
    Op::OSet,
    Op::OGet,
    Op::OType,
    Op::Eat,
    Op::Emit,
    Op::Bud,
    Op::CopyB,
    Op::Split,
    Op::Join,
    Op::Leave,
    Op::JXfer,
    Op::JLen,
    Op::SetKey,
    Op::Inject,
];

/// Canonical mnemonics, parallel to [`OPS`].
const NAMES: [&str; 64] = [
    "NOP0",
    "NOP1",
    "IMM",
    "ZERO",
    "ONE",
    "DUP",
    "DROP",
    "SWAP",
    "OVER",
    "ROT",
    "LOAD",
    "STORE",
    "RLOAD",
    "RSTORE",
    "RAND",
    "RESERVED_0",
    "ADD",
    "SUB",
    "MUL",
    "DIV",
    "MOD",
    "NEG",
    "ABS",
    "MIN",
    "MAX",
    "SHL",
    "SHR",
    "AND",
    "OR",
    "XOR",
    "NOT",
    "CMP",
    "JMPF",
    "JMPB",
    "JMPZ",
    "JMPNZ",
    "CALL",
    "RET",
    "GENE",
    "EXPRESS",
    "SKIPZ",
    "SETPA",
    "SETPB",
    "SETLN",
    "GLEN",
    "LOOPLN",
    "HALT",
    "RESERVED_1",
    "BUILD",
    "TEAR",
    "OSET",
    "OGET",
    "OTYPE",
    "EAT",
    "EMIT",
    "BUD",
    "COPYB",
    "SPLIT",
    "JOIN",
    "LEAVE",
    "JXFER",
    "JLEN",
    "SETKEY",
    "INJECT",
];

impl Op {
    /// Decode a genome byte. Total by construction: `b % 64` always indexes [`OPS`].
    #[inline(always)]
    #[must_use]
    pub const fn from_byte(b: u8) -> Op {
        OPS[(b % OPCODE_COUNT) as usize]
    }

    /// The canonical (lowest) byte encoding this opcode. Three further bytes decode to it.
    #[inline]
    #[must_use]
    pub const fn canonical_byte(self) -> u8 {
        self as u8
    }

    /// Mnemonic as written in `.mm` assembly.
    #[inline]
    #[must_use]
    pub const fn name(self) -> &'static str {
        NAMES[(self as u8) as usize]
    }

    /// Parse a canonical mnemonic. Case-insensitive at the caller's discretion.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Op> {
        let mut i = 0usize;
        while i < NAMES.len() {
            if NAMES[i] == name {
                return Some(OPS[i]);
            }
            i = i.saturating_add(1);
        }
        None
    }

    /// Whether this opcode consumes the `NOP` run that follows it as a template (SPEC §4.3).
    ///
    /// A byte belongs to a template only by virtue of the instruction in front of it; the
    /// same byte executed directly is an ordinary no-op.
    #[inline(always)]
    #[must_use]
    pub const fn takes_template(self) -> bool {
        matches!(
            self,
            Op::Imm
                | Op::JmpF
                | Op::JmpB
                | Op::JmpZ
                | Op::JmpNz
                | Op::Call
                | Op::Gene
                | Op::Express
                | Op::LoopLn
        )
    }

    /// Whether this opcode is a template letter.
    #[inline(always)]
    #[must_use]
    pub const fn is_nop(self) -> bool {
        matches!(self, Op::Nop0 | Op::Nop1)
    }

    /// All 64 opcodes in canonical order.
    #[must_use]
    pub const fn all() -> &'static [Op; 64] {
        &OPS
    }
}

/// A run of template letters: `len` letters whose bits, read least-significant-first with
/// `NOP0` = 0 and `NOP1` = 1, form `value` (SPEC §4.3).
///
/// `value` is always masked to `len` bits, so equality of `(len, value)` is equality of the
/// letter sequence.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
#[repr(C)]
pub struct Template {
    pub len: u8,
    pub value: u8,
}

impl Template {
    pub const EMPTY: Template = Template { len: 0, value: 0 };

    /// Build a template, masking `value` to `len` bits and clamping `len` to 8.
    #[inline]
    #[must_use]
    pub const fn new(len: u8, value: u8) -> Template {
        let len = if len > MAX_TEMPLATE_LEN {
            MAX_TEMPLATE_LEN
        } else {
            len
        };
        Template {
            len,
            value: value & mask(len),
        }
    }

    #[inline(always)]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// The base-paired template: `NOP0` <-> `NOP1`, same length. Jump instructions search
    /// for this (SPEC §4.3).
    #[inline(always)]
    #[must_use]
    pub const fn complement(self) -> Template {
        Template {
            len: self.len,
            value: (!self.value) & mask(self.len),
        }
    }

    /// The `i`th letter, 0 or 1. Out-of-range indices yield 0.
    #[inline]
    #[must_use]
    pub const fn letter(self, i: u8) -> u8 {
        if i >= self.len {
            return 0;
        }
        (self.value >> i) & 1
    }

    /// Hamming distance for promoter binding (SPEC §4.4).
    ///
    /// Patterns of unequal length are compared over the shorter; each missing bit counts as
    /// a half-mismatch, rounded up. Computed in halves to avoid any rounding ambiguity.
    #[inline]
    #[must_use]
    pub const fn promoter_distance(self, other: Template) -> u16 {
        let shared = if self.len < other.len {
            self.len
        } else {
            other.len
        };
        let diff = (self.value ^ other.value) & mask(shared);
        let full = diff.count_ones() as u16;
        let longer = if self.len > other.len {
            self.len
        } else {
            other.len
        };
        let missing = (longer - shared) as u16;
        // full mismatches + ceil(missing / 2)
        full.saturating_add(missing.saturating_add(1) / 2)
    }
}

/// Low-`n`-bits mask, saturating at 8 bits.
#[inline(always)]
#[must_use]
pub const fn mask(n: u8) -> u8 {
    if n >= 8 {
        0xFF
    } else {
        (1u8 << n).wrapping_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_is_byte_mod_64() {
        for b in 0..=255u8 {
            assert_eq!(Op::from_byte(b), OPS[(b as usize) % 64]);
            assert_eq!(Op::from_byte(b).canonical_byte(), b % 64);
        }
    }

    #[test]
    fn discriminants_match_table_position() {
        for (i, op) in OPS.iter().enumerate() {
            assert_eq!(*op as usize, i, "opcode {} is out of position", op.name());
        }
    }

    #[test]
    fn names_round_trip() {
        for op in OPS {
            assert_eq!(Op::from_name(op.name()), Some(op));
        }
    }

    #[test]
    fn complement_is_involutive() {
        for len in 0..=8u8 {
            for value in 0..=255u8 {
                let t = Template::new(len, value);
                assert_eq!(t.complement().complement(), t);
                if len > 0 {
                    assert_ne!(t.complement(), t);
                }
            }
        }
    }

    #[test]
    fn promoter_distance_is_symmetric_and_zero_on_self() {
        for la in 0..=8u8 {
            for lb in 0..=8u8 {
                for va in 0..=15u8 {
                    for vb in 0..=15u8 {
                        let a = Template::new(la, va);
                        let b = Template::new(lb, vb);
                        assert_eq!(a.promoter_distance(b), b.promoter_distance(a));
                        assert_eq!(a.promoter_distance(a), 0);
                    }
                }
            }
        }
    }

    #[test]
    fn promoter_distance_counts_missing_bits_as_half() {
        let a = Template::new(4, 0b1010);
        // same shared prefix, three fewer letters -> ceil(3/2) = 2
        let b = Template::new(1, 0b0);
        assert_eq!(a.promoter_distance(b), 2);
        // one full mismatch plus two missing letters -> 1 + 1
        let c = Template::new(2, 0b11);
        assert_eq!(a.promoter_distance(c), 2);
    }
}
