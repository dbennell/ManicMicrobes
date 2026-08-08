//! The cell inspector (SPEC §14, M4).
//!
//! > Cell inspector showing live registers, stack, organelle slots, internal chemistry,
//! > junctions and current species.
//!
//! A read-only transcript of one cell, taken the same way a [`crate::slide::Frame`] is taken:
//! everything is copied out, so the panel that displays it holds no borrow of the world and
//! cannot write back through one. Inspecting is looking, and looking is free.
//!
//! The stack is presented **bottom-to-top in the order the VM would pop**, not in the order
//! the underlying array happens to be laid out. The data stack is circular (SPEC §3), so the
//! raw array is rotated by however many pushes have happened and reading it directly shows a
//! stack that appears to shuffle itself between ticks. Anyone debugging a genome needs
//! `[.., under, top]`, so that is what this produces.

use mm_core::cell::{CellArena, CellId};
use mm_core::chem::CHEM_COUNT;
use mm_core::config::VmConfig;
use mm_core::organelle::{OrganelleType, SLOT_COUNT};
use mm_core::vm::{CALL_STACK_LEN, DATA_STACK_LEN, RAM_WORDS, REGISTER_COUNT};
use mm_core::World;

/// One organelle slot, as the panel shows it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotView {
    pub index: usize,
    pub kind: OrganelleType,
    pub param: u8,
    pub control: [i16; 2],
    /// Build progress: `None` once finished, otherwise the bytes still owed.
    pub remaining_build: Option<u16>,
    pub active: bool,
}

/// Everything worth knowing about one cell, copied out.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Inspection {
    pub id: CellId,
    pub species: u32,
    pub parent: CellId,
    pub birth_tick: u64,
    pub age: u32,
    pub x: i32,
    pub y: i32,

    pub mass: i32,
    pub energy: i32,
    pub damage: i32,
    pub key: u8,

    /// How firmly this cell holds its own shape, `Q10`. Wall times turgor — see
    /// [`mm_core::biology::rigidity`].
    ///
    /// Here because it is the one reading that explains what a cell *looks like* and cannot be
    /// worked out from anything else on this panel. A slide of round separate cells and a slide of
    /// polygons differ in this number and in nothing visible beside it; the membrane parameter is
    /// half of it and the interior chemistry is the other half, and multiplying two readings in
    /// your head to find out why the picture is what it is is not an instrument.
    ///
    /// It was added the first time somebody looked at a slide that should have been marbles, saw
    /// polygons, and reasonably concluded that something softer had overtaken the lineage. Nothing
    /// had — every cell had the wall its genome asked for, and the wall was 200 of a possible 255,
    /// which is 0.78 firm and still visibly angular. That is a five-second answer with this on the
    /// panel and a probe run without it.
    pub rigidity: i32,

    pub genome_len: usize,
    pub genome_hash: u64,
    /// The genome itself, so the panel can disassemble it and show the cell reading its own
    /// code. An `Arc` clone of the interned genome rather than a copy of the bytes — the
    /// whole population shares a handful of these, and taking an inspection every frame must
    /// not mean copying a few hundred bytes every frame.
    pub genome: std::sync::Arc<mm_core::Genome>,
    /// Nucleus copy fidelity, `Q10`, or `None` if the cell has no working nucleus — in which
    /// case it cannot copy its genome at all and cannot divide (SPEC §4.1).
    pub fidelity: Option<i32>,
    /// How much genome the nucleus can hold, in bytes. Beside `genome_len` this is the thing
    /// that silently sterilises a lineage when it goes the wrong way (SPEC §4.1), so it is
    /// worth being able to see rather than having to work out from the nucleus `param`.
    pub nucleus_capacity: usize,

    /// Data stack, deepest first, top last.
    pub stack: Vec<i16>,
    /// Return addresses, outermost first.
    pub call_stack: Vec<u16>,
    pub registers: [i16; REGISTER_COUNT],
    pub ram: [i16; RAM_WORDS],
    pub ip: u16,
    /// Copy pointers and counter — the state of a division in progress.
    pub pa: u16,
    pub pb: u16,
    pub ln: u16,
    pub halted: bool,

    pub slots: [SlotView; SLOT_COUNT],
    pub interior: [i32; CHEM_COUNT],

    /// The VM parameters this cell is actually running under.
    ///
    /// Carried rather than assumed. Resolving a jump or a promoter binding depends on
    /// `template_search_range` and `promoter_bind_threshold`, and since M10.2 both are
    /// editable while the world runs — so a listing that resolved against `VmConfig::DEFAULT`
    /// would quietly describe a different world from the one on the slide, and would keep
    /// doing it convincingly.
    pub vm_config: mm_core::config::VmConfig,
}

impl Inspection {
    /// Take a reading of one cell, or `None` if it is not alive.
    #[must_use]
    pub fn of(world: &World, id: CellId) -> Option<Inspection> {
        let cells = world.cells();
        let i = cells.index(id)?;
        Some(Inspection::of_slot(world, cells, i))
    }

