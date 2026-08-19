//! What `genomes/sleeper.mm` actually does, and the three probes it took to find out.
//!
//! All ignored — run them on purpose. They are kept because the first two answers were wrong and
//! the way they were wrong is instructive: a gate that looks like it is failing to pay may not be
//! running at all, and a gate that is running may be reading a number its own genome has already
//! rewritten.

use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::chem::CARBON_DIOXIDE;
use mm_core::fixed::{pos, q10, Q10_ONE};
use mm_core::{MutationRates, Organelle, OrganelleType, Scenario, World};

fn root(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn assembled(genome: &str) -> Vec<u8> {
    let source = std::fs::read_to_string(root(genome)).expect("genome");
    mm_asm::assemble(&source).expect("assemble").bytes
}

/// Does the gate work at all, in isolation?
///
/// The one that settled it. A cell with a body and nothing to fix shuts its chloroplast by tick
/// 10 and keeps it shut, so any claim that the gate "never fires" in a world is a claim about the
/// world rather than about the gene — which is what sent the next probe looking in the right
/// place.
#[test]
#[ignore = "a probe; run it on purpose"]
fn the_gate_shuts_a_chloroplast_that_has_nothing_to_fix() {
    let mut world = World::new(Scenario {
        seed: 7,
        width: 16,
        height: 16,
        ..Scenario::default()
    })
    .expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    let genome = world.genomes().intern(assembled("genomes/sleeper.mm")).expect("intern");
    let id = world.spawn_cell(CellSeed {
        x: pos(8),
        y: pos(8),
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
    // The body the genome would build. Installed directly, because a bare slide has no chemistry
    // to build one out of — the first run of this probe reported "the cell builds nothing", which
    // was true and had nothing to do with the gate.
    let i = world.cells().index(id).expect("alive");
    world.cells_mut().slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 48);
    world.cells_mut().slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
    world.cells_mut().slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);

    let mut shut_by = None;
    for tick in 0..60 {
        world.run(1);
        let Some(i) = world.cells().index(id) else { break };
        world.cells_mut().interior_mut(i)[CARBON_DIOXIDE] = 0;
        let control = world.cells().slots(i)[3].control[0];
        if control == 0 && shut_by.is_none() {
            shut_by = Some(tick);
        }
    }
    eprintln!("chloroplast shut at tick {shut_by:?}");
    assert!(shut_by.is_some(), "the gate never fired with nothing to fix");
}

/// How often the gate fires across a whole population, and the gene-order trap.
///
/// **Measure the population, not a cell.** The first version followed `cells().iter().next()`
/// after five thousand ticks and reported 0 firings — a statement about one arbitrary descendant
/// of a mutating lineage, not about the genome.
///
/// What it found once it was looking at everybody: with `EXPRESS #feed` ahead of `EXPRESS #sun`,
/// a third of cell-ticks sat below the gate's threshold and the gate fired on **none** of them,
/// because `#feed` tops the cell up immediately before `#sun` reads it. The cells measured as low
/// were low at *end* of tick, having spent their carbon on photosynthesis after the gate had
/// already looked. The gene was testing a condition its own genome prevented. Swapping the two
/// `EXPRESS` lines takes it from 0% to 33%.
#[test]
#[ignore = "a probe; run it on purpose"]
fn how_often_the_gate_fires() {
    for (file, mutate) in [
        ("scenarios/the_thicket.ron", false),
        ("scenarios/the_thicket.ron", true),
        ("scenarios/the_lean_water.ron", false),
        ("scenarios/the_lean_water.ron", true),
    ] {
        let scenario = Scenario::from_ron(&std::fs::read_to_string(root(file)).expect("scenario"))
            .expect("parse");
        let mut world = World::new(scenario).expect("world");
        if !mutate {
            let biology = BiologyConfig {
                mutation: MutationRates::none(),
                ..world.biology().clone()
            };
            world.set_biology(biology);
        }
        world.place_founders(&assembled("genomes/sleeper.mm"), 16);
        world.run(5_000);

        let (mut shut, mut ticks, mut low) = (0u64, 0u64, 0u64);
        for _ in 0..200 {
            world.run(1);
            for i in world.cells().iter() {
                let Some(c) = world
                    .cells()
                    .slots(i)
                    .iter()
                    .find(|o| o.kind == OrganelleType::Chloroplast && o.is_active())
                else {
                    continue;
                };
                ticks += 1;
                if c.control[0] == 0 {
                    shut += 1;
                }
                if world.cells().interior(i)[CARBON_DIOXIDE] < 4 * Q10_ONE {
                    low += 1;
                }
            }
        }
        let pct = |n: u64| if ticks > 0 { n * 100 / ticks } else { 0 };
        eprintln!(
            "{file} mutation {}: {ticks} cell-ticks, {}% below the threshold, {}% shut",
            if mutate { "on" } else { "off" },
            pct(low),
            pct(shut),
        );
    }
}

/// Does idling pay? Five seeds, because a stochastic result stated on one is not a result.
#[test]
#[ignore = "a probe; run it on purpose"]
fn does_the_gate_pay() {
    for file in ["scenarios/the_thicket.ron", "scenarios/the_lean_water.ron"] {
        let mut wins = 0;
        for seed in [1u64, 2, 3, 4, 5] {
            let run = |genome: &str| {
                let mut scenario =
                    Scenario::from_ron(&std::fs::read_to_string(root(file)).expect("scenario"))
                        .expect("parse");
                scenario.seed = seed;
                let mut world = World::new(scenario).expect("world");
                world.place_founders(&assembled(genome), 16);
                world.run(20_000);
                world.cells().len()
            };
            let (a, s) = (run("genomes/ancestor.mm"), run("genomes/sleeper.mm"));
            if s > a {
                wins += 1;
            }
            eprintln!("{file} seed {seed}: ancestor {a}, sleeper {s}");
        }
        eprintln!("  -> sleeper wins {wins} of 5 seeds");
    }
}

/// Does the night gate fire on the beat, and does it pay?
///
/// The one that could not be written before ISA 15: `nightjar.mm` reads ambient light, which was
/// a constant zero until the reading got a resolution. Five seeds.
#[test]
#[ignore = "a probe; run it on purpose"]
fn does_the_night_gate_pay() {
    let file = "scenarios/the_short_night.ron";

    // First, that it actually tracks the sky rather than merely firing.
    let scenario =
        Scenario::from_ron(&std::fs::read_to_string(root(file)).expect("scenario")).expect("parse");
    let mut world = World::new(scenario).expect("world");
    let biology = BiologyConfig {
        mutation: MutationRates::none(),
        ..world.biology().clone()
    };
    world.set_biology(biology);
    world.place_founders(&assembled("genomes/nightjar.mm"), 16);
    for warm in [1000u64, 2000, 3000, 4000] {
        world.run(1000);
        eprintln!("  warmup {warm}: population {}", world.cells().len());
    }
    let (mut shut, mut ticks) = (0u64, 0u64);
    let mut trace = Vec::new();
    for step in 0..2_000 {
        world.run(1);
        let mut s = 0u32;
        let mut n = 0u32;
        for i in world.cells().iter() {
            if let Some(c) = world
                .cells()
                .slots(i)
                .iter()
                .find(|o| o.kind == OrganelleType::Chloroplast && o.is_active())
            {
                n += 1;
                if c.control[0] == 0 {
                    s += 1;
                }
            }
        }
        shut += s as u64;
        ticks += n as u64;
        if step % 200 == 0 {
            trace.push(format!(
                "t+{step}: light {} shut {}%",
                world.substrate().light_at(32, 32),
                if n > 0 { s * 100 / n } else { 0 }
            ));
        }
    }
    // What does a cell actually carry, and what does its eye actually say?
    if let Some(i) = world.cells().iter().next() {
        let kinds: Vec<String> = world.cells().slots(i).iter().enumerate()
            .filter(|(_, o)| o.is_present())
            .map(|(n, o)| format!("{n}:{}{}", o.kind.name(), if o.is_active() { "" } else { "(building)" }))
            .collect();
        eprintln!("  loadout: {}", kinds.join(" "));
        if let Some(eye) = world.cells().slots(i).iter().find(|o| o.kind == OrganelleType::Photosensor) {
            let reading = mm_core::sensing::read_sensor(eye, 0, mm_core::sensing::SensorContext {
                substrate: world.substrate(), x: 32, y: 32,
                tick: 0, cell_key: 1, touch: Default::default(),
                glow: Default::default(), shell_cover: 0,
            });
            eprintln!("  eye reads {reading:?} at light {}", world.substrate().light_at(32, 32));
        } else {
            eprintln!("  NO PHOTOSENSOR BUILT");
        }
    }
    eprintln!("{file}: {}% of cell-ticks shut over one period", if ticks > 0 { shut * 100 / ticks } else { 0 });
    for line in &trace {
        eprintln!("  {line}");
    }

    let mut wins = 0;
    for seed in [1u64, 2, 3, 4, 5] {
        let run = |genome: &str| {
            let mut scenario =
                Scenario::from_ron(&std::fs::read_to_string(root(file)).expect("scenario"))
                    .expect("parse");
            scenario.seed = seed;
            let mut world = World::new(scenario).expect("world");
            world.place_founders(&assembled(genome), 16);
            world.run(20_000);
            world.cells().len()
        };
        let (a, n) = (run("genomes/ancestor.mm"), run("genomes/nightjar.mm"));
        if n > a {
            wins += 1;
        }
        eprintln!("{file} seed {seed}: ancestor {a}, nightjar {n}");
    }
    eprintln!("  -> nightjar wins {wins} of 5 seeds");
}

/// The night gate's economics: how big an eye, and how dark before it pays to sleep?
///
/// The first `nightjar.mm` went extinct by tick 3,000 with a param-40 photosensor, and the
/// arithmetic says why before any run does: that eye costs 28 `Q10` a tick *always*, and a
/// param-60 chloroplast has only 42 `Q10` a tick to save and only while it is dark. A sensor has
/// to cost less than the decision it informs. `read_sensor`'s ambient-light path never reads
/// `param`, so a photosensor's size buys nothing here and is pure cost.
#[test]
#[ignore = "a probe; run it on purpose"]
fn how_big_an_eye_and_how_dark() {
    let file = "scenarios/the_short_night.ron";
    let base = std::fs::read_to_string(root("genomes/nightjar.mm")).expect("genome");

    let ancestor: Vec<usize> = [1u64, 2, 3]
        .iter()
        .map(|seed| {
            let mut scenario =
                Scenario::from_ron(&std::fs::read_to_string(root(file)).expect("scenario"))
                    .expect("parse");
            scenario.seed = *seed;
            let mut world = World::new(scenario).expect("world");
            world.place_founders(&assembled("genomes/ancestor.mm"), 16);
            world.run(20_000);
            world.cells().len()
        })
        .collect();
    eprintln!("ancestor: {ancestor:?}");

    for eye in [0u32, 8, 40] {
        for dusk in [32u32, 64, 128] {
            let source = base
                .replace("        IMM     40              ; photosensor", &format!("        IMM     {eye}              ; photosensor"))
                .replace("        IMM     128             ; DUSK", &format!("        IMM     {dusk}             ; DUSK"));
            let bytes = mm_asm::assemble(&source).expect("assemble").bytes;
            let pops: Vec<usize> = [1u64, 2, 3]
                .iter()
                .map(|seed| {
                    let mut scenario =
                        Scenario::from_ron(&std::fs::read_to_string(root(file)).expect("scenario"))
                            .expect("parse");
                    scenario.seed = *seed;
                    let mut world = World::new(scenario).expect("world");
                    world.place_founders(&bytes, 16);
                    world.run(20_000);
                    world.cells().len()
                })
                .collect();
            let wins = pops.iter().zip(&ancestor).filter(|(n, a)| n > a).count();
            eprintln!("  eye {eye:>2}, dusk {dusk:>3}: {pops:?}  wins {wins}/3");
        }
    }
}

/// Is there a night dark enough to be worth sleeping through?
///
/// `how_big_an_eye_and_how_dark` never found a win on `the_short_night`, whose night floor is 128
/// — an eighth of full daylight, and a chloroplast at an eighth of full light still out-earns the
/// 42 `Q10` a tick a param-60 one can save by shutting. So the question is about the world, not
/// the genome: hold everything else and darken the night.
#[test]
#[ignore = "a probe; run it on purpose"]
fn how_dark_a_night_has_to_be() {
    let base = std::fs::read_to_string(root("genomes/nightjar.mm")).expect("genome");
    let tuned = base
        .replace("        IMM     40              ; photosensor", "        IMM     0              ; photosensor")
        .replace("        IMM     128             ; DUSK", "        IMM     64             ; DUSK");
    let night_bytes = mm_asm::assemble(&tuned).expect("assemble").bytes;
    let anc_bytes = assembled("genomes/ancestor.mm");

    for night in [128i32, 64, 16, 0] {
        let mut pops = Vec::new();
        for seed in [1u64, 2] {
            let run = |bytes: &[u8]| {
                let mut scenario = Scenario::from_ron(
                    &std::fs::read_to_string(root("scenarios/the_short_night.ron")).expect("scenario"),
                )
                .expect("parse");
                scenario.seed = seed;
                scenario.light = mm_core::light::LightRegime::DayNight {
                    period_ticks: 2000,
                    day: 1024,
                    night,
                };
                let mut world = World::new(scenario).expect("world");
                world.place_founders(bytes, 16);
                world.run(20_000);
                world.cells().len()
            };
            pops.push((run(&anc_bytes), run(&night_bytes)));
        }
        let wins = pops.iter().filter(|(a, n)| n > a).count();
        eprintln!("night floor {night:>4}: {pops:?}  nightjar wins {wins}/2");
    }
}

/// Does a night gate pay if there is something worth switching off?
///
/// `how_dark_a_night_has_to_be` found darkening the night made the gate *worse*, which rules out
/// "the night is too bright" and points at the size of the prize. A param-60 chloroplast's whole
/// upkeep is 57 `Q10` a tick against a cell bill of about 450, so the gate was playing for nine
/// per cent of the bill and losing more than that in fixation across the cycle's shoulders. A
/// param-255 chloroplast has 153 `Q10` a tick to save — three and a half times as much.
#[test]
#[ignore = "a probe; run it on purpose"]
fn does_a_bigger_chloroplast_make_it_worth_switching_off() {
    let base = std::fs::read_to_string(root("genomes/nightjar.mm")).expect("genome");
    let anc = std::fs::read_to_string(root("genomes/ancestor.mm")).expect("genome");
    for chloroplast in [60u32, 160, 255] {
        // The control is the same body with the gate left out, so the comparison is the gate and
        // not the chloroplast — a bigger chloroplast changes the economics on its own.
        let control = anc.replace("        IMM     60
", &format!("        IMM     {chloroplast}
"));
        let gated = base
            .replace("        IMM     40              ; photosensor", "        IMM     0              ; photosensor")
            .replace("        IMM     128             ; DUSK", "        IMM     32             ; DUSK")
            .replace("        IMM     60
", &format!("        IMM     {chloroplast}
"));
        let (cb, gb) = (
            mm_asm::assemble(&control).expect("control").bytes,
            mm_asm::assemble(&gated).expect("gated").bytes,
        );
        let mut pops = Vec::new();
        for seed in [1u64, 2] {
            let run = |bytes: &[u8]| {
                let mut scenario = Scenario::from_ron(
                    &std::fs::read_to_string(root("scenarios/the_short_night.ron")).expect("scenario"),
                )
                .expect("parse");
                scenario.seed = seed;
                scenario.light = mm_core::light::LightRegime::DayNight {
                    period_ticks: 2000,
                    day: 1024,
                    night: 0,
                };
                let mut world = World::new(scenario).expect("world");
                world.place_founders(bytes, 16);
                world.run(20_000);
                world.cells().len()
            };
            pops.push((run(&cb), run(&gb)));
        }
        let wins = pops.iter().filter(|(c, g)| g > c).count();
        eprintln!("chloroplast {chloroplast:>3}: (ungated, gated) {pops:?}  gate wins {wins}/2");
    }
}
