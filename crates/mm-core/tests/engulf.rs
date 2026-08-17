//! Swallowing another cell whole.
//!
//! # Why this and not another way of stabbing something
//!
//! `docs/FEEDING.md` §4 measured why predation does not pay, and drew two conclusions that
//! together rule out most of the obvious fixes:
//!
//! > A new acquisition route that delivers a burnable substrate delivers nothing, because
//! > conversion and not supply is the limit. A route pays only if it delivers *structural matter*
//! > … **Ownership of the kill matters more than the yield of the kill.** The largest term in the
//! > loss is spatial: the food lands somewhere else.
//!
//! A spike-and-scavenge kill fails both. Half the corpse becomes carrion, digestion recovers two
//! thirds of that, the deposit lands where the *victim* died and diffuses from there — and what
//! does arrive is a substrate the mitochondrion's capacity was already the binding term on.
//!
//! Engulfment satisfies both by construction: the matter arrives **inside** the predator.
//!
//! What it arrives *as* changed once a cell could digest what it carries. It used to be
//! structure — the victim's `mass`, converted straight to build material, and nothing else. That
//! satisfied §4's first conclusion and quietly failed the whole point: a cell is four compartments
//! and only one of them was being taken, so the victim's cytoplasm and its organelles' minerals
//! were deposited into the *water* by `apply_deaths` and the eater got bricks and no bread.
//! `genomes/engulfer.mm` swallowed and starved.
//!
//! Now **you get what it had**: its cytoplasm crosses as itself, its minerals cross as themselves,
//! and its body arrives as carrion inside the eater, to be digested by a lysosome. So swallowing
//! is a *species change* (carbon to carrion) where it used to be a move, and the tests below say
//! so — see `swallowing_moves_matter_and_creates_none`.
//!
//! # And why it is a size comparison
//!
//! There is no predator flag here and there must not be. What decides a kill is bulk — and a
//! victim's shell counts towards its bulk, so armour is what a cell grows when it does not intend
//! to be swallowed. That is the arms race slot 15 was filled for; until now the shell only
//! blunted damage, which is the weaker of the two channels.
//!
//! It also makes size a *weapon*. Everywhere else in this engine being large is a bill — more
//! upkeep, more neighbours, more matter tied up — and its only income was the filter's frontal
//! area. This is the second.

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::{LightRegime, Organelle, OrganelleType, Scenario, Seeding, World, Q10_ONE};

const STRUCTURAL: usize = 4;
use mm_core::ecology::CARRION;

fn slide() -> Scenario {
    Scenario {
        name: "engulf".to_string(),
        seed: 12,
        width: 32,
        height: 32,
        light: LightRegime::Uniform {
            intensity: Q10_ONE,
        },
        seeding: vec![Seeding::Uniform {
            chemical: 11,
            per_square: q10(200),
        }],
        ..Scenario::default()
    }
}

/// A cell at `(x, y)` of a given mass, with `appetite` on a vacuole and `shells` shells.
fn cell(world: &mut World, x: i32, y: i32, mass: i32, appetite: i16, shells: usize) -> CellId {
    let genome = world
        .genomes()
        .intern(mm_asm::assemble("HALT\n").expect("assembles").bytes)
        .expect("interned");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(y),
        mass,
        energy: q10(100_000),
        membrane: 48,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    let i = world.cells_mut().index(id).expect("spawned");
    let cells = world.cells_mut();
    if appetite > 0 {
        let mut v = Organelle::finished(OrganelleType::Vacuole, 200);
        // `control[1]`: the word that starts shut. See the note at the engulfment site — appetite
        // on `control[0]` made every vacuole in the library a mouth.
        v.control[1] = appetite;
        cells.slots_mut(i)[1] = v;
    }
    for k in 0..shells {
        let mut sh = Organelle::finished(OrganelleType::Shell, 255);
        sh.control[0] = Q10_ONE as i16;
        cells.slots_mut(i)[8 + k] = sh;
    }
    id
}

/// A big hungry cell swallows a small one, and the small one is gone.
#[test]
fn a_larger_cell_swallows_a_smaller_one() {
    let mut world = World::new(slide()).expect("world");
    let predator = cell(&mut world, 16, 16, q10(400), Q10_ONE as i16, 0);
    let prey = cell(&mut world, 16, 16, q10(40), 0, 0);
    let before = world.cells().len();

    world.run(3);

    assert_eq!(world.cells().len(), before - 1, "nothing was swallowed");
    assert!(
        world.cells_mut().index(prey).is_none(),
        "the prey survived being eaten"
    );
    let i = world.cells_mut().index(predator).expect("the predator lived");
    assert!(
        world.cells().interior(i)[CARRION] > 0,
        "the predator swallowed something and got no carrion out of it. The body arrives as \
         flesh now rather than as build material — `carrion_fraction` describes a corpse rotting \
         in water and being eaten is not rotting — so a lysosome is what turns a kill into food."
    );
}