    fn of_slot(world: &World, cells: &CellArena, i: usize) -> Inspection {
        let vm = &cells.vm[i];
        let mut slots = [SlotView {
            index: 0,
            kind: OrganelleType::Membrane,
            param: 0,
            control: [0; 2],
            remaining_build: None,
            active: false,
        }; SLOT_COUNT];
        for (n, o) in cells.slots(i).iter().enumerate() {
            slots[n] = SlotView {
                index: n,
                kind: o.kind,
                param: o.param,
                control: o.control,
                remaining_build: (o.remaining_build > 0).then_some(o.remaining_build),
                active: o.is_active(),
            };
        }
        let mut interior = [0i32; CHEM_COUNT];
        interior.copy_from_slice(cells.interior(i));

        Inspection {
            id: cells.id_at(i),
            species: cells.species[i],
            parent: cells.parent[i],
            birth_tick: cells.birth_tick[i],
            age: cells.age[i],
            x: cells.x[i],
            y: cells.y[i],
            mass: cells.mass[i],
            energy: cells.energy[i],
            damage: cells.damage[i],
            key: cells.key[i],
            rigidity: mm_core::biology::rigidity(
                cells,
                i,
                &world.biology().metabolism.rates,
            ),
            genome_len: cells.genome[i].len(),
            genome_hash: cells.genome[i].hash(),
            genome: std::sync::Arc::clone(&cells.genome[i]),
            fidelity: mm_core::biology::nucleus_fidelity(cells, i),
            nucleus_capacity: mm_core::biology::nucleus_capacity(cells, i),
            stack: unwind(&vm.data, vm.dsp, vm.dlen, DATA_STACK_LEN),
            call_stack: unwind(&vm.call, vm.csp, vm.clen, CALL_STACK_LEN),
            registers: vm.regs,
            ram: vm.ram,
            ip: vm.ip,
            pa: vm.pa,
            pb: vm.pb,
            ln: vm.ln,
            halted: vm.halted,
            slots,
            interior,
            vm_config: world.scenario().vm,
        }
    }
}

/// One organelle placed for the panel's schematic.
///
/// The same ring the renderer draws on the slide, at panel scale, so the diagram in the panel
/// and the cell under the microscope agree about which blob is which. Offsets are `-1.0..=1.0`
/// fractions of the cell's radius, so the panel picks its own size.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SlotPlacement {
    pub slot: usize,
    pub kind: OrganelleType,
    pub dx: f32,
    pub dy: f32,
    /// Radius as a fraction of the cell's, from `param`.
    pub radius: f32,
    /// `0.0..=1.0`. Scaffolding still being built is drawn faint.
    pub built: f32,
}

/// Lay the active slots out on a ring for the schematic.
///
/// The membrane is not in the list: it is the circle everything else is drawn inside, so
/// giving it a blob on its own boundary would be drawing the container twice.
#[must_use]
pub fn placements(slots: &[SlotView; SLOT_COUNT]) -> Vec<SlotPlacement> {
    let present: Vec<&SlotView> = slots
        .iter()
        .filter(|s| s.index != 0 && (s.active || s.param > 0 || s.remaining_build.is_some()))
        .collect();
    let count = present.len().max(1) as f32;
    present
        .iter()
        .enumerate()
        .map(|(nth, s)| {
            // A ring, evenly spaced, starting at the top. One organelle sits in the middle
            // rather than off to one side, because a single blob orbiting nothing looks like
            // a mistake.
            let angle = std::f32::consts::TAU * nth as f32 / count - std::f32::consts::FRAC_PI_2;
            let ring = if present.len() == 1 { 0.0 } else { 0.5 };
            SlotPlacement {
                slot: s.index,
                kind: s.kind,
                dx: ring * angle.cos(),
                dy: ring * angle.sin(),
                // Square-rooted, so `param` reads as an area rather than a radius — a
                // chloroplast at 60 against one at 15 should look twice as big, not four times.
                radius: (0.12 + 0.30 * (s.param as f32 / 255.0).sqrt()).min(0.45),
                built: match s.remaining_build {
                    None => 1.0,
                    Some(_) => 0.35,
                },
            }
        })
        .collect()
}

