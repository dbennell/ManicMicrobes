//! The per-cell virtual machine (SPEC §5).
//!
//! # Totality
//!
//! Invariant I3 says no byte sequence, in any register/stack/memory state, can panic, hang
//! or abort. Everything in this file is written to that standard, and the mechanisms are:
//!
//! * **Circular stacks.** Popping an empty stack yields 0; pushing a full one overwrites the
//!   oldest entry. There is no fault state, so no opcode needs to check arity.
//! * **Wrapping addressing.** Every index — register, RAM word, genome offset — is reduced
//!   modulo its range. The stacks, register file and RAM are all power-of-two sized so this
//!   is a mask, and the mask also proves the index in range to the compiler, so the array
//!   accesses carry no bounds check to elide.
//! * **Saturating magnitudes.** Arithmetic is performed in `i32` and clamped on store to
//!   `i16`. `DIV` and `MOD` by zero yield 0. Nothing wraps.
//! * **Bounded search.** The complementary jump search probes at most
//!   `min(template_search_range, genome length)` offsets, and `EXPRESS` scans a promoter
//!   table built once at genome construction. Every instruction is therefore O(1) in the
//!   instruction budget, which is what makes "no hangs" structural rather than a matter of
//!   testing enough cases.
//! * **Arbitrary initial state.** `run` normalises stack pointers and the instruction
//!   pointer on entry, so a `Vm` filled with random bytes is a legal `Vm`.
//!
//! The worst a program can do is waste energy.
//!
//! # Why saturate rather than wrap
//!
//! Wrapping arithmetic puts a cliff in the fitness landscape: a one-bit mutation flips a
//! cell from "very fast forward" to "very fast reverse". Saturation keeps the landscape
//! continuous and climbable. Addresses are the other way round — wrapping keeps every index
//! legal, which is what serves totality (SPEC §3).

use crate::config::VmConfig;
use crate::genome::Genome;
use crate::host::Host;
use crate::isa::{mask, Op, Template};
use crate::rng::{Purpose, RandCtx};
use crate::state_hash::{StateHash, StateHasher};

/// Data stack depth (SPEC §5). Power of two: the index is a mask, never a bounds check.
pub const DATA_STACK_LEN: usize = 16;
/// Call stack depth (SPEC §5).
pub const CALL_STACK_LEN: usize = 8;
/// Register file size (SPEC §5). Addressed `idx % 16`.
pub const REGISTER_COUNT: usize = 16;
/// Scratch RAM size in `i16` words (SPEC §5). Addressed `addr % 64`.
pub const RAM_WORDS: usize = 64;

const DATA_MASK: u8 = (DATA_STACK_LEN - 1) as u8;
const CALL_MASK: u8 = (CALL_STACK_LEN - 1) as u8;
const REG_MASK: u16 = (REGISTER_COUNT - 1) as u16;
const RAM_MASK: u16 = (RAM_WORDS - 1) as u16;

/// Clamp to the cell-visible range. All arithmetic is done in `i32` and lands here.
#[inline(always)]
#[must_use]
const fn sat(v: i32) -> i16 {
    if v > i16::MAX as i32 {
        i16::MAX
    } else if v < i16::MIN as i32 {
        i16::MIN
    } else {
        v as i16
    }
}

/// Complete VM state for one cell.
///
/// Fields are public because the fuzz harness must be able to construct arbitrary states,
/// and because there is no such thing as an invalid one: [`Vm::run`] normalises what it
/// needs on entry and masks every access thereafter.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Vm {
    /// Circular data stack.
    pub data: [i16; DATA_STACK_LEN],
    /// Circular call stack of return offsets.
    pub call: [u16; CALL_STACK_LEN],
    pub regs: [i16; REGISTER_COUNT],
    pub ram: [i16; RAM_WORDS],
    /// Instruction pointer, wraps modulo genome length.
    pub ip: u16,
    /// Copy source pointer, a genome offset.
    pub pa: u16,
    /// Copy destination pointer, an offset into the daughter buffer or a target nucleus.
    pub pb: u16,
    /// Copy-length counter. `COPYB` decrements it, `LOOPLN` tests it.
    pub ln: u16,
    /// How many values `RAND` has drawn. Part of the random draw, so two `RAND`s in one tick
    /// differ; part of saved state, so a resumed run draws the same sequence (I7).
    pub rand_ctr: u32,
    /// Index of the top of the data stack.
    pub dsp: u8,
    /// Live entries on the data stack, `0..=16`.
    pub dlen: u8,
    /// Index of the top of the call stack.
    pub csp: u8,
    /// Live entries on the call stack, `0..=8`.
    pub clen: u8,
    /// Set by `HALT`; cleared by [`Vm::tick`] at the start of each tick.
    pub halted: bool,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

