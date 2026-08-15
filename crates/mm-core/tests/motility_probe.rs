//! Why nothing swims: what a cilium buys, what a crowd charges for it, and what the
//! saturated slide is actually doing.
//!
//! `economy_probe` asked what each loadout earns and answered "the mitochondrion, and nothing
//! else". This asks the question the microscope keeps raising instead: **the slide is full of
//! cells that sit still and photosynthesise, and no lineage has ever been seen to move under
//! its own steam.** `ECONOMY.md` §4b says motility is cheap and useless — "under 7% of gross,
//! and there is nowhere better to be" — and that measurement was taken on one cell alone in
//! open water. Every cell anybody has ever *watched* is in a mat.
//!
//! Run with
//! `cargo test --release -p mm-core --test motility_probe -- --ignored --nocapture --test-threads=1`.
//!
//! `#[ignore]`, like every other probe in the tree. A probe answers a question once.

use std::path::Path;

use mm_core::fixed::{pos, q10, Q10_ONE, POS_ONE};
use mm_core::organelle::{Organelle, OrganelleType, SLOT_COUNT};
use mm_core::sensing::{cilium_thrust, THRUST_ENERGY};
use mm_core::{
    CellId, CellSeed, LightRegime, MutationRates, Scenario, World,
};

fn assemble(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    mm_asm::assemble(&src)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .bytes
}

/// A cell that keeps house and nothing else: it feeds, it vents its peroxide, and it never
/// builds or divides.
///
/// The vehicle for every "one body, measured" row below. `dormant.mm` would not do — it cannot
/// feed, so it runs its interior down, cannot photosynthesise, cannot pay for its own cilia and
/// coasts to a halt, which measures starvation and calls it drag. `ancestor.mm` would not do
/// either: its founder fills the dish with daughters and then cannot move for them.
const HOUSEKEEPER: &str = "
        EXPRESS #feed
        HALT

        GENE    #feed
        IMM     40
        IMM     11
        EAT
        DROP
        IMM     20
        IMM     14
        EAT
        DROP
        IMM     16
        IMM     4
        EAT
        DROP
        IMM     255
        IMM     13
        EMIT
        DROP
        IMM     8
        IMM     8
        EMIT
        DROP
        RET
";

fn housekeeper() -> Vec<u8> {
    mm_asm::assemble(HOUSEKEEPER).expect("housekeeper").bytes
}

/// A lit, still, well-fed dish, with mutation off — `economy_probe`'s, so the two agree.
fn dish(w: u32, h: u32) -> Scenario {
    Scenario {
        name: "motility".to_string(),
        seed: 0x_C1_11_A,
        width: w,
        height: h,
        light: LightRegime::Uniform {
            intensity: Q10_ONE,
        },
        current: mm_core::CurrentField::Still,
        jitter: 0,
        seeding: vec![
            mm_core::Seeding::Uniform {
                chemical: 11,
                per_square: q10(400),
            },
            mm_core::Seeding::Uniform {
                chemical: 14,
                per_square: q10(400),
            },
            mm_core::Seeding::Uniform {
                chemical: 4,
                per_square: q10(400),
            },
        ],
        biology: mm_core::BiologyConfig {
            mutation: MutationRates::none(),
            ..Default::default()
        },
        ..Scenario::default()
    }
}