/// Name the genes in a genome `gene a`, `gene b`, ... in the order they are declared.
///
/// Positional rather than looked up in a dictionary of the names the shipped `.mm` files
/// happen to use. A dictionary would label `ancestor.mm` nicely and have nothing to say about
/// the evolved descendant anybody actually wants to read, which is the wrong way round.
///
/// Public because the genome pane's gene list has to call the genes the same thing the listing
/// beside it does, and two positional namings that agree by coincidence would eventually stop.
#[must_use]
pub fn gene_label(nth: usize) -> String {
    // a..z, then aa, ab, ... — a genome with more than 26 genes is unusual but not illegal.
    let mut n = nth;
    let mut out = String::new();
    loop {
        out.insert(0, (b'a' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    format!("gene {out}")
}

/// The template the VM will actually read as this line's operand.
///
/// Not `line.template`, and the difference is the whole reason this function exists. The
/// disassembler reports `Template::EMPTY` when a template's letters are non-canonical `NOP`
/// bytes, because rendering them as `%101` would reassemble to different bytes — that is
/// deliberate and load-bearing for the round trip. But the VM does not read source, it reads
/// bytes, so at such a line it sees a perfectly ordinary template and jumps accordingly.
///
/// Resolving `line.template` there would print `IMM 0` for an instruction that pushes 60, and
/// `binds nothing` for an `EXPRESS` that binds. So the reading asks the genome's own
/// precomputed table, at the offset the VM would read from.
fn operand_of(genome: &mm_core::Genome, line: &mm_asm::Line) -> mm_core::isa::Template {
    if !line.op.takes_template() {
        return mm_core::isa::Template::EMPTY;
    }
    genome.template_at(genome.wrap((line.offset as usize).wrapping_add(1)))
}

/// Where a line's own template run ends, which is where its host falls through to.
fn falls_through_to(genome: &mm_core::Genome, line: &mm_asm::Line) -> u16 {
    let after = genome.wrap(
        (line.offset as usize)
            .wrapping_add(1)
            .wrapping_add(operand_of(genome, line).len as usize),
    );
    u16::try_from(after).unwrap_or(0)
}

/// The label for one disassembled line, if it deserves one.
///
/// Shown beside the source form, where the operand is still `%`-bits and a gene binding cannot
/// be read off it. In the reading form the binding *is* the operand, so [`reading_of`] renders
/// it there instead and this column is left out.
fn label_for(genome: &mm_core::Genome, line: &mm_asm::Line, cfg: VmConfig) -> Option<String> {
    match line.op {
        mm_core::Op::Gene => gene_at(genome, line).map(gene_label),
        mm_core::Op::Express => Some(match binding_of(genome, line, cfg) {
            Some((nth, 0)) => format!("→ {}", gene_label(nth)),
            // A drifted promoter still binds, and how far it has drifted is the thing worth
            // knowing: at the threshold it is one mutation from binding something else
            // entirely.
            Some((nth, d)) => format!("→ {} (drift {d})", gene_label(nth)),
            None => "binds nothing".to_string(),
        }),
        _ => None,
    }
}

/// Which declaration a `GENE` line is, by offset.
fn gene_at(genome: &mm_core::Genome, line: &mm_asm::Line) -> Option<usize> {
    genome
        .promoters()
        .iter()
        .position(|p| p.offset == line.offset as u16)
}

/// The gene an `EXPRESS` binds and how far its promoter has drifted, or `None` for a miss.
///
/// Asked of the VM's own binding search rather than a copy of it here, so the pane cannot
/// describe a jump that does not happen. A miss is worth showing too: an `EXPRESS` that binds
/// nothing falls through, and that is usually the bug.
fn binding_of(
    genome: &mm_core::Genome,
    line: &mm_asm::Line,
    cfg: VmConfig,
) -> Option<(usize, u16)> {
    let t = operand_of(genome, line);
    let entry = mm_core::vm::find_promoter(genome, t, cfg.promoter_bind_threshold)?;
    let promoters = genome.promoters();
    let nth = promoters.iter().position(|p| p.entry == entry)?;
    let distance = promoters
        .get(nth)
        .map(|p| t.promoter_distance(p.template))
        .unwrap_or(0);
    Some((nth, distance))
}

/// One line with its template operand resolved to what it means (M10.3b).
///
/// The other half of the pane's two documents. `Line::to_source` is byte-exact and the only
/// text the editor ever round-trips; this one is unassemblable on purpose and exists to be
/// read. Every resolution here comes from the VM's own search, so where the two documents
/// disagree it is because a `%` operand and its meaning genuinely are different things.
fn reading_of(genome: &mm_core::Genome, line: &mm_asm::Line, cfg: VmConfig) -> String {
    let mut s = String::new();
    s.push_str(line.op.name());
    if line.variant != 0 {
        s.push('~');
        s.push_str(&line.variant.to_string());
    }
    let Some(operand) = resolved_operand(genome, line, cfg) else {
        return s;
    };
    while s.len() < 8 {
        s.push(' ');
    }
    s.push(' ');
    s.push_str(&operand);
    s
}

/// The resolved operand text, or `None` for an instruction that takes no template.
fn resolved_operand(
    genome: &mm_core::Genome,
    line: &mm_asm::Line,
    cfg: VmConfig,
) -> Option<String> {
    use mm_core::Op;

    let t = operand_of(genome, line);
    let range = cfg.template_search_range;
    // `MAX_GENOME_LEN` is 65,536, so the last legal offset is 65,535 and this always fits.
    // Written as a conversion rather than a cast so that stops being true loudly.
    let ip = u16::try_from(line.offset).ok()?;

    // A zero-length template makes its host a no-op, except IMM, which still pushes 0
    // (SPEC §4.3). Saying so is more useful than an empty operand column, because a template
    // eaten by a mutation is a common way for a lineage to go quiet.
    let dead = t.is_empty() && line.op != Op::Imm;

    Some(match line.op {
        _ if !line.op.takes_template() => return None,
        _ if dead => "· no template".to_string(),
        // `Template::new` sets bit i for letter i, so `%001111` is 0b111100 = 60. This one
        // line removes most of the binary from a typical listing on its own.
        Op::Imm => t.value.to_string(),
        Op::JmpF | Op::JmpZ | Op::JmpNz | Op::Call => {
            match mm_core::vm::find_forward(genome, ip, t, range) {
                Some(target) => jump_text(genome, target),
                None => format!("✗ falls through to {}", falls_through_to(genome, line)),
            }
        }
        // LOOPLN's template is a backward jump target like JMPB's — it is `LN` that counts,
        // and `LN` is a register set by SETLN rather than anything encoded here.
        Op::JmpB | Op::LoopLn => match mm_core::vm::find_backward(genome, ip, t, range) {
            Some(target) => format!("↺ {target}"),
            None => format!("✗ falls through to {}", falls_through_to(genome, line)),
        },
        Op::Gene => match gene_at(genome, line) {
            Some(nth) => gene_label(nth),
            None => "· unreachable declaration".to_string(),
        },
        Op::Express => match binding_of(genome, line, cfg) {
            Some((nth, 0)) => format!("→ {}", gene_label(nth)),
            Some((nth, d)) => format!("→ {} (drift {d})", gene_label(nth)),
            None => format!(
                "✗ binds nothing, falls through to {}",
                falls_through_to(genome, line)
            ),
        },
        _ => return None,
    })
}

/// A jump target, named if it happens to be a gene's entry point.
///
/// The name is worth the lookup for `CALL` in particular: a call that lands on a promoter's
/// entry is a call to that gene, and saying so turns a number back into a subroutine.
fn jump_text(genome: &mm_core::Genome, target: u16) -> String {
    match genome
        .promoters()
        .iter()
        .position(|p| p.entry == target)
        .map(gene_label)
    {
        Some(name) => format!("→ {target} ({name})"),
        None => format!("→ {target}"),
    }
}

/// What the camera should do about the cell it is tracking, this frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Track {
    /// Centre on this point, in substrate squares.
    MoveTo(f32, f32),
    /// The cell being tracked is gone. Drop the selection and stop following.
    Lost,
    /// Leave the camera where it is.
    Stay,
}

/// Decide what tracking does, given what the inspection found.
///
/// A decision with a right answer, so it lives here where it can be tested rather than inside
/// a Bevy system where it cannot. The answer that matters is `Lost`: when the tracked cell
/// dies, following stops. Leaving the camera pinned to the square where a cell used to be
/// gives you a view that has stopped responding to the arrow keys with nothing on screen
/// saying why.
///
/// # Why there are two selections
///
/// `wanted` is the cell the front end has chosen. `read` is the cell the reading was taken of,
/// on the simulation thread, possibly a frame ago.
///
/// They are usually the same and the distinction looks like pedantry. It is not. Since M10.1
/// the reading is gathered on the other thread, so for a frame or two after a click the newest
/// bundle was built before the click happened and has no reading for the new cell. Judging
/// that as a death — which is what this did, because "no reading" was the only signal — clears
/// the selection immediately after it is made. The cell appears in the panel for a few frames
/// and vanishes, which is exactly what it looked like.
///
/// So a reading is only evidence about a cell when it was taken *of* that cell. Anything else
/// is a bundle that predates the question.
#[must_use]
pub fn tracking(
    inspection: Option<&Inspection>,
    following: bool,
    wanted: Option<mm_core::CellId>,
    read: Option<mm_core::CellId>,
) -> Track {
    if wanted != read {
        // The reading answers a question nobody is asking any more. It says nothing about the
        // cell now selected, and silence is not death.
        return Track::Stay;
    }
    match inspection {
        Some(c) if following => Track::MoveTo(
            c.x as f32 / mm_core::fixed::POS_ONE as f32,
            c.y as f32 / mm_core::fixed::POS_ONE as f32,
        ),
        Some(_) => Track::Stay,
        // Only `Lost` if there was something to lose. A frame with nothing selected is not a
        // bereavement, and reporting one would clear the follow flag every frame before the
        // user had picked anything.
        None if wanted.is_some() => Track::Lost,
        None => Track::Stay,
    }
}

/// What a run of a listing line is, for colour.
///
/// A listing is not `.mm` source — `→ 47 (gene b)` is not something any assembler would take —
/// so it cannot simply be handed to [`mm_asm::highlight`]. But the parts of it that *are*
/// source have to be coloured by the same rules the editor uses, or the two panes disagree
/// about what an opcode looks like and the disagreement is the thing you are staring at. So
/// [`spans`] classifies with the assembler's own lexer and names only what the reading adds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ink {
    /// An opcode name, including its `~n` encoding suffix.
    Opcode,
    /// A number the instruction acts on: an immediate's value, a jump's target.
    Number,
    /// A `%` template, in the source form.
    Pattern,
    /// A gene name this listing invented.
    Gene,
    /// `→` or `↺`: control goes somewhere.
    Marker,
    /// `✗`: it does not. A jump that never fires is usually why a lineage went quiet, so it
    /// gets a colour of its own rather than sharing one with the jumps that work.
    Miss,
    /// Punctuation, padding, and prose like `falls through to`.
    Note,
}

/// A classified run of a rendered listing line, in byte offsets into that line.
///
/// Offsets rather than owned strings, the same shape as [`mm_asm::highlight::Token`], so the
/// spans cost nothing beyond the line they describe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub ink: Ink,
    pub start: usize,
    pub end: usize,
}

