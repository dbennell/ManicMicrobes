//! What a tick costs down each route evolution might actually take.
//!
//! # Why this exists
//!
//! `population.rs` measures two grown worlds and a composition bound, and every conclusion drawn
//! from it inherits their shape. That is not a small caveat. On the autotroph slide the junction
//! array is 0.0% occupied and on the mixed slide it is 18.6%; photosensors are 1.8% of the mixed
//! population and 0% of the autotroph one — and the cost of the glow scan is linear in exactly
//! that number. A phase that is invisible on both of the worlds we measure can be the whole tick
//! on a world we do not.
//!
//! So this is a panel of *routes*: named populations standing for outcomes the physics could
//! plausibly select for, each one built to make a different phase expensive. It answers "what
//! does a tick cost if evolution goes this way", which is the question a performance gate on an
//! open-ended simulator actually has to answer.
//!
//! # Why the slide is 256 and not 512
//!
//! Because that is what the product runs. `mm_app::slide_size` defaults to 256 and clamps to
//! 1024; the 512-square slide in `population.rs` exists so fifty thousand cells are a density the
//! simulation produces rather than a number forced onto a small world. Both are worth measuring
//! and they are not the same measurement — this one is the one a user is looking at.
//!
//! # Mutation is off
//!
//! For the reason `population.rs::MIXED` gives: a selecting population converges on the
//! autotroph and the composition is gone long before the target is reached. A route is a
//! hypothesis about what evolution produced, so the run has to preserve it rather than re-run
//! the selection that would destroy it. That makes each row a *bound* on its route, not an
//! ecology — the honest reading is the same one `MIXED` asks for.

use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use mm_core::biology::BiologyConfig;
use mm_core::cell::{CellId, CellSeed};
use mm_core::fixed::{pos, q10};
use mm_core::light::CurrentField;
use mm_core::{LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding, World};

/// The product's default slide.
const SIDE: u32 = 256;
/// Where growth stops. Chosen so every route reaches it on a 256-square slide rather than
/// stalling at its own carrying capacity, which would make the rows incomparable.
const TARGET: usize = 12_000;
/// The most growth any route gets before it is reported at whatever it reached.
const GROW_TICKS: u64 = 40_000;
/// Bumped whenever a route's membership or world changes, so a stale cache cannot be mistaken
/// for the population the current definition would grow.
const CACHE_V: u32 = 2;

/// A route, and the cells that make it up.
///
/// Each entry names the phase it is here to make expensive. An entry that cannot name one is a
/// duplicate of another and should not be added — the same rule `balance::PanelEntry::poses`
/// applies to worlds.
struct Route {
    label: &'static str,
    /// Genome file and how many founders of it.
    members: &'static [(&'static str, u32)],
    /// A light regime to put this route in, when a uniformly lit buffet would not sustain it.
    ///
    /// A route is a population *and* the world that makes its strategy payable. The first
    /// version of this panel put every route on the same uniformly lit, uniformly seeded,
    /// still slide, and two of the seven died on it — the swimmers collapsed to 17 cells and
    /// the phototrophs went extinct outright. Neither was an economy result. A chemotaxis
    /// genome on a slide with no gradient is paying for a cilium and buying nothing, and a
    /// hunter seeded as a monoculture has nothing to hunt. The world was wrong, not the cell.
    light: Option<LightRegime>,
    /// What this route puts under load that the others do not.
    stresses: &'static str,
}

const ROUTES: &[Route] = &[
    Route {
        label: "autotroph",
        members: &[("ancestor.mm", 64)],
        light: None,
        stresses: "the floor: photosynthesis, no motility, no ecology",
    },
    Route {
        label: "swimmers",
        members: &[("drifter.mm", 32), ("ancestor.mm", 32)],
        // A gradient to climb, so a cilium can earn its upkeep. Directional light is the
        // regime `light.rs` says phototaxis has a reason to evolve in.
        light: Some(LightRegime::Directional {
            bright: mm_core::Q10_ONE,
            dark: mm_core::Q10_ONE / 8,
            from: mm_core::light::Edge::Left,
        }),
        stresses: "physics and the velocity field — every cell beats a cilium",
    },
    Route {
        label: "predators",
        // Far more prey than predators. At 24 against 40 the prey simply outbred the predators
        // and the row settled at 6% lysosome, which is not a predator route.
        members: &[("predator.mm", 8), ("ancestor.mm", 56)],
        light: None,
        stresses: "the ecology phase: spikes wound, lysosomes digest",
    },
    Route {
        label: "colonial",
        members: &[("reflex.mm", 40), ("parasite.mm", 24)],
        light: None,
        stresses: "the junction solver and the components rebuild",
    },
    Route {
        label: "phototrophs",
        // Prey to see and to eat. A stalker monoculture starves: it hunts by the glow of other
        // cells and a slide of nothing but stalkers is a slide of nothing worth catching.
        members: &[("stalker.mm", 8), ("ancestor.mm", 56)],
        light: None,
        stresses: "the glow scan — the one phase whose cost is linear in how common a sensor is",
    },
    Route {
        label: "grazers",
        members: &[("scavenger.mm", 32), ("hoarder.mm", 32)],
        light: None,
        stresses: "carrion digestion and vacuole storage",
    },
    Route {
        label: "mixed",
        members: &[
            ("ancestor.mm", 12),
            ("drifter.mm", 8),
            ("stalker.mm", 8),
            ("sponge.mm", 8),
            ("predator.mm", 8),
            ("scavenger.mm", 6),
            ("reflex.mm", 4),
            ("oscillator.mm", 4),
            ("hoarder.mm", 3),
            ("parasite.mm", 3),
        ],
        light: None,
        stresses: "everything at once — the bound, as `population::MIXED` is at 512",
    },
];