/// A cell that exists to be pushed: the standard metabolic kit plus `cilia` cilia.
///
/// Built by hand rather than grown from a genome, because the question is what the *body* can
/// do and a genome would spend its instruction budget deciding to do it. See [`HOUSEKEEPER`] for
/// why it runs that rather than anything in `genomes/`.
fn swimmer(world: &mut World, x: i32, y: i32, cilia: usize, param: u8, power: i16) -> CellId {
    let genome = world
        .genomes()
        .intern(housekeeper())
        .expect("intern");
    let id = world.spawn_cell(CellSeed {
        x: pos(x) + POS_ONE / 2,
        y: pos(y) + POS_ONE / 2,
        mass: q10(30),
        energy: q10(4000),
        membrane: 24,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    let Some(i) = world.cells_mut().index(id) else {
        return id;
    };
    for slot in 1..SLOT_COUNT {
        world.cells_mut().slots_mut(i)[slot] = Organelle::empty();
    }
    world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
    world.cells_mut().slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
    world.cells_mut().slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
    for k in 0..cilia {
        let mut o = Organelle::finished(OrganelleType::Cilium, param);
        o.control[0] = power;
        // All mounted the same way, which is what a genome writing a constant gets and what
        // `Organelle::finished` gives for free: control[1] defaults to 0, and 0 is due east.
        o.control[1] = 0;
        world.cells_mut().slots_mut(i)[4 + k] = o;
    }
    world.cells_mut().interior_mut(i)[4] = q10(200);
    world.cells_mut().interior_mut(i)[11] = q10(40);
    world.cells_mut().interior_mut(i)[14] = q10(40);
    id
}

/// Where a cell is, in `Q10` of a square, so a sub-square step is visible.
fn at(world: &World, id: CellId) -> Option<(i64, i64)> {
    let i = world.cells().index(id)?;
    Some((
        (world.cells().x[i] as i64 * Q10_ONE as i64) / POS_ONE as i64,
        (world.cells().y[i] as i64 * Q10_ONE as i64) / POS_ONE as i64,
    ))
}

/// What one body's cilia cost per tick at the power they are set to, `Q10`.
fn thrust_bill(world: &World, id: CellId) -> i32 {
    let Some(i) = world.cells().index(id) else {
        return 0;
    };
    let mut bill = 0;
    for o in world.cells().slots(i) {
        let t = cilium_thrust(o);
        bill += mm_core::fixed::q10_scale(t.abs(), THRUST_ENERGY);
    }
    bill
}

// ---------------------------------------------------------------------------------------------

/// What a cilium moves in open water, and what it charges for it.
///
/// The control for everything below. `ECONOMY.md` §4b's "motility is nearly free" is this
/// table's cost column, and it is right.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_a_cilium_moves_in_open_water() {
    eprintln!("\none sterile body, alone on an empty 300x16 slide, still water, 60 ticks.");
    eprintln!("long enough for drag to reach a terminal speed, short enough not to hit the wall.");
    eprintln!("power 1024 is what `Organelle::finished` gives a new organelle for free;");
    eprintln!("255 is the largest value `IMM` can push, and what every shipped genome writes.\n");
    eprintln!(
        "{:>6} {:>6} {:>6} {:>9} {:>9} {:>9} {:>8}",
        "cilia", "param", "power", "sq/tick", "squares", "Q10/tick", "% gross"
    );
    for (cilia, param, power) in [
        (1usize, 80u8, 1024i16),
        (1, 80, 255),
        (2, 80, 1024),
        (2, 80, 255),
        (4, 80, 1024),
        (2, 255, 1024),
    ] {
        let mut world = World::new(dish(300, 16)).expect("world");
        let id = swimmer(&mut world, 4, 8, cilia, param, power);
        let bill = thrust_bill(&world, id);
        let start = at(&world, id).expect("placed");
        world.run(60);
        let Some(end) = at(&world, id) else {
            eprintln!("{cilia:>6} {param:>6} {power:>6}   died");
            continue;
        };
        let i = world.cells().index(id).expect("alive");
        let speed = world.cells().vx[i].abs() + world.cells().vy[i].abs();
        let moved = (end.0 - start.0).abs() + (end.1 - start.1).abs();
        eprintln!(
            "{:>6} {:>6} {:>6} {:>9} {:>9} {:>9} {:>7}%",
            cilia,
            param,
            power,
            format!("{:.3}", speed as f64 / Q10_ONE as f64),
            format!("{:.1}", moved as f64 / Q10_ONE as f64),
            bill,
            bill * 100 / 2400,
        );
    }
    eprintln!("\n`gross` is 2400 Q10/tick, the income of a standard body (ECONOMY.md §1).");
}

/// Whether a swimmer is fighting its own wake, and how much of it.
///
/// `step_physics` puts the equal and opposite of a cilium's thrust into the square the cell is
/// standing on, which is right — a cilium cannot push on nothing. But the injection is clamped
/// to `±Q10_ONE` **per square**, so the backwash a weak swimmer creates is a large share of its
/// own thrust and the backwash a strong one creates is capped. That makes speed superlinear in
/// thrust before a single neighbour is involved.
///
/// The control is the same body with the fluid stepped so rarely it never runs: same thrust,
/// same drag, no wake.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_a_swimmer_does_to_its_own_water() {
    eprintln!("\none sterile body, alone, 60 ticks, still water — with the fluid solver running");
    eprintln!("and with it switched off, so the difference is the cell's own wake.\n");
    eprintln!(
        "{:>6} {:>6} {:>6} {:>8} | {:>9} {:>9} {:>8} {:>9}",
        "cilia", "param", "power", "thrust", "with wake", "no wake", "lost", "wake Q10"
    );
    for (cilia, param, power) in [
        (1usize, 20u8, 1024i16),
        (1, 40, 1024),
        (1, 80, 1024),
        (2, 80, 255),
        (2, 80, 1024),
        (3, 80, 1024),
        (4, 80, 1024),
        (2, 255, 1024),
    ] {
        let mut with = World::new(dish(300, 16)).expect("world");
        let a = swimmer(&mut with, 4, 8, cilia, param, power);
        let astart = at(&with, a).unwrap_or((0, 0));
        with.run(60);
        let amoved = at(&with, a).unwrap_or(astart).0 - astart.0;

        let mut cfg = dish(300, 16);
        cfg.fluid_interval = 1_000_000;
        let mut without = World::new(cfg).expect("world");
        let b = swimmer(&mut without, 4, 8, cilia, param, power);
        let bstart = at(&without, b).unwrap_or((0, 0));
        without.run(60);
        let bmoved = at(&without, b).unwrap_or(bstart).0 - bstart.0;

        // What the water under the cell ended up doing, which is the wake itself.
        let wake = with
            .cells()
            .index(a)
            .map(|i| {
                let sq = with.substrate().index(
                    mm_core::fixed::pos_to_square(with.cells().x[i]),
                    mm_core::fixed::pos_to_square(with.cells().y[i]),
                );
                with.substrate().velocity().0.get(sq).copied().unwrap_or(0)
            })
            .unwrap_or(0);

        eprintln!(
            "{:>6} {:>6} {:>6} {:>8} | {:>9} {:>9} {:>7}% {:>9}",
            cilia,
            param,
            power,
            mm_core::fixed::q10_scale(4 * param as i32, power as i32) * cilia as i32,
            format!("{:.1}", amoved as f64 / Q10_ONE as f64),
            format!("{:.1}", bmoved as f64 / Q10_ONE as f64),
            if bmoved > 0 { 100 - amoved * 100 / bmoved } else { 0 },
            wake,
        );
    }
    eprintln!("\n`thrust` is Q10 of a square per tick the cilia produce; the impulse the water");
    eprintln!("receives is that, clamped to +/-1024 per square. `lost` is the share of its own");
    eprintln!("distance the cell gives back to the wake it made.");
}

