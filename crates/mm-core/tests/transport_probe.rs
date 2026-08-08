//! Does slowing the water give the pack an inside?
//!
//! Run with
//! `cargo test -p mm-core --test transport_probe -- --ignored --nocapture --test-threads=1`.
//!
//! # The question
//!
//! SPEC §17.8 measured that a dense pack has no interior — a tracer through 306 cells is
//! bit-identical to one through empty water, the oxidant varies by 0.02% from rind to core, and
//! a buried cell is *heavier, richer and fuller* than one on the surface. It then named the cause
//! and stopped:
//!
//! > The cause is a **rate mismatch, not a missing mechanism**. Diffusion rates run from
//! > `Q10_ONE/64` to `Q10_ONE/4` per fluid step, and the fluid steps every tick... On the
//! > timescale a cell lives at, the fluid is a well-stirred bath. Any mechanism that wants
//! > stratification has to close that gap.
//!
//! There are two ways to close it: impede transport *where cells are* — the porosity term the
//! solver does not have — or slow it *everywhere*, which is `Scenario::fluid_interval` and costs
//! nothing. Ten of the eleven shipped scenarios set it to 1. **Nobody has tried the free one.**
//!
//! # Why it is two-dimensional
//!
//! Transport rate cannot be swept alone, because a gradient needs something worth having a
//! gradient *in*. `docs/CHEMISTRY.md` §6 measured the carbon seeding and found the population
//! flat across the top forty-fold and then a cliff: 985 cells at 400 units a square, 1032 at 10,
//! 347 at 4, 65 at 1. The shipped soup seeds 400 — two orders of magnitude above the knee — so in
//! every run this project has ever made, **the structural monomer has been effectively
//! infinite**, and an infinite resource cannot be locally depleted however slowly it moves.
//!
//! So the sweep is `fluid_interval` × carbon, spanning that knee: 400 units a square (what
//! ships), 40 (just above), and 4 (just below).
//!
//! # What is measured
//!
//! For each run, the pack's centroid, the radius containing 90% of its cells, and then the mean
//! of each chemical over the **inner half** of that radius against the **outer half**, as a
//! percentage. 100% is no gradient at all, which is what §17.8 found. Plus the same split read
//! through the cells — mean energy inside against outside — because §17.8's finding was not only
//! that the medium is flat but that being buried *pays*, and the sign of that is the thing that
//! has to change.
//!
//! Mutation is **off**, deliberately. The question is whether the physics produces a gradient,
//! not which mutant happened to appear in which cell of the sweep.
//!
//! # What it found
//!
//! **1. At the shipped carbon level nothing happens, at any transport rate.** Contrast stays
//! within 95–101% from `fluid_interval` 1 to 32 and the population does not move off 286. An
//! effectively infinite resource cannot be locally depleted however slowly it travels, so the
//! two orders of magnitude `CHEMISTRY.md` §6 found between `soup.ron`'s seeding and the knee are
//! not a spare margin — they are why every measurement of this world has been flat.
//!
//! **2. At carbon 40 there is a regime change at `fluid_interval` 8.** Below it the contrast is
//! noise wider than any signal — 124 [67–184], 209 [134–260], 158 [94–201]. At and above it the
//! core holds consistently **half to two-thirds** the carbon of the rind: 57 [37–68], 57 [44–73],
//! 68 [51–80], in every seed. **And the population does not care**: 297–307 against 312 at
//! interval 1. That is the first real gradient anyone has measured here — §17.8 got 0.3%
//! variation in the structural monomer — and it costs nothing.
//!
//! **3. Being buried still pays.** 102–134% in almost every cell of the sweep. This is §17.8's
//! actual complaint and slowing the water does not touch it.
//!
//! # Which is the finding, because the reason is not what the plan assumed
//!
//! [`which_scarcity_makes_the_middle_a_bad_place`] went looking for a resource whose depletion
//! would make the core hostile, on the theory that carbon gates growth rather than energy. It
//! found something better and worse.
//!
//! **Seeding the oxidant scarce does nothing whatever** — the rows at oxidant 400 and oxidant 40
//! are identical to the digit. Photosynthesis *produces* the oxidant, so no cell that photosynthesises
//! can be starved of it by the scenario.
//!
//! **Seeding the waste scarce makes being buried much better, not worse**: `buried` goes from
//! 118% to **169% [152–203]**. Which is the whole answer:
//!
//! > A pack is not a consumer of a shared pool. It is a **producer of the thing it consumes** —
//! > respiration exhales the waste that photosynthesis eats — so the interior of a crowd is a
//! > microenvironment the crowd makes for itself. The scarcer the waste in the water, the more
//! > valuable it is to be surrounded by cells exhaling it. **The pack is its own atmosphere.**
//!
//! Slowing transport does not make the core hostile; it makes the recycling more local, which
//! *helps* the interior. Only at waste 40 and interval 32 does `buried` fall to 98% [68–139] —
//! straddling 100, on a population that has halved to 133. That is attrition, not structure.
//!
//! **So the porosity term §17.8 proposes would not work either**, and for the same reason: it
//! impedes transport, and impeded transport concentrates the pack's own exhaust where the pack
//! is. This measurement is the argument against building it.
//!
//! # What would work
//!
//! The core can only be a bad place to be if something the cells **cannot manufacture** and which
//! **gates energy** runs out there. In this world there is exactly one such thing.
//!
//! Energy enters only as light (SPEC §7.3). A cell cannot make light, cannot store it, and cannot
//! get it from a neighbour. And light is currently non-rival: the plane is prescribed from a
//! closed form every fluid step, a chloroplast reads the value at its centre square and does not
//! decrement it, so **nothing shades anything but a barrier**.
//!
//! Make light rival and the pack acquires a lit rind and a dark core, with no porosity term, no
//! transport change and nothing in the fluid solver at all. That is `docs/FEEDING.md` §8 item 8,
//! which was ranked eighth on cost-effectiveness and is on this evidence the foundation.

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, pos_to_square, q10, POS_ONE, Q10_ONE};
use mm_core::{LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding, World};

