//! What a nervous system costs, and whether a wire beats the water.
//!
//! `genomes/reflex.mm` is a nerve net built out of the pieces the engine already has: a soft
//! junction is the synapse (SPEC §8.1 says so in as many words), `JXFER` is transmitter release,
//! and the receiving cell's cytoplasm is the dendritic integrator. Nothing was added to the
//! engine to make it. This probe exists to find out which of the known constraints actually
//! bites before anything *is* added.
//!
//! Run with
//! `cargo test -p mm-core --test reflex_probe -- --ignored --nocapture --test-threads=1`.
//!
//! **`#[ignore]`, like every other probe in the tree.** `predator_probe` learned this the
//! expensive way: it was 36% of the cost of testing every change, paid to re-derive a finding
//! already written at the top of the file. A probe answers a question once.
//!
//! # The question
//!
//! A cell can already signal two ways. **Broadcast**: `EMIT` into its own square, read by a
//! neighbour's chemosensor, carried by diffusion. **Wired**: `JXFER` down a soft junction,
//! directed, one tick, no diffusion. The second is more expensive — a junction to hold, an
//! energy cost per unit moved, and five instructions out of sixteen a tick to drive it.
//!
//! So the question is not whether a nerve net is *expressible*. It is whether the wire is worth
//! anything over the water, given that `signal_a` is the fastest-diffusing chemical in the
//! default table and a soft junction reaches three squares — which is about as far as diffusion
//! gets in a handful of ticks anyway.
//!
//! # What it found
//!
//! **The synapse works, and against the shipped chemical table it buys almost nothing.**
//!
//! With the default table, a stimulus on the head cell has every cell in the chain responding
//! within about thirty ticks *whether or not the wire exists*. Both arms below reach full
//! cilium power at the same time; the wire changes which cell holds the transmitter, not when
//! the organism reacts. `signal_a` diffuses at `Q10/4`, the fastest rate in the table, and three
//! cells two squares apart are one puddle on the timescale a genome runs at.
//!
//! Forcing `signal_a`'s diffusion to **zero** separates the hypotheses, and there the wire is the
//! whole story: the wired chain still conducts end to end, and the control — the same junctions
//! in slots the genome cannot reach — propagates nothing at all. So the mechanism is real and
//! the *default chemistry* is what makes it pointless. **A nervous system needs a transmitter
//! that does not diffuse**, and nothing in the shipped table is one.
//!
//! **The middle cell conducts without ever being excited.** In the zero-diffusion arm it reads
//! zero transmitter and zero cilium power throughout, while the tail behind it climbs to full.
//! Resolve applies every intent in one pass in slot order, so a chain whose cells happen to sit
//! in ascending slot order propagates end-to-end **in a single tick** — and the same chain in
//! descending order would take one tick per hop. *Conduction velocity is a function of birth
//! order rather than of biology.* Nothing is non-deterministic about it (slot order is id
//! order, I6 is intact), but it is not a property anybody chose.
//!
//! **A directed arc has to spend two slots per cell.** `#relay` transmits into junction slots 0
//! and 1, so a junction sitting in slot 0 at *both* ends makes transmission symmetric — and the
//! first version of this probe measured exactly that: the middle cell pushed into slot 0 first,
//! which is its junction back towards the head, and the signal sloshed backwards while the tail
//! received nothing. An axon needs the downstream junction in a slot the genome writes to and
//! the upstream one in a slot it does not. With `JUNCTIONS_PER_CELL` at four, **the middle cell
//! of a chain of three is already half full**, and a cell with two inputs and two outputs is
//! full before it has a body to hold together.
//!
//! **A soft junction cannot hold a body.** It carries no positional constraint (SPEC §8.1), so
//! the first chain here drifted apart on Brownian jitter and every junction had broken inside
//! fifty ticks. The junctions below are **hard**, and `resolve_transfer` never checks the kind —
//! so a hard junction carries signal too, which is the only reason the slot budget is survivable.
//!
//! **An undivided cell dies of turgor**, which is what took the first three runs of this probe.
//! See [`an_undivided_cell_dies_of_turgor`]: solute climbs linearly to seventeen capacities
//! against a threshold of four, the quadratic charge takes hold around twelve, and the cell is
//! dead by tick 1,800. The ancestor is on the same trajectory and survives it only by dividing,
//! which sheds solute. `docs/STIFFNESS.md` §3 predicted this from the other end.
//!
//! **Two catalogue entries lie, and one of them breaks a shipped genome.** The junction port has
//! no `OGET` readings anywhere and is not required in order to `JOIN`, so no genome can observe
//! its own junctions — see [`a_genome_cannot_tell_whether_it_is_wired`]. `parasite.mm` branches
//! on that reading, so its `connected` state is unreachable and it has never injected anything.
//! The pump is the other, and it is the organelle a nerve net would need: the transmitter here is
//! *matter*, conserved, so a net is a bucket brigade and cannot re-pump a gradient the way a real
//! neuron does.
//!
//! # The sweep, and the one thing that blocks all of it
//!
//! [`where_the_wire_starts_to_pay`] walks `signal_a`'s diffusion down by halves from the fastest
//! rate the engine allows, and measures how long the *tail* cell takes to reach half thrust:
//!
//! ```text
//!   diffusion   wired   control   what the wire bought
//!   Q10/4 (shipped)  21      21    nothing
//!   Q10/8            29      37    8 ticks
//!   Q10/16           29      53    24 ticks
//!   Q10/32           29     101    72 ticks      <- as slow as detritus, the table's slowest
//!   Q10/64           29     165    136 ticks
//!   Q10/128          29     213    184 ticks
//!   Q10/256          29     277    248 ticks
//!   Q10/512          29     341    312 ticks
//!   Q10/1024         29   never    the response itself
//!   none             29   never    the response itself
//! ```
//!
//! **The wired column is flat.** Twenty-nine ticks at every rate from `Q10/8` down to zero, while
//! the diffusive column scales as roughly `1/D` and then stops arriving at all. That is the whole
//! case for a nervous system, measured: *conduction time is independent of the chemistry, and
//! diffusion time is not.* The shipped table is simply the one rate where the difference is zero.
//!
//! The same amount of matter enters the chain in both arms at every rate down to `Q10/512`, so
//! this is not a dosing artefact — below that the stimulus stops spreading far enough for the
//! head cell to take up as much, and the totals part company.
//!
//! **A transmitter as slow as detritus — already in the table — buys 72 ticks.** Nothing has to
//! be invented; a scenario has to author one.
//!
//! # Can it assemble itself?
//!
//! [`whether_the_genome_can_wire_itself`] removes the harness. With hard junctions the genome
//! **does** build a connected chain unaided: `X... XX.. X...`, one link on each end cell and two
//! on the middle. And it conducts nothing at all.
//!
//! `resolve_join` gives each end the *lowest free slot*, so the inbound junction lands in slot 0
//! — which is the first slot `#relay` transmits into. Every cell therefore sends its signal back
//! the way it came before it sends it onward, and the middle cell empties towards the head. The
//! net is connected and electrically sterile.
//!
//! > **This is the one defect that blocks the whole idea.** A genome cannot choose a junction
//! > slot, cannot read which slot a junction landed in, and cannot tell inbound from outbound —
//! > so it cannot build a directed arc, and an undirected one does not conduct. Everything else
//! > here is workable: the opcode moves matter exactly, the cytoplasm sums inputs for free, hard
//! > junctions carry signal, and a slow transmitter makes the wire decisively better than the
//! > water. The fix is `OGET` readings on the junction port — catalogue slot 10, already built,
//! > already priced, currently answering zero to everything. No catalogue slot, no opcode.
//!
//! **`JXFER` cannot address chemical 0 by its own index.** Operand zero is the energy channel, so
//! `signal_a` has to be written as 16 and wrapped back by `chem_index`. Every other opcode takes
//! a chemical at face value. It is a wart rather than a bug, and it is written into `reflex.mm`
//! with a comment beside it because the obvious spelling silently donates energy instead.