fn assemble(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = std::fs::read_to_string(&path).expect("genome file");
    mm_asm::assemble(&src).expect("it assembles").bytes
}

fn slide(seed: u64) -> Scenario {
    Scenario {
        name: "route".to_string(),
        seed,
        width: SIDE,
        height: SIDE,
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

fn cache_path(label: &str) -> std::path::PathBuf {
    let root = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../target").to_string());
    std::path::PathBuf::from(root)
        .join("bench-cache")
        .join(format!("route-{label}-{SIDE}-{TARGET}-v{CACHE_V}.mmslide"))
}

/// Grow a route, from cache when there is one. See `population::grown` for why the cache is not
/// an approximation of the grown world but the grown world itself.
fn grown(route: &Route) -> Option<World> {
    let path = cache_path(route.label);
    if std::env::var("MM_BENCH_REGROW").is_err() {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(world) = mm_core::Snapshot::read(&bytes) {
                eprintln!("  {}: {} cells from cache", route.label, world.cells().len());
                return Some(world);
            }
        }
    }
    let world = grow(route)?;
    if let Ok(bytes) = mm_core::Snapshot::write(&world) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, bytes);
    }
    Some(world)
}

fn grow(route: &Route) -> Option<World> {
    let scenario = match route.light.clone() {
        Some(light) => Scenario { light, ..slide(1) },
        None => slide(1),
    };
    let mut world = World::new(scenario).expect("world");
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });
    let mut k = 0u32;
    for (file, count) in route.members {
        let bytes = assemble(file);
        for _ in 0..*count {
            let genome = world.genomes().intern(bytes.clone()).expect("interned");
            let id = world.spawn_cell(CellSeed {
                x: pos((16 + (k % 8) * 28) as i32),
                y: pos((16 + (k / 8) * 28) as i32),
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
                let cells = world.cells_mut();
                cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
                cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
                cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
                cells.interior_mut(i)[11] = q10(40);
                cells.interior_mut(i)[14] = q10(40);
            }
            k += 1;
        }
    }
    world.adopt_current_contents_as_baseline();
    for _ in 0..(GROW_TICKS / 25) {
        world.run(25);
        let n = world.cells().len();
        if n >= TARGET {
            return Some(world);
        }
        if n == 0 {
            eprintln!("  {}: went extinct while growing", route.label);
            return None;
        }
    }
    eprintln!(
        "  {}: only reached {} cells, not {TARGET}",
        route.label,
        world.cells().len()
    );
    Some(world)
}

/// How common each organelle is in a grown route, as a percentage of the population.
///
/// Reported because it is what makes a row interpretable. A route labelled "phototrophs" whose
/// photosensor prevalence came out at 2% is measuring the same thing the autotroph row is, and
/// without this column that would not be visible.
fn composition(world: &World) -> Vec<(OrganelleType, f64)> {
    let cells = world.cells();
    let n = cells.len().max(1) as f64;
    let mut counts = [0u32; mm_core::organelle::SLOT_COUNT];
    for i in cells.iter() {
        let mut seen = [false; mm_core::organelle::SLOT_COUNT];
        for o in cells.slots(i) {
            let k = o.kind as usize;
            if k < seen.len() && !seen[k] {
                seen[k] = true;
                counts[k] += 1;
            }
        }
    }
    let mut out: Vec<(OrganelleType, f64)> = OrganelleType::all()
        .iter()
        .filter(|k| !matches!(k, OrganelleType::Empty))
        .map(|k| (*k, f64::from(counts[*k as usize]) / n * 100.0))
        .filter(|(_, pct)| *pct >= 1.0)
        .collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// The panel. Reported rather than asserted, like the other gates here — a route that is slow is
/// a finding about the physics, not a broken build.
fn panel(_c: &mut Criterion) {
    if cfg!(debug_assertions) {
        return;
    }
    mm_core::threads::use_performance_cores();
    let threads = rayon::current_num_threads();
    eprintln!("\nRoute panel — {SIDE}x{SIDE}, mutation off, {threads} threads:");

    // The same slide with nothing on it, so each row can say how much of its tick was the cells.
    let mut empty = World::new(slide(1)).expect("world");
    empty.run(20);
    let n = 200;
    let t = Instant::now();
    empty.run(n);
    let bare = t.elapsed().as_secs_f64() / n as f64;
    eprintln!("  bare slide, no cells: {:.2} ms/tick", bare * 1000.0);

    let only = std::env::var("MM_BENCH_ROUTES").unwrap_or_default();
    eprintln!("\n  {:<12} {:>7} {:>10} {:>10} {:>8}   composition", "route", "cells", "ms/tick", "ticks/s", "cells%");
    for route in ROUTES {
        if !only.is_empty() && !only.split(',').any(|r| r.trim() == route.label) {
            continue;
        }
        let Some(mut world) = grown(route) else { continue };
        let population = world.cells().len();
        world.run(20);
        let n = 120;
        let t = Instant::now();
        world.run(n);
        let per = t.elapsed().as_secs_f64() / n as f64;
        let share = (per - bare).max(0.0) / per * 100.0;
        let comp: Vec<String> = composition(&world)
            .iter()
            .take(5)
            .map(|(k, pct)| format!("{k:?} {pct:.0}%"))
            .collect();
        eprintln!(
            "  {:<12} {population:>7} {:>10.2} {:>10.1} {share:>7.0}%   {}",
            route.label,
            per * 1000.0,
            1.0 / per,
            comp.join(", ")
        );
    }
    eprintln!("\n  what each route is for:");
    for route in ROUTES {
        eprintln!("    {:<12} {}", route.label, route.stresses);
    }
}

criterion_group!(benches, panel);
criterion_main!(benches);
