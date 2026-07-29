//! Arena mode: coder against coder (M6, SPEC §0).
//!
//! > Fixed seed, mutation off, N cells per side, defined win conditions, reproducible match
//! > reports.
//!
//! # What makes a match a match
//!
//! Two people write genomes, seed them onto the same slide, and find out whose survives. That
//! is only a contest if it is **reproducible**: a match whose outcome moved between runs would
//! not be a result, it would be a coin toss with extra steps. So an arena match fixes
//! everything the open-ended half of the simulation deliberately leaves free —
//!
//! * one seed, recorded in the match report;
//! * mutation off, so a genome is the genome its author wrote and stays that way;
//! * a fixed starting layout, mirrored so neither side gets the better half of the slide;
//! * a tick limit, so a stalemate ends rather than running forever.
//!
//! Everything else is the same simulation. There is no arena-specific physics and no scoring
//! function inside the tick — the win conditions are read off the world afterwards, never fed
//! back into it. A fitness function anywhere in the simulation is the one thing this project
//! must never have (CLAUDE.md), and "it is only for arena mode" would be exactly how one got
//! in.
//!
//! # The report is the artefact
//!
//! [`MatchReport`] holds the seed, both genomes' hashes, the tick-by-tick population of each
//! side and the outcome. It is enough to replay the match exactly, which is M6's first
//! acceptance test, and enough to argue about afterwards, which is the point.

use crate::biology::BiologyConfig;
use crate::cell::{CellId, CellSeed};
use crate::fixed::{pos, q10};
use crate::light::CurrentField;
use crate::mutation::MutationRates;
use crate::organelle::{Organelle, OrganelleType};
use crate::phylogeny::SpeciesId;
use crate::scenario::{Scenario, Seeding};
use crate::world::World;
use crate::LightRegime;

/// How a match ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// One side had cells and the other did not.
    Elimination { winner: Side, at_tick: u64 },
    /// The tick limit ran out. Decided on population, which is the only measure both sides
    /// were playing for.
    OnPopulation { winner: Side, left: u32, right: u32 },
    /// The limit ran out with both sides equal, or both sides dead.
    Draw { left: u32, right: u32 },
}

impl Outcome {
    #[must_use]
    pub fn winner(&self) -> Option<Side> {
        match self {
            Outcome::Elimination { winner, .. } | Outcome::OnPopulation { winner, .. } => {
                Some(*winner)
            }
            Outcome::Draw { .. } => None,
        }
    }
}

/// Which competitor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

/// One competitor's entry.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub name: String,
    pub genome: Vec<u8>,
}

impl Entry {
    #[must_use]
    pub fn new(name: impl Into<String>, genome: Vec<u8>) -> Entry {
        Entry {
            name: name.into(),
            genome,
        }
    }
}

/// The rules of a match. Everything here is recorded in the report, so a match can be rebuilt
/// from it exactly.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MatchRules {
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    /// Cells per side at the start.
    pub cells_per_side: u32,
    pub tick_limit: u64,
    /// Ticks between population samples in the report.
    pub sample_every: u64,
    /// Starting energy and mass, identical for both sides.
    pub start_energy: i32,
    pub start_mass: i32,
}

impl Default for MatchRules {
    fn default() -> MatchRules {
        MatchRules {
            seed: 1,
            width: 64,
            height: 64,
            cells_per_side: 8,
            tick_limit: 20_000,
            sample_every: 500,
            start_energy: q10(400),
            start_mass: q10(30),
        }
    }
}

/// A population reading at one tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Standing {
    pub tick: u64,
    pub left: u32,
    pub right: u32,
}

/// Everything a match produced.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MatchReport {
    pub rules: MatchRules,
    pub left: Entry,
    pub right: Entry,
    /// Content hashes of both genomes, so a report cannot be paired with the wrong source.
    pub left_hash: u64,
    pub right_hash: u64,
    pub outcome: Outcome,
    pub standings: Vec<Standing>,
    pub ended_at: u64,
    /// The world's state hash when the match ended. Two runs of one match must agree on it,
    /// which is a stronger claim than agreeing on who won.
    pub final_hash: u64,
    /// Cells finishing the match on a genome that is not the one their side entered, because
    /// they ran short of energy mid-copy. See [`side_of`].
    pub copy_damaged: u32,
}