/// The shipped swimmers, measured as they are actually written.
///
/// The hand-built bodies above are a proxy. This is `genomes/`, assembled, seeded and run, so
/// that nothing here depends on my having reconstructed a loadout correctly.
#[test]
#[ignore = "a probe; run it on purpose"]
fn where_the_shipped_swimmers_actually_get_to() {
    eprintln!("\none founder of each, alone on an empty 300x16 slide, still water, 600 ticks.");
    eprintln!("`dx` is east-west displacement — every cilium in the library is mounted at");
    eprintln!("control[1] = 0, which is due east, because no genome writes control[1] at all.\n");
    eprintln!(
        "{:>18} {:>8} {:>10} {:>10} {:>9} {:>9}",
        "genome", "cilia", "thrust", "dx (wake)", "dx (none)", "cells"
    );
    for name in ["drifter.mm", "drifter_blind.mm", "stalker.mm", "reflex.mm", "ancestor.mm"] {
        let mut row = Vec::new();
        let mut cilia = 0usize;
        let mut thrust = 0i32;
        let mut cells = 0usize;
        for interval in [1u32, 1_000_000] {
            let mut cfg = dish(300, 16);
            cfg.fluid_interval = interval;
            let mut world = World::new(cfg).expect("world");
            world.place_founders_at(&assemble(name), 1, Some((8, 8)));
            let Some(i) = world.cells().iter().next() else { continue };
            let id = world.cells().id_at(i);
            let start = at(&world, id).unwrap_or((0, 0));
            world.run(600);
            if interval == 1 {
                if let Some(i) = world.cells().index(id) {
                    cilia = world
                        .cells()
                        .slots(i)
                        .iter()
                        .filter(|o| o.is_active() && o.kind == OrganelleType::Cilium)
                        .count();
                    thrust = world
                        .cells()
                        .slots(i)
                        .iter()
                        .map(|o| cilium_thrust(o).abs())
                        .sum();
                }
                cells = world.cells().len();
            }
            row.push(at(&world, id).map(|e| e.0 - start.0).unwrap_or(0));
        }
        eprintln!(
            "{:>18} {:>8} {:>10} {:>10} {:>10} {:>8}",
            name,
            cilia,
            thrust,
            format!("{:.1}", row.first().copied().unwrap_or(0) as f64 / Q10_ONE as f64),
            format!("{:.1}", row.get(1).copied().unwrap_or(0) as f64 / Q10_ONE as f64),
            cells,
        );
    }
    eprintln!("\n`dx (none)` is the same run with the fluid solver switched off, so the");
    eprintln!("difference between the two columns is the genome's own backwash.");
}