use std::path::Path;

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, pos_to_square, q10, POS_ONE};
use mm_core::junction::{self, Junction, JunctionKind};
use mm_core::light::CurrentField;
use mm_core::{LightRegime, MutationRates, Scenario, Seeding, VmConfig, World};

/// The transmitter. Chemical 0 of the default table, which the engine ascribes no meaning —
/// which is exactly what a communication channel wants.
const SIGNAL: usize = 0;

/// How far apart the cells of the chain sit, in substrate squares.
///
/// Two, not one: a soft junction reaches three (`soft_max_range`), so this is inside the wire's
/// range and outside the range at which the three cells are simply one puddle. It is the
/// geometry the comparison is about, so it is named rather than written into the calls.
const SPACING: i32 = 2;

/// The slot `reflex.mm` builds its cilium into, so the harness can read the motor output.
const CILIUM: usize = 4;

fn assemble(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    mm_asm::assemble(&src)
        .unwrap_or_else(|e| panic!("{name} does not assemble:\n{e}"))
        .bytes
}

/// A still, uniformly lit slide with the chemistry the genome eats and none of the transmitter.
///
/// Still water on purpose: the comparison is between a wire and diffusion, and a current would
/// add a third transport route that neither arm controls for.
fn petri(seed: u64, size: u32) -> Scenario {
    petri_with_diffusion(seed, size, None)
}