/// And the groceries, not only the bricks.
///
/// The failure this exists for is silent and is what `engulfer.mm` died of: engulfment took
/// `mass` alone, so the victim's cytoplasm was deposited into the water by `apply_deaths` and the
/// eater swallowed a whole cell without acquiring one unit of anything burnable.
#[test]
fn a_swallowed_cell_hands_over_its_cytoplasm() {
    let mut world = World::new(slide()).expect("world");
    let predator = cell(&mut world, 16, 16, q10(400), Q10_ONE as i16, 0);
    let prey = cell(&mut world, 16, 16, q10(40), 0, 0);

    // Something in the prey worth having, and none of it in the predator.
    let sugar = 8;
    {
        let p = world.cells_mut().index(prey).expect("prey");
        world.cells_mut().interior_mut(p)[sugar] = q10(30);
        let e = world.cells_mut().index(predator).expect("predator");
        world.cells_mut().interior_mut(e)[sugar] = 0;
    }
    world.adopt_current_contents_as_baseline();

    world.run(3);

    assert!(
        world.cells_mut().index(prey).is_none(),
        "nothing was swallowed, so this measures nothing"
    );
    let i = world.cells_mut().index(predator).expect("the predator lived");
    assert!(
        world.cells().interior(i)[sugar] > 0,
        "the predator swallowed a cell holding thirty units of substrate and got none of it. \
         That is what `engulfer.mm` starved of: the body was taken and the food left behind."
    );
    world.check_matter().expect("books balance");
}

/// Not if it is not big enough. The gate is bulk, and nothing else.
#[test]
fn a_cell_cannot_swallow_something_its_own_size() {
    let mut world = World::new(slide()).expect("world");
    cell(&mut world, 16, 16, q10(200), Q10_ONE as i16, 0);
    let peer = cell(&mut world, 16, 16, q10(200), 0, 0);

    world.run(3);

    assert!(
        world.cells_mut().index(peer).is_some(),
        "a cell swallowed something its own size; the ratio is not being applied"
    );
}

/// A shell is bulk. Armour is what a cell grows when it does not intend to be swallowed.
#[test]
fn a_shell_makes_a_cell_too_much_of_a_mouthful() {
    let bare_eaten = {
        let mut world = World::new(slide()).expect("world");
        cell(&mut world, 16, 16, q10(400), Q10_ONE as i16, 0);
        let prey = cell(&mut world, 16, 16, q10(180), 0, 0);
        world.run(3);
        world.cells_mut().index(prey).is_none()
    };
    let armoured_eaten = {
        let mut world = World::new(slide()).expect("world");
        cell(&mut world, 16, 16, q10(400), Q10_ONE as i16, 0);
        // The same prey, at the same mass, wearing shells.
        let prey = cell(&mut world, 16, 16, q10(180), 0, 6);
        world.run(3);
        world.cells_mut().index(prey).is_none()
    };
    assert!(bare_eaten, "the bare prey was not eaten, so there is no contrast to draw");
    assert!(
        !armoured_eaten,
        "armour made no difference to being swallowed, which is the channel the shell is for"
    );
}

/// Appetite is a behaviour, not an organ. A vacuole that is not asking does not swallow.
#[test]
fn a_cell_that_is_not_asking_does_not_swallow() {
    let mut world = World::new(slide()).expect("world");
    cell(&mut world, 16, 16, q10(400), 0, 0);
    let prey = cell(&mut world, 16, 16, q10(40), 0, 0);
    world.run(3);
    assert!(
        world.cells_mut().index(prey).is_some(),
        "a cell with its appetite at zero ate something anyway"
    );
}

/// And the books close. A swallowed cell's matter moves; none of it is created or destroyed.
#[test]
fn swallowing_moves_matter_and_creates_none() {
    let mut world = World::new(slide()).expect("world");
    // The carbonate buffer is held still, because this asserts matter **per chemical** and the
    // buffer's whole job is to move carbon between two of them. That is a species change like
    // the diazosome's, accounted through `Ledger::convert` and checked by `check_matter` below;
    // what would break here is the stricter per-species equality, which is the right assertion
    // for an engulfment and the wrong one for a world with a buffer running in it.
    let mut biology = world.biology().clone();
    biology.minerals.buffer_rate = 0;
    world.set_biology(biology);
    cell(&mut world, 16, 16, q10(400), Q10_ONE as i16, 0);
    let prey = cell(&mut world, 16, 16, q10(40), 0, 0);
    world.adopt_current_contents_as_baseline();
    let before = world.total_matter();

    world.run(4);
    assert!(
        world.cells_mut().index(prey).is_none(),
        "nothing was swallowed, so this test is measuring nothing"
    );
    // **The sum, not the per-species array.** Swallowing now converts the victim's body from
    // structural carbon into carrion, which is a balanced reaction reported through
    // `Ledger::convert` — so per-species totals are *supposed* to move, exactly as they do for
    // the buffer this test switches off and for the diazosome. I4 in its exact form is "total
    // matter is invariant, and a per-species total may change only through a reported reaction",
    // and that is what the two assertions below are between them: the sum cannot move, and
    // `check_matter` recomputes every species against the ledger's claim and fails on any drift.
    let after = world.total_matter();
    assert_eq!(
        after.iter().sum::<i64>(),
        before.iter().sum::<i64>(),
        "matter was created or destroyed over a swallowing: {before:?} then {after:?}"
    );
    assert!(
        after[CARRION] > before[CARRION] && after[STRUCTURAL] < before[STRUCTURAL],
        "a swallowed body did not become carrion: structural {} -> {}, carrion {} -> {}",
        before[STRUCTURAL],
        after[STRUCTURAL],
        before[CARRION],
        after[CARRION]
    );
    world.check_matter().expect("I4 broke over an engulfment");
}