/// What the drifter's cilia are doing on the one slide where they pay.
///
/// `ECONOMY.md` §2a has `drifter.mm` at 19 of 1000 in the still soup and **927 in the drift**,
/// and reads that as a swimmer finally being worth its keep. `the_drift.ron` runs a uniform
/// current east at 128 `Q10`, and the question this asks is whether the cilia are carrying the
/// cell somewhere or holding it where it is.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_the_drifters_cilia_are_for() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/the_drift.ron"),
    )
    .expect("scenario");
    let base = Scenario::from_ron(&src).expect("parse");
    eprintln!("\nthe_drift.ron, 16 founders of one body, 20,000 ticks. The current runs east");
    eprintln!("at 128 Q10 and the channel's downstream wall is at x=90.\n");
    eprintln!(
        "{:>10} {:>8} {:>8} {:>8} {:>8} {:>10}",
        "power", "thrust", "wake", "cells", "mean x", "at x>80"
    );
    // `drifter.mm` itself, with one immediate changed: the value its `#swim` gene writes to
    // both cilia. 1024 is the row where `#swim` is deleted instead, because `IMM` cannot push
    // more than 255 and `Organelle::finished` leaves a new cilium at 1024 anyway.
    let drifter = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/drifter.mm"),
    )
    .expect("drifter");
    for power in [0i16, 64, 128, 255, 1024] {
        let source = if power == 1024 {
            drifter.replace("        EXPRESS #swim\n", "")
        } else {
            drifter.replace(
                "        IMM     255\n        ZERO\n        IMM     6",
                &format!("        IMM     {power}\n        ZERO\n        IMM     6"),
            )
            .replace(
                "        IMM     255\n        ZERO\n        IMM     8",
                &format!("        IMM     {power}\n        ZERO\n        IMM     8"),
            )
        };
        let bytes = mm_asm::assemble(&source).expect("variant").bytes;
        let mut scenario = base.clone();
        scenario.inhabitants.clear();
        let mut world = World::new(scenario).expect("world");
        world.place_founders(&bytes, 16);
        let thrust = mm_core::fixed::q10_scale(4 * 80, power as i32) * 2;
        world.run(20_000);
        let n = world.cells().len();
        let (sum, downstream) = world.cells().iter().fold((0i64, 0u32), |(s, d), i| {
            let x = (world.cells().x[i] / POS_ONE) as i64;
            (s + x, d + u32::from(x > 80))
        });
        // What the water under the population ended up doing.
        let wake = world
            .cells()
            .iter()
            .next()
            .map(|i| {
                let sq = world.substrate().index(
                    mm_core::fixed::pos_to_square(world.cells().x[i]),
                    mm_core::fixed::pos_to_square(world.cells().y[i]),
                );
                world.substrate().velocity().0.get(sq).copied().unwrap_or(0)
            })
            .unwrap_or(0);
        eprintln!(
            "{:>10} {:>8} {:>8} {:>8} {:>8} {:>10}",
            power,
            thrust,
            wake,
            n,
            if n > 0 { sum / n as i64 } else { 0 },
            downstream,
        );
    }
    eprintln!("\nevery cilium here is mounted at control[1] = 0, which is due east — *with*");
    eprintln!("the current. If thrust were locomotion, more of it would put more cells at the");
    eprintln!("downstream wall and fewer of them alive.");
}

