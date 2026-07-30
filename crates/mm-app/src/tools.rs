//! Laboratory tools: tweezers, barriers and slide transplants (M6).
//!
//! > Tweezers: select, isolate, relocate, copy genome to editor, transplant to a fresh slide.
//! > Barrier drawing and erasing on the substrate grid.
//!
//! # These are the tools that *are* allowed to touch the world
//!
//! Everything else in `mm-app` is built so it cannot reach the simulation: the renderer gets a
//! [`crate::slide::Frame`], the inspector gets a copy, the debugger gets a sandbox. This module
//! is the exception, and it is deliberate — a pair of tweezers that could not pick anything up
//! would not be tweezers.
//!
//! So the rule here is different, and stricter for being explicit: **every operation is one the
//! user asked for, and every one conserves matter**. A tool that quietly created or destroyed
//! matter would break I4 just as thoroughly as a bug in the fluid solver, and it would be
//! harder to find because the ledger would blame the physics.
//!
//! Picking a cell up moves it; it does not copy it. Dropping it on a fresh slide moves it
//! there. Deleting one returns everything it held to the water it was standing in, exactly as
//! death does — the same code path, because inventing a second way for a cell to stop existing
//! is how the two drift apart.

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos_to_square, POS_ONE};
use mm_core::genome_file::GenomeFile;
use mm_core::World;

/// What a tool did, for the undo log and for telling the user.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ToolEvent {
    Picked(CellId),
    Dropped {
        cell: CellId,
        x: i32,
        y: i32,
    },
    Removed(CellId),
    Transplanted {
        from: CellId,
        to: CellId,
    },
    BarrierDrawn {
        x: u32,
        y: u32,
    },
    BarrierErased {
        x: u32,
        y: u32,
    },
    /// A cell's genome was replaced while it was running.
    Rewritten {
        cell: CellId,
        bytes: usize,
    },
    /// The operation could not be done, and why.
    Refused(String),
}

/// Move a cell to a square, keeping everything it holds.
///
/// Position is not conserved quantity — moving a cell creates and destroys nothing — so this
/// is the one tool that needs no ledger care at all. It clamps rather than wrapping, because
/// dragging a cell off the edge of the slide should stop at the edge rather than teleport it
/// to the opposite one.
pub fn relocate(world: &mut World, cell: CellId, x: i32, y: i32) -> ToolEvent {
    let (w, h) = (
        world.substrate().width() as i32,
        world.substrate().height() as i32,
    );
    let Some(i) = world.cells().index(cell) else {
        return ToolEvent::Refused("that cell is no longer alive".to_string());
    };
    let cx = (x.clamp(0, w - 1) * POS_ONE) + POS_ONE / 2;
    let cy = (y.clamp(0, h - 1) * POS_ONE) + POS_ONE / 2;
    let cells = world.cells_mut();
    cells.x[i] = cx;
    cells.y[i] = cy;
    // Velocity goes with it. A cell picked up mid-swim and put down elsewhere should not carry
    // on at the speed it was going, which would look like it had been flicked.
    cells.vx[i] = 0;
    cells.vy[i] = 0;
    ToolEvent::Dropped {
        cell,
        x: cx / POS_ONE,
        y: cy / POS_ONE,
    }
}

/// Take a cell's genome out as a shareable file.
///
/// Reading, so it cannot fail on the world's side. `None` only if the cell has died.
#[must_use]
pub fn copy_genome(world: &World, cell: CellId) -> Option<GenomeFile> {
    let i = world.cells().index(cell)?;
    let genome = &world.cells().genome[i];
    let species = world.archive().get(world.cells().species[i]);
    let name = species.map_or_else(
        || format!("cell-{}", cell.ordering_key()),
        |s| s.name.full(),
    );
    let mut file = GenomeFile::new(genome.bytes().to_vec(), name);
    if let Some(s) = species {
        file = file.with_note(format!(
            "picked from {} at tick {}",
            s.name.full(),
            world.tick_count()
        ));
    }
    Some(file)
}