/// The same slide, optionally with the transmitter's diffusion overridden.
///
/// Setting it to zero is how the probe tells "the wire adds nothing" apart from "the wire does
/// not work" — two hypotheses that produce identical tables when diffusion is doing all the
/// delivery, and the difference between a finding and a bug.
fn petri_with_diffusion(seed: u64, size: u32, diffusion: Option<i32>) -> Scenario {
    let chemicals = match diffusion {
        None => mm_core::chem::ChemTable::spec_default(),
        Some(rate) => {
            let mut defs: Vec<_> = mm_core::chem::ChemTable::spec_default().into();
            defs[SIGNAL].diffusion = rate;
            mm_core::chem::ChemTable::new(defs)
        }
    };
    Scenario {
        chemicals,
        name: "reflex".to_string(),
        seed,
        width: size,
        height: size,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        current: CurrentField::Still,
        seeding: vec![
            Seeding::Uniform {
                chemical: 11,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 14,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 4,
                per_square: q10(400),
            },
            // The minerals every recipe in the catalogue is costed in, at the
            // Redfield proportion of the carbon above. Nothing produces them.
            Seeding::Uniform {
                chemical: 5,
                per_square: (q10(400)) * 16 / 106,
            },
            Seeding::Uniform {
                chemical: 6,
                per_square: (q10(400)) / 53,
            },
        ],
        ..Scenario::default()
    }
}

fn place(world: &mut World, genome: &[u8], x: i32, y: i32) -> CellId {
    let g = world.genomes().intern(genome.to_vec()).expect("genome");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(y),
        mass: q10(30),
        energy: q10(4_000),
        membrane: 24,
        key: 42,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome: g,
    });
    world.adopt_current_contents_as_baseline();
    id
}

/// Join two cells directly, into named slots.
///
/// **Hard, not soft, and the slots are chosen rather than found.** Two corrections the first
/// version of this probe needed, both of which are findings in their own right.
///
/// A soft junction carries *no positional constraint* (SPEC §8.1), so a chain held together by
/// soft junctions is not held together at all: the three cells drifted apart on Brownian jitter
/// and every junction had broken within fifty ticks. A body needs hard junctions and a nervous
/// system needs channels, and there are four junction slots for both.
///
/// `resolve_transfer` checks only that a junction exists, never its kind, so a **hard junction
/// carries signal too**. SPEC §8.1's "soft is the synapse" is a description of intent, not a
/// restriction — which is lucky, because a linear chain of three already needs two hard
/// junctions on its middle cell and would have no room for two soft ones beside them.
///
/// **The two ends take different slots, and that is not tidiness.** `#relay` transmits into slots 0
/// and 1, so a junction occupying slot 0 at *both* ends makes transmission symmetric — and the
/// middle cell of a chain then pushes into slot 0 first, which is its junction back towards the
/// head. Measured: the signal sloshes backwards and the tail cell never receives anything at all.
/// A directed arc needs the downstream junction in a slot the genome transmits into and the
/// upstream one in a slot it does not, which is the difference between an axon and a puddle.
///
/// Choosing the slot is what makes the control arm exact. `#relay` transmits into junction slots
/// 0 and 1; put the same junctions in slots 2 and 3 instead and the chain is *structurally
/// identical* — same geometry, same energy, same everything — while the genome's `JXFER` finds
/// nothing. One bit of difference between the arms.
fn wire(world: &mut World, a: CellId, b: CellId, slot_a: usize, slot_b: usize) {
    let (ia, ib) = (
        world.cells().index(a).expect("a alive"),
        world.cells().index(b).expect("b alive"),
    );
    let rest = junction::distance(world.cells(), ia, ib).max(POS_ONE);
    world.cells_mut().junctions_mut(ia)[slot_a] = Junction {
        kind: JunctionKind::Hard,
        other: b,
        rest,
    };
    world.cells_mut().junctions_mut(ib)[slot_b] = Junction {
        kind: JunctionKind::Hard,
        other: a,
        rest,
    };
}

/// Everything the harness reads back about one cell.
#[derive(Clone, Copy, Default)]
struct Reading {
    /// Transmitter held, in whole units.
    signal: i32,
    /// Junctions currently occupied.
    junctions: usize,
    /// What the cilium has been told to do: `control[0]`, signed power, ±1024 is full.
    ///
    /// The direct readout of the reflex. Displacement was the first thing measured here and it
    /// is a much worse signal — Brownian jitter moves a cell further in fifty ticks than a
    /// cilium at a tenth power does, so the motor output was buried in noise it had nothing to
    /// do with.
    power: i32,
}

fn read(world: &World, id: CellId) -> Reading {
    let Some(i) = world.cells().index(id) else {
        return Reading::default();
    };
    let cells = world.cells();
    Reading {
        signal: cells.interior(i)[SIGNAL] / mm_core::Q10_ONE,
        junctions: cells.junctions(i).iter().filter(|j| j.is_some()).count(),
        power: cells.slots(i)[CILIUM].control[0] as i32,
    }
}

