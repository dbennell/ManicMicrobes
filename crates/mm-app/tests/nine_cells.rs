//! Nine cells in a clump, immortal and inert, stirred — the smallest thing that can show it.
//!
//! Every earlier measurement ran on hundreds of cells, where a cell can be pressed against a
//! dozen neighbours and the seam cap is in play. The report is that the artefact happens with
//! **nine cells and six neighbours apiece**, jostling gently, which no argument about a cap on
//! twenty-four seams can explain.
//!
//! Built the way the packing bench is: a `HALT` genome, a million units of energy, and the
//! damage and upkeep rates zeroed, so the cells neither die nor divide nor grow and the
//! population is nine for as long as anyone cares to watch. What is left moving is the current,
//! the Brownian jitter and the pull towards the middle — which is exactly the "movement and
//! jiggle" the artefact is reported to need.

use mm_app::slide::Slide;
use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10, POS_ONE};
use mm_core::isa::Op;
use mm_core::light::CurrentField;
use mm_core::{LightRegime, MutationRates, Organelle, OrganelleType, Scenario, World};

const PACKING: f32 = 1.15;

/// Nine immortal cells, close enough to touch, in a dish that stirs.
fn nine(gravity: i32, current: CurrentField, jitter: i32) -> Slide {
    let mut world = World::new(Scenario {
        name: "nine".into(),
        seed: 1,
        width: 16,
        height: 16,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        current,
        gravity,
        jitter,
        seeding: vec![],
        ..Scenario::default()
    })
    .expect("world");
    let mut biology = BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    };
    biology.metabolism.rates.background_damage = 0;
    biology.metabolism.rates.metabolic_floor = 0;
    biology.metabolism.rates.growth_rate = 0;
    biology.ecology.crowding_damage = 0;
    biology.ecology.spike_damage = 0;
    world.set_biology(biology);

    let inert = world
        .genomes()
        .intern(vec![Op::Halt.canonical_byte()])
        .expect("genome");
    for k in 0..9u32 {
        let span = POS_ONE * 5 / 4;
        let start = (pos(16) - 2 * span) / 2;
        let id = world.spawn_cell(CellSeed {
            x: start + (k % 3) as i32 * span,
            y: start + (k / 3) as i32 * span,
            // A spread of sizes, as a real clump has. All the same and the packing is a lattice,
            // which is the one arrangement that never asks an awkward question.
            mass: q10(18 + (k * 7 % 26) as i32),
            energy: q10(1_000_000),
            membrane: 24,
            key: 11,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome: std::sync::Arc::clone(&inert),
        });
        if let Some(i) = world.cells_mut().index(id) {
            let cells = world.cells_mut();
            cells.slots_mut(i)[0] =
                Organelle::finished(OrganelleType::Membrane, 24 + (k % 5) as u8 * 40);
            // The loadout a real cell carries, because the organelles are drawn *inside* the
            // cell and a bench cell with none cannot show whether they stay there.
            cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
            cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
            cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
        }
    }
    world.adopt_current_contents_as_baseline();

    let mut slide = Slide::new(Scenario::stress(8, 8)).expect("slide");
    slide.set_world(world);
    slide.set_camera(8.0, 8.0, 16.0, 16.0);
    slide.set_zoom(64.0);
    slide
}

/// Watch a clump for a while and report every pair drawn overlapping with no wall between them.
fn watch(
    slide: &mut Slide,
    ticks: u64,
) -> (usize, usize, usize, usize, usize, usize, Option<String>) {
    let (mut ticks_with, mut events) = (0usize, 0usize);
    let (mut down, mut up, mut side) = (0usize, 0usize, 0usize);
    let mut most = 0usize;
    let mut example: Option<String> = None;
    for _ in 0..ticks {
        slide.advance(1);
        let frame = slide.frame();
        let cells = &frame.cells;
        let mut this_tick = 0usize;
        for (a, i) in cells.iter().enumerate() {
            most = most.max(i.squash.len());
            for j in cells.iter().skip(a + 1) {
                let (dx, dy) = (j.x - i.x, j.y - i.y);
                let d = (dx * dx + dy * dy).sqrt();
                if d <= 0.0001 {
                    continue;
                }
                let ri = i.radius * PACKING * i.area_swell;
                let rj = j.radius * PACKING * j.area_swell;
                if d >= ri + rj {
                    continue;
                }
                let (ux, uy) = (dx / d, dy / d);
                let i_sees = i.squash.iter().any(|s| s.nx * ux + s.ny * uy > 0.999);
                let j_sees = j.squash.iter().any(|s| s.nx * -ux + s.ny * -uy > 0.999);
                if i_sees && j_sees {
                    continue;
                }
                this_tick += 1;
                events += 1;
                // World +y is down on screen.
                let by = if i_sees { -uy } else { uy };
                if by > 0.3 {
                    down += 1;
                } else if by < -0.3 {
                    up += 1;
                } else {
                    side += 1;
                }
                if example.is_none() {
                    example = Some(format!(
                        "d {d:.4}  drawn {ri:.4}+{rj:.4}  overlapping by {:.4}  seams {}/{}  \
                         sees {i_sees}/{j_sees}",
                        ri + rj - d,
                        i.squash.len(),
                        j.squash.len(),
                    ));
                }
            }
        }
        if this_tick > 0 {
            ticks_with += 1;
        }
    }
    (ticks_with, events, down, up, side, most, example)
}

