//! The instruction set (SPEC §5.1) and the template encoding (SPEC §4.3).
//!
//! Any change to this file is an ISA version bump (hard rule 8), and so is any change to the
//! organelle catalogue or to template semantics. Version 2 made metabolism a set of pathways
//! (M10.3); version 3 filled the `RESERVED_A` catalogue slot with the holdfast (SPEC §17.1);
//! version 6 filled the last one, `RESERVED_B`, with the shell; version 7 widened the catalogue
//! from sixteen types to thirty-two on the `n + 16` pairing `docs/FEEDING.md` §6 designs; version
//! 8 appended a crowding reading to the membrane; version 9 made the widening at 7 actually
//! reach `BUILD`; version 10 gave the catalogue recipes in nitrogen and phosphorus; version 11
//! put the atmosphere on the slide and turned the diazosome the right way round.
//!
//! A genome archived under 5 or earlier that built a type-15 organelle was paying for a no-op.
//! Under 6 the same byte builds armour that shades it, so those genomes have to be replayed
//! under the version they evolved in — which is what the stamp is for.
//!
//! Widening at 7 renumbers nothing — 0..=15 keep their meanings exactly — but it **changes what
//! an out-of-range operand means**, because the wrap changes. `BUILD 19` reduced to the
//! chloroplast under ISA ≤ 6 and names the chemosynthetic granule under 7. Mutation produces such
//! operands constantly, so this is the widest-reaching of these bumps even though it takes
//! nothing away.
//!
//! # 9, which is 7 finished
//!
//! 7 widened the catalogue and `OrganelleType::from_operand` with it, and missed the one line
//! where a genome's operand actually enters: `CellHost::build` reduced the type modulo
//! `SLOT_COUNT` rather than `CATALOGUE_SIZE`. Those had been the same number until 7 split them.
//! So under 7 and 8 the upper half existed, worked, could be installed by a test and read by the
//! wiki — and **no genome could build any of it**. `BUILD 22` made a cilium.
//!
//! That is a bump rather than a bug fix because it changes what a byte means: under 7 and 8
//! `BUILD 22` built a cilium and now it builds a flagellum, and there is no reading of the stamp
//! under which both are true. It also restores the property the whole `n + 16` layout exists for
//! — bit 4 of a type operand is one copy error away, so a cilium is one mutation from a
//! flagellum (`docs/FEEDING.md` §6) — which masking that bit off had quietly made a no-op.
//!
//! # 10, the stoichiometry
//!
//! ISA 7 gave `OrganelleSpec` a `build_trace` and one entry used it: the shell, in silicon. 10
//! fills the rest of the table on the Redfield ratio — nitrogen at 16/106 of a type's carbon on
//! the enzymatic machinery and the sensors, phosphorus at 1/106 on the nucleus — which is a
//! catalogue change and so a bump by hard rule 8.
//!
//! **It is the widest-reaching bump so far in what it asks of a genome**, and the reason is worth
//! stating: a recipe is charged against the cell's *interior*, so an organelle that costs
//! nitrogen can only be built by a lineage that eats nitrogen. No genome written before this did,
//! because there was nothing to eat it for. Under 10 an archived genome that never learned to
//! feed on chemical 5 cannot build a mitochondrion, a chloroplast, a lysosome or a sensor, and
//! one that never eats chemical 6 cannot build a nucleus and therefore cannot divide. That is not
//! a compatibility wrinkle, it is a different world, and it is exactly what the stamp is for.
//!
//! # 11, the atmosphere
//!
//! A seventeenth chemical — inert dinitrogen — and the diazosome reversed to crack it. Both are
//! bumps on their own: `CHEM_COUNT` decides what `EAT 20` means, exactly as `CATALOGUE_SIZE`
//! decides what `BUILD 19` means, and mutation produces such operands constantly.
//!
//! The reversal is the substantive half. Until 10 the diazosome *spent* nitrogen to make carbon,
//! which is a monomer transmutation and the wrong shape twice over: nitrogen never entered a body
//! as nitrogen, and a requirement that can be manufactured out of something else is a price
//! rather than a constraint. It now converts the inert pool into the bioavailable one at a steep
//! energy price, which is what fixation is.
//!
//! The atmosphere is *on the slide* because only energy crosses the wall of this world. A
//! reservoir off-plane would need an organelle calling `record_injected` every tick, and a closed
//! system with a tap is a flow reactor. What the slot buys in exchange is that total nitrogen is
//! fixed at seeding while the split between locked and available evolves — scarcity as a
//! property a world arrives at rather than a number somebody set.