/// Replace a living cell's genome and let it carry on running.
///
/// The laboratory's most invasive instrument: it rewrites what a cell *is* without stopping
/// the world. Everything else about the cell — its body, its chemistry, its position, its
/// energy — is left exactly as it was, so what you get is the same organism running different
/// code, which is the only way to ask what a change does to a cell that already exists.
///
/// # What happens to the machine
///
/// The instruction pointer is **kept**, reduced modulo the new length. A small edit to a
/// genome of the same length therefore carries on from the same place, which is what "let it
/// carry on running" has to mean to be worth anything. It may well resume in the middle of
/// what used to be an instruction — that is safe, because every byte is a legal instruction
/// (hard rule 3), and it is honest, because that is exactly what happens to a descendant
/// whose genome mutated under it.
///
/// A division in flight is **abandoned**. `pa`, `pb` and `ln` point into the old genome, and
/// letting the copy finish would build a daughter out of two different genomes spliced at
/// whatever offset the edit happened to land on. The daughter buffer is dropped and the
/// counter cleared, so the cell simply divides again later.
///
/// The cell keeps its species. It is the same individual with new instructions, not a new
/// lineage — and if the new genome has drifted far enough, the ordinary speciation check will
/// found a species for its descendants at the next division without being told to.
pub fn rewrite_genome(world: &mut World, cell: CellId, bytes: Vec<u8>) -> ToolEvent {
    if bytes.is_empty() {
        return ToolEvent::Refused("a cell cannot have an empty genome".to_string());
    }
    if world.cells().index(cell).is_none() {
        return ToolEvent::Refused("that cell is no longer alive".to_string());
    }
    let length = bytes.len();
    let Ok(genome) = world.genomes().intern(bytes) else {
        return ToolEvent::Refused("that genome is longer than this engine allows".to_string());
    };
    // Interned, so this points the one cell at a shared genome rather than editing anything
    // its clones are also using. Every other cell on the old genome keeps it.
    let Some(i) = world.cells_mut().index(cell) else {
        return ToolEvent::Refused("that cell is no longer alive".to_string());
    };
    let cells = world.cells_mut();
    cells.genome[i] = genome;
    cells.daughter[i] = None;
    let vm = &mut cells.vm[i];
    vm.ln = 0;
    vm.pa = 0;
    vm.pb = 0;
    vm.ip = (vm.ip as usize % length) as u16;

    ToolEvent::Rewritten {
        cell,
        bytes: length,
    }
}

/// Remove a cell, returning everything it held to the water.
///
/// Uses the simulation's own death path rather than a second one. A tool that deleted a cell
/// by clearing its slot would destroy the matter inside it, and I4 would start failing in a
/// way that pointed at the physics.
pub fn remove(world: &mut World, cell: CellId) -> ToolEvent {
    if world.cells().index(cell).is_none() {
        return ToolEvent::Refused("that cell is no longer alive".to_string());
    }
    world.kill_cell(cell);
    ToolEvent::Removed(cell)
}

/// Copy a cell onto a fresh slide, whole: genome, organelles, chemistry and all.
///
/// The transplant is a *spawn* on the destination, so the destination gains matter that was
/// not in it. That is scenario setup rather than simulation — the same act as seeding a world
/// — so the destination's baseline is re-adopted, exactly as `spawn_cell`'s callers do. Said
/// out loud because silently moving matter between two conserved systems is the kind of thing
/// that makes a conservation test fail three milestones later.
pub fn transplant(from: &World, cell: CellId, to: &mut World, x: i32, y: i32) -> ToolEvent {
    let Some(i) = from.cells().index(cell) else {
        return ToolEvent::Refused("that cell is no longer alive".to_string());
    };
    let bytes = from.cells().genome[i].bytes().to_vec();
    let Ok(genome) = to.genomes().intern(bytes) else {
        return ToolEvent::Refused("that genome is longer than this engine allows".to_string());
    };

    let (w, h) = (
        to.substrate().width() as i32,
        to.substrate().height() as i32,
    );
    let src = from.cells();
    let seed = CellSeed {
        x: (x.clamp(0, w - 1) * POS_ONE) + POS_ONE / 2,
        y: (y.clamp(0, h - 1) * POS_ONE) + POS_ONE / 2,
        mass: src.mass[i],
        energy: src.energy[i],
        membrane: src.slots(i)[0].param,
        key: src.key[i],
        species: 0,
        parent: CellId::NONE,
        birth_tick: to.tick_count(),
        genome,
    };
    let organelles: Vec<mm_core::Organelle> = src.slots(i).to_vec();
    let interior: Vec<i32> = src.interior(i).to_vec();

    let new_id = to.spawn_cell(seed);
    if let Some(j) = to.cells_mut().index(new_id) {
        let cells = to.cells_mut();
        cells.slots_mut(j).copy_from_slice(&organelles);
        cells.interior_mut(j).copy_from_slice(&interior);
    }
    to.adopt_current_contents_as_baseline();
    ToolEvent::Transplanted {
        from: cell,
        to: new_id,
    }
}