#[test]
fn nine_immortal_cells_jostling_in_a_clump() {
    eprintln!("\nnine immortal cells, 16-square dish, 2000 ticks watched after settling:");
    eprintln!(
        "{:>22}  {:>7}  {:>7}  {:>8}  {:>5}  {:>4}  {:>5}",
        "what is moving", "ticks", "events", "max seams", "down", "up", "side"
    );
    for (label, gravity, current, jitter) in [
        ("nothing", 2, CurrentField::Still, 0),
        ("jitter only", 2, CurrentField::Still, 24),
        (
            "swirl only",
            2,
            CurrentField::Rotational {
                strength: mm_core::Q10_ONE / 2,
            },
            0,
        ),
        (
            "swirl and jitter",
            2,
            CurrentField::Rotational {
                strength: mm_core::Q10_ONE / 2,
            },
            24,
        ),
        (
            "swirl, jitter, gravity",
            24,
            CurrentField::Rotational {
                strength: mm_core::Q10_ONE / 2,
            },
            24,
        ),
    ] {
        let mut slide = nine(gravity, current, jitter);
        slide.advance(500);
        let (ticks_with, events, down, up, side, most, example) = watch(&mut slide, 2000);
        eprintln!(
            "{label:>22}  {ticks_with:>7}  {events:>7}  {most:>8}  {down:>5}  {up:>4}  {side:>5}"
        );
        if let Some(e) = example {
            eprintln!("        {e}");
        }
    }
}

/// Do organelles stay inside the cell that owns them?
///
/// They are placed on a ring at a fraction of the cell's *radius* — and a cell in a clump is not
/// a circle of that radius. Its neighbours cut it flat, sometimes a long way in, and nothing
/// about the ring knows that. An organelle on the ring where a seam has cut is drawn outside its
/// own cell, over whatever is on the other side of the wall.
///
/// Which would look like exactly what was reported: a rogue blob extending over a neighbour, on
/// a clump of nine, appearing and going away as the seams move — and nothing to do with how many
/// seams a cell is allowed.
#[test]
fn do_organelles_stay_inside_their_own_cell() {
    let mut slide = nine(
        24,
        CurrentField::Rotational {
            strength: mm_core::Q10_ONE / 2,
        },
        24,
    );
    slide.advance(500);

    let (mut outside, mut total, mut worst) = (0usize, 0usize, 0.0f32);
    let (mut down, mut up, mut side) = (0usize, 0usize, 0usize);
    let mut ticks_with = 0usize;
    for _ in 0..2000 {
        slide.advance(1);
        let mut this_tick = 0;
        for c in slide.frame().cells.iter() {
            // The outline in a given direction: the swollen radius, cut back by whichever seam
            // bites first. Exactly what the shader draws.
            let outline = |ux: f32, uy: f32| -> f32 {
                let mut r = c.radius * PACKING * c.area_swell;
                for s in &c.squash {
                    let along = s.nx * ux + s.ny * uy;
                    if along > 1e-4 {
                        r = r.min(s.face * c.radius * PACKING * c.area_swell / along);
                    }
                }
                r
            };
            for o in &c.organelles {
                total += 1;
                let dist = (o.dx * o.dx + o.dy * o.dy).sqrt();
                if dist <= 1e-5 {
                    continue;
                }
                let (ux, uy) = (o.dx / dist, o.dy / dist);
                let reach = dist + o.radius;
                let wall = outline(ux, uy);
                // A hair's breadth over is the clamp landing exactly on the wall, which is
                // float equality and not a cell drawn over its neighbour.
                if reach > wall * 1.001 {
                    outside += 1;
                    this_tick += 1;
                    worst = worst.max((reach - wall) / o.radius.max(0.0001));
                    // World +y is down on screen.
                    if uy > 0.3 {
                        down += 1;
                    } else if uy < -0.3 {
                        up += 1;
                    } else {
                        side += 1;
                    }
                }
            }
        }
        if this_tick > 0 {
            ticks_with += 1;
        }
    }
    eprintln!("\nnine immortal cells, 2000 ticks, organelles against their own outline:");
    eprintln!("  organelle-ticks checked:  {total}");
    eprintln!("  drawn outside their cell: {outside}");
    eprintln!("  ticks with at least one:  {ticks_with} of 2000");
    eprintln!(
        "  worst overshoot:          {:.2} of the organelle's own radius",
        worst
    );
    eprintln!("  direction: down {down}, up {up}, sideways {side}");
    assert_eq!(
        outside, 0,
        "{outside} organelles were drawn outside the cell that owns them, on {ticks_with} of \
         2000 ticks. They sit on a ring at a fraction of the cell's *nominal* radius, and a cell \
         in a clump is not a circle of that radius — its neighbours cut it flat. The ring has to \
         be pulled in to whatever room there is in each organelle's own direction."
    );
}