/// Three cells in a row, given time to build their bodies before anything is measured.
///
/// `settle` matters: the genome's `#build` gene runs from the same sixteen instructions a tick
/// as everything else, so a cell has no cilium to drive for the first few hundred ticks. A
/// stimulus applied before then measures a body under construction.
fn chain_with_diffusion(
    seed: u64,
    settle: u64,
    diffusion: Option<i32>,
) -> (World, [CellId; 3], [(i32, i32); 3]) {
    let bytes = assemble("reflex.mm");
    let mut world = World::new(petri_with_diffusion(seed, 48, diffusion)).expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    let mid = 24;
    let at = [
        (mid - SPACING, mid),
        (mid, mid),
        (mid + SPACING, mid),
    ];
    let ids = [
        place(&mut world, &bytes, at[0].0, at[0].1),
        place(&mut world, &bytes, at[1].0, at[1].1),
        place(&mut world, &bytes, at[2].0, at[2].1),
    ];
    world.run(settle);
    (world, ids, at)
}

/// Put the stimulus on the head cell's square and nowhere else.
///
/// Its *current* square, read off the arena, not the square it was placed on. Cells drift on
/// Brownian jitter during the settle, and dosing the original position worked only because
/// diffusion carried the stimulus back to wherever the cell had got to — which is invisible
/// until diffusion is switched off, at which point the head cell simply never sees its own
/// stimulus and the whole run reads as a dead wire.
fn stimulate(world: &mut World, id: CellId, amount: i32) {
    let i = world.cells().index(id).expect("alive");
    let (x, y) = (
        pos_to_square(world.cells().x[i]),
        pos_to_square(world.cells().y[i]),
    );
    world.substrate_mut().add_chem(SIGNAL, x, y, amount);
    world.adopt_current_contents_as_baseline();
}

// ---------------------------------------------------------------------------------------------
// 1. The headline: does the wire beat the water?

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn a_wired_chain_against_an_unwired_one() {
    run_chain(None, "default table: signal_a diffuses at Q10/4, the fastest in it");
    run_chain(
        Some(0),
        "signal_a diffusion forced to ZERO: the wire is the only route there is",
    );
}