thread_local! {
    /// Set by the rigidity sweep, so the world builders above do not all grow a parameter that
    /// only one test varies.
    static RIGIDITY: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// Carbon, carbon dioxide, oxygen, peroxide. The four the default loop actually moves.
const WATCHED: [(usize, &str); 4] = [(4, "carbon"), (11, "CO2"), (14, "oxygen"), (13, "perox")];

fn ancestor() -> Vec<u8> {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm"),
    )
    .expect("ancestor.mm");
    mm_asm::assemble(&src).expect("assembles").bytes
}

/// A slide grown from one founder, with transport speed and carbon as parameters.
///
/// Everything else is `gradient_probe::growth_world`'s recipe, so the numbers are comparable with
/// the measurements §17.8 is built on.
fn growth_world(size: u32, fluid_interval: u32, carbon_units: i32, seed: u64) -> World {
    growth_world_seeded(size, fluid_interval, carbon_units, 400, 400, seed)
}

/// The same, with every seeded species under control rather than only the monomer.
fn growth_world_seeded(
    size: u32,
    fluid_interval: u32,
    carbon_units: i32,
    waste_units: i32,
    oxidant_units: i32,
    seed: u64,
) -> World {
    growth_world_shaded(size, fluid_interval, carbon_units, waste_units, oxidant_units, seed, 0)
}