impl Vm {
    #[must_use]
    pub const fn new() -> Vm {
        Vm {
            data: [0; DATA_STACK_LEN],
            call: [0; CALL_STACK_LEN],
            regs: [0; REGISTER_COUNT],
            ram: [0; RAM_WORDS],
            ip: 0,
            pa: 0,
            pb: 0,
            ln: 0,
            rand_ctr: 0,
            dsp: 0,
            dlen: 0,
            csp: 0,
            clen: 0,
            halted: false,
        }
    }

    /// Run one tick: clear `halted` and execute up to `instr_per_tick` instructions.
    ///
    /// `HALT` yields the rest of the budget, which is why dormancy is cheap and therefore
    /// evolvable (SPEC §5).
    pub fn tick<H: Host>(
        &mut self,
        genome: &Genome,
        cfg: &VmConfig,
        ctx: &RandCtx,
        host: &mut H,
    ) -> u32 {
        self.halted = false;
        self.run(genome, cfg, ctx, host, cfg.instr_per_tick as u32)
    }

    /// Execute up to `budget` instructions, returning how many actually ran.
    ///
    /// Returns early only on `HALT`. Guaranteed to return: the loop is bounded by `budget`
    /// and every instruction takes bounded time.
    pub fn run<H: Host>(
        &mut self,
        genome: &Genome,
        cfg: &VmConfig,
        ctx: &RandCtx,
        host: &mut H,
        budget: u32,
    ) -> u32 {
        let len = genome.len();
        // Before the empty-genome exit, so that a `Vm` is left well-formed either way.
        self.normalise(len);
        if len == 0 {
            // Nothing to execute. Report the budget as spent so a caller looping until the
            // budget runs out still terminates.
            self.halted = true;
            return budget;
        }

        let range = cfg.template_search_range;
        let threshold = cfg.promoter_bind_threshold;
        let mut executed: u32 = 0;

        while executed < budget {
            let ip = self.ip as usize;
            let op = Op::from_byte(genome.byte(ip));
            executed = executed.wrapping_add(1);

            // Default: step one byte. Template-consuming opcodes and jumps overwrite this.
            let mut next = ip.wrapping_add(1);
            if next >= len {
                next = 0;
            }

            match op {
                // ---- 0x00-0x0F templates, literals, stack, memory ----
                Op::Nop0 | Op::Nop1 => {}
                Op::Imm => {
                    let (t, after) = read_template(genome, next, len);
                    // A zero-length template makes its host a no-op — except IMM, which
                    // still pushes, with value 0 (SPEC §4.3).
                    self.push(t.value as i16);
                    next = after;
                }
                Op::Zero => self.push(0),
                Op::One => self.push(1),
                Op::Dup => {
                    let a = self.peek();
                    self.push(a);
                }
                Op::Drop => {
                    self.pop();
                }
                Op::Swap => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(b);
                    self.push(a);
                }
                Op::Over => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(a);
                    self.push(b);
                    self.push(a);
                }
                Op::Rot => {
                    let c = self.pop();
                    let b = self.pop();
                    let a = self.pop();
                    self.push(b);
                    self.push(c);
                    self.push(a);
                }
                Op::Load => {
                    let addr = self.pop();
                    self.push(self.ram_at(addr));
                }
                Op::Store => {
                    let addr = self.pop();
                    let v = self.pop();
                    self.set_ram(addr, v);
                }
                Op::RLoad => {
                    let idx = self.pop();
                    self.push(self.reg_at(idx));
                }
                Op::RStore => {
                    let idx = self.pop();
                    let v = self.pop();
                    self.set_reg(idx, v);
                }
                Op::Rand => {
                    let v = ctx.draw_i16(Purpose::Rand, self.rand_ctr as u64);
                    self.rand_ctr = self.rand_ctr.wrapping_add(1);
                    self.push(v);
                }
                Op::SetBadge => {
                    let v = self.pop();
                    host.set_badge(v as u16 & 0x7FFF);
                }

                // ---- 0x10-0x1F arithmetic and logic, all saturating ----
                Op::Add => self.binary(|a, b| sat(a.wrapping_add(b))),
                Op::Sub => self.binary(|a, b| sat(a.wrapping_sub(b))),
                Op::Mul => self.binary(|a, b| sat(a.wrapping_mul(b))),
                Op::Div => self.binary(|a, b| if b == 0 { 0 } else { sat(a.wrapping_div(b)) }),
                Op::Mod => self.binary(|a, b| if b == 0 { 0 } else { sat(a.wrapping_rem(b)) }),
                Op::Neg => {
                    let a = self.pop();
                    self.push(sat((a as i32).wrapping_neg()));
                }
                Op::Abs => {
                    let a = self.pop();
                    self.push(sat((a as i32).wrapping_abs()));
                }
                Op::Min => self.binary(|a, b| sat(a.min(b))),
                Op::Max => self.binary(|a, b| sat(a.max(b))),
                // Shift counts are indices into the word, so they wrap rather than saturate;
                // the shifted value then saturates like any other magnitude.
                Op::Shl => self.binary(|a, b| sat(a.wrapping_shl((b as u32) & 15))),
                Op::Shr => self.binary(|a, b| sat(a.wrapping_shr((b as u32) & 15))),
                Op::And => self.binary(|a, b| sat(a & b)),
                Op::Or => self.binary(|a, b| sat(a | b)),
                Op::Xor => self.binary(|a, b| sat(a ^ b)),
                Op::Not => {
                    let a = self.pop();
                    self.push(!a);
                }
                Op::Cmp => self.binary(|a, b| a.wrapping_sub(b).signum() as i16),

                // ---- 0x20-0x2F control flow and replication machinery ----
                Op::JmpF => {
                    let (t, after) = read_template(genome, next, len);
                    next = match search_forward(genome, ip, t, range, len) {
                        Some(target) => target,
                        None => after,
                    };
                }
                Op::JmpB => {
                    let (t, after) = read_template(genome, next, len);
                    next = match search_backward(genome, ip, t, range, len) {
                        Some(target) => target,
                        None => after,
                    };
                }
                Op::JmpZ => {
                    let (t, after) = read_template(genome, next, len);
                    let a = self.pop();
                    next = if a == 0 {
                        search_forward(genome, ip, t, range, len).unwrap_or(after)
                    } else {
                        after
                    };
                }
                Op::JmpNz => {
                    let (t, after) = read_template(genome, next, len);
                    let a = self.pop();
                    next = if a != 0 {
                        search_forward(genome, ip, t, range, len).unwrap_or(after)
                    } else {
                        after
                    };
                }
                Op::Call => {
                    let (t, after) = read_template(genome, next, len);
                    if let Some(target) = search_forward(genome, ip, t, range, len) {
                        self.push_call(after as u16);
                        next = target;
                    } else {
                        next = after;
                    }
                }
                Op::Ret => {
                    // An empty call stack pops 0, so RET without CALL restarts the genome.
                    next = genome.wrap(self.pop_call() as usize);
                }
                Op::Gene => {
                    // A promoter marker. Reached by fall-through it does nothing but step
                    // over its own template (SPEC §4.4).
                    let (_, after) = read_template(genome, next, len);
                    next = after;
                }
                Op::Express => {
                    let (t, after) = read_template(genome, next, len);
                    match find_promoter(genome, t, threshold) {
                        Some(entry) => {
                            self.push_call(after as u16);
                            next = entry as usize;
                        }
                        None => next = after,
                    }
                }
                Op::SkipZ => {
                    let a = self.pop();
                    if a == 0 {
                        next = skip_one(genome, next, len);
                    }
                }
                Op::SetPa => {
                    let v = self.pop();
                    self.pa = v as u16;
                }
                Op::SetPb => {
                    let v = self.pop();
                    self.pb = v as u16;
                }
                Op::SetLn => {
                    let v = self.pop();
                    // A negative length is no length. Clamping here is what keeps the copy
                    // loop finite.
                    self.ln = if v < 0 { 0 } else { v as u16 };
                }
                Op::GLen => self.push(sat(len as i32)),
                Op::LoopLn => {
                    let (t, after) = read_template(genome, next, len);
                    next = if self.ln != 0 {
                        search_backward(genome, ip, t, range, len).unwrap_or(after)
                    } else {
                        after
                    };
                }
                Op::Halt => self.halted = true,
                Op::Reserved1 => {}

                // ---- 0x30-0x3F body and world ----
                // Stack effects happen here; the effect itself goes through the host, which
                // at M0 is a world that does not exist.
                Op::Build => {
                    let slot = self.pop();
                    let ty = self.pop();
                    let param = self.pop();
                    host.build(param, ty, slot);
                }
                Op::Tear => {
                    let slot = self.pop();
                    host.tear(slot);
                }
                Op::OSet => {
                    let slot = self.pop();
                    let idx = self.pop();
                    let v = self.pop();
                    host.oset(v, idx, slot);
                }
                Op::OGet => {
                    let slot = self.pop();
                    let idx = self.pop();
                    let v = host.oget(idx, slot);
                    self.push(v);
                }
                Op::OType => {
                    let slot = self.pop();
                    let v = host.otype(slot);
                    self.push(v);
                }
                Op::Eat => {
                    let chem = self.pop();
                    let amount = self.pop();
                    let got = host.eat(amount, chem);
                    self.push(got);
                }
                Op::Emit => {
                    let chem = self.pop();
                    let amount = self.pop();
                    let sent = host.emit(amount, chem);
                    self.push(sent);
                }
                Op::Bud => {
                    let size = self.pop();
                    let ok = host.bud(size);
                    self.pb = 0;
                    self.push(ok);
                }
                Op::CopyB => {
                    let src = genome.byte(genome.wrap(self.pa as usize));
                    host.copy_byte(self.pb, src);
                    self.advance_copy();
                }
                Op::Split => host.split(),
                Op::Join => {
                    let handle = self.pop();
                    let kind = self.pop();
                    let key = self.pop();
                    let ok = host.join(key, kind, handle);
                    self.push(ok);
                }
                Op::Leave => {
                    let jidx = self.pop();
                    host.leave(jidx);
                }
                Op::JXfer => {
                    let jidx = self.pop();
                    let what = self.pop();
                    let amount = self.pop();
                    let moved = host.jxfer(amount, what, jidx);
                    self.push(moved);
                }
                Op::JLen => {
                    let jidx = self.pop();
                    let v = self.pop();
                    host.jlen(v, jidx);
                }
                Op::SetKey => {
                    let v = self.pop();
                    host.set_key((v as u16 & 0x7F) as u8);
                }
                Op::Inject => {
                    // Symmetric with COPYB: same pointers, same advance. Writing to your own
                    // nucleus and writing to a neighbour's are one mechanism (SPEC §8.3).
                    let jidx = self.pop();
                    let src = genome.byte(genome.wrap(self.pa as usize));
                    let ok = host.inject(jidx, self.pb, src);
                    self.advance_copy();
                    self.push(ok);
                }
            }