/// What an anchored ciliate reads as water going past it.
///
/// `slip` is what `ecology::captured` charges a filter on, and a cell reads it at **its own
/// square**. Before the wake moved astern, a beating cell's own backwash sat in that square, so
/// an anchored ciliate read its own stirring as a current and could in principle have filtered
/// on it — `FEEDING.md` §7 and `ECONOMY.md` §8 both flag that as never run. Moving the wake one
/// square back is exactly the thing that would take it away, so it wants measuring rather than
/// assuming.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_a_stirring_cell_reads_as_slip() {
    eprintln!("\nwater velocity around a beating cell, `Q10`, after 200 ticks.");
    eprintln!("a holdfast needs a barrier, so the anchored rows sit against a wall.\n");
    eprintln!(
        "{:>10} {:>7} {:>8} {:>9} {:>9} {:>9}",
        "body", "cilia", "thrust", "under", "astern", "moved"
    );
    for (label, anchored, cilia) in [
        ("free", false, 0usize),
        ("free", false, 1),
        ("free", false, 2),
        ("anchored", true, 0),
        ("anchored", true, 1),
        ("anchored", true, 2),
    ] {
        let mut cfg = dish(32, 32);
        // A wall along the bottom for a holdfast to grip.
        cfg.barriers = vec![mm_core::Barrier::Rect {
            x: 0,
            y: 30,
            width: 32,
            height: 2,
        }];
        let mut world = World::new(cfg).expect("world");
        let id = swimmer(&mut world, 16, 29, cilia, 80, 1024);
        if anchored {
            if let Some(i) = world.cells_mut().index(id) {
                // Gripping, said out loud. `HOUSEKEEPER` only eats — it never `OSET`s anything —
                // so nothing else was going to set this word, and a holdfast at rest grips with
                // exactly the force a genome asked it to, which is none. Every "anchored" row of
                // this table was a free cell with an inert organelle on it, printed beside the
                // free rows it was supposed to differ from.
                let mut hold = Organelle::finished(OrganelleType::Holdfast, 200);
                hold.control[0] = Q10_ONE as i16;
                world.cells_mut().slots_mut(i)[8] = hold;
            }
        }
        let start = at(&world, id).unwrap_or((0, 0));
        world.run(200);
        let Some(i) = world.cells().index(id) else {
            eprintln!("{label:>10} {cilia:>7}   died");
            continue;
        };
        let end = at(&world, id).unwrap_or(start);
        let sx = mm_core::fixed::pos_to_square(world.cells().x[i]);
        let sy = mm_core::fixed::pos_to_square(world.cells().y[i]);
        let water = |dx: i32| {
            let at = world.substrate().index((sx + dx).clamp(0, 31), sy);
            world.substrate().velocity().0.get(at).copied().unwrap_or(0)
        };
        eprintln!(
            "{:>10} {:>7} {:>8} {:>9} {:>9} {:>9}",
            label,
            cilia,
            world
                .cells()
                .slots(i)
                .iter()
                .map(|o| cilium_thrust(o).abs())
                .sum::<i32>(),
            water(0),
            water(-1),
            format!(
                "{:.1}",
                ((end.0 - start.0).abs() + (end.1 - start.1).abs()) as f64 / Q10_ONE as f64
            ),
        );
    }
    eprintln!("\n`slip` is read at the cell's own square — the `under` column. `captured`");
    eprintln!("multiplies concentration by it, so a zero there is a filter that catches nothing");
    eprintln!("however hard the cell is beating.");
}

