//! Can a cell find another by the light it cannot help giving off? (SPEC §8.2, ISA 5.)
//!
//! `emission.rs` establishes that the signature exists, is honest, and can be read at range.
//! This asks the question that matters: is it *usable* — can a genome steer by it and end up
//! somewhere it would not otherwise have got to?
//!
//! The comparison is `drifter.mm`'s own: the same body, the same cilia, the same power, and the
//! only difference is whether the four instructions between the sensor and the thrusters are
//! there. That genome's header says exactly this about the chemical gradient it ignores.

use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10, POS_ONE};
use mm_core::light::CurrentField;
use mm_core::organelle::{Organelle, OrganelleType};
use mm_core::{LightRegime, Scenario, World};

fn slide() -> Scenario {
    Scenario {
        name: "hunting".to_string(),
        seed: 0x5EE1,
        width: 64,
        height: 64,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        current: CurrentField::Still,
        // No Brownian nudge: the question is whether it *steered*, and a cell that wandered
        // into its dinner would answer it wrongly.
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
            // The minerals every recipe in the catalogue is costed in, at the
            // Redfield proportion of the carbon above. Nothing produces them.
            mm_core::Seeding::Uniform {
                chemical: 5,
                per_square: (q10(400)) * 16 / 106,
            },
            mm_core::Seeding::Uniform {
                chemical: 6,
                per_square: (q10(400)) / 53,
            },
        ],
        ..Scenario::default()
    }
}

/// The body both variants carry: `drifter.mm`'s, with a photosensor where its chemosensor goes.
const BODY: &str = "
        GENE    #build
        IMM     40
        IMM     1
        IMM     1
        BUILD
        IMM     60
        IMM     3
        IMM     3
        BUILD
        IMM     50
        IMM     2
        IMM     2
        BUILD
        IMM     40
        IMM     8               ; photosensor
        IMM     4
        BUILD
        IMM     80
        IMM     6               ; cilium
        IMM     6
        BUILD
        IMM     80
        IMM     6               ; cilium
        IMM     8
        BUILD
        ZERO
        ONE
        IMM     6
        OSET                    ; cilium 6 mounted along +x
        IMM     12
        ONE
        IMM     8
        OSET                    ; cilium 8 mounted along +y
        RET

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
        RET
";

/// Beats both cilia flat out and never reads anything. `drifter.mm`'s whole point.
const BLIND: &str = "
        GENE    #swim
        IMM     255
        ZERO
        IMM     6
        OSET
        IMM     255
        ZERO
        IMM     8
        OSET
        RET
";

/// The same, with the four instructions connected: thrust along each axis is that axis's
/// gradient in the metabolic band — which is to say, towards whatever is spending most nearby.
const STEERED: &str = "
        GENE    #swim
        IMM     7               ; photosensor reading 7: metabolic glow, gradient x
        IMM     4
        OGET
        ZERO
        IMM     6
        OSET
        IMM     8               ; reading 8: gradient y
        IMM     4
        OGET
        ZERO
        IMM     8
        OSET
        RET
";

fn dress(world: &mut World, id: CellId, photo: bool) {
    if let Some(i) = world.cells_mut().index(id) {
        let cells = world.cells_mut();
        cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
        cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
        cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
        if photo {
            cells.slots_mut(i)[4] = Organelle::finished(OrganelleType::Photosensor, 40);
            let mut a = Organelle::finished(OrganelleType::Cilium, 80);
            a.control[1] = 0;
            cells.slots_mut(i)[6] = a;
            let mut b = Organelle::finished(OrganelleType::Cilium, 80);
            b.control[1] = 12;
            cells.slots_mut(i)[8] = b;
        }
        cells.interior_mut(i)[4] = q10(200);
        cells.interior_mut(i)[11] = q10(40);
        cells.interior_mut(i)[14] = q10(40);
    }
}

fn spawn(world: &mut World, bytes: Vec<u8>, x: i32, y: i32, photo: bool) -> CellId {
    let g = world.genomes().intern(bytes).expect("intern");
    let id = world.spawn_cell(CellSeed {
        x: pos(x),
        y: pos(y),
        mass: q10(30),
        energy: q10(4000),
        membrane: 24,
        key: 11,
        badge: 0,
        species: 0,
        parent: CellId::NONE,
        birth_tick: 0,
        genome: g,
    });
    dress(world, id, photo);
    id
}

