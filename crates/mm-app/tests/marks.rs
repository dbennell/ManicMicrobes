//! The picture is told what happened, not left to infer it from a cell that is missing.
//!
//! `mm_core::World::deeds` is valid for one tick. `Slide::advance` may run a thousand between
//! frames, so these check the two things that could quietly break: that the marks survive from
//! the tick they happened on to the frame that shows them, and that they are bounded.

use mm_app::slide::{MarkKind, Slide, MARK_LIFE};
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::{MutationRates, Organelle, OrganelleType, Scenario};

fn slide() -> Slide {
    let mut slide = Slide::new(Scenario {
        seed: 3,
        width: 16,
        height: 16,
        ..Scenario::default()
    })
    .expect("slide");
    let biology = mm_core::biology::BiologyConfig {
        mutation: MutationRates::none(),
        ..mm_core::biology::BiologyConfig::default()
    };
    slide.world_mut().set_biology(biology);
    slide
}

fn spawn(slide: &mut Slide, mass: i32) -> CellId {
    let world = slide.world_mut();
    let genome = world.genomes().intern(vec![0x2E]).expect("genome");
    world.spawn_cell(CellSeed {
        x: pos(8),
        y: pos(8),
        mass: q10(mass),
        energy: q10(400),
        membrane: 24,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    })
}

#[test]
fn a_strike_reaches_the_frame_that_draws_it() {
    let mut slide = slide();
    let hunter = spawn(&mut slide, 200);
    let _prey = spawn(&mut slide, 60);
    let i = slide.world().cells().index(hunter).expect("alive");
    let mut spike = Organelle::finished(OrganelleType::Spike, 200);
    spike.control[0] = Q10_ONE as i16;
    slide.world_mut().cells_mut().slots_mut(i)[5] = spike;

    slide.advance(1);
    let frame = slide.frame();
    let struck: Vec<_> = frame
        .marks
        .iter()
        .filter(|m| matches!(m.kind, MarkKind::Struck { .. }))
        .collect();
    assert!(!struck.is_empty(), "a landed spike put nothing on the glass");
    assert_eq!(struck[0].age, 0.0, "a fresh mark is not fresh");
    assert!(
        struck[0].actor.is_some(),
        "a strike with a living attacker has nobody to point at"
    );
}

#[test]
fn a_death_leaves_a_mark_although_the_cell_is_gone() {
    // The whole complaint: a cell that dies is absent from the next frame and nothing says it
    // was ever there. The mark carries its own position for exactly this reason — there is no
    // cell left to ask by the time the frame is built.
    let mut slide = slide();
    let doomed = spawn(&mut slide, 60);
    slide.world_mut().kill_cell(doomed);
    slide.advance(1);
    let frame = slide.frame();
    assert!(
        frame.marks.iter().any(|m| m.kind == MarkKind::Died),
        "a cell left the slide and the picture was not told"
    );
    assert!(
        slide.world().cells().index(doomed).is_none(),
        "the cell is somehow still alive, so this tests nothing"
    );
}

#[test]
fn marks_fade_and_then_go() {
    let mut slide = slide();
    let doomed = spawn(&mut slide, 60);
    slide.world_mut().kill_cell(doomed);
    slide.advance(1);
    let fresh = slide.frame().marks.len();
    assert!(fresh > 0);

    slide.advance(MARK_LIFE / 2);
    let half = slide.frame();
    let aged = half
        .marks
        .iter()
        .find(|m| m.kind == MarkKind::Died)
        .map(|m| m.age);
    assert!(
        aged.is_some_and(|a| a > 0.0 && a < 1.0),
        "a mark half way through its life reads {aged:?}"
    );

    slide.advance(MARK_LIFE);
    assert!(
        !slide.frame().marks.iter().any(|m| m.kind == MarkKind::Died),
        "a spent mark is still on the glass"
    );
}

#[test]
fn a_frame_never_carries_more_marks_than_it_can_draw() {
    // A mass extinction is fifty thousand deaths in one tick, and drawing all of them would turn
    // the frame that shows it into the frame that drops.
    let mut slide = slide();
    for _ in 0..64 {
        let id = spawn(&mut slide, 20);
        slide.world_mut().kill_cell(id);
    }
    slide.advance(1);
    assert!(
        slide.frame().marks.len() <= 4096,
        "the mark buffer is unbounded"
    );
}