/// Classify one rendered listing line for colour.
///
/// Covers the line exactly, gaps included, so a caller can paint straight through it without
/// having to work out what fell between two spans.
#[must_use]
pub fn spans(text: &str) -> Vec<Span> {
    use mm_asm::highlight::TokenKind;

    // `gene b` is two words and only the first is recognisable. Set by the word `gene` and
    // spent by the one after it, so the name is coloured as one thing.
    let mut naming = false;
    mm_asm::highlight::line(text)
        .into_iter()
        .map(|t| {
            let word = text.get(t.start..t.end).unwrap_or("");
            let ink = match t.kind {
                TokenKind::Opcode => Ink::Opcode,
                TokenKind::Number => Ink::Number,
                TokenKind::Pattern => Ink::Pattern,
                TokenKind::Promoter => Ink::Gene,
                TokenKind::Space => Ink::Note,
                _ => extra_ink(word, &mut naming),
            };
            Span {
                ink,
                start: t.start,
                end: t.end,
            }
        })
        .collect()
}

/// Classify a word the assembler's lexer had no name for.
fn extra_ink(word: &str, naming: &mut bool) -> Ink {
    // The reading brackets its gene names — `→ 47 (gene b)` — so compare on the word itself
    // rather than on the punctuation around it.
    let bare = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    if word.contains('→') || word.contains('↺') {
        return Ink::Marker;
    }
    if word.contains('✗') {
        return Ink::Miss;
    }
    if bare == "gene" {
        *naming = true;
        return Ink::Gene;
    }
    if std::mem::take(naming) {
        return Ink::Gene;
    }
    // `ADD~2` is a real opcode in a non-canonical encoding. The assembler's lexer does not
    // know the `~` form because it is disassembly output rather than anything anyone writes.
    if let Some((name, _)) = word.split_once('~') {
        if mm_core::isa::Op::from_name(name).is_some() {
            return Ink::Opcode;
        }
    }
    Ink::Note
}