/// Draw or erase a barrier square.
///
/// A barrier displaces whatever chemistry was in the square. `Substrate::set_blocked` returns
/// what it pushed out so the caller can put it somewhere; here it goes to the neighbours, so
/// drawing a wall through a rich patch does not quietly delete the food that was in it.
pub fn set_barrier(world: &mut World, x: u32, y: u32, blocked: bool) -> ToolEvent {
    let (w, h) = (world.substrate().width(), world.substrate().height());
    if x >= w || y >= h {
        return ToolEvent::Refused("that square is off the slide".to_string());
    }
    world.set_barrier(x, y, blocked);
    if blocked {
        ToolEvent::BarrierDrawn { x, y }
    } else {
        ToolEvent::BarrierErased { x, y }
    }
}

/// Every cell inside a rectangle, in slot order.
///
/// Slot order, so a box-select gives the same list twice running and the tools applied to it
/// apply in a reproducible order.
#[must_use]
pub fn cells_in(world: &World, x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<CellId> {
    let (lo_x, hi_x) = (x0.min(x1), x0.max(x1));
    let (lo_y, hi_y) = (y0.min(y1), y0.max(y1));
    let cells = world.cells();
    cells
        .iter()
        .filter(|i| {
            let (sx, sy) = (pos_to_square(cells.x[*i]), pos_to_square(cells.y[*i]));
            sx >= lo_x && sx <= hi_x && sy >= lo_y && sy <= hi_y
        })
        .map(|i| cells.id_at(i))
        .collect()
}

/// Isolate a cell: remove everything else from the slide.
///
/// Every other cell goes through the death path, so its matter returns to the water rather
/// than vanishing. The slide afterwards holds the same total it did before.
pub fn isolate(world: &mut World, keep: CellId) -> Vec<ToolEvent> {
    let doomed: Vec<CellId> = world
        .cells()
        .iter()
        .map(|i| world.cells().id_at(i))
        .filter(|id| *id != keep)
        .collect();
    doomed.into_iter().map(|id| remove(world, id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_core::biology::BiologyConfig;
    use mm_core::fixed::{pos, q10};
    use mm_core::light::CurrentField;
    use mm_core::{LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding};

    fn petri(size: u32) -> Scenario {
        Scenario {
            name: "petri".to_string(),
            seed: 1,
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
            ],
            ..Scenario::default()
        }
    }

    fn living(size: u32, n: u32) -> World {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../genomes/ancestor.mm"
        ))
        .expect("the ancestor is in the repository");
        let bytes = mm_asm::assemble(&src).expect("assembles").bytes;
        let mut world = World::new(petri(size)).expect("world");
        world.set_biology(BiologyConfig {
            mutation: MutationRates::none(),
            ..BiologyConfig::default()
        });
        for k in 0..n {
            let genome = world.genomes().intern(bytes.clone()).expect("genome");
            let id = world.spawn_cell(CellSeed {
                x: pos((4 + (k % 4) * 8) as i32),
                y: pos((4 + (k / 4) * 8) as i32),
                mass: q10(30),
                energy: q10(400),
                membrane: 24,
                key: 11,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome,
            });
            if let Some(i) = world.cells_mut().index(id) {
                let cells = world.cells_mut();
                cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
                cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
                cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
                cells.interior_mut(i)[11] = q10(40);
                cells.interior_mut(i)[14] = q10(40);
            }
        }
        world.adopt_current_contents_as_baseline();
        world
    }

    #[test]
    fn rewriting_a_genome_changes_one_cell_and_leaves_its_clones_alone() {
        // Genomes are interned and shared, so the mistake this guards against is editing one
        // cell and silently editing every clone of it along with it.
        let mut world = living(32, 4);
        let target = first(&world);
        let before: Vec<u64> = world
            .cells()
            .iter()
            .map(|i| world.cells().genome[i].hash())
            .collect();
        assert!(
            before.windows(2).all(|w| w[0] == w[1]),
            "the fixture should start as clones"
        );

        let new_bytes = vec![0x2Eu8; 64];
        assert!(matches!(
            rewrite_genome(&mut world, target, new_bytes),
            ToolEvent::Rewritten { bytes: 64, .. }
        ));

        let i = world.cells().index(target).expect("still alive");
        assert_eq!(world.cells().genome[i].len(), 64);
        let others: Vec<u64> = world
            .cells()
            .iter()
            .filter(|j| *j != i)
            .map(|j| world.cells().genome[j].hash())
            .collect();
        assert!(
            others.iter().all(|h| *h == before[0]),
            "editing one cell edited its clones too"
        );
    }

    #[test]
    fn a_rewritten_cell_keeps_its_body_and_carries_on_running() {
        let mut world = living(32, 1);
        let target = first(&world);
        world.run(60);
        let (mass, energy, organelles, ip_before) = {
            let i = world.cells().index(target).expect("alive");
            let c = world.cells();
            (
                c.mass[i],
                c.energy[i],
                c.slots(i).to_vec(),
                c.vm[i].ip,
            )
        };
        assert!(ip_before > 0, "the fixture should have got somewhere first");

        // Same length, so the pointer is untouched and the cell really does carry on from
        // where it was rather than restarting.
        let same_length = vec![0x2Eu8; world.cells().genome[world.cells().index(target).unwrap()].len()];
        rewrite_genome(&mut world, target, same_length);

        let i = world.cells().index(target).expect("alive");
        assert_eq!(world.cells().vm[i].ip, ip_before, "the pointer was reset");
        assert_eq!(world.cells().mass[i], mass, "the body was disturbed");
        assert_eq!(world.cells().energy[i], energy);
        assert_eq!(world.cells().slots(i), organelles.as_slice());

        // And the world runs on without complaint.
        world.run(200);
        world.check_invariants().expect("rewriting broke an invariant");
    }

    #[test]
    fn a_shorter_genome_pulls_the_pointer_inside_it() {
        let mut world = living(32, 1);
        let target = first(&world);
        world.run(60);
        assert!(world.cells().vm[world.cells().index(target).unwrap()].ip > 8);

        rewrite_genome(&mut world, target, vec![0x2Eu8; 8]);
        let i = world.cells().index(target).expect("alive");
        assert!(
            (world.cells().vm[i].ip as usize) < 8,
            "the pointer is outside the genome it points into"
        );
        world.run(50);
        world.check_invariants().expect("invariant");
    }

    #[test]
    fn rewriting_abandons_a_division_in_flight() {
        // `pa`, `pb` and `ln` index the old genome. Letting the copy run on would build a
        // daughter spliced out of two different genomes at whatever offset the edit landed on.
        let mut world = living(32, 1);
        let target = first(&world);
        // Run until the ancestor is mid-copy.
        let mut copying = false;
        for _ in 0..400 {
            world.run(1);
            let Some(i) = world.cells().index(target) else {
                break;
            };
            if world.cells().vm[i].ln > 0 {
                copying = true;
                break;
            }
        }
        assert!(copying, "the fixture never started a division");

        rewrite_genome(&mut world, target, vec![0x2Eu8; 40]);
        let i = world.cells().index(target).expect("alive");
        assert_eq!(world.cells().vm[i].ln, 0, "the copy counter survived");
        assert!(world.cells().daughter[i].is_none(), "the daughter buffer survived");
        world.run(100);
        world.check_invariants().expect("invariant");
    }

    #[test]
    fn rewriting_refuses_the_impossible_rather_than_doing_something_odd() {
        let mut world = living(32, 1);
        let target = first(&world);
        assert!(matches!(
            rewrite_genome(&mut world, target, Vec::new()),
            ToolEvent::Refused(_)
        ));
        remove(&mut world, target);
        assert!(matches!(
            rewrite_genome(&mut world, target, vec![0x2Eu8; 16]),
            ToolEvent::Refused(_)
        ));
    }

    fn first(world: &World) -> CellId {
        world
            .cells()
            .iter()
            .next()
            .map(|i| world.cells().id_at(i))
            .expect("a cell")
    }

    #[test]
    fn moving_a_cell_conserves_matter() {
        let mut world = living(32, 6);
        let before = world.total_matter();
        let cell = first(&world);
        assert!(matches!(
            relocate(&mut world, cell, 20, 25),
            ToolEvent::Dropped { .. }
        ));
        assert_eq!(world.total_matter(), before, "relocating moved matter");
        world.check_matter().expect("books balance");
        let i = world.cells().index(cell).expect("still alive");
        assert_eq!(pos_to_square(world.cells().x[i]), 20);
        assert_eq!(pos_to_square(world.cells().y[i]), 25);
    }

    #[test]
    fn a_cell_dropped_off_the_edge_lands_on_the_edge() {
        let mut world = living(32, 2);
        let cell = first(&world);
        relocate(&mut world, cell, -50, 9_999);
        let i = world.cells().index(cell).expect("alive");
        assert_eq!(pos_to_square(world.cells().x[i]), 0);
        assert_eq!(pos_to_square(world.cells().y[i]), 31);
    }

    #[test]
    fn removing_a_cell_returns_what_it_held_to_the_water() {
        // The tool that most obviously could break I4, and the reason it uses the death path.
        let mut world = living(32, 6);
        let before: i64 = world.total_matter().iter().sum();
        let cell = first(&world);
        assert_eq!(remove(&mut world, cell), ToolEvent::Removed(cell));
        assert!(world.cells().index(cell).is_none());
        // Summed across chemicals rather than compared per chemical: since M8 a death turns
        // part of the body into carrion, which is a balanced conversion the ledger accounts
        // for, so the per-species totals move by design while the total cannot.
        let after: i64 = world.total_matter().iter().sum();
        assert_eq!(after, before, "removing a cell lost matter");
        world.check_matter().expect("books balance");
    }

    #[test]
    fn isolating_leaves_one_cell_and_all_the_matter() {
        let mut world = living(32, 6);
        let before: i64 = world.total_matter().iter().sum();
        let keep = first(&world);
        let events = isolate(&mut world, keep);
        assert_eq!(events.len(), 5);
        assert_eq!(world.cells().len(), 1);
        assert!(world.cells().index(keep).is_some());
        let after: i64 = world.total_matter().iter().sum();
        assert_eq!(after, before, "isolating lost matter");
        world.check_matter().expect("books balance");
    }

    #[test]
    fn a_transplant_carries_the_whole_cell() {
        let mut source = living(32, 4);
        source.run(50);
        let cell = first(&source);
        let (mass, energy, interior, slots) = {
            let i = source.cells().index(cell).expect("alive");
            let c = source.cells();
            (
                c.mass[i],
                c.energy[i],
                c.interior(i).to_vec(),
                c.slots(i).to_vec(),
            )
        };

        let mut fresh = World::new(petri(24)).expect("world");
        let event = transplant(&source, cell, &mut fresh, 12, 12);
        let ToolEvent::Transplanted { to, .. } = event else {
            panic!("expected a transplant, got {event:?}");
        };
        let j = fresh.cells().index(to).expect("the transplant is alive");
        assert_eq!(fresh.cells().mass[j], mass);
        assert_eq!(fresh.cells().energy[j], energy);
        assert_eq!(fresh.cells().interior(j), interior.as_slice());
        assert_eq!(fresh.cells().slots(j), slots.as_slice());
        // And the destination's books balance from its new baseline.
        fresh.check_matter().expect("books balance");
        // The source is untouched: a transplant copies onto the new slide.
        assert!(source.cells().index(cell).is_some());
    }

    #[test]
    fn a_transplanted_cell_keeps_running_its_genome() {
        let mut source = living(32, 4);
        source.run(30);
        let cell = first(&source);
        let mut fresh = World::new(petri(24)).expect("world");
        transplant(&source, cell, &mut fresh, 12, 12);
        let before = fresh.cells().len();
        fresh.run(400);
        assert!(
            !fresh.cells().is_empty(),
            "the transplant died immediately on the fresh slide"
        );
        let _ = before;
        fresh.check_matter().expect("books balance");
    }

    #[test]
    fn drawing_a_barrier_does_not_delete_what_was_in_the_square() {
        let mut world = living(32, 4);
        let before = world.total_matter();
        assert_eq!(
            set_barrier(&mut world, 10, 10, true),
            ToolEvent::BarrierDrawn { x: 10, y: 10 }
        );
        assert!(world.substrate().is_blocked(10, 10));
        assert_eq!(
            world.total_matter(),
            before,
            "drawing a barrier destroyed the chemistry that was in the square"
        );
        world.check_matter().expect("books balance");

        assert_eq!(
            set_barrier(&mut world, 10, 10, false),
            ToolEvent::BarrierErased { x: 10, y: 10 }
        );
        assert!(!world.substrate().is_blocked(10, 10));
        assert_eq!(world.total_matter(), before);
    }

    #[test]
    fn a_barrier_off_the_slide_is_refused() {
        let mut world = living(32, 2);
        assert!(matches!(
            set_barrier(&mut world, 99, 3, true),
            ToolEvent::Refused(_)
        ));
        assert!(matches!(
            set_barrier(&mut world, 3, 99, true),
            ToolEvent::Refused(_)
        ));
    }

    #[test]
    fn box_select_finds_what_is_in_the_box_and_nothing_else() {
        let world = living(32, 8);
        let all = cells_in(&world, 0, 0, 31, 31);
        assert_eq!(all.len(), world.cells().len());
        // A box around nothing.
        assert!(cells_in(&world, 28, 28, 31, 31).is_empty());
        // The corners are inclusive, and the order does not depend on which was dragged first.
        let a = cells_in(&world, 0, 0, 12, 12);
        let b = cells_in(&world, 12, 12, 0, 0);
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn a_genome_copied_out_is_the_one_the_cell_is_running() {
        let mut world = living(32, 4);
        world.run(20);
        let cell = first(&world);
        let file = copy_genome(&world, cell).expect("a genome");
        let i = world.cells().index(cell).expect("alive");
        assert_eq!(file.bytes, world.cells().genome[i].bytes());
        assert_eq!(file.isa, mm_core::isa::ISA_VERSION);
        assert!(!file.name.is_empty(), "the genome came out unnamed");
        // And it survives the round trip that sharing it requires.
        let text = file.to_text();
        assert_eq!(
            GenomeFile::from_text(&text).expect("parses").bytes,
            file.bytes
        );
    }

    #[test]
    fn tools_refuse_a_cell_that_has_died_rather_than_acting_on_a_stale_id() {
        let mut world = living(32, 4);
        let cell = first(&world);
        world.kill_cell(cell);
        assert!(matches!(remove(&mut world, cell), ToolEvent::Refused(_)));
        assert!(matches!(
            relocate(&mut world, cell, 5, 5),
            ToolEvent::Refused(_)
        ));
        assert!(copy_genome(&world, cell).is_none());
        let mut fresh = World::new(petri(16)).expect("world");
        assert!(matches!(
            transplant(&world, cell, &mut fresh, 4, 4),
            ToolEvent::Refused(_)
        ));
    }
}