/// How far the swimmer is from the middle of the crowd, in whole squares.
fn distance_to(world: &World, id: CellId, cx: i32, cy: i32) -> i32 {
    world.cells().index(id).map_or(-1, |i| {
        let dx = (world.cells().x[i] - pos(cx)) as i64;
        let dy = (world.cells().y[i] - pos(cy)) as i64;
        (((dx * dx + dy * dy).isqrt()) / POS_ONE as i64) as i32
    })
}

#[test]
fn steering_by_signature_finds_the_crowd_and_swimming_blind_does_not() {
    let prey = {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm");
        let src = std::fs::read_to_string(path).expect("genome");
        mm_asm::assemble(&src).expect("assembles").bytes
    };

    // Five squares out, which is inside `em_range`. Twenty was not, and the steered swimmer sat
    // perfectly still reading a gradient of zero — which is the honest answer and worth knowing:
    // **a signature is a homing sense and not a searching one.** It cannot cross a slide.
    eprintln!("\nswimmer starts 5 squares from a crowd of nine, 120 ticks:");
    eprintln!("{:>10}  {:>9}  {:>9}", "variant", "start", "end");
    let mut ended = Vec::new();
    for (label, swim) in [("blind", BLIND), ("steered", STEERED)] {
        let mut world = World::new(slide()).expect("world");
        let src = format!(
            "        EXPRESS #build\n        EXPRESS #feed\n        EXPRESS #swim\n        HALT\n{BODY}{swim}"
        );
        let bytes = mm_asm::assemble(&src).expect("assembles").bytes;
        let swimmer = spawn(&mut world, bytes, 28, 32, true);

        // A crowd, off to one side, all of them metabolising and none of them armed.
        for k in 0..9 {
            let id = spawn(&mut world, prey.clone(), 32 + (k % 3), 31 + (k / 3), false);
            let _ = id;
        }
        world.adopt_current_contents_as_baseline();

        let start = distance_to(&world, swimmer, 33, 32);
        let mut end = start;
        for tick in 0..120 {
            world.step();
            let d = distance_to(&world, swimmer, 33, 32);
            if d < 0 {
                eprintln!("{label:>10}  died at tick {tick}, last distance {end}");
                break;
            }
            end = d;
            if tick % 30 == 0 {
                let i = world.cells().index(swimmer).expect("alive");
                eprintln!(
                    "{label:>10}  tick {tick:>4}  distance {d:>3}  energy {:>7}  gx {:>6} gy {:>6}",
                    world.cells().energy[i] / mm_core::Q10_ONE,
                    world.cells().slots(i)[6].control[0],
                    world.cells().slots(i)[8].control[0],
                );
            }
        }
        eprintln!("{label:>10}  {start:>9}  {end:>9}");
        ended.push(end);
    }

    assert!(
        ended[1] >= 0,
        "the steered swimmer died, so this measures nothing"
    );
    assert!(
        ended[1] < ended[0],
        "steering by the signature got no closer than swimming blind: {} against {}",
        ended[1],
        ended[0]
    );
}

/// What the eyes and the fins cost, against the lineage they were added to.
///
/// Every organelle is upkeep every tick, and this lineage was already the most expensive thing
/// in `genomes/` before it grew eyes. So the question is not whether homing works — the test
/// above settles that — but whether a cell can afford to do it and still breed.
#[test]
fn what_hunting_by_signature_costs() {
    fn assemble(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../genomes")
            .join(name);
        let src = std::fs::read_to_string(path).expect("genome");
        mm_asm::assemble(&src).expect("assembles").bytes
    }

    eprintln!("\none founder in a lit dish, 2400 ticks:");
    eprintln!("{:>14}  {:>6}  {:>5}", "genome", "bytes", "pop");
    for name in ["predator.mm", "sentinel.mm", "stalker.mm"] {
        let bytes = assemble(name);
        let mut world = World::new(Scenario {
            width: 64,
            height: 64,
            ..slide()
        })
        .expect("world");
        world.place_founders(&bytes, 1);
        world.run(2400);
        eprintln!("{name:>14}  {:>6}  {:>5}", bytes.len(), world.cells().len());
    }
}