/// A disassembled genome, kept so the panel does not re-disassemble every frame.
///
/// Keyed by genome hash. At sixty frames a second, taking three hundred bytes apart sixty
/// times a second to draw the same listing is work nobody asked for; and because genomes are
/// interned, following one cell means the key almost never changes.
#[derive(Default)]
pub struct Listing {
    hash: Option<u64>,
    /// Part of the key, because the reading resolves jumps and bindings against these. Change
    /// the search range in the parameter editor and every resolved target in the pane may
    /// move; a cache keyed on the genome alone would go on showing the old world's answers.
    cfg: Option<VmConfig>,
    lines: Vec<ListingLine>,
}

/// One disassembled instruction, ready to draw.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ListingLine {
    /// Byte offset of the opcode, which is what `ip` is compared against.
    pub offset: u32,
    /// The reassemblable form: `IMM      %001111`. Byte-exact, and the only text the editor
    /// ever hands back.
    pub text: String,
    /// [`spans`] of `text`, classified once here rather than on every frame it is drawn.
    pub text_spans: Vec<Span>,
    /// The same instruction with its template resolved: `IMM      60` (M10.3b).
    ///
    /// A separate document rather than a replacement. `%001111` is not a mistake to be
    /// corrected — it is the source form, and `assemble(disassemble(b)) == b` is an M0
    /// acceptance test that rendering it as `60` would break. So the pane holds both and shows
    /// one.
    pub reading: String,
    /// [`spans`] of `reading`.
    pub reading_spans: Vec<Span>,
    /// For a `GENE`, the label this listing gives it. For an `EXPRESS`, the label of the gene
    /// it will actually bind, with the Hamming distance if it is not an exact match.
    ///
    /// The names in a `.mm` file do not survive assembly — `#build` is hashed to an eight-bit
    /// promoter pattern and the string is gone (SPEC §4.4) — so a disassembly can only show
    /// the bits. Which is why every `EXPRESS` in the panel read `%01000111`, correct and
    /// useless. Labels are invented here from the genome's own promoter table instead, so an
    /// evolved genome that never had names gets them too.
    pub label: Option<String>,
}

impl Listing {
    /// The listing for this genome, disassembling only if it is not the one already held.
    pub fn of(&mut self, genome: &mm_core::Genome, hash: u64, cfg: VmConfig) -> &[ListingLine] {
        if self.hash != Some(hash) || self.cfg != Some(cfg) {
            // `Line::to_source` rather than a second renderer here: it is the one the editor
            // and the round-trip test already use, so the listing in the panel is the same
            // text that would reassemble to these bytes.
            let d = mm_asm::disassemble(genome.bytes());
            self.lines = d
                .lines
                .iter()
                .map(|l| {
                    // The readable rendering rather than `%` letters, so the pane's source
                    // column is the same text the editor is loaded with. Both reassemble to
                    // the identical bytes.
                    let text = l.to_readable();
                    let reading = reading_of(genome, l, cfg);
                    ListingLine {
                        offset: l.offset,
                        text_spans: spans(&text),
                        reading_spans: spans(&reading),
                        text,
                        reading,
                        label: label_for(genome, l, cfg),
                    }
                })
                .collect();
            self.hash = Some(hash);
            self.cfg = Some(cfg);
        }
        &self.lines
    }

