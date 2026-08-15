//! The shell: catalogue slot 15, and the two things it costs.
//!
//! It is the catalogue's first *defence*. Before it, a spike met bare membrane or nothing, so
//! predation was free or worthless with nothing in between and no arms race had anywhere to go
//! from either end — `docs/FEEDING.md` §4 measures the predator's half of that.
//!
//! What keeps it a strategy rather than an upgrade every lineage grows is that it is paid for
//! twice: in matter, as everything is, and in **shade**. A shell is opaque, so the fraction of
//! the body behind it is the fraction of the light that never reaches a chloroplast. Armour and
//! photosynthesis are rival on one control word, which is the whole design, and both halves are
//! asserted here.

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::organelle::{shell_admits, shell_cover, SHELL_MAX_COVER};
use mm_core::{LightRegime, Organelle, OrganelleType, Scenario, Seeding, World, Q10_ONE};

/// A lit, still, well-fed slide with nothing else going on.
fn slide() -> Scenario {
    Scenario {
        name: "shell".to_string(),
        seed: 4,
        width: 32,
        height: 32,
        light: LightRegime::Uniform {
            intensity: Q10_ONE,
        },
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

fn spawn(world: &mut World, x: i32, y: i32) -> usize {
    let genome = world
        .genomes()
        .intern(mm_asm::assemble("HALT\n").expect("assembles").bytes)
        .expect("interned");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(y),
        mass: q10(40),
        energy: q10(100_000),
        membrane: 48,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome,
    });
    world.cells_mut().index(id).expect("spawned")
}

/// Put `n` shells of full `param` on a cell, closed to `closure`.
fn shell(world: &mut World, i: usize, n: usize, closure: i16) {
    let cells = world.cells_mut();
    for k in 0..n {
        let mut sh = Organelle::finished(OrganelleType::Shell, 255);
        sh.control[0] = closure;
        cells.slots_mut(i)[8 + k] = sh;
    }
}

/// A spike deals less to a shelled cell than to a bare one.
#[test]
fn a_shell_turns_a_spike() {
    let mut world = World::new(slide()).expect("world");
    let attacker = spawn(&mut world, 16, 16);
    let bare = spawn(&mut world, 17, 16);
    let armoured = spawn(&mut world, 15, 16);
    {
        let cells = world.cells_mut();
        let mut sp = Organelle::finished(OrganelleType::Spike, 200);
        sp.control[0] = Q10_ONE as i16;
        cells.slots_mut(attacker)[4] = sp;
    }
    shell(&mut world, armoured, 4, Q10_ONE as i16);
    assert!(
        shell_cover(world.cells(), armoured) > 0,
        "the armoured cell built no shell"
    );

    world.run(40);
    let cells = world.cells();
    let (hurt_bare, hurt_armoured) = (cells.damage[bare], cells.damage[armoured]);
    assert!(
        hurt_bare > 0,
        "the bare cell took no damage, so this test is measuring nothing"
    );
    assert!(
        hurt_armoured < hurt_bare,
        "armoured took {hurt_armoured} against {hurt_bare} bare: the shell turned nothing"
    );
}

/// And it is never total, because an invulnerable prey has no arms race in it.
#[test]
fn a_shell_can_never_close_completely() {
    let mut world = World::new(slide()).expect("world");
    let i = spawn(&mut world, 16, 16);
    // Every remaining slot, every one at full size and fully closed — far past what any
    // affordable cell would carry.
    shell(&mut world, i, 8, Q10_ONE as i16);
    let cover = shell_cover(world.cells(), i);
    assert!(
        cover <= SHELL_MAX_COVER,
        "cover {cover} passed the cap {SHELL_MAX_COVER}"
    );
    assert!(
        shell_admits(cover) > 0,
        "a fully shelled cell admitted nothing at all; SPEC §3 keeps cliffs like that out of \
         the landscape, and total immunity is one"
    );
}

/// The other half of the price: a shell shades the chloroplasts under it.
///
/// Two identical autotrophs, one shelled, on the same lit slide. The shelled one must bank less
/// substrate — if it does not, the armour is free and every lineage should grow it.
#[test]
fn a_shell_shades_its_own_chloroplasts() {
    let mut world = World::new(slide()).expect("world");
    let open = spawn(&mut world, 10, 10);
    let closed = spawn(&mut world, 22, 22);
    {
        let cells = world.cells_mut();
        for i in [open, closed] {
            cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
            cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 200);
            // The waste photosynthesis runs on, so neither is limited by having nothing to fix.
            cells.interior_mut(i)[11] = q10(200);
        }
    }
    shell(&mut world, closed, 4, Q10_ONE as i16);

    world.run(30);
    let cells = world.cells();
    let sugar = |i: usize| i64::from(cells.interior(i)[8]);
    assert!(
        sugar(open) > 0,
        "the unshelled cell banked nothing, so this test is measuring nothing"
    );
    assert!(
        sugar(closed) < sugar(open),
        "shelled banked {} against {} open: the shell cast no shade, and armour that costs \
         nothing is not a trade",
        sugar(closed),
        sugar(open)
    );
}

