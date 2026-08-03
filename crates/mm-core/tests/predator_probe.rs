//! Why `predator.mm` stopped reproducing.
//!
//! `m8_ecology::the_shipped_organisms_reproduce` requires every shipped genome to reach four
//! cells from one founder. `predator.mm` reaches one, in debug and in release, and has done
//! since before this probe was written. CLAUDE.md is explicit that a failure like this is a
//! finding rather than a bug to tune away, so this exists to say *which* number is starving it
//! rather than to make the test pass.
//!
//! Run with `cargo test -p mm-core --test predator_probe -- --nocapture --test-threads=1`.
//!
//! # What it found
//!
//! **`predator.mm` reproduces.** Every fifty-one ticks: `births: 1`, population 1 → 2 → 1. The
//! daughters die. "It is not reproducing" was the wrong description of the failure and sent the
//! first three hours of this in the wrong direction.
//!
//! **The energy guard in `#divide` does not guard.** The idiom is `JMPNZ enough / HALT / HALT /
//! enough:`, and `HALT` yields the rest of the *tick* — `vm.rs` advances the instruction pointer
//! past it before the halted check breaks the loop. So the two HALTs delay the division by two
//! ticks and then it happens anyway, at any energy at all. `ancestor.mm` carries the identical
//! code under a comment reading "guarded by an energy check … the guard is worth its
//! instructions". It is not a guard. The ancestor survives it by being rich enough that dividing
//! whenever is harmless.
//!
//! **The predator is a tenth as rich.** Net income per cell is +887 raw energy a tick against the
//! ancestor's +1,748, and it settles near 14,000 where the ancestor sits above 150,000. Upkeep is
//! 0.75 a tick against 0.41 — 83% more — on an *identical* mitochondrion (param 50, capacity
//! exactly 3,200 a tick in both) and a smaller chloroplast, 55 against 60.
//!
//! **A division costs more than it owns.** `division_energy` is q10(20) = 20,480, the copy is
//! Q10/64 a byte over 342 bytes = 5,472, and the daughter takes half of what is left: measured at
//! 34,000–45,000 a division against a steady state of 14,000. The daughter inherits about 7,000
//! and must build a nucleus at 12,288 before it can build anything else. It cannot, so it dies.
//!
//! **Neither half of the predation strategy earns anything measurable.** The spike: extension 0
//! and extension 1,024 produce identical traces. The lysosome: carrion placed directly under the
//! cell raises its internal sugar from 3–17 units to 64–75 and the energy trace is *bit-identical*
//! — because `burn = min(mitochondrion capacity, substrate, oxidant)` and the capacity is already
//! the binding term. **Food it cannot burn is not food.** That is the finding that matters for the
//! design: bolting a stomach onto a hunter cannot pay while conversion, not supply, is the limit.

mod common;

use std::path::Path;

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::{BiologyConfig, MutationRates, Organelle, OrganelleType, Scenario, World};

fn assemble(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path).expect("genome file");
    mm_asm::assemble(&src).expect("it assembles").bytes
}

fn scenario(name: &str) -> Scenario {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scenarios")
        .join(name);
    let text = std::fs::read_to_string(&path).expect("scenario file");
    ron::from_str(&text).expect("it parses")
}

/// The same seeding `m8_ecology` uses, so this probe and the failing test see one world.
fn seed(world: &mut World, genome: &[u8]) -> CellId {
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    let structural = world.biology().structural_chemical;
    let g = world.genomes().intern(genome.to_vec()).expect("intern");
    let id = world.spawn_cell(CellSeed {
        x: pos(32),
        y: pos(32),
        mass: q10(30),
        energy: q10(400),
        membrane: 24,
        key: 11,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome: g,
    });
    if let Some(i) = world.cells_mut().index(id) {
        let cells = world.cells_mut();
        cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
        cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
        cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
        cells.interior_mut(i)[structural] = q10(200);
        cells.interior_mut(i)[11] = q10(40);
        cells.interior_mut(i)[14] = q10(40);
    }
    world.adopt_current_contents_as_baseline();
    id
}