/// ISA version stamped into save files, scenarios and archived genomes (SPEC §16).
pub const ISA_VERSION: u16 = 11;

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
    /// Set this cell's public surface badge (ISA 4).
    ///
    /// Took a `RESERVED` slot rather than displacing anything, so every genome written under
    /// ISA 3 means exactly what it meant: the byte was a no-op then and is a badge now, which
    /// can only make an old program do *more*, never something different.
    SetBadge = 0x0F,

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
    Op::SetBadge,
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

/// One line on what each opcode does, parallel to [`OPS`] and [`NAMES`].
///
/// Beside the table rather than in the front end, for the same reason the mnemonics are: a
/// second description of the instruction set kept somewhere else is a description that will
/// eventually disagree with the instruction set. The editor's reference panel reads these.
///
/// Documentation only. Adding a line here is **not** an ISA change and does not bump
/// [`ISA_VERSION`], because nothing a genome can observe is different.
const NOTES: [&str; 64] = [
    // NOP0
    "template letter 0. Executed on its own it does nothing — a byte belongs to a template only by virtue of the instruction in front of it.",
    // NOP1
    "template letter 1. A no-op wherever no instruction claimed it.",
    // IMM
    "( -- v ) push the template's value. This is how a genome writes down a number.",
    // ZERO
    "( -- 0 ) push zero, in one byte and with no template.",
    // ONE
    "( -- 1 ) push one. Cheaper than IMM for the commonest constant there is.",
    // DUP
    "( a -- a a ) copy the top.",
    // DROP
    "( a -- ) throw the top away.",
    // SWAP
    "( a b -- b a ) exchange the top two.",
    // OVER
    "( a b -- a b a ) copy the second over the top.",
    // ROT
    "( a b c -- b c a ) bring the third to the top.",
    // LOAD
    "( addr -- v ) read scratch RAM. Addresses wrap, so every address is legal.",
    // STORE
    "( v addr -- ) write scratch RAM.",
    // RLOAD
    "( idx -- v ) read a register. Registers survive across ticks; the stack does not have to.",
    // RSTORE
    "( v idx -- ) write a register.",
    // RAND
    "( -- v ) a number from hash(seed, tick, cell, purpose). No generator has any state (I1).",
    // SETBADGE
    "( v -- ) set this cell's public badge, 15 bits. What the world can see, and what a forger can copy (ISA 4).",
    // ADD
    "( a b -- a+b ) saturating at the i16 bounds, like every arithmetic opcode here.",
    // SUB
    "( a b -- a-b ) saturating.",
    // MUL
    "( a b -- a*b ) saturating.",
    // DIV
    "( a b -- a/b ) division by zero yields 0 rather than trapping. Any byte sequence has to be a legal program.",
    // MOD
    "( a b -- a%b ) by zero yields 0.",
    // NEG
    "( a -- -a ) saturating, so negating the minimum does not wrap.",
    // ABS
    "( a -- |a| ) saturating.",
    // MIN
    "( a b -- the smaller )",
    // MAX
    "( a b -- the larger )",
    // SHL
    "( a b -- a shifted left by b )",
    // SHR
    "( a b -- a shifted right by b )",
    // AND
    "( a b -- a AND b ) bitwise.",
    // OR
    "( a b -- a OR b ) bitwise.",
    // XOR
    "( a b -- a XOR b ) bitwise.",
    // NOT
    "( a -- complement of a ) bitwise.",
    // CMP
    "( a b -- sign of a-b ) minus one, zero or one. The comparison the jumps are built on.",
    // JMPF
    "jump forward to this template's complement. Not found, and it falls through.",
    // JMPB
    "jump backward to this template's complement — how a loop closes.",
    // JMPZ
    "( a -- ) jump forward if the top is zero.",
    // JMPNZ
    "( a -- ) jump forward if the top is not zero.",
    // CALL
    "jump forward to the complement and push where to come back to.",
    // RET
    "return to the offset on the call stack.",
    // GENE
    "a promoter marker. Fallen through it does nothing but step over its own template; what it is for is being found by EXPRESS.",
    // EXPRESS
    "call whichever gene's promoter best matches this template. Binding is by similarity and not by name, which is why a mutation to a promoter changes when a gene runs rather than whether the genome still parses.",
    // SKIPZ
    "( a -- ) if the top is zero, skip the next instruction and its template.",
    // SETPA
    "( v -- ) set the source pointer, which is where COPYB reads from.",
    // SETPB
    "( v -- ) set the destination pointer, which is where COPYB writes to.",
    // SETLN
    "( v -- ) set the copy counter. A negative operand clamps to zero.",
    // GLEN
    "( -- n ) push this genome's own length. The usual way to say: copy all of me.",
    // LOOPLN
    "if the copy counter is not zero, jump backward to the complement. COPYB and LOOPLN together are the whole inner copy loop.",
    // HALT
    "give up the rest of this tick's instruction budget. Not death — the cell runs again next tick.",
    // RESERVED_1
    "reserved. A no-op, and a slot a future ISA can take without displacing anything.",
    // BUILD
    "( param type slot -- ) begin building an organelle. It takes matter and ticks to finish.",
    // TEAR
    "( slot -- ) dismantle an organelle, recovering some of its matter.",
    // OSET
    "( v idx slot -- ) write one of an organelle's control inputs. This is how an organelle is told which pathway to run.",
    // OGET
    "( idx slot -- v ) read one of an organelle's outputs. The cell's own instrumentation.",
    // OTYPE
    "( slot -- type ) what is in a slot, or nothing.",
    // EAT
    "( amount chem -- got ) take a chemical from the water under the cell. You get what was there, which may be less than you asked for.",
    // EMIT
    "( amount chem -- sent ) put a chemical back into the water.",
    // BUD
    "( size -- ok ) allocate the daughter's genome buffer and set PB to zero. Division starts here.",
    // COPYB
    "copy one byte from PA to the daughter at PB, advancing both and decrementing LN. Where copy errors happen (SPEC §12).",
    // SPLIT
    "finalise the division. The daughter becomes a cell with whatever bytes were copied into it — including none.",
    // JOIN
    "( key kind handle -- ok ) try to make a junction with a neighbour. It fails unless the key matches.",
    // LEAVE
    "( jidx -- ) dissolve a junction.",
    // JXFER
    "( amount what jidx -- moved ) move matter or energy across a soft junction to whoever is on the other end.",
    // JLEN
    "( v jidx -- ) offset a junction's rest length. Contract them in sequence and the organism moves.",
    // SETKEY
    "( v -- ) set this cell's receptor key, seven bits. What it will accept a junction from.",
    // INJECT
    "( jidx -- ok ) write a byte from PA into a neighbour's nucleus at PB. A parasite is a cell that does this; there is no virus flag anywhere.",
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
    "SETBADGE",
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

    /// One line on what this opcode does, for a reference the editor can show beside the
    /// caret. See [`NOTES`].
    #[inline]
    #[must_use]
    pub const fn note(self) -> &'static str {
        NOTES[(self as u8) as usize]
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
    fn every_opcode_says_what_it_does() {
        // A reference panel with a blank row in it is worse than no reference panel: it reads
        // as "this opcode does nothing".
        for op in Op::all() {
            assert!(!op.note().is_empty(), "{} has no note", op.name());
        }
    }

    #[test]
    fn no_two_opcodes_share_a_note() {
        // The failure mode of writing sixty-four one-liners in one sitting. A duplicate is
        // always a paste that was not finished.
        for (i, a) in Op::all().iter().enumerate() {
            for b in Op::all().iter().skip(i + 1) {
                assert_ne!(
                    a.note(),
                    b.note(),
                    "{} and {} have the same note",
                    a.name(),
                    b.name()
                );
            }
        }
    }

    #[test]
    fn a_note_never_names_the_wrong_opcode() {
        // Sixty-four notes written together, and the way one goes wrong is that it describes
        // its neighbour. Anywhere a note opens by naming an opcode, it has to be its own.
        for op in Op::all() {
            let first = op.note().split_whitespace().next().unwrap_or("");
            if let Some(named) = Op::from_name(first) {
                assert_eq!(
                    named,
                    *op,
                    "{}'s note opens by naming {}",
                    op.name(),
                    named.name()
                );
            }
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