/// It reports the trade from both ends, and the light reading is the half that cannot be
/// derived from anything else the cell can see.
#[test]
fn a_shell_reports_its_coverage_and_what_light_gets_through() {
    let mut world = World::new(slide()).expect("world");
    let i = spawn(&mut world, 16, 16);
    shell(&mut world, i, 2, Q10_ONE as i16);
    world.run(1);

    let read = |idx: i16| {
        mm_core::biology::read_organelle(
            world.cells(),
            world.substrate(),
            world.neighbours(),
            i,
            8,
            idx,
            world.biology().ecology.spike_damage,
            world.biology().ecology.em_range,
            world.biology().metabolism.catalogue.metabolism,
            // Not crowded; this test is about the shell's own readings.
            0,
        )
    };
    let cover = read(0);
    let through = read(1);
    assert!(cover > 0, "a built shell reported no coverage");
    assert!(
        through > 0 && i32::from(through) < Q10_ONE,
        "light through was {through}: a partly shelled cell in full daylight should see less \
         than all of it and more than none"
    );
}

/// The recipe binds: no silicon, no shell.
///
/// `BUILD` checks every ingredient before it spends any of them. A cell short of one and rich in
/// the others must come away having spent nothing — a half-charged build is matter destroyed, and
/// I4 does not have a tolerance for that.
#[test]
fn a_shell_cannot_be_built_without_silicon() {
    const SILICON: usize = 7;
    // param 100 into slot 5, twice: once starved of silicon and once not.
    let src = "IMM 100\nIMM 15\nIMM 5\nBUILD\nHALT\n";
    for (silicon, expected) in [(0, false), (q10(200), true)] {
        let mut world = World::new(slide()).expect("world");
        let genome = world
            .genomes()
            .intern(mm_asm::assemble(src).expect("assembles").bytes)
            .expect("interned");
        let id = world.spawn_cell(CellSeed {
            x: pos(16),
            y: pos(16),
            mass: q10(40),
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
        {
            let cells = world.cells_mut();
            // Plenty of structural matter, so silicon is the only thing that can be short.
            cells.interior_mut(i)[4] = q10(400);
            cells.interior_mut(i)[SILICON] = silicon;
        }
        world.run(4);
        let built = world.cells().slots(i)[5].kind == OrganelleType::Shell;
        assert_eq!(
            built, expected,
            "with {silicon} silicon the shell was {}built",
            if built { "" } else { "not " }
        );
        if !expected {
            // Not equality: the cell is alive, and metabolism grows its body out of the
            // cytoplasm whether or not a `BUILD` succeeds. What must not have happened is the
            // build's own charge, which is far larger than a few ticks of growth.
            let cost = world
                .biology()
                .metabolism
                .catalogue
                .spec(OrganelleType::Shell)
                .matter_cost(100);
            let spent = q10(400) - world.cells().interior(i)[4];
            assert!(
                spent < cost,
                "a build that could not afford its silicon still spent {spent} carbon, and the \
                 shell's structural cost is {cost}"
            );
        }
    }
}

/// Silicon put into a shell is still in the world, and comes back out as silicon.
///
/// The reason the recipe is held in the organelle rather than folded into `mass`: mass returns as
/// the structural chemical, so a shell routed through it would hand back carbon. That is the
/// one-way conversion `carrion`'s decay used to be.
#[test]
fn the_silicon_in_a_shell_is_still_in_the_world() {
    const SILICON: usize = 7;
    let mut world = World::new(slide()).expect("world");
    let genome = world
        .genomes()
        .intern(
            mm_asm::assemble("IMM 100\nIMM 15\nIMM 5\nBUILD\nHALT\n")
                .expect("assembles")
                .bytes,
        )
        .expect("interned");
    let id = world.spawn_cell(CellSeed {
        x: pos(16),
        y: pos(16),
        mass: q10(40),
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
    {
        let cells = world.cells_mut();
        cells.interior_mut(i)[4] = q10(400);
        cells.interior_mut(i)[SILICON] = q10(200);
    }
    world.adopt_current_contents_as_baseline();
    let before = world.total_matter()[SILICON];

    world.run(6);
    assert_eq!(
        world.cells().slots(i)[5].kind,
        OrganelleType::Shell,
        "nothing was built, so this test is measuring nothing"
    );
    assert!(
        world.cells().interior(i)[SILICON] < q10(200),
        "the build took no silicon out of the interior"
    );
    assert_eq!(
        world.total_matter()[SILICON],
        before,
        "silicon went missing between the interior and the slot"
    );

    world.kill_cell(id);
    world.run(1);
    assert_eq!(
        world.total_matter()[SILICON],
        before,
        "silicon went missing when the cell died"
    );
    world.check_matter().expect("I4 broke over a recipe");
}