impl MatchReport {
    /// A human-readable summary.
    #[must_use]
    pub fn summary(&self) -> String {
        let describe = |side: Side| match side {
            Side::Left => &self.left.name,
            Side::Right => &self.right.name,
        };
        let headline = match self.outcome {
            Outcome::Elimination { winner, at_tick } => format!(
                "{} eliminated {} at tick {at_tick}",
                describe(winner),
                describe(match winner {
                    Side::Left => Side::Right,
                    Side::Right => Side::Left,
                })
            ),
            Outcome::OnPopulation {
                winner,
                left,
                right,
            } => {
                // The winner's count first. Printing them in seating order made a mirror
                // match read "ancestor wins ... 1262 to 1522", which invites the reader to
                // think the smaller number won.
                let (mine, theirs) = match winner {
                    Side::Left => (left, right),
                    Side::Right => (right, left),
                };
                format!(
                    "{} wins on population at the limit, {mine} to {theirs}",
                    describe(winner)
                )
            }
            Outcome::Draw { left, right } => {
                format!("draw at the limit, {left} to {right}")
            }
        };
        format!(
            "{} vs {} — {headline}\n  seed {}, {} ticks, genomes {:016x} and {:016x}\n  \
             final state hash {:016x}",
            self.left.name,
            self.right.name,
            self.rules.seed,
            self.ended_at,
            self.left_hash,
            self.right_hash,
            self.final_hash
        )
    }
}

/// Why a match could not be set up.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ArenaError {
    Scenario(String),
    Genome(String),
}

impl std::fmt::Display for ArenaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArenaError::Scenario(e) => write!(f, "arena scenario: {e}"),
            ArenaError::Genome(e) => write!(f, "arena genome: {e}"),
        }
    }
}

impl std::error::Error for ArenaError {}