/// The same swimmer, in the mat.
///
/// This is the measurement `ECONOMY.md` §4b never took. A cell on the microscope is never alone
/// in open water; it is one of four thousand in a pack that fills the slide.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_a_crowd_charges_a_swimmer() {
    const TICKS: u64 = 100;
    eprintln!("\nthe same bodies, dropped into one end of a saturated ancestor mat, against the");
    eprintln!("same bodies alone in open water. 100 ticks either way.");
    eprintln!("the mat is 300x24 and the swimmer starts at x=8, so nothing here reaches the");
    eprintln!("far wall and every ceiling in the table is the crowd rather than the box.\n");

    // Both halves of the contact model, because `firmness` gates both and `rigidity_gain`
    // is **zero by default** — so in every scenario but `the_marbles` and `the_thicket`,
    // `firmness` returns 0 for every cell whatever its membrane and turgor, a touching cell
    // keeps a quarter of its sliding speed, and REST_SPEED pins it outright below Q10/24.
    for gain in [0i32, Q10_ONE, Q10_ONE * 16] {
        let mut world_cfg = dish(300, 24);
        world_cfg.biology.metabolism.rates.rigidity_gain = gain;
        let mut mat = World::new(world_cfg).expect("world");
        mat.place_founders(&assemble("ancestor.mm"), 64);
        mat.run(8000);
        eprintln!(
            "rigidity_gain {gain}  —  mat of {} cells, {} contacts a tick{}",
            mat.cells().len(),
            mat.report().physics.separated,
            if gain == 0 { "   (the default)" } else { "" },
        );
        eprintln!(
            "{:>6} {:>6} {:>6} {:>9} | {:>9} {:>9} {:>9} {:>7} {:>7}",
            "cilia", "param", "power", "open sq", "mat sq", "mat path", "ticks v>0", "Q10/t", "of open"
        );
        for (cilia, param, power) in [
            (0usize, 0u8, 0i16),
            (1, 80, 1024),
            (2, 80, 1024),
            (3, 80, 1024),
            (4, 80, 1024),
            (2, 80, 255),
            (2, 255, 1024),
        ] {
            // Open water, same body, same duration — the control.
            let mut open_cfg = dish(300, 16);
            open_cfg.biology.metabolism.rates.rigidity_gain = gain;
            let mut open = World::new(open_cfg).expect("world");
            let oid = swimmer(&mut open, 4, 8, cilia, param, power);
            let ostart = at(&open, oid).unwrap_or((0, 0));
            open.run(TICKS);
            let oend = at(&open, oid).unwrap_or(ostart);
            let omoved = (oend.0 - ostart.0).abs() + (oend.1 - ostart.1).abs();

            let mut world = mat.clone();
            let id = swimmer(&mut world, 8, 12, cilia, param, power);
            let bill = thrust_bill(&world, id);
            let Some(start) = at(&world, id) else { continue };
            // Path length as well as displacement, sampled every tick, because the final
            // position says nothing about a cell that was stopped and restarted a hundred times.
            let (mut path, mut moving) = (0i64, 0u32);
            let mut was = start;
            for _ in 0..TICKS {
                world.step();
                let Some(i) = world.cells().index(id) else { break };
                let now = at(&world, id).unwrap_or(was);
                path += (now.0 - was.0).abs() + (now.1 - was.1).abs();
                was = now;
                if world.cells().vx[i] != 0 || world.cells().vy[i] != 0 {
                    moving += 1;
                }
            }
            let Some(end) = at(&world, id) else {
                eprintln!("{cilia:>6} {param:>6} {power:>6}   died");
                continue;
            };
            let moved = (end.0 - start.0).abs() + (end.1 - start.1).abs();
            eprintln!(
                "{:>6} {:>6} {:>6} {:>9} | {:>9} {:>9} {:>9} {:>7} {:>6}%",
                cilia,
                param,
                power,
                format!("{:.1}", omoved as f64 / Q10_ONE as f64),
                format!("{:.1}", moved as f64 / Q10_ONE as f64),
                format!("{:.1}", path as f64 / Q10_ONE as f64),
                moving,
                bill,
                if omoved > 0 { moved * 100 / omoved } else { 0 },
            );
        }
        eprintln!();
    }
    eprintln!("`mat path` is distance travelled and `mat sq` is distance got, so the gap");
    eprintln!("between them is shoving. `of open` is the last column that matters: how much of");
    eprintln!("its own open-water speed a body keeps where cells actually live.");
    eprintln!("`ticks v>0` is how many of the hundred the cell had any velocity at all.");
    eprintln!("The 0-cilia row is the passenger's baseline.");
}