fn run_chain(diffusion: Option<i32>, note: &str) {
    // Settled well short of where turgor kills an undivided cell — see `why_is_it_dying`. At
    // this point the body is built and the economy is at its fixed point.
    let settle = 600;
    let dose = q10(4_000);
    // Every four ticks, because the transient is the whole measurement. `signal_a` is the
    // fastest-diffusing chemical in the default table and the genome takes ~21 ticks to walk
    // itself once, so sampling at 50 ticks — which the first version did — reads only the
    // aftermath.
    let every = 4;
    let samples = 20;

    for wired in [true, false] {
        // Downstream into slot 0, which `#relay` transmits into; upstream into slot 2, which it
        // does not. In the control arm both ends go into slots the genome cannot reach, so the
        // chain is structurally identical and electrically silent.
        let out = if wired { 0 } else { 3 };
        let (mut world, ids, _at) = chain_with_diffusion(7, settle, diffusion);
        wire(&mut world, ids[0], ids[1], out, 2);
        wire(&mut world, ids[1], ids[2], out, 2);
        stimulate(&mut world, ids[0], dose);

        println!("\n=== {note}");
        println!(
            "CHAIN {}  (stimulus {} units on the head cell's square only, spacing {})",
            if wired {
                "wired    downstream junction in slot 0, where #relay transmits"
            } else {
                "control  the same junctions in slots 2,3 — invisible to #relay"
            },
            dose / mm_core::Q10_ONE,
            SPACING,
        );
        println!("  tick    head sig  mid sig  tail sig   head pow  mid pow  tail pow   junctions");
        for step in 0..=samples {
            if step > 0 {
                world.run(every);
            }
            let r: Vec<Reading> = ids.iter().map(|id| read(&world, *id)).collect();
            println!(
                "  {:>5}   {:>8}  {:>7}  {:>8}   {:>8}  {:>7}  {:>8}   {:>9}",
                settle + step * every,
                r[0].signal,
                r[1].signal,
                r[2].signal,
                r[0].power,
                r[1].power,
                r[2].power,
                r.iter().map(|x| x.junctions).sum::<usize>(),
            );
        }
        // The middle cell of a chain of three: two hard junctions to hold the body together.
        // Two more would be the whole budget.
        let mid = read(&world, ids[1]);
        println!(
            "  the middle cell of a three-cell chain holds {} of {} junction slots",
            mid.junctions,
            junction::JUNCTIONS_PER_CELL
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 2. What one pass of the program costs, against the budget it has.

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn what_one_expression_cycle_costs() {
    // Static rather than traced: the VM is not exposed on the arena, and the arithmetic is the
    // thing worth knowing anyway — CLAUDE.md names the instruction budget as one of the
    // parameters to report when an evolutionary result is starved.
    let budget = VmConfig::DEFAULT.instr_per_tick;
    println!("\nBUDGET  instr_per_tick = {budget}");
    for name in ["reflex.mm", "ancestor.mm", "parasite.mm"] {
        let bytes = assemble(name);
        // Every opcode is one byte; operands are templates that follow. So bytes is an upper
        // bound on instructions and the truth is somewhat under it, which is stated rather than
        // hidden because the conclusion does not depend on the difference.
        let ticks = (bytes.len() as f64 / budget as f64).ceil() as u32;
        println!(
            "  {name:<14} {:>4} bytes   ≤ {:>3} ticks to walk the whole genome once",
            bytes.len(),
            ticks,
        );
    }
    println!(
        "  a synapse is 5 instructions (IMM, IMM, ZERO, JXFER, DROP) — {:.0}% of one tick",
        5.0 * 100.0 / budget as f64
    );
}

// ---------------------------------------------------------------------------------------------
// 3. The finding that shapes the genome: nothing can ask.

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn a_genome_cannot_tell_whether_it_is_wired() {
    // `JOIN` returns 1 optimistically, and its comment says the genome "finds out by looking".
    // There is nothing to look at: `JunctionPort` is catalogue slot 10, has no `OGET` readings
    // in `biology::oget` or in `sensing::read_sensor`, and is not required in order to `JOIN` —
    // the four junction slots are per-cell state independent of the loadout.
    //
    // Asserted through the arena rather than argued: wire a pair, and show that the junction is
    // real while every reading a genome could take of it is zero.
    let bytes = assemble("reflex.mm");
    let mut world = World::new(petri(3, 32)).expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    let a = place(&mut world, &bytes, 16, 16);
    let b = place(&mut world, &bytes, 16 + SPACING, 16);
    world.run(1_500);
    wire(&mut world, a, b, 0, 2);

    let ia = world.cells().index(a).expect("alive");
    let held = world.cells().junctions(ia).iter().filter(|j| j.is_some()).count();
    println!("\nPORT  junctions actually held: {held}");
    assert_eq!(held, 1, "the harness failed to wire the pair");

    // Slot 10 is the junction port. Build one and read every index it has.
    {
        let cells = world.cells_mut();
        cells.slots_mut(ia)[10] =
            mm_core::Organelle::finished(mm_core::OrganelleType::JunctionPort, 12);
    }
    println!("PORT  the genome is holding a junction and can see none of it.");
    println!("PORT  `OGET` on slot 10 falls through to `read_sensor`, which returns None for");
    println!("PORT  JunctionPort, so `oget` answers 0 for every index — see the assertion below.");

    // The behavioural consequence, which is the part that matters: `parasite.mm`'s `#infect`
    // branches on exactly this reading, so its `connected` state is unreachable.
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/parasite.mm"),
    )
    .expect("parasite source");
    assert!(
        src.contains("JMPNZ   connected"),
        "parasite.mm no longer branches on the port reading; re-check this finding"
    );
    println!("PORT  parasite.mm still branches on it, so its `connected` state cannot be reached.");
}

// ---------------------------------------------------------------------------------------------
// 4. Housekeeping: the genome has to be a legal organism even if it is not a competitive one.

#[test]
fn reflex_assembles_and_fits_its_own_nucleus() {
    // Not `#[ignore]`d. It is the cheap guard `m8_ecology` applies to the organisms it lists,
    // and `reflex.mm` is deliberately not in that list — it does not divide, so it cannot pass
    // the reproduction half. The nucleus half still applies and still catches the failure that
    // cost an afternoon on `predator.mm`: a genome one byte past its own nucleus is truncated
    // at division and the lineage quietly stops.
    let bytes = assemble("reflex.mm");
    // `#build` asks for a nucleus of param 48, and capacity is `param * 8` bytes.
    let capacity = 48 * 8;
    assert!(
        bytes.len() <= capacity,
        "reflex.mm is {} bytes against a nucleus of {capacity}",
        bytes.len()
    );
}


// ---------------------------------------------------------------------------------------------
// 5. The mechanism on its own, with no gene structure in the way.

#[test]
fn the_synapse_moves_matter_exactly() {
    // Not `#[ignore]`d and it costs nothing: it is the only thing in the tree that asserts
    // `JXFER` moves a chemical at all. It exists because the first three runs of the chain
    // experiment above showed nothing arriving, and "the wire adds nothing" and "the wire does
    // not work" produce identical tables when diffusion is doing all the delivery. Four
    // instructions, no promoters, no diffusion: whatever else is wrong, this says whether the
    // opcode is.
    // **`IMM 17`, and the number matters.** `JXFER`'s `what` reserves 0 for *energy*, so a
    // genome naming a chemical has to avoid landing on zero — and this was written as `IMM 16`
    // because 16 wrapped to signal_a in a table of sixteen while not being 0. It was correct and
    // it was correct *by way of the wrap*.
    //
    // ISA 11 added dinitrogen at index 16. The operand stopped wrapping, `IMM 16` began naming a
    // chemical the cell holds none of, and the test reported "nothing crossed" — which was true
    // and had nothing to do with the opcode it exists to check. The obvious repair, writing 0 for
    // signal_a, is worse: it transfers *energy*, silently, and the chemical column stays empty in
    // exactly the same way. 17 was the operand that reached signal_a after that.
    //
    // **ISA 12 moved it again**, to 19, when calcium and carbonate joined the table — and this
    // failed in exactly the same way, with exactly the same message, for the third time. Which is
    // the sharp edge of widening a chemical table, recorded here because it cost two wrong fixes
    // to find the first time: an out-of-range operand is not a mistake a genome makes, it is a
    // thing genomes rely on, and what it means is part of the ISA. The operand that reaches
    // signal_a is `CHEM_COUNT` itself, and it will move again the next time the table does.
    let bytes = mm_asm::assemble("IMM 4\nIMM 19\nZERO\nJXFER\nDROP\nHALT\n")
        .expect("assembles")
        .bytes;
    let mut world = World::new(petri_with_diffusion(3, 32, Some(0))).expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    let a = place(&mut world, &bytes, 16, 16);
    let b = place(&mut world, &bytes, 17, 16);
    wire(&mut world, a, b, 0, 2);
    let (ia, ib) = (
        world.cells().index(a).expect("a"),
        world.cells().index(b).expect("b"),
    );
    world.cells_mut().interior_mut(ia)[SIGNAL] = q10(200);
    world.adopt_current_contents_as_baseline();

    let before = world.cells().interior(ia)[SIGNAL] + world.cells().interior(ib)[SIGNAL];
    world.run(10);
    let (held_a, held_b) = (
        world.cells().interior(ia)[SIGNAL],
        world.cells().interior(ib)[SIGNAL],
    );
    assert!(held_b > 0, "nothing crossed the junction");
    assert_eq!(
        held_a + held_b,
        before,
        "a synapse created or destroyed matter"
    );
    // Four units a tick, as asked for, for ten ticks.
    assert_eq!(held_b, q10(40), "the weight is not what the genome asked for");
}

// ---------------------------------------------------------------------------------------------
// 6. Why the first three runs of this probe measured a corpse.

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn an_undivided_cell_dies_of_turgor() {
    // `reflex.mm` does not divide, so it cannot shed solute the way every other shipped genome
    // does. The first version of it kept the ancestor's diet and was dead by tick 1,800 — and
    // the chain experiment above was settling for 1,500 ticks, so it was measuring a body in
    // the last quarter of its life.
    //
    // The fix in the genome was to stop hoarding: eat a quarter as much carbon, and excrete the
    // oxidant, which photosynthesis *makes* and which the ancestor eats anyway. Nothing about
    // the nervous system changed.
    //
    // Kept because the ancestor is on the same curve — it is at thirteen capacities and climbing
    // at tick 2,000 — and because it is `docs/STIFFNESS.md` §3 arriving from the other side: the
    // quadratic turgor charge is the largest thing in a settled cell's budget and it buys
    // nothing at all.
    for name in ["reflex.mm", "ancestor.mm"] {
        let bytes = assemble(name);
        let mut world = World::new(petri(7, 48)).expect("world");
        world.set_biology(BiologyConfig {
            mutation: MutationRates::none(),
            ..BiologyConfig::default()
        });
        let id = place(&mut world, &bytes, 24, 24);
        println!("\nTURGOR {name}");
        println!("  tick   energy   mass   solute   capacities");
        for step in 0..=10 {
            if step > 0 {
                world.run(200);
            }
            let Some(i) = world.cells().index(id) else {
                println!("  {:>4}   DEAD", step * 200);
                break;
            };
            let c = world.cells();
            let solute: i64 = c.interior(i).iter().map(|v| *v as i64).sum();
            let cap = mm_core::biology::interior_capacity(c, i).max(1) as i64;
            println!(
                "  {:>4}   {:>6}   {:>4}   {:>6}   {:>10}",
                step * 200,
                c.energy[i] / mm_core::Q10_ONE,
                c.mass[i] / mm_core::Q10_ONE,
                solute / mm_core::Q10_ONE as i64,
                solute / cap,
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 7. One mutation from a gene that means something else.

#[test]
fn the_promoters_of_reflex_are_far_enough_apart() {
    // `find_promoter` takes the closest match and stops at distance zero, so an exact reference
    // always binds its own gene and a collision costs nothing *today*. It costs something the
    // first time a promoter mutates: at `promoter_bind_threshold` of 2, two promoters two bits
    // apart are one copy error from being interchangeable.
    //
    // `#wire` (239) and `#grow` (47) are exactly two apart. Recorded rather than renamed — the
    // pair is a real property of the hash and of the threshold, and a genome that survives its
    // own promoters drifting is the interesting case, not a bug to design around. What would be
    // a bug is not knowing.
    let names = ["build", "feed", "wire", "relay", "grow"];
    let pats: Vec<_> = names.iter().map(|n| mm_asm::promoter_pattern(n)).collect();
    let mut collisions = Vec::new();
    for i in 0..pats.len() {
        for j in (i + 1)..pats.len() {
            if pats[i].promoter_distance(pats[j]) <= 2 {
                collisions.push((names[i], names[j]));
            }
        }
    }
    assert_eq!(
        collisions,
        vec![("wire", "grow")],
        "the promoter collisions in reflex.mm have changed"
    );
}

// ---------------------------------------------------------------------------------------------
// 8. The transmitter sweep: where does a wire start to be worth having?

/// Cilium power that counts as "this cell has responded", `Q10`. Half thrust.
const RESPONDED: i32 = mm_core::Q10_ONE / 2;

/// Run one arm and report when the tail responded and how much reached it.
///
/// Returns `(ticks_to_respond, tail_signal_at_horizon, chain_total_at_horizon)`; the first is
/// `None` if the tail never reached [`RESPONDED`] inside the horizon.
fn arm(diffusion: i32, wired: bool, horizon: u64) -> (Option<u64>, i32, i32) {
    let out = if wired { 0 } else { 3 };
    let (mut world, ids, _at) = chain_with_diffusion(7, 600, Some(diffusion));
    wire(&mut world, ids[0], ids[1], out, 2);
    wire(&mut world, ids[1], ids[2], out, 2);
    stimulate(&mut world, ids[0], q10(4_000));

    let mut responded = None;
    for t in 1..=horizon {
        world.run(1);
        if responded.is_none() && read(&world, ids[2]).power >= RESPONDED {
            responded = Some(t);
        }
    }
    let tail = read(&world, ids[2]).signal;
    let total: i32 = ids.iter().map(|id| read(&world, *id).signal).sum();
    (responded, tail, total)
}

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn where_the_wire_starts_to_pay() {
    // The finding from `a_wired_chain_against_an_unwired_one` was that the wire buys nothing
    // against the shipped table and everything when diffusion is zero. Neither is a design: the
    // question is where between them the crossover sits, because that is the number a scenario
    // has to be authored around.
    //
    // `ChemTable::new` clamps diffusion at `MAX_DIFFUSION` = `Q10_ONE / 4` = 256, which is what
    // `signal_a` ships at — so the sweep starts at the fastest rate the engine allows and walks
    // down by halves.
    let horizon = 400;
    println!("\nSWEEP  tail cell responds at half thrust; horizon {horizon} ticks after stimulus");
    println!("  signal_a          wired            control          what the wire bought");
    println!("  diffusion    ticks  sig  chain   ticks  sig  chain");
    for rate in [256, 128, 64, 32, 16, 8, 4, 2, 1, 0] {
        let (wt, ws, wc) = arm(rate, true, horizon);
        let (ct, cs, cc) = arm(rate, false, horizon);
        let verdict = match (wt, ct) {
            (Some(w), Some(c)) if c > w => format!("{} ticks sooner", c - w),
            (Some(w), Some(c)) if w > c => format!("{} ticks LATER", w - c),
            (Some(_), Some(_)) => "nothing".to_string(),
            (Some(_), None) => "the response itself".to_string(),
            (None, Some(_)) => "worse than nothing".to_string(),
            (None, None) => "neither responded".to_string(),
        };
        let show = |t: Option<u64>| match t {
            Some(t) => format!("{t}"),
            None => "never".to_string(),
        };
        println!(
            "  {:>9}   {:>6} {:>4} {:>6}  {:>6} {:>4} {:>6}   {}",
            rate,
            show(wt),
            ws,
            wc,
            show(ct),
            cs,
            cc,
            verdict
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 9. Could a nerve net ever assemble without a harness?

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn whether_the_genome_can_wire_itself() {
    // Everything above hand-wires the chain, which is fine for measuring a mechanism and says
    // nothing about whether one could ever arise. `#wire` joins the nearest cell it is *touching*
    // — so cells have to be in contact, not merely near — and it cannot see the result, so it
    // cannot choose a slot or a direction.
    //
    // Placed touching, with no harness wiring at all.
    let bytes = assemble("reflex.mm");
    for spacing in [1i32, 2] {
        let mut world = World::new(petri_with_diffusion(7, 48, Some(0))).expect("world");
        world.set_biology(BiologyConfig {
            mutation: MutationRates::none(),
            ..BiologyConfig::default()
        });
        let ids: Vec<CellId> = (0..3)
            .map(|k| place(&mut world, &bytes, 24 + k * spacing, 24))
            .collect();
        world.run(900);
        stimulate(&mut world, ids[0], q10(4_000));
        world.run(200);

        let slots: Vec<String> = ids
            .iter()
            .map(|id| {
                let i = world.cells().index(*id).expect("alive");
                world
                    .cells()
                    .junctions(i)
                    .iter()
                    .map(|j| if j.is_some() { 'X' } else { '.' })
                    .collect()
            })
            .collect();
        let sig: Vec<i32> = ids.iter().map(|id| read(&world, *id).signal).collect();
        let pow: Vec<i32> = ids.iter().map(|id| read(&world, *id).power).collect();
        println!(
            "  spacing {spacing}:  junctions {}  signal {:?}  power {:?}",
            slots.join(" "),
            sig,
            pow
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 10. Why it cannot: a genome can feel much further than it can reach.

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn a_cell_can_feel_further_than_it_can_join() {
    // `junction::reach`'s own documentation says:
    //
    //   > Its own radius plus the target's, plus a margin — the same "touching" test the touch
    //   > sensor uses, so a genome that can feel a neighbour can join it.
    //
    // It is not the same test. `neighbours::feel` counts a contact at `rj + 3*ri`; `reach` is
    // `2*ri + 1`. For equal radii those are `4r` and `2r + 1`, which agree only at r = 0.5
    // squares and diverge linearly above it. Every cell in every run is above it.
    //
    // This is why `whether_the_genome_can_wire_itself` finds nothing: `#wire` is doing exactly
    // what the documentation says will work.
    println!("\nREACH  equal radii, in squares");
    println!("  radius   feel (4r)   join (2r+1)   tangent (2r)   can a felt neighbour be joined?");
    for tenths in [5i32, 8, 10, 15, 20, 30] {
        let r = (mm_core::Q10_ONE * tenths) / 10;
        let feel = 4 * r;
        let join = 2 * r + mm_core::Q10_ONE;
        let show = |v: i32| format!("{:.2}", v as f64 / mm_core::Q10_ONE as f64);
        println!(
            "  {:>6}   {:>9}   {:>11}   {:>12}   {}",
            show(r),
            show(feel),
            show(join),
            show(2 * r),
            if feel <= join { "always" } else { "only inside 2r+1" }
        );
    }

    // And the consequence, measured on real cells rather than arithmetic.
    let bytes = assemble("reflex.mm");
    let mut world = World::new(petri_with_diffusion(7, 48, Some(0))).expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    let ids: Vec<CellId> = (0..3).map(|k| place(&mut world, &bytes, 24 + k, 24)).collect();
    world.run(900);
    let idx: Vec<usize> = ids.iter().map(|i| world.cells().index(*i).unwrap()).collect();
    let sq = |v: i32| format!("{:.2}", v as f64 / POS_ONE as f64);
    println!("\n  three cells placed one square apart and left for 900 ticks:");
    for (n, i) in idx.iter().enumerate() {
        let t = world.neighbours().touch(world.cells(), *i);
        println!(
            "    cell {n}: radius {}  feels {} contacts  join reach {}  nearest neighbour at {}",
            sq(mm_core::fixed::q10_to_pos(mm_core::biology::radius(world.cells(), *i))),
            t.contacts,
            sq(junction::reach(world.cells(), *i)),
            sq(junction::distance(world.cells(), *i, t.nearest as usize)),
        );
    }
    println!(
        "  junctions formed by #wire in 900 ticks: {}",
        idx.iter()
            .map(|i| world.cells().junctions(*i).iter().filter(|j| j.is_some()).count())
            .sum::<usize>()
    );

    // The other half, and it is independent of the first: a *soft* junction breaks past
    // `soft_max_range`, which is an absolute 3 squares rather than a multiple of the rest length.
    // Two tangent cells are 2r apart, so a soft junction between adults of radius 1.5 sits
    // exactly on the break threshold before any drift at all.
    println!(
        "\n  soft_max_range is {} squares, absolute. Two tangent cells of radius {} are {} apart.",
        sq(mm_core::junction::JunctionConfig::default().soft_max_range),
        sq(mm_core::fixed::q10_to_pos(mm_core::biology::radius(world.cells(), idx[0]))),
        sq(mm_core::fixed::q10_to_pos(
            mm_core::biology::radius(world.cells(), idx[0]) * 2
        )),
    );
    println!("  So the channel SPEC §8.1 calls the synapse cannot span two grown cells at all.");
}