/// The same again, with `MetabolicRates::light_occlusion` under control.
#[allow(clippy::too_many_arguments)]
fn growth_world_shaded(
    size: u32,
    fluid_interval: u32,
    carbon_units: i32,
    waste_units: i32,
    oxidant_units: i32,
    seed: u64,
    occlusion: i32,
) -> World {
    let sc = Scenario {
        name: "transport".into(),
        seed,
        width: size,
        height: size,
        light: LightRegime::Uniform { intensity: Q10_ONE },
        fluid_interval,
        seeding: vec![
            Seeding::Uniform {
                chemical: 11,
                per_square: q10(waste_units),
            },
            Seeding::Uniform {
                chemical: 14,
                per_square: q10(oxidant_units),
            },
            Seeding::Uniform {
                chemical: 4,
                per_square: q10(carbon_units),
            },
        ],
        ..Scenario::default()
    };
    let mut world = World::new(sc).expect("world");
    let mut biology = BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    };
    biology.metabolism.rates.light_occlusion = occlusion;
    biology.metabolism.rates.rigidity_gain = RIGIDITY.with(|r| r.get());
    world.set_biology(biology);
    let genome = world.genomes().intern(ancestor()).expect("intern");
    let id = world.spawn_cell(CellSeed {
        x: pos(size as i32 / 2),
        y: pos(size as i32 / 2),
        mass: q10(30),
        energy: q10(400),
        membrane: 24,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    if let Some(i) = world.cells_mut().index(id) {
        let c = world.cells_mut();
        c.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
        c.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
    }
    world.adopt_current_contents_as_baseline();
    world
}

/// What one run of the sweep says.
struct Profile {
    population: usize,
    /// Inner-half mean over outer-half mean, per watched chemical, in percent. 100 is flat.
    contrast: [i32; 4],
    /// Mean energy of cells inside the half-radius over those outside, in percent.
    /// Above 100 means being buried still pays, which is what §17.8 found.
    buried_energy: i32,
    /// How many cells fell inside the half-radius.
    inner_cells: usize,
}

fn profile(world: &World) -> Option<Profile> {
    let cells = world.cells();
    if cells.len() < 16 {
        return None;
    }
    let (mut cx, mut cy) = (0f64, 0f64);
    for i in cells.iter() {
        cx += pos_to_square(cells.x[i]) as f64;
        cy += pos_to_square(cells.y[i]) as f64;
    }
    cx /= cells.len() as f64;
    cy /= cells.len() as f64;

    // The pack's edge: the radius holding 90% of its cells, so a single wanderer does not set it.
    let mut radii: Vec<f64> = cells
        .iter()
        .map(|i| {
            ((pos_to_square(cells.x[i]) as f64 - cx).powi(2)
                + (pos_to_square(cells.y[i]) as f64 - cy).powi(2))
            .sqrt()
        })
        .collect();
    radii.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    let edge = radii[(radii.len() * 9 / 10).min(radii.len() - 1)].max(1.0);
    let half = edge / 2.0;

    // The medium, split at the half-radius.
    let sub = world.substrate();
    let (w, h) = (sub.width() as i32, sub.height() as i32);
    let mut inner = [0f64; 4];
    let mut outer = [0f64; 4];
    let (mut n_in, mut n_out) = (0f64, 0f64);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt();
            if d > edge {
                continue;
            }
            let idx = sub.index(x, y);
            let (bucket, count): (&mut [f64; 4], &mut f64) = if d <= half {
                (&mut inner, &mut n_in)
            } else {
                (&mut outer, &mut n_out)
            };
            *count += 1.0;
            for (k, (c, _)) in WATCHED.iter().enumerate() {
                bucket[k] += sub.chem_plane(*c)[idx] as f64;
            }
        }
    }
    if n_in == 0.0 || n_out == 0.0 {
        return None;
    }
    let mut contrast = [0i32; 4];
    for k in 0..4 {
        let a = inner[k] / n_in;
        let b = outer[k] / n_out;
        contrast[k] = if b > 0.0 { (a * 100.0 / b) as i32 } else { 0 };
    }

    // The same split read through the cells.
    let (mut e_in, mut e_out) = (0f64, 0f64);
    let (mut c_in, mut c_out) = (0usize, 0usize);
    for i in cells.iter() {
        let d = ((pos_to_square(cells.x[i]) as f64 - cx).powi(2)
            + (pos_to_square(cells.y[i]) as f64 - cy).powi(2))
        .sqrt();
        let e = cells.energy[i] as f64 / Q10_ONE as f64;
        if d <= half {
            e_in += e;
            c_in += 1;
        } else {
            e_out += e;
            c_out += 1;
        }
    }
    let buried_energy = if c_in > 0 && c_out > 0 && e_out > 0.0 {
        ((e_in / c_in as f64) * 100.0 / (e_out / c_out as f64)) as i32
    } else {
        0
    };

    Some(Profile {
        population: cells.len(),
        contrast,
        buried_energy,
        inner_cells: c_in,
    })
}

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn does_slowing_the_water_give_the_pack_an_inside() {
    let size = 32u32;
    let ticks = 4_000u64;
    // Five seeds, because `CHEMISTRY.md` §6 is explicit that a population on these slides
    // oscillates — it overshoots, starves back and settles — so one reading at one tick is a
    // phase sample and two of them can rank either way by luck. The first run of this sweep had
    // a 134% carbon contrast sitting between a 93% and a 94%, which is exactly that.
    let seeds = [1u64, 2, 3, 4, 5];
    println!(
        "\nTRANSPORT  {size}² slide, one founder, {ticks} ticks, mutation off, {} seeds.\n\
         Carbon contrast is the inner half-radius mean over the outer half, per cent — 100 is flat.\n\
         `buried` is mean energy inside over outside — above 100 means being buried still pays.\n\
         Every figure is the mean over seeds; the bracket is the spread across them.",
        seeds.len()
    );
    for carbon in [400i32, 40, 4] {
        println!(
            "\n  carbon {carbon} units/square   {}",
            match carbon {
                400 => "(what soup.ron ships — two orders above the knee)",
                40 => "(just above the knee CHEMISTRY.md §6 found)",
                _ => "(below the knee: a genuinely scarce world)",
            }
        );
        println!("  interval        pop        carbon contrast      perox contrast       buried");
        for interval in [1u32, 2, 4, 8, 16, 32] {
            let mut pops = Vec::new();
            let mut carb = Vec::new();
            let mut per = Vec::new();
            let mut bur = Vec::new();
            for seed in seeds {
                let mut world = growth_world(size, interval, carbon, seed);
                world.run(ticks);
                pops.push(world.cells().len() as f64);
                if let Some(p) = profile(&world) {
                    carb.push(p.contrast[0] as f64);
                    per.push(p.contrast[3] as f64);
                    bur.push(p.buried_energy as f64);
                }
            }
            let stat = |v: &[f64]| -> String {
                if v.is_empty() {
                    return "        —      ".to_string();
                }
                let mean = v.iter().sum::<f64>() / v.len() as f64;
                let lo = v.iter().cloned().fold(f64::INFINITY, f64::min);
                let hi = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                format!("{mean:>5.0} [{lo:>3.0}-{hi:>3.0}]")
            };
            println!(
                "  {interval:>8}   {}   {}      {}    {}",
                stat(&pops),
                stat(&carb),
                stat(&per),
                stat(&bur),
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The follow-up the first sweep demanded.

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn which_scarcity_makes_the_middle_a_bad_place() {
    // The first sweep produced a real gradient and **did not change who wins**: a buried cell was
    // still richer than one on the rind in almost every condition. That is §17.8's actual
    // complaint and slowing the water did not touch it.
    //
    // The reason is visible in which chemical the gradient was in. Carbon is *structural* — it is
    // what a body is built from, not what it burns. A core cell short of carbon grows slower and
    // does not starve, because its energy comes from light, which is non-rival and unshaded, and
    // from the carbon dioxide and oxidant loop, both of which were seeded at 400 units a square
    // and are effectively infinite.
    //
    // So the hypothesis: **a gradient only bites if it is in something that gates energy.** This
    // starves the metabolic loop instead of the building material.
    let size = 32u32;
    let ticks = 4_000u64;
    let seeds = [1u64, 2, 3, 4, 5];
    println!(
        "\nSCARCITY  {size}² slide, {ticks} ticks, mutation off, {} seeds. Carbon held at 40.\n\
         `buried` above 100 means the middle of the pack is still the best place to be.",
        seeds.len()
    );
    println!("  waste  oxidant  interval        pop         carbon        CO2       buried");
    for (waste, oxidant) in [(400i32, 400i32), (40, 400), (400, 40), (40, 40)] {
        for interval in [1u32, 8, 32] {
            let mut pops = Vec::new();
            let (mut carb, mut co2, mut bur) = (Vec::new(), Vec::new(), Vec::new());
            for seed in seeds {
                let mut world =
                    growth_world_seeded(size, interval, 40, waste, oxidant, seed);
                world.run(ticks);
                pops.push(world.cells().len() as f64);
                if let Some(p) = profile(&world) {
                    carb.push(p.contrast[0] as f64);
                    co2.push(p.contrast[1] as f64);
                    bur.push(p.buried_energy as f64);
                }
            }
            let stat = |v: &[f64]| -> String {
                if v.is_empty() {
                    return "      —       ".to_string();
                }
                let mean = v.iter().sum::<f64>() / v.len() as f64;
                let lo = v.iter().cloned().fold(f64::INFINITY, f64::min);
                let hi = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                format!("{mean:>4.0} [{lo:>3.0}-{hi:>3.0}]")
            };
            println!(
                "  {waste:>5}  {oxidant:>7}  {interval:>8}   {}  {}  {}  {}",
                stat(&pops),
                stat(&carb),
                stat(&co2),
                stat(&bur),
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The mechanism the first two sweeps pointed at.

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn how_dark_does_the_core_have_to_be() {
    // Light is the only thing in this world a cell cannot manufacture, cannot store and cannot
    // get from a neighbour, so it is the only resource whose depletion can make the middle of a
    // pack a bad place. `MetabolicRates::light_occlusion` makes it rival by the one route the
    // geometry allows — cells lying over one another — and this is what it costs and buys.
    //
    // Two conditions: the world as it ships, and the world `transport_probe` recommends
    // (carbon near the knee, transport slowed).
    let size = 32u32;
    let ticks = 4_000u64;
    let seeds = [1u64, 2, 3, 4, 5];
    println!(
        "\nOCCLUSION  {size}² slide, {ticks} ticks, mutation off, {} seeds.\n\
         `buried` is mean energy inside the half-radius over outside — above 100 means the middle\n\
         of the pack is still the best place to be, which is what §17.8 found and wants changed.",
        seeds.len()
    );
    for (carbon, interval, label) in [
        (400i32, 1u32, "as shipped: carbon 400, fluid_interval 1"),
        (40, 8, "recommended: carbon 40, fluid_interval 8"),
    ] {
        println!("\n  {label}");
        println!("  occlusion         pop         carbon        buried");
        for occ in [0i32, Q10_ONE / 8, Q10_ONE / 4, Q10_ONE / 2, Q10_ONE, Q10_ONE * 2] {
            let mut pops = Vec::new();
            let (mut carb, mut bur) = (Vec::new(), Vec::new());
            for seed in seeds {
                let mut world =
                    growth_world_shaded(size, interval, carbon, 400, 400, seed, occ);
                world.run(ticks);
                pops.push(world.cells().len() as f64);
                if let Some(p) = profile(&world) {
                    carb.push(p.contrast[0] as f64);
                    bur.push(p.buried_energy as f64);
                }
            }
            let stat = |v: &[f64]| -> String {
                if v.is_empty() {
                    return "      —       ".to_string();
                }
                let mean = v.iter().sum::<f64>() / v.len() as f64;
                let lo = v.iter().cloned().fold(f64::INFINITY, f64::min);
                let hi = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                format!("{mean:>4.0} [{lo:>3.0}-{hi:>3.0}]")
            };
            let name = if occ == 0 {
                "off".to_string()
            } else if occ >= Q10_ONE {
                format!("{}x", occ / Q10_ONE)
            } else {
                format!("1/{}", Q10_ONE / occ)
            };
            println!(
                "  {name:>9}   {}  {}  {}",
                stat(&pops),
                stat(&carb),
                stat(&bur),
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The hypothesis the occlusion sweep left behind.

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn does_stiffness_stop_a_pack_shrinking_its_way_out_of_the_dark() {
    // Occlusion removes the burial advantage and then stops: pushing it harder does not push
    // further, because a shaded cell grows less, shrinks, overlaps less and is shaded less. The
    // escape route is *size*.
    //
    // `MetabolicRates::rigidity_gain` should close it, and the mechanism is specific enough to be
    // wrong. `pressure` is normalised against the band between touching and the core, so a rigid
    // cell — whose band is narrow — reads near-maximum pressure as soon as it touches anything at
    // all. Stiffness turns `pressure` from a measure of depth into a measure of *count*, and a
    // count of neighbours does not fall when a cell shrinks. Turgor does not fall either:
    // `osmotic_load` is what a cell holds and interior capacity does not scale with mass.
    //
    // If that is right, `buried` should go *below* 100 as gain rises, and it should do so at
    // occlusion settings where occlusion alone had stalled.
    let size = 32u32;
    let ticks = 4_000u64;
    let seeds = [1u64, 2, 3, 4, 5];
    println!(
        "\nRIGIDITY  {size}² slide, {ticks} ticks, mutation off, {} seeds, thicket conditions\n\
         (carbon 40, fluid_interval 8). `buried` below 100 is the middle of the pack finally\n\
         being a worse place to be than the rind.",
        seeds.len()
    );
    for occ in [0i32, Q10_ONE / 8, Q10_ONE / 2] {
        let occ_name = if occ == 0 {
            "off".to_string()
        } else {
            format!("1/{}", Q10_ONE / occ)
        };
        println!("\n  occlusion {occ_name}");
        println!("  gain           pop         carbon        buried");
        for gain in [0i32, Q10_ONE / 4, Q10_ONE, Q10_ONE * 4, Q10_ONE * 16] {
            RIGIDITY.with(|r| r.set(gain));
            let mut pops = Vec::new();
            let (mut carb, mut bur) = (Vec::new(), Vec::new());
            for seed in seeds {
                let mut world = growth_world_shaded(size, 8, 40, 400, 400, seed, occ);
                world.run(ticks);
                pops.push(world.cells().len() as f64);
                if let Some(p) = profile(&world) {
                    carb.push(p.contrast[0] as f64);
                    bur.push(p.buried_energy as f64);
                }
            }
            let stat = |v: &[f64]| -> String {
                if v.is_empty() {
                    return "      —       ".to_string();
                }
                let mean = v.iter().sum::<f64>() / v.len() as f64;
                let lo = v.iter().cloned().fold(f64::INFINITY, f64::min);
                let hi = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                format!("{mean:>4.0} [{lo:>3.0}-{hi:>3.0}]")
            };
            let name = if gain == 0 {
                "off".to_string()
            } else if gain >= Q10_ONE {
                format!("{}x", gain / Q10_ONE)
            } else {
                format!("1/{}", Q10_ONE / gain)
            };
            println!("  {name:>4}   {}  {}  {}", stat(&pops), stat(&carb), stat(&bur));
        }
    }
    RIGIDITY.with(|r| r.set(0));
}

// ---------------------------------------------------------------------------------------------
// Is the rest distance itself moving?

#[test]
#[ignore = "probe; --release --ignored --nocapture"]
fn does_a_settled_pack_hold_still() {
    // Reported from the microscope: cells in the thicket jitter, visible only between consecutive
    // frames. `core_permille` is derived from `osmotic_load`, which changes every tick as a cell
    // eats and excretes — so the distance at which two cells stop compressing is now a moving
    // target, and that is a mechanism for exactly this. It is also brand new and only switched on
    // in one scenario, which makes it the first suspect.
    //
    // Measured against the same slide with rigidity off, so the question is answered rather than
    // argued.
    let size = 32u32;
    let settle = 4_000u64;
    for (label, gain) in [("rigidity off", 0i32), ("rigidity 16x", Q10_ONE * 16)] {
        RIGIDITY.with(|r| r.set(gain));
        let mut world = growth_world_shaded(size, 8, 40, 400, 400, 1, Q10_ONE / 8);
        world.run(settle);

        // Per cell, over twenty consecutive ticks: how far it moves, and how much its own core
        // moves under it.
        let rates = world.biology().metabolism.rates;
        let ids: Vec<mm_core::cell::CellId> =
            world.cells().iter().map(|i| world.cells().id_at(i)).collect();
        let sample = |w: &World| -> Vec<(i64, i64, i32)> {
            ids.iter()
                .filter_map(|id| w.cells().index(*id))
                .map(|i| {
                    (
                        w.cells().x[i] as i64,
                        w.cells().y[i] as i64,
                        mm_core::neighbours::core_permille(w.cells(), i, &rates),
                    )
                })
                .collect()
        };
        let mut prev = sample(&world);
        let (mut moved, mut core_swing, mut n) = (0i64, 0i64, 0i64);
        let mut core_min = i32::MAX;
        let mut core_max = i32::MIN;
        for _ in 0..20 {
            world.run(1);
            let now = sample(&world);
            for (a, b) in prev.iter().zip(now.iter()) {
                let d = (((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)) as f64).sqrt();
                moved += (d * 1000.0 / POS_ONE as f64) as i64;
                core_swing += (a.2 - b.2).abs() as i64;
                core_min = core_min.min(b.2);
                core_max = core_max.max(b.2);
                n += 1;
            }
            prev = now;
        }
        println!(
            "  {label:<14} pop {:>4}   mean move/tick {:>5} thousandths of a square   \
             mean core change/tick {:>4}   core range {}..{}",
            world.cells().len(),
            moved / n.max(1),
            core_swing / n.max(1),
            core_min,
            core_max,
        );
    }
    RIGIDITY.with(|r| r.set(0));
}