/// The slide a match is played on.
///
/// Uniform light, still water, evenly seeded food: no feature of the terrain favours either
/// side, because a match should be decided by the genomes.
#[must_use]
pub fn arena_scenario(rules: &MatchRules) -> Scenario {
    Scenario {
        name: "arena".to_string(),
        seed: rules.seed,
        width: rules.width,
        height: rules.height,
        light: LightRegime::Uniform {
            intensity: crate::Q10_ONE,
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

/// Set a match up without running it, for the debugger and the viewer.
///
/// # Errors
///
/// A scenario this engine cannot honour, or a genome it will not intern.
pub fn setup(rules: &MatchRules, left: &Entry, right: &Entry) -> Result<World, ArenaError> {
    let mut world =
        World::new(arena_scenario(rules)).map_err(|e| ArenaError::Scenario(e.to_string()))?;

    // Mutation off. A match is between two genomes, and a genome that drifts mid-match is no
    // longer the one its author entered.
    world.set_biology(BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    });

    // Each side gets its own root species, founded once and then shared by that side's cells.
    //
    // The archive merges identical seedings by design (M5): twelve founders of one ancestor
    // are one species, not twelve rivals. That is right for a slide and wrong for a match —
    // the first version of this used the merging path and every mirror match ended 12-0 at
    // tick one, because both sides resolved to the same root and one was always checked first.
    let mut roots: Vec<(Side, SpeciesId)> = Vec::new();
    for k in 0..rules.cells_per_side {
        for side in [Side::Left, Side::Right] {
            let entry = match side {
                Side::Left => left,
                Side::Right => right,
            };
            let genome = world
                .genomes()
                .intern(entry.genome.clone())
                .map_err(|e| ArenaError::Genome(e.to_string()))?;
            let (x, y) = start_position(rules, side, k);
            let seed = CellSeed {
                x: pos(x),
                y: pos(y),
                mass: rules.start_mass,
                energy: rules.start_energy,
                membrane: 24,
                key: 11,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome,
            };
            let existing = roots.iter().find(|(s, _)| *s == side).map(|(_, id)| *id);
            let id = match existing {
                // This side already has a root; join it rather than founding another.
                Some(root) => {
                    let id = world.spawn_cell(seed);
                    if let Some(i) = world.cells_mut().index(id) {
                        world.cells_mut().species[i] = root;
                    }
                    id
                }
                None => {
                    let id = world.spawn_cell_as_new_species(seed);
                    if let Some(i) = world.cells().index(id) {
                        roots.push((side, world.cells().species[i]));
                    }
                    id
                }
            };
            if let Some(i) = world.cells_mut().index(id) {
                let cells = world.cells_mut();
                // Both sides start with the same body, so the match is about the code.
                cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
                cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
                cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
                cells.interior_mut(i)[11] = q10(40);
                cells.interior_mut(i)[14] = q10(40);
            }
        }
    }
    world.adopt_current_contents_as_baseline();
    Ok(world)
}

/// Where a side's `k`th cell starts.
///
/// Mirrored across the vertical midline: left starts on the left, right on the right, at
/// matching heights. Neither side gets more room, more light or more food, and swapping the
/// two entries produces the mirror image of the same match rather than a different one.
fn start_position(rules: &MatchRules, side: Side, k: u32) -> (i32, i32) {
    let margin = (rules.width / 8).max(2) as i32;
    let rows = rules.cells_per_side.max(1);
    let spacing = (rules.height as i32 - 2 * margin).max(1) / rows.max(1) as i32;
    let y = margin + spacing * k as i32 + spacing / 2;
    let x = match side {
        Side::Left => margin,
        Side::Right => rules.width as i32 - 1 - margin,
    };
    (x, y.min(rules.height as i32 - 1))
}

/// Which side a cell belongs to, by descent.
///
/// # Why not by the genome it is running
///
/// Because with mutation entirely off, a genome still changes. `COPYB` charges energy per
/// byte, and a cell that cannot pay skips the byte — the daughter keeps a zero there while the
/// genome's own loop counter moves on regardless. The first version of this test found 68 cells
/// running two genomes neither competitor entered, all 230 bytes long, all descended from
/// cells that ran short mid-copy.
///
/// That is not a bug. Accuracy costs energy and a cell that cannot afford it copies badly,
/// which is the same mechanism that makes fidelity an evolvable trait (SPEC §9). But it does
/// mean "mutation off" is not "genomes are frozen", and anything that identifies a competitor
/// by its genome bytes will lose cells as a match goes on.
///
/// Descent is exact and cheap: each entry founds a root species (M5), and every descendant
/// chains to it however damaged its genome has become.
fn side_of(world: &World, i: usize, left_root: SpeciesId, right_root: SpeciesId) -> Option<Side> {
    let root = *world.archive().ancestry(world.cells().species[i]).last()?;
    if root == left_root {
        Some(Side::Left)
    } else if root == right_root {
        Some(Side::Right)
    } else {
        None
    }
}

/// Which root species each side's founders belong to.
///
/// Read off the world rather than assumed, because the archive assigns ids in seeding order
/// and a caller that hard-coded 0 and 1 would be relying on an implementation detail.
#[must_use]
pub fn roots(world: &World, left: &Entry, right: &Entry) -> (SpeciesId, SpeciesId) {
    // By seeding order, not by genome. `setup` seeds the left side first, so the two lowest
    // root ids are left then right — and when both sides entered the *same* genome, matching
    // on bytes would give both of them the first one.
    let mut found: Vec<SpeciesId> = world
        .archive()
        .iter()
        .filter(|s| s.parent.is_none())
        .map(|s| s.id)
        .collect();
    found.sort_unstable();
    let _ = (left, right);
    (
        found.first().copied().unwrap_or(u32::MAX),
        found.get(1).copied().unwrap_or(u32::MAX),
    )
}

/// Count each side's living cells, and how many are no longer running the genome that was
/// entered.
#[must_use]
pub fn standing(world: &World, left_root: SpeciesId, right_root: SpeciesId) -> (u32, u32) {
    let cells = world.cells();
    let mut left = 0;
    let mut right = 0;
    for i in cells.iter() {
        match side_of(world, i, left_root, right_root) {
            Some(Side::Left) => left += 1,
            Some(Side::Right) => right += 1,
            None => {}
        }
    }
    (left, right)
}

/// Living cells whose genome is not byte-identical to the one their side entered.
///
/// Real information about a match rather than an error: it says how hard the slide was on the
/// competitors' replication, and a match where most of one side is copy-damaged was not really
/// won by the genome its author wrote.
#[must_use]
pub fn copy_damaged(world: &World, left: &Entry, right: &Entry) -> u32 {
    let (lh, rh) = (
        crate::genome::content_hash(&left.genome),
        crate::genome::content_hash(&right.genome),
    );
    let cells = world.cells();
    cells
        .iter()
        .filter(|i| {
            let h = cells.genome[*i].hash();
            h != lh && h != rh
        })
        .count() as u32
}

/// Play a match and report what happened.
///
/// # Errors
///
/// A scenario this engine cannot honour, or a genome it will not intern.
pub fn play(rules: &MatchRules, left: &Entry, right: &Entry) -> Result<MatchReport, ArenaError> {
    let mut world = setup(rules, left, right)?;
    let left_hash = crate::genome::content_hash(&left.genome);
    let right_hash = crate::genome::content_hash(&right.genome);
    let (left_root, right_root) = roots(&world, left, right);

    let mut standings = Vec::new();
    let mut outcome = None;
    let mut ended_at = 0;

    let sample_every = rules.sample_every.max(1);
    for tick in 0..rules.tick_limit {
        world.step();
        ended_at = tick + 1;

        // Elimination is checked every tick, not on the sample interval: a side that died at
        // tick 1,001 should be reported as dying at 1,001, and a match that continued past
        // its own end would keep simulating something already decided.
        let (l, r) = standing(&world, left_root, right_root);
        if ended_at % sample_every == 0 {
            standings.push(Standing {
                tick: ended_at,
                left: l,
                right: r,
            });
        }
        if l == 0 || r == 0 {
            // Both dying on the same tick is a draw, not a win for whoever is checked first.
            outcome = Some(if l == r {
                Outcome::Draw { left: l, right: r }
            } else {
                Outcome::Elimination {
                    winner: if r == 0 { Side::Left } else { Side::Right },
                    at_tick: ended_at,
                }
            });
            break;
        }
    }

    let (l, r) = standing(&world, left_root, right_root);
    if standings.last().map(|s| s.tick) != Some(ended_at) {
        standings.push(Standing {
            tick: ended_at,
            left: l,
            right: r,
        });
    }
    let outcome = outcome.unwrap_or(match l.cmp(&r) {
        std::cmp::Ordering::Greater => Outcome::OnPopulation {
            winner: Side::Left,
            left: l,
            right: r,
        },
        std::cmp::Ordering::Less => Outcome::OnPopulation {
            winner: Side::Right,
            left: l,
            right: r,
        },
        std::cmp::Ordering::Equal => Outcome::Draw { left: l, right: r },
    });

    Ok(MatchReport {
        rules: rules.clone(),
        left: left.clone(),
        right: right.clone(),
        left_hash,
        right_hash,
        outcome,
        standings,
        ended_at,
        final_hash: world.state_hash(),
        copy_damaged: copy_damaged(&world, left, right),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assemble(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../genomes")
            .join(name);
        let src = std::fs::read_to_string(&path).expect("genome file");
        mm_asm::assemble(&src).expect("assembles").bytes
    }

    fn quick() -> MatchRules {
        MatchRules {
            tick_limit: 400,
            sample_every: 100,
            cells_per_side: 4,
            width: 48,
            height: 48,
            ..MatchRules::default()
        }
    }

    #[test]
    fn a_match_replays_identically() {
        // M6 acceptance 1, in miniature. The long version is in `tests/m6_arena.rs`.
        let left = Entry::new("tidy", assemble("ancestor.mm"));
        let right = Entry::new("sloppy", assemble("ancestor_sloppy.mm"));
        let a = play(&quick(), &left, &right).expect("match");
        let b = play(&quick(), &left, &right).expect("match");
        assert_eq!(a.final_hash, b.final_hash);
        assert_eq!(a.outcome, b.outcome);
        assert_eq!(a.standings, b.standings);
    }

    #[test]
    fn the_seed_is_what_makes_a_match_that_match() {
        let left = Entry::new("tidy", assemble("ancestor.mm"));
        let right = Entry::new("sloppy", assemble("ancestor_sloppy.mm"));
        let a = play(&quick(), &left, &right).expect("match");
        let other = MatchRules {
            seed: 999,
            ..quick()
        };
        let b = play(&other, &left, &right).expect("match");
        assert_ne!(
            a.final_hash, b.final_hash,
            "changing the seed changed nothing; the match is not seeded at all"
        );
    }

    #[test]
    fn both_sides_start_with_the_same_advantages() {
        let rules = quick();
        for k in 0..rules.cells_per_side {
            let (lx, ly) = start_position(&rules, Side::Left, k);
            let (rx, ry) = start_position(&rules, Side::Right, k);
            assert_eq!(ly, ry, "the two sides start at different heights");
            // Mirrored across the midline: equally far from their own edge.
            assert_eq!(lx, rules.width as i32 - 1 - rx);
            assert!(lx >= 0 && rx < rules.width as i32);
        }
    }

    #[test]
    fn mutation_off_stops_the_operators_but_not_copy_damage() {
        // This test started life asserting that with mutation off every cell runs one of the
        // two entered genomes, and it failed: 68 cells were running two genomes neither
        // competitor entered, all the right length.
        //
        // The cause is not mutation. `COPYB` charges energy per byte and skips the byte when
        // the cell cannot pay, while the genome's own loop counter moves on — so a cell that
        // runs short mid-copy produces a daughter with zeros where bytes should be. Correct
        // physics, and the same mechanism that makes fidelity an evolvable trait, but it means
        // a match cannot identify its competitors by genome bytes.
        //
        // What is asserted now is the thing that actually has to hold: no structural mutation,
        // every cell still attributable to a side, and the damage counted rather than hidden.
        let left = Entry::new("tidy", assemble("ancestor.mm"));
        let right = Entry::new("sloppy", assemble("ancestor_sloppy.mm"));
        let mut world = setup(&quick(), &left, &right).expect("setup");
        let (lr, rr) = roots(&world, &left, &right);
        world.run(400);

        for i in world.cells().iter() {
            assert!(
                side_of(&world, i, lr, rr).is_some(),
                "a cell belongs to neither side; descent is not being tracked"
            );
            // No structural operator fired, so lengths never move.
            assert_eq!(
                world.cells().genome[i].len(),
                left.genome.len(),
                "a genome changed length, so a structural operator fired"
            );
        }

        let damaged = copy_damaged(&world, &left, &right);
        eprintln!(
            "{} of {} cells finished on a copy-damaged genome",
            damaged,
            world.cells().len()
        );
    }

    #[test]
    fn a_damaged_daughter_still_counts_for_its_side() {
        // The property the fix rests on: descent survives a genome that no longer matches.
        let left = Entry::new("tidy", assemble("ancestor.mm"));
        let right = Entry::new("sloppy", assemble("ancestor_sloppy.mm"));
        let mut world = setup(&quick(), &left, &right).expect("setup");
        let (lr, rr) = roots(&world, &left, &right);
        assert_ne!(lr, u32::MAX, "the left entry founded no species");
        assert_ne!(rr, u32::MAX, "the right entry founded no species");
        assert_ne!(lr, rr, "both entries were put in one species");
        world.run(400);
        let (l, r) = standing(&world, lr, rr);
        assert_eq!(
            l + r,
            world.cells().len() as u32,
            "cells went missing from the standings"
        );
        assert!(l > 0 && r > 0, "a side vanished in 400 ticks: {l} and {r}");
    }

    #[test]
    fn a_side_wiped_out_ends_the_match() {
        // The empty genome does nothing at all, so it cannot divide and its cells age out.
        let left = Entry::new("real", assemble("ancestor.mm"));
        let right = Entry::new("inert", vec![0x2E; 8]);
        let rules = MatchRules {
            tick_limit: 20_000,
            ..quick()
        };
        let report = play(&rules, &left, &right).expect("match");
        match report.outcome {
            Outcome::Elimination { winner, at_tick } => {
                assert_eq!(winner, Side::Left);
                assert!(at_tick <= report.rules.tick_limit);
                assert_eq!(report.ended_at, at_tick, "the match ran on past its end");
            }
            other => panic!("expected an elimination, got {other:?}"),
        }
    }

    #[test]
    fn a_report_says_enough_to_rebuild_the_match() {
        let left = Entry::new("tidy", assemble("ancestor.mm"));
        let right = Entry::new("sloppy", assemble("ancestor_sloppy.mm"));
        let report = play(&quick(), &left, &right).expect("match");
        // Everything needed to play it again is in the report.
        let replay = play(&report.rules, &report.left, &report.right).expect("match");
        assert_eq!(replay.final_hash, report.final_hash);

        let text = report.summary();
        assert!(text.contains("tidy"), "{text}");
        assert!(text.contains("sloppy"), "{text}");
        assert!(
            text.contains(&format!("{:016x}", report.left_hash)),
            "{text}"
        );
    }

    #[test]
    fn a_report_counts_copy_damage_rather_than_hiding_it() {
        let left = Entry::new("tidy", assemble("ancestor.mm"));
        let right = Entry::new("sloppy", assemble("ancestor_sloppy.mm"));
        let report = play(&quick(), &left, &right).expect("match");
        // Whatever the number is, it is reported and it is not larger than the population.
        let total = report.standings.last().map_or(0, |s| s.left + s.right);
        assert!(
            report.copy_damaged <= total,
            "{} damaged of {total} alive",
            report.copy_damaged
        );
    }
}