            self.ip = next as u16;
            if self.halted {
                break;
            }
        }
        executed
    }

    /// Make an arbitrary `Vm` well-formed. Only pointers need it; every value field is
    /// already legal whatever its bit pattern.
    #[inline]
    fn normalise(&mut self, genome_len: usize) {
        self.ip = if (self.ip as usize) < genome_len {
            self.ip
        } else {
            ((self.ip as usize) % genome_len.max(1)) as u16
        };
        self.dsp &= DATA_MASK;
        self.csp &= CALL_MASK;
        if self.dlen as usize > DATA_STACK_LEN {
            self.dlen = DATA_STACK_LEN as u8;
        }
        if self.clen as usize > CALL_STACK_LEN {
            self.clen = CALL_STACK_LEN as u8;
        }
    }

    #[inline(always)]
    fn advance_copy(&mut self) {
        self.pa = self.pa.wrapping_add(1);
        self.pb = self.pb.wrapping_add(1);
        self.ln = self.ln.saturating_sub(1);
    }

    /// `( a b -- f(a, b) )`, computed in `i32`.
    #[inline(always)]
    fn binary(&mut self, f: impl Fn(i32, i32) -> i16) {
        let b = self.pop();
        let a = self.pop();
        self.push(f(a as i32, b as i32));
    }

    /// Push onto the circular data stack, overwriting the oldest entry when full.
    #[inline(always)]
    pub fn push(&mut self, v: i16) {
        self.dsp = self.dsp.wrapping_add(1) & DATA_MASK;
        if let Some(slot) = self.data.get_mut(self.dsp as usize) {
            *slot = v;
        }
        if (self.dlen as usize) < DATA_STACK_LEN {
            self.dlen = self.dlen.wrapping_add(1);
        }
    }

    /// Pop the circular data stack. Empty yields 0.
    #[inline(always)]
    pub fn pop(&mut self) -> i16 {
        if self.dlen == 0 {
            return 0;
        }
        let v = match self.data.get(self.dsp as usize) {
            Some(v) => *v,
            None => 0,
        };
        self.dsp = self.dsp.wrapping_sub(1) & DATA_MASK;
        self.dlen = self.dlen.wrapping_sub(1);
        v
    }

    /// Top of the data stack without removing it. Empty yields 0.
    #[inline(always)]
    #[must_use]
    pub fn peek(&self) -> i16 {
        if self.dlen == 0 {
            return 0;
        }
        match self.data.get(self.dsp as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    #[inline(always)]
    fn push_call(&mut self, v: u16) {
        self.csp = self.csp.wrapping_add(1) & CALL_MASK;
        if let Some(slot) = self.call.get_mut(self.csp as usize) {
            *slot = v;
        }
        if (self.clen as usize) < CALL_STACK_LEN {
            self.clen = self.clen.wrapping_add(1);
        }
    }

    #[inline(always)]
    fn pop_call(&mut self) -> u16 {
        if self.clen == 0 {
            return 0;
        }
        let v = match self.call.get(self.csp as usize) {
            Some(v) => *v,
            None => 0,
        };
        self.csp = self.csp.wrapping_sub(1) & CALL_MASK;
        self.clen = self.clen.wrapping_sub(1);
        v
    }

    #[inline(always)]
    #[must_use]
    fn reg_at(&self, idx: i16) -> i16 {
        match self.regs.get(((idx as u16) & REG_MASK) as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    #[inline(always)]
    fn set_reg(&mut self, idx: i16, v: i16) {
        if let Some(slot) = self.regs.get_mut(((idx as u16) & REG_MASK) as usize) {
            *slot = v;
        }
    }

    #[inline(always)]
    #[must_use]
    fn ram_at(&self, addr: i16) -> i16 {
        match self.ram.get(((addr as u16) & RAM_MASK) as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    #[inline(always)]
    fn set_ram(&mut self, addr: i16, v: i16) {
        if let Some(slot) = self.ram.get_mut(((addr as u16) & RAM_MASK) as usize) {
            *slot = v;
        }
    }
}

impl StateHash for Vm {
    fn hash_state(&self, h: &mut StateHasher) {
        h.i16_slice(&self.data);
        h.u64(self.call.len() as u64);
        for v in self.call {
            h.u16(v);
        }
        h.i16_slice(&self.regs);
        h.i16_slice(&self.ram);
        h.u16(self.ip);
        h.u16(self.pa);
        h.u16(self.pb);
        h.u16(self.ln);
        h.u32(self.rand_ctr);
        h.u8(self.dsp);
        h.u8(self.dlen);
        h.u8(self.csp);
        h.u8(self.clen);
        h.bool(self.halted);
    }
}

/// Read the template beginning at `start`, and the offset just past it.
///
/// Template runs never cross the end of the genome, so `start + len <= genome length` and
/// the wrap is at most one subtraction.
#[inline(always)]
fn read_template(genome: &Genome, start: usize, len: usize) -> (Template, usize) {
    let t = genome.template_at(start);
    let mut after = start.wrapping_add(t.len as usize);
    if after >= len {
        after = after.wrapping_sub(len);
    }
    (t, after)
}

/// Scan forward for the complement of `t`, and return where to resume.
///
/// Probes at most `min(range, genome length)` offsets, so a genome shorter than the search
/// range is scanned once rather than repeatedly. A zero-length template matches nothing,
/// which is what makes its host instruction a no-op.
///
/// A candidate offset matches when the template run starting there is at least as long as
/// the query and its first `t.len` letters are the complement — and because letters are
/// read least-significant-first, "first `k` letters" is "low `k` bits", so the test is one
/// mask and one compare against the precomputed run.
#[inline]
fn search_forward(
    genome: &Genome,
    ip: usize,
    t: Template,
    range: u16,
    len: usize,
) -> Option<usize> {
    if t.is_empty() {
        return None;
    }
    let want = t.complement();
    let m = mask(want.len);
    let probes = (range as usize).min(len);
    let mut off = ip;
    for _ in 0..probes {
        off = off.wrapping_add(1);
        if off >= len {
            off = 0;
        }
        let run = genome.template_at(off);
        if run.len >= want.len && (run.value & m) == want.value {
            // Resume just past the matched letters.
            let mut target = off.wrapping_add(want.len as usize);
            if target >= len {
                target = target.wrapping_sub(len);
            }
            return Some(target);
        }
    }
    None
}

/// Scan backward for the complement of `t`. Same match rule as [`search_forward`].
#[inline]
fn search_backward(
    genome: &Genome,
    ip: usize,
    t: Template,
    range: u16,
    len: usize,
) -> Option<usize> {
    if t.is_empty() {
        return None;
    }
    let want = t.complement();
    let m = mask(want.len);
    let probes = (range as usize).min(len);
    let mut off = ip;
    for _ in 0..probes {
        off = if off == 0 {
            len.wrapping_sub(1)
        } else {
            off.wrapping_sub(1)
        };
        let run = genome.template_at(off);
        if run.len >= want.len && (run.value & m) == want.value {
            let mut target = off.wrapping_add(want.len as usize);
            if target >= len {
                target = target.wrapping_sub(len);
            }
            return Some(target);
        }
    }
    None
}

/// `EXPRESS`: the `GENE` promoter closest in Hamming distance to `t` (SPEC §4.4).
///
/// Public so the inspector can label an `EXPRESS` with the gene it will actually reach. A
/// second implementation of binding in the front-end would eventually disagree with this one,
/// and the moment it did the panel would be confidently describing a jump that does not
/// happen.
///
/// This is transcription-factor binding, and it is why deleting a gene does not orphan its
/// callers — they bind the next-best match. Ties resolve to the lowest genome offset, which
/// falls out of the promoter table being in ascending offset order and this scan keeping
/// only a strict improvement.
#[inline]
pub fn find_promoter(genome: &Genome, t: Template, threshold: u16) -> Option<u16> {
    if t.is_empty() {
        return None;
    }
    let mut best: Option<(u16, u16)> = None;
    for p in genome.promoters() {
        let d = t.promoter_distance(p.template);
        let better = match best {
            Some((bd, _)) => d < bd,
            None => true,
        };
        if better {
            best = Some((d, p.entry));
            if d == 0 {
                break;
            }
        }
    }
    match best {
        Some((d, entry)) if d <= threshold => Some(entry),
        _ => None,
    }
}

/// Where a forward-scanning jump from `ip` lands: `JMPF`, `CALL`, and `JMPZ`/`JMPNZ` when
/// they take the branch.
///
/// Public for exactly the reason [`find_promoter`] is. The genome pane resolves a jump to the
/// offset it reaches, and it has to ask the VM's own scan rather than keep a second copy of
/// the match rule. A listing that says `→ 47` while the VM goes to 12 is worse than showing
/// the raw template, because the template does not claim anything and the listing does.
///
/// `None` is not an error: a jump that matches nothing falls through to the byte after its own
/// template, which is a real and common thing for an evolved genome to do.
#[inline]
#[must_use]
pub fn find_forward(genome: &Genome, ip: u16, t: Template, range: u16) -> Option<u16> {
    let target = search_forward(genome, ip as usize, t, range, genome.len())?;
    u16::try_from(target).ok()
}

/// Where a backward-scanning jump from `ip` lands: `JMPB`, and `LOOPLN` while `LN` is nonzero.
///
/// [`find_forward`], scanning the other way.
#[inline]
#[must_use]
pub fn find_backward(genome: &Genome, ip: u16, t: Template, range: u16) -> Option<u16> {
    let target = search_backward(genome, ip as usize, t, range, genome.len())?;
    u16::try_from(target).ok()
}

/// `SKIPZ`: the offset just past the instruction at `at`, including its template if it takes
/// one.
#[inline]
fn skip_one(genome: &Genome, at: usize, len: usize) -> usize {
    let op = Op::from_byte(genome.byte(at));
    let mut after = at.wrapping_add(1);
    if after >= len {
        after = 0;
    }
    if op.takes_template() {
        let (_, past) = read_template(genome, after, len);
        past
    } else {
        after
    }
}