fn loadout(world: &World, i: usize) -> String {
    world
        .cells()
        .slots(i)
        .iter()
        .enumerate()
        .filter(|(_, o)| o.is_present())
        .map(|(s, o)| {
            format!(
                "{s}:{}{}",
                o.kind.name().chars().take(4).collect::<String>(),
                if o.is_active() { "" } else { "(building)" }
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Follow one founder and report what it is doing, for each genome in turn.
#[test]
fn what_the_shipped_genomes_do_from_one_founder() {
    for name in ["ancestor.mm", "scavenger.mm", "predator.mm", "hunter.mm"] {
        let bytes = assemble(name);
        let mut world = World::new(scenario("soup.ron")).expect("world");
        let _id = seed(&mut world, &bytes);

        eprintln!("\n=== {name} ({} bytes) ===", bytes.len());
        eprintln!(
            "{:>6} {:>4} {:>9} {:>7} {:>7} {:>6}  {}",
            "tick", "pop", "energy", "mass", "damage", "upkeep", "loadout"
        );
        for step in 0..=12 {
            let tick = step * 200u64;
            if tick > 0 {
                world.run(200);
            }
            let pop = world.cells().len();
            let Some(i) = world.cells().iter().next() else {
                eprintln!("{tick:>6} {pop:>4}   -- extinct --");
                break;
            };
            let cells = world.cells();
            let slots: &[Organelle; 16] = cells
                .slots(i)
                .try_into()
                .expect("a cell has SLOT_COUNT slots");
            eprintln!(
                "{tick:>6} {pop:>4} {:>9} {:>7} {:>7} {:>6}  {}",
                cells.energy[i] / Q10_ONE,
                cells.mass[i] / Q10_ONE,
                cells.damage[i] / Q10_ONE,
                world.biology().metabolism.catalogue.upkeep(slots) * 100 / Q10_ONE,
                loadout(&world, i),
            );
        }
    }
}

/// Where each organelle's upkeep goes, for the loadouts above.
#[test]
fn what_each_loadout_costs_to_carry() {
    let catalogue = mm_core::organelle::OrganelleCatalogue::balanced();
    // The organelles each genome's `#build` actually asks for, at the params it asks for.
    let loadouts: [(&str, &[(OrganelleType, u8)]); 4] = [
        (
            "ancestor",
            &[
                (OrganelleType::Membrane, 24),
                (OrganelleType::Nucleus, 40),
                (OrganelleType::Chloroplast, 60),
                (OrganelleType::Mitochondrion, 50),
            ],
        ),
        (
            "scavenger",
            &[
                (OrganelleType::Membrane, 24),
                (OrganelleType::Nucleus, 48),
                (OrganelleType::Chloroplast, 60),
                (OrganelleType::Mitochondrion, 50),
                (OrganelleType::Lysosome, 70),
            ],
        ),
        (
            "hunter",
            &[
                (OrganelleType::Membrane, 24),
                (OrganelleType::Nucleus, 48),
                (OrganelleType::Chloroplast, 60),
                (OrganelleType::Mitochondrion, 50),
                (OrganelleType::Spike, 80),
            ],
        ),
        (
            "predator",
            &[
                (OrganelleType::Membrane, 24),
                (OrganelleType::Nucleus, 56),
                (OrganelleType::Chloroplast, 55),
                (OrganelleType::Mitochondrion, 50),
                (OrganelleType::Spike, 80),
                (OrganelleType::Lysosome, 70),
            ],
        ),
    ];
    eprintln!("\nupkeep per tick, Q10 x100 — and where it goes:");
    for (name, parts) in loadouts {
        let mut total = 0i32;
        let mut detail = Vec::new();
        for (kind, param) in parts {
            let cost = catalogue.spec(*kind).upkeep_cost(*param);
            total += cost;
            detail.push(format!("{} {}", kind.name(), cost * 100 / Q10_ONE));
        }
        eprintln!(
            "  {name:<10} {:>4}   ({})",
            total * 100 / Q10_ONE,
            detail.join(", ")
        );
    }
}

/// The predator with something to eat, and what it costs it to hold the spike out.
///
/// The solo trace says it bleeds energy from tick zero. This asks whether prey fixes that, and
/// whether the *extension* upkeep — charged per tick per unit of spike, on top of the
/// organelle upkeep that `what_each_loadout_costs_to_carry` reports — is what it cannot afford.
#[test]
fn what_the_predator_earns_and_what_it_pays() {
    let predator = assemble("predator.mm");
    let prey = assemble("ancestor.mm");

    // Spike held at a range of extensions, by rewriting `control[0]` after the genome has
    // built its body. Zero is a predator carrying a spike it never deploys.
    for extension in [0i16, 256, 512, 1024] {
        let mut world = World::new(scenario("soup.ron")).expect("world");
        world.set_biology(BiologyConfig {
            mutation: MutationRates::none(),
            ..BiologyConfig::default()
        });
        let structural = world.biology().structural_chemical;
        let mut spawn = |world: &mut World, bytes: &[u8], x: i32, y: i32| {
            let g = world.genomes().intern(bytes.to_vec()).expect("intern");
            let id = world.spawn_cell(CellSeed {
                x: pos(x),
                y: pos(y),
                mass: q10(30),
                energy: q10(400),
                membrane: 24,
                key: 11,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome: g,
            });
            if let Some(i) = world.cells_mut().index(id) {
                let cells = world.cells_mut();
                cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
                cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
                cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
                cells.interior_mut(i)[structural] = q10(200);
                cells.interior_mut(i)[11] = q10(40);
                cells.interior_mut(i)[14] = q10(40);
            }
            id
        };
        for k in 0..16 {
            spawn(&mut world, &prey, 12 + (k % 4) * 13, 12 + (k / 4) * 13);
        }
        let hunter = spawn(&mut world, &predator, 32, 32);
        world.adopt_current_contents_as_baseline();

        eprintln!("\n--- spike extension {extension} ---");
        eprintln!(
            "{:>6} {:>6} {:>10} {:>9} {:>7} {:>8}",
            "tick", "total", "predators", "energy", "mass", "carrion"
        );
        for step in 0..=8 {
            if step > 0 {
                world.run(400);
                // Held there against the genome, which rewrites it every tick.
                for i in world.cells().iter().collect::<Vec<_>>() {
                    if world.cells().genome[i].len() == predator.len() {
                        world.cells_mut().slots_mut(i)[5].control[0] = extension;
                    }
                }
            }
            let cells = world.cells();
            let mine: Vec<usize> = cells
                .iter()
                .filter(|i| cells.genome[*i].len() == predator.len())
                .collect();
            let (energy, mass) = match mine.first() {
                Some(i) => (cells.energy[*i] / Q10_ONE, cells.mass[*i] / Q10_ONE),
                None => (0, 0),
            };
            let carrion: i64 = (0..world.substrate().width())
                .flat_map(|x| (0..world.substrate().height()).map(move |y| (x, y)))
                .map(|(x, y)| {
                    i64::from(world.substrate().chem_at(
                        mm_core::ecology::CARRION,
                        x as i32,
                        y as i32,
                    ))
                })
                .sum::<i64>()
                / i64::from(Q10_ONE);
            eprintln!(
                "{:>6} {:>6} {:>10} {:>9} {:>7} {:>8}",
                world.tick_count(),
                cells.len(),
                mine.len(),
                energy,
                mass,
                carrion
            );
        }
        let _ = hunter;
    }
}

/// Whether the predator is starving in the middle of plenty because the plenty is elsewhere.
///
/// `apply_deaths` deposits carrion on the square the prey died on. `ecology::step` lets a
/// lysosome digest the carrion in the square its own cell is standing on. A predator kills what
/// it is *next to*, so the meal lands one square away — and `predator.mm` has no cilia.
///
/// This holds a corpse's worth of carrion under the predator's own feet and asks whether that
/// is the whole difference.
#[test]
fn a_predator_standing_on_its_food() {
    let predator = assemble("predator.mm");
    for (label, feed) in [("carrion elsewhere", false), ("carrion underfoot", true)] {
        let mut world = World::new(scenario("soup.ron")).expect("world");
        world.set_biology(BiologyConfig {
            mutation: MutationRates::none(),
            ..BiologyConfig::default()
        });
        let structural = world.biology().structural_chemical;
        let g = world.genomes().intern(predator.clone()).expect("intern");
        let id = world.spawn_cell(CellSeed {
            x: pos(32),
            y: pos(32),
            mass: q10(30),
            energy: q10(400),
            membrane: 24,
            key: 11,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome: g,
        });
        if let Some(i) = world.cells_mut().index(id) {
            let cells = world.cells_mut();
            cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
            cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
            cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
            cells.interior_mut(i)[structural] = q10(200);
            cells.interior_mut(i)[11] = q10(40);
            cells.interior_mut(i)[14] = q10(40);
        }
        world.adopt_current_contents_as_baseline();

        eprintln!("\n--- {label} ---");
        eprintln!(
            "{:>6} {:>5} {:>9} {:>7} {:>7} {:>7} {:>7} {:>7}",
            "tick", "pop", "energy", "mass", "sugar", "oxid", "waste", "perox"
        );
        for step in 0..=8 {
            if step > 0 {
                for _ in 0..250 {
                    if feed {
                        // A steady supply where the cell actually is. Injected rather than
                        // conjured: it goes through the ledger, so I4 still holds.
                        for i in world.cells().iter().collect::<Vec<_>>() {
                            let (x, y) = (
                                mm_core::fixed::pos_to_square(world.cells().x[i]),
                                mm_core::fixed::pos_to_square(world.cells().y[i]),
                            );
                            let before = world.substrate().chem_at(mm_core::ecology::CARRION, x, y);
                            let added = world.substrate_mut().add_chem(
                                mm_core::ecology::CARRION,
                                x,
                                y,
                                q10(4),
                            );
                            world
                                .ledger_mut()
                                .record_injected(mm_core::ecology::CARRION, added);
                            let after = world.substrate().chem_at(mm_core::ecology::CARRION, x, y);
                            let _ = (before, added, after);
                        }
                    }
                    world.run(1);
                }
            }
            let cells = world.cells();
            let Some(i) = cells.iter().next() else {
                eprintln!("{:>6} {:>5}  -- extinct --", world.tick_count(), 0);
                break;
            };
            let (sx, sy) = (
                mm_core::fixed::pos_to_square(cells.x[i]),
                mm_core::fixed::pos_to_square(cells.y[i]),
            );
            let _ = (sx, sy);
            let inside = cells.interior(i);
            eprintln!(
                "{:>6} {:>5} {:>9} {:>7} {:>7} {:>7} {:>7} {:>7}",
                world.tick_count(),
                cells.len(),
                cells.energy[i] / Q10_ONE,
                cells.mass[i] / Q10_ONE,
                inside[8] / Q10_ONE,
                inside[14] / Q10_ONE,
                inside[11] / Q10_ONE,
                inside[13] / Q10_ONE,
            );
        }
    }
}

/// The energy books for one cell, tick by tick, for the ancestor and the predator side by side.
///
/// Everything above narrows it to "abundant food, no more energy". This is the ledger entry
/// that says where the energy is going instead of guessing from levels.
#[test]
fn where_the_energy_goes_tick_by_tick() {
    for name in ["ancestor.mm", "predator.mm"] {
        let bytes = assemble(name);
        let mut world = World::new(scenario("soup.ron")).expect("world");
        let _ = seed(&mut world, &bytes);
        // Past the build-out, so what is measured is steady-state running.
        world.run(300);

        eprintln!("\n=== {name}, one cell, per tick ===");
        if let Some(i) = world.cells().iter().next() {
            for (n, o) in world.cells().slots(i).iter().enumerate() {
                if o.is_present() {
                    eprintln!(
                        "   slot {n}: {:<14} param {:>3}  control {:?}  active {}",
                        o.kind.name(),
                        o.param,
                        o.control,
                        o.is_active()
                    );
                }
            }
        }
        eprintln!(
            "{:>5} {:>8} {:>7} {:>8} {:>8} {:>7} {:>7}",
            "tick", "energy", "d", "carbon", "mass", "pop", "births"
        );
        let mut last = world
            .cells()
            .iter()
            .next()
            .map_or(0, |i| world.cells().energy[i]);
        for _ in 0..2000 {
            world.run(1);
            let Some(i) = world.cells().iter().next() else {
                break;
            };
            let e = world.cells().energy[i];
            let structural = world.biology().structural_chemical;
            let carbon = world.cells().interior(i)[structural];
            let mass = world.cells().mass[i];
            let births = world.report().biology.births;
            let pop = world.cells().len();
            if (e - last).abs() > 2000 || births > 0 {
                eprintln!(
                    "{:>5} {:>8} {:>7} {:>8} {:>8} {:>7} {:>7}  <-- big move",
                    world.tick_count(),
                    e,
                    e - last,
                    carbon,
                    mass,
                    pop,
                    births
                );
            }
            last = e;
        }
    }
}