/// What the slide in the microscope is doing, tick by tick.
///
/// `predator_introduction.ron`, the world every screenshot of a full slide comes from. The
/// question is not what the cells are but what the *population* is: whether a saturated mat is
/// a turnover equilibrium with vacancies to compete for, or a jam.
#[test]
#[ignore = "a probe; run it on purpose"]
fn what_the_saturated_slide_is_doing() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/predator_introduction.ron"),
    )
    .expect("scenario");
    let scenario = Scenario::from_ron(&src).expect("parse");
    let mut world = World::new(scenario).expect("world");
    world.place_founders(&assemble("ancestor.mm"), 16);

    eprintln!("\npredator_introduction.ron, 16 ancestor founders, mutation as shipped.\n");
    eprintln!(
        "{:>7} {:>7} {:>8} {:>8} {:>9} {:>9} {:>10} {:>9} {:>8}",
        "tick", "cells", "births", "deaths", "refused", "contacts", "moved/cell", "thrust", "cilia"
    );

    let mut births = 0u64;
    let mut deaths = 0u64;
    let mut refused = 0u64;
    let mut moved = 0i64;
    let mut thrust = 0i64;
    for t in 1..=10_000u64 {
        world.step();
        let r = world.report();
        births += r.biology.births as u64;
        deaths += r.biology.deaths as u64;
        refused += r.biology.failed_splits as u64;
        moved += r.physics.moved;
        thrust += r.physics.energy_spent;
        if t % 1000 == 0 {
            let n = world.cells().len().max(1) as i64;
            let with_cilium = world
                .cells()
                .iter()
                .filter(|&i| {
                    world.cells().slots(i)
                        .iter()
                        .any(|o| o.is_present() && o.kind == OrganelleType::Cilium)
                })
                .count();
            eprintln!(
                "{:>7} {:>7} {:>8} {:>8} {:>9} {:>9} {:>10} {:>9} {:>8}",
                t,
                world.cells().len(),
                births,
                deaths,
                refused,
                world.report().physics.separated,
                // POS units of travel per cell over the last thousand ticks, as squares.
                format!("{:.2}", moved as f64 / n as f64 / POS_ONE as f64),
                thrust,
                with_cilium,
            );
            births = 0;
            deaths = 0;
            refused = 0;
            moved = 0;
            thrust = 0;
        }
    }

    eprintln!("\n`moved` includes drift: this slide has a rotational current.");
    eprintln!("`thrust` is Q10 the population spent on cilia over the interval — zero means");
    eprintln!("not one cell in the world beat anything.\n");

    // What the population is made of, at the end.
    eprintln!("what the {} survivors carry:", world.cells().len());
    for kind in (0..16).map(|t| OrganelleType::from_operand(t)) {
        let n = world
            .cells()
            .iter()
            .filter(|&i| {
                world
                    .cells()
                    .slots(i)
                    .iter()
                    .any(|o| o.is_present() && o.kind == kind)
            })
            .count();
        if n > 0 {
            eprintln!("  {:>16}  {}", kind.name(), n);
        }
    }
}

/// What the first mutational step towards swimming actually costs.
///
/// A mutation that turns a `BUILD`'s type immediate into a cilium produces exactly this: one
/// cilium, at whatever `param` the neighbouring immediate happened to be, with `control` at
/// `Organelle::finished`'s default — **full power, mounted due east**, because that is what
/// the constructor sets and no genome in the library writes control[1] at all.
#[test]
#[ignore = "a probe; run it on purpose"]
fn the_first_step_towards_swimming() {
    eprintln!("\nan ancestor with one extra organelle bolted on, alone in a lit dish,");
    eprintln!("4000 ticks. `end` is where the founder's lineage got to.\n");
    eprintln!(
        "{:>26} {:>8} {:>10} {:>10} {:>10}",
        "body", "cells", "Q10/tick", "drift x", "drift y"
    );
    for (label, kind, param, power) in [
        ("ancestor", OrganelleType::Empty, 0u8, 0i16),
        ("+ cilium 80, default", OrganelleType::Cilium, 80, 1024),
        ("+ cilium 80, quarter", OrganelleType::Cilium, 80, 255),
        ("+ cilium 20, default", OrganelleType::Cilium, 20, 1024),
        ("+ chemosensor 60", OrganelleType::Chemosensor, 60, 1024),
    ] {
        let mut world = World::new(dish(64, 64)).expect("world");
        world.place_founders_at(&assemble("ancestor.mm"), 1, Some((32, 32)));
        let Some(i) = world.cells().iter().next() else {
            continue;
        };
        let id = world.cells().id_at(i);
        if kind != OrganelleType::Empty {
            let mut o = Organelle::finished(kind, param);
            o.control[0] = power;
            world.cells_mut().slots_mut(i)[5] = o;
        }
        let bill = thrust_bill(&world, id);
        let start = at(&world, id).unwrap_or((0, 0));
        world.run(4000);
        let end = at(&world, id).unwrap_or(start);
        eprintln!(
            "{:>26} {:>8} {:>10} {:>10} {:>10}",
            label,
            world.cells().len(),
            bill,
            (end.0 - start.0) / Q10_ONE as i64,
            (end.1 - start.1) / Q10_ONE as i64,
        );
    }
    eprintln!("\nthe cilium is rebuilt by `#build`? no: `#build` only writes slots 1, 2 and 3,");
    eprintln!("so slot 5 survives as bolted on and is inherited by every daughter.");
}