    /// Which line the instruction pointer is on, if any.
    ///
    /// `ip` is a byte offset and a line can span several bytes, so this is the last line that
    /// starts at or before it rather than an exact match — otherwise the marker would vanish
    /// whenever the pointer sat on a template letter.
    #[must_use]
    pub fn line_at(&self, ip: u16) -> Option<usize> {
        let ip = ip as u32;
        self.lines
            .iter()
            .rposition(|l| l.offset <= ip)
            .filter(|_| !self.lines.is_empty())
    }

    #[must_use]
    pub fn lines(&self) -> &[ListingLine] {
        &self.lines
    }
}

/// Read a circular stack out in pop order: deepest entry first, top of stack last.
fn unwind<T: Copy>(buf: &[T], sp: u8, len: u8, capacity: usize) -> Vec<T> {
    let live = (len as usize).min(capacity).min(buf.len());
    if live == 0 || capacity == 0 {
        return Vec::new();
    }
    // `sp` indexes the top. Walk back `live` entries and read forward, so the caller gets the
    // order they would pop in reversed — which is the order a stack is drawn in.
    (0..live)
        .rev()
        .filter_map(|back| {
            let at = (sp as usize).wrapping_sub(back) % capacity;
            buf.get(at).copied()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_core::fixed::{pos, q10};
    use mm_core::{CellSeed, Organelle, Scenario};

    fn slots_with(kinds: &[(usize, OrganelleType, u8)]) -> [SlotView; SLOT_COUNT] {
        let mut slots = [SlotView {
            index: 0,
            kind: OrganelleType::Empty,
            param: 0,
            control: [0; 2],
            remaining_build: None,
            active: false,
        }; SLOT_COUNT];
        for (n, s) in slots.iter_mut().enumerate() {
            s.index = n;
        }
        slots[0].kind = OrganelleType::Membrane;
        slots[0].param = 24;
        slots[0].active = true;
        for (slot, kind, param) in kinds {
            slots[*slot].kind = *kind;
            slots[*slot].param = *param;
            slots[*slot].active = true;
        }
        slots
    }

    #[test]
    fn the_schematic_places_one_organelle_per_occupied_slot() {
        let slots = slots_with(&[
            (1, OrganelleType::Nucleus, 40),
            (2, OrganelleType::Mitochondrion, 50),
            (3, OrganelleType::Chloroplast, 60),
        ]);
        let placed = placements(&slots);
        assert_eq!(placed.len(), 3, "one blob per organelle");
        assert!(
            placed.iter().all(|p| p.slot != 0),
            "the membrane is the circle the others are drawn inside, not a blob on it"
        );
        // Everything has to land inside the cell, or the diagram draws outside its own outline.
        for p in &placed {
            let reach = (p.dx * p.dx + p.dy * p.dy).sqrt() + p.radius;
            assert!(
                reach <= 1.0,
                "{:?} at {} sticks out of the cell",
                p.kind,
                reach
            );
        }
    }

    #[test]
    fn a_lone_organelle_sits_in_the_middle() {
        // Rather than orbiting a centre with nothing at it, which reads as a bug.
        let placed = placements(&slots_with(&[(1, OrganelleType::Nucleus, 40)]));
        assert_eq!(placed.len(), 1);
        assert_eq!((placed[0].dx, placed[0].dy), (0.0, 0.0));
    }

    #[test]
    fn scaffolding_is_drawn_faint_and_a_bigger_organelle_is_drawn_bigger() {
        let mut slots = slots_with(&[
            (1, OrganelleType::Nucleus, 40),
            (2, OrganelleType::Chloroplast, 200),
        ]);
        slots[1].remaining_build = Some(8);
        let placed = placements(&slots);
        let of = |slot: usize| *placed.iter().find(|p| p.slot == slot).expect("placed");
        assert!(
            of(1).built < of(2).built,
            "a half-built organelle is not faint"
        );
        assert!(of(2).radius > of(1).radius, "param does not scale the blob");
    }

    #[test]
    fn an_empty_cell_places_nothing_and_does_not_divide_by_zero() {
        assert!(placements(&slots_with(&[])).is_empty());
    }

    #[test]
    fn a_listing_is_disassembled_once_per_genome() {
        let world = World::new(Scenario::stress(8, 8)).unwrap();
        let a = world.genomes().intern(vec![0x2Eu8; 24]).unwrap();
        let b = world.genomes().intern(vec![0x11u8; 24]).unwrap();

        let mut listing = Listing::default();
        let first = listing.of(&a, a.hash(), VmConfig::DEFAULT).len();
        assert!(first > 0, "a genome disassembled to nothing");
        // Same genome again: the cache holds, which is the whole point at sixty frames a
        // second following one cell.
        let ptr = listing.lines().as_ptr();
        assert_eq!(listing.of(&a, a.hash(), VmConfig::DEFAULT).len(), first);
        assert_eq!(listing.lines().as_ptr(), ptr, "it disassembled again");
        // A different genome replaces it.
        listing.of(&b, b.hash(), VmConfig::DEFAULT);
        assert_ne!(listing.lines().as_ptr(), ptr);
    }

    #[test]
    fn the_instruction_pointer_finds_a_line_wherever_it_lands() {
        let world = World::new(Scenario::stress(8, 8)).unwrap();
        let g = world.genomes().intern(vec![0x2Eu8; 24]).unwrap();
        let mut listing = Listing::default();
        listing.of(&g, g.hash(), VmConfig::DEFAULT);

        // Every byte offset in the genome must resolve to some line — an `ip` sitting on a
        // template letter is the common case, and a marker that vanished for it would flicker
        // exactly when someone was watching a template being read.
        for ip in 0..24u16 {
            let line = listing.line_at(ip);
            assert!(line.is_some(), "ip {ip} is on no line at all");
            let at = listing.lines()[line.expect("some")].offset;
            assert!(at <= ip as u32, "line at {at} is after ip {ip}");
        }
    }

    #[test]
    fn an_empty_listing_reports_no_line_rather_than_index_zero() {
        let listing = Listing::default();
        assert_eq!(listing.line_at(0), None);
    }

    #[test]
    fn genes_are_labelled_and_every_express_says_what_it_binds() {
        // The names in a `.mm` file are hashed away at assembly, so a disassembly can only
        // show eight bits of promoter — which is what the panel was doing, correctly and
        // uselessly. These labels are recovered from the genome's own promoter table.
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../genomes/ancestor.mm"
        ))
        .expect("the ancestor is in the repository");
        let bytes = mm_asm::assemble(&src).expect("assembles").bytes;
        let world = World::new(Scenario::stress(8, 8)).unwrap();
        let genome = world.genomes().intern(bytes).expect("interned");

        let mut listing = Listing::default();
        listing.of(&genome, genome.hash(), VmConfig::DEFAULT);
        let lines = listing.lines().to_vec();

        let genes: Vec<&ListingLine> = lines
            .iter()
            .filter(|l| l.text.to_lowercase().starts_with("gene"))
            .collect();
        assert!(genes.len() >= 4, "the ancestor has four genes and a driver");
        // Each declaration gets its own label, in declaration order.
        let labels: Vec<&String> = genes.iter().filter_map(|l| l.label.as_ref()).collect();
        assert_eq!(labels.len(), genes.len(), "a GENE went unlabelled");
        assert_eq!(labels[0], "gene a");
        assert_eq!(labels[1], "gene b");

        // And every EXPRESS says where it actually lands, which is the thing the bits do not
        // tell you. The ancestor's are exact matches, so none should report drift.
        let expresses: Vec<&ListingLine> = lines
            .iter()
            .filter(|l| l.text.to_lowercase().starts_with("express"))
            .collect();
        assert!(expresses.len() >= 4, "the driver expresses four genes");
        for e in &expresses {
            let label = e.label.as_ref().expect("an EXPRESS with no binding");
            assert!(
                label.starts_with("→ gene "),
                "{label:?} does not name the gene it binds"
            );
            assert!(
                !label.contains("drift"),
                "{label:?} — the ancestor's are exact"
            );
        }
        // Every gene the driver names is one that exists.
        for e in &expresses {
            let named = e.label.as_ref().expect("label").trim_start_matches("→ ");
            assert!(
                labels.iter().any(|l| l.as_str() == named),
                "{named} is bound but never declared"
            );
        }
    }

    #[test]
    fn a_promoter_that_binds_nothing_says_so() {
        // An EXPRESS that matches no gene falls through and does nothing, which is almost
        // always the bug. Silence would be the worst possible label for it.
        let world = World::new(Scenario::stress(8, 8)).unwrap();
        // A genome of EXPRESS with no GENE anywhere in it.
        let src = "        EXPRESS #nothing
        HALT
";
        let bytes = mm_asm::assemble(src).expect("assembles").bytes;
        let genome = world.genomes().intern(bytes).expect("interned");
        let mut listing = Listing::default();
        listing.of(&genome, genome.hash(), VmConfig::DEFAULT);
        let express = listing
            .lines()
            .iter()
            .find(|l| l.text.to_lowercase().starts_with("express"))
            .expect("an EXPRESS");
        assert_eq!(express.label.as_deref(), Some("binds nothing"));
    }

    #[test]
    fn gene_labels_keep_going_past_the_alphabet() {
        assert_eq!(gene_label(0), "gene a");
        assert_eq!(gene_label(25), "gene z");
        assert_eq!(gene_label(26), "gene aa");
        assert_eq!(gene_label(27), "gene ab");
    }

    #[test]
    fn tracking_follows_a_living_cell_and_lets_go_of_a_dead_one() {
        let (world, id) = world_with_a_cell();
        let c = Inspection::of(&world, id).expect("alive");
        let picked = Some(id);

        assert_eq!(
            tracking(Some(&c), false, picked, picked),
            Track::Stay,
            "not following"
        );
        match tracking(Some(&c), true, picked, picked) {
            Track::MoveTo(x, y) => {
                assert!((x - 4.0).abs() < 0.01 && (y - 5.0).abs() < 0.01, "{x},{y}");
            }
            other => panic!("following a live cell gave {other:?}"),
        }

        // The cell dies: the simulation thread looked for this exact cell and found nothing.
        // Following has to stop, or the camera stays pinned to an empty square and the view
        // looks broken rather than finished.
        assert_eq!(tracking(None, true, picked, picked), Track::Lost);
        assert_eq!(tracking(None, false, picked, picked), Track::Lost);

        // But a frame with nothing selected at all is not a loss — reporting one would clear
        // the flag every frame before anybody had picked a cell.
        assert_eq!(tracking(None, true, None, None), Track::Stay);
    }

    #[test]
    fn a_freshly_picked_cell_is_not_reported_dead_before_it_has_been_read() {
        // The bug this argument exists for. A click sets the selection on this thread; the
        // reading is taken on the other one and arrives a frame or two later. Until it does,
        // the newest bundle was built against the *previous* selection and has nothing to say
        // about the new cell — and "nothing to say" was being read as "died", which cleared the
        // selection immediately after it was made. The cell flickered into the panel and
        // vanished.
        let (world, id) = world_with_a_cell();
        let c = Inspection::of(&world, id).expect("alive");

        // Just clicked: wanted is the new cell, the bundle was built with nothing selected.
        assert_eq!(tracking(None, true, Some(id), None), Track::Stay);
        assert_eq!(tracking(None, false, Some(id), None), Track::Stay);

        // And the other way: the bundle still carries the previous cell's reading. It must not
        // be followed, or the camera flies to a cell that is no longer selected.
        assert_eq!(tracking(Some(&c), true, None, Some(id)), Track::Stay);

        // Once the reading catches up, it is authoritative again.
        match tracking(Some(&c), true, Some(id), Some(id)) {
            Track::MoveTo(..) => {}
            other => panic!("a caught-up reading gave {other:?}"),
        }
    }

    #[test]
    fn a_reading_of_a_different_cell_is_never_evidence() {
        // Selection changed from one live cell to another. The bundle in hand describes the old
        // one. Neither "follow it" nor "the new one is dead" is a defensible reading of that.
        let (world, id) = world_with_a_cell();
        let c = Inspection::of(&world, id).expect("alive");
        let other = CellId::NONE;
        assert_eq!(tracking(Some(&c), true, Some(other), Some(id)), Track::Stay);
        assert_eq!(tracking(None, true, Some(other), Some(id)), Track::Stay);
    }

    fn world_with_a_cell() -> (World, CellId) {
        let mut world = World::new(Scenario::stress(16, 16)).unwrap();
        let genome = world.genomes().intern(vec![0u8; 32]).unwrap();
        let id = world.spawn_cell(CellSeed {
            x: pos(4),
            y: pos(5),
            mass: q10(20),
            energy: q10(100),
            membrane: 16,
            key: 7,
            badge: 0,
            species: 3,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        if let Some(i) = world.cells_mut().index(id) {
            world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
            world.cells_mut().interior_mut(i)[8] = q10(5);
        }
        world.adopt_current_contents_as_baseline();
        (world, id)
    }

    #[test]
    fn an_inspection_describes_the_cell_it_was_taken_from() {
        let (world, id) = world_with_a_cell();
        let v = Inspection::of(&world, id).expect("the cell is alive");
        assert_eq!(v.id, id);
        // Not the 3 the seed asked for: since M5, `World::spawn_cell` assigns a species from
        // the archive by fingerprint rather than taking the caller's word for it, and this is
        // the first cell so it founds species 0.
        assert_eq!(v.species, 0);
        assert_eq!(v.key, 7);
        assert_eq!(v.genome_len, 32);
        assert_eq!(v.energy, q10(100));
        assert_eq!(v.interior[8], q10(5));
        assert_eq!(v.slots[1].kind, OrganelleType::Nucleus);
        assert_eq!(v.slots[1].param, 40);
        assert!(
            v.slots[1].remaining_build.is_none(),
            "it was built finished"
        );
        assert_eq!(
            v.fidelity,
            Some(mm_core::Q10_ONE),
            "a fresh nucleus is at full"
        );
    }

    #[test]
    fn inspecting_a_dead_cell_finds_nothing() {
        let (mut world, id) = world_with_a_cell();
        world.cells_mut().despawn(id);
        assert_eq!(Inspection::of(&world, id), None);
    }

    #[test]
    fn inspecting_does_not_change_the_world() {
        let (mut world, id) = world_with_a_cell();
        world.step();
        let before = world.state_hash();
        for _ in 0..100 {
            let _ = Inspection::of(&world, id);
        }
        assert_eq!(world.state_hash(), before);
    }

    #[test]
    fn the_stack_reads_top_last() {
        // The data stack is circular, so the raw array is rotated by however many pushes have
        // happened. What the panel must show is push order, whatever the rotation is.
        let capacity = 8usize;
        let buf: Vec<i16> = (0..capacity as i16).collect();
        // Three live entries with the top at index 1: pushes landed on 7, 0, 1.
        assert_eq!(unwind(&buf, 1, 3, capacity), vec![7, 0, 1]);
        // Not wrapped: top at 4, four live.
        assert_eq!(unwind(&buf, 4, 4, capacity), vec![1, 2, 3, 4]);
        // Empty and over-full both behave.
        assert_eq!(unwind(&buf, 4, 0, capacity), Vec::<i16>::new());
        assert_eq!(unwind(&buf, 0, 200, capacity).len(), capacity);
    }
}
