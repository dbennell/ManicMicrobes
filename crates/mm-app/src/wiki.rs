//! The wiki, the tree and the timeline, as data the UI draws (M5, SPEC §10.5).
//!
//! Same shape as [`crate::slide`] and for the same reason: everything here is copied out of
//! the world, so the panel that shows it holds no borrow and cannot write back through one.
//! Reading about a species is looking, and looking is free.
//!
//! Laying the tree out is the only real work. It is done here rather than in the renderer
//! because "which row does this species sit on" is a decision with a right answer, and a
//! decision with a right answer wants a test — which it can have here, on a machine with no
//! display, and could not have inside a Bevy system.

use mm_core::events::{Event, Occurrence};
use mm_core::phylogeny::{Phylogeny, SpeciesId};

/// How a species earns its living, as far as a colour can say.
///
/// Read off the founder's organelles, the same three questions [`mm_core::ecology::TrophicMix`]
/// asks of a live population — and, like it, **not exclusive**. A cell carrying a chloroplast
/// and a spike is both, a mixotroph is a real thing, and forcing every species into one box
/// would be inventing the cell-type enum by the back door for the sake of a swatch. So this is
/// a set, and the tree blends the colours of whatever is in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Guild {
    /// Carries a chloroplast: makes its own food.
    pub producer: bool,
    /// Carries a spike: the machinery of predation.
    pub predator: bool,
    /// Carries a lysosome: the machinery of scavenging.
    pub scavenger: bool,
}

impl Guild {
    /// Read the guilds off a founder's loadout.
    #[must_use]
    pub fn of(traits: &mm_core::names::Traits) -> Guild {
        use mm_core::OrganelleType;
        let has = |k: OrganelleType| traits.counts.get(k as usize).copied().unwrap_or(0) > 0;
        Guild {
            producer: has(OrganelleType::Chloroplast),
            predator: has(OrganelleType::Spike),
            scavenger: has(OrganelleType::Lysosome),
        }
    }

    /// Nothing that names a way of making a living: it lives on what is dissolved in the water.
    #[must_use]
    pub fn is_osmotroph(&self) -> bool {
        !self.producer && !self.predator && !self.scavenger
    }

    /// The colour to draw it in, `0..=1` per channel.
    ///
    /// The mean of whatever guilds it belongs to, so a producer that also hunts comes out
    /// between green and red rather than being rounded to whichever the code checked first.
    /// The same four colours the metrics and the food web use, so one glance transfers.
    #[must_use]
    pub fn rgb(&self) -> [f32; 3] {
        let mut sum = [0.0f32; 3];
        let mut n = 0.0f32;
        for (member, rgb) in [
            (self.producer, [0.42f32, 0.78, 0.42]),
            (self.predator, [0.85, 0.38, 0.34]),
            (self.scavenger, [0.90, 0.70, 0.30]),
        ] {
            if member {
                for k in 0..3 {
                    sum[k] += rgb[k];
                }
                n += 1.0;
            }
        }
        if n == 0.0 {
            // Osmotroph: living on what is dissolved, and drawn the colour of the water.
            return [0.45, 0.62, 0.80];
        }
        [sum[0] / n, sum[1] / n, sum[2] / n]
    }

    /// What to call it in a tooltip.
    #[must_use]
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.producer {
            parts.push("producer");
        }
        if self.predator {
            parts.push("predator");
        }
        if self.scavenger {
            parts.push("scavenger");
        }
        if parts.is_empty() {
            return "osmotroph".to_string();
        }
        parts.join(" + ")
    }
}

/// One species, laid out for the tree view.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TreeNode {
    pub id: SpeciesId,
    pub parent: Option<SpeciesId>,
    pub name: String,
    /// Depth from the root: the column, if the tree is drawn left to right.
    pub depth: u32,
    /// Row, assigned so that no two nodes share one and children sit under their parent.
    pub row: u32,
    pub founded_tick: u64,
    pub extinct_tick: Option<u64>,
    pub population: u32,
    pub peak_population: u32,
    pub alive: bool,
    /// How it earns its living, for the branch's colour.
    pub guild: Guild,
}

/// The phylogenetic tree, ready to draw.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<TreeNode>,
    pub rows: u32,
    pub max_depth: u32,
}

/// Lay out the species tree.
///
/// Depth-first from each root, in id order, assigning each node the next free row as it is
/// reached. That puts a species directly under the parent it diverged from and keeps a whole
/// subtree contiguous, which is what makes a tree readable — a breadth-first layout would
/// interleave unrelated branches on adjacent rows and the eye could not follow a lineage.
#[must_use]
pub fn layout(archive: &Phylogeny) -> Tree {
    let mut nodes: Vec<TreeNode> = Vec::with_capacity(archive.len());
    let mut row = 0u32;
    let mut max_depth = 0u32;

    // Iterative rather than recursive: a long thin lineage is exactly the shape a run
    // produces, and a few thousand deep would blow the stack on a recursive walk.
    let roots: Vec<SpeciesId> = archive
        .iter()
        .filter(|s| s.parent.is_none())
        .map(|s| s.id)
        .collect();
    let mut stack: Vec<SpeciesId> = roots.into_iter().rev().collect();

    while let Some(id) = stack.pop() {
        let Some(s) = archive.get(id) else {
            continue;
        };
        max_depth = max_depth.max(s.depth);
        nodes.push(TreeNode {
            id: s.id,
            parent: s.parent,
            name: s.name.full(),
            depth: s.depth,
            row,
            founded_tick: s.founded_tick,
            extinct_tick: s.extinct_tick,
            population: s.population,
            peak_population: s.peak_population,
            alive: s.population > 0,
            guild: Guild::of(&s.traits),
        });
        row += 1;
        // Reversed so that children come off the stack in ascending id order, which is
        // founding order — the tree reads top to bottom in the order things happened.
        for child in s.children.iter().rev() {
            stack.push(*child);
        }
    }

    Tree {
        nodes,
        rows: row,
        max_depth,
    }
}

/// One species drawn as a branch, in `0..=1` space (M10.4).
///
/// The tree was laid out as rows and depths since M5 and drawn as an indented text list, which
/// is a footnote about a tree rather than a tree. This turns the layout into geometry so the
/// renderer only has to paint it — and so the decisions in it, which are the part that can be
/// wrong, are testable without a window.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Branch {
    pub id: SpeciesId,
    /// Where the lineage began and ended, across the pane. `x1` is the present for a species
    /// still alive, so living branches all reach the right-hand edge and extinct ones stop
    /// where they stopped.
    pub x0: f32,
    pub x1: f32,
    /// Its row, down the pane.
    pub y: f32,
    /// `0..=1`, from peak population against the largest peak in the tree. The renderer turns
    /// it into a stroke width; what it means is "how much of the world was ever this".
    pub weight: f32,
    pub alive: bool,
}

/// The line from a species to the parent it diverged from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Fork {
    /// The divergence: the child's founding tick, at the parent's row.
    pub x: f32,
    pub y_parent: f32,
    pub y_child: f32,
}

/// The tree as something to paint.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Plot {
    pub branches: Vec<Branch>,
    pub forks: Vec<Fork>,
    /// How many rows the whole tree occupies, so the caller can size the scroll area.
    pub rows: u32,
}

/// Turn a laid-out tree into coordinates.
///
/// `now` is the current tick, which is what a living branch runs to. `floor` drops species that
/// never got anywhere: a long run makes thousands of them, most one cell that divided twice,
/// and drawing every one turns the tree into a solid block.
#[must_use]
pub fn plot(tree: &Tree, now: u64, floor: u32) -> Plot {
    let span = now.max(1) as f32;
    let rows = tree.rows.max(1) as f32;
    // Against the largest peak rather than against the population: a species that dominated the
    // world for a million ticks and then died is the most important thing on the chart, and
    // scaling by what is alive now would draw it as nothing.
    let heaviest = tree
        .nodes
        .iter()
        .map(|n| n.peak_population)
        .max()
        .unwrap_or(1)
        .max(1) as f32;

    let shown: Vec<&TreeNode> = tree
        .nodes
        .iter()
        .filter(|n| n.peak_population >= floor)
        .collect();

    let branches: Vec<Branch> = shown
        .iter()
        .map(|n| Branch {
            id: n.id,
            x0: (n.founded_tick as f32 / span).clamp(0.0, 1.0),
            x1: n
                .extinct_tick
                .map_or(1.0, |t| (t as f32 / span).clamp(0.0, 1.0)),
            y: n.row as f32 / rows,
            // Square root, not the raw fraction. Peak populations span four orders of
            // magnitude in a long run, so linear width draws the winner and hairlines for
            // everything else — and the everything else is the interesting part of a tree.
            weight: (n.peak_population as f32 / heaviest).clamp(0.0, 1.0).sqrt(),
            alive: n.alive,
        })
        .collect();

    // A fork is only drawn when both ends are on the chart. Joining a visible child to a
    // parent that was pruned would draw a line to a row holding something else.
    let row_of: std::collections::BTreeMap<SpeciesId, u32> =
        shown.iter().map(|n| (n.id, n.row)).collect();
    let forks: Vec<Fork> = shown
        .iter()
        .filter_map(|n| {
            let parent = n.parent?;
            let parent_row = row_of.get(&parent)?;
            Some(Fork {
                x: (n.founded_tick as f32 / span).clamp(0.0, 1.0),
                y_parent: *parent_row as f32 / rows,
                y_child: n.row as f32 / rows,
            })
        })
        .collect();

    Plot {
        branches,
        forks,
        rows: tree.rows,
    }
}

/// A species' page, as the wiki shows it.
#[derive(Clone, PartialEq, Debug)]
pub struct Page {
    pub id: SpeciesId,
    pub name: String,
    pub abbreviated: String,
    /// The generated prose of SPEC §10.5.
    pub description: String,
    pub parent: Option<(SpeciesId, String)>,
    pub children: Vec<(SpeciesId, String)>,
    pub founded_tick: u64,
    pub extinct_tick: Option<u64>,
    pub extinction: Option<String>,
    pub population: u32,
    pub peak_population: u32,
    pub peak_tick: u64,
    pub births: u64,
    pub deaths: u64,
    pub depth: u32,
    /// The population curve, normalised to `0..=1` for drawing, with its own peak alongside.
    pub curve: Vec<(u64, f32)>,
    pub curve_peak: u32,
    /// The founder's genome, for the editor to load (M6).
    pub founder_genome: Vec<u8>,
    pub fingerprint: u64,
}

/// Build a species' wiki page.
#[must_use]
pub fn page(archive: &Phylogeny, id: SpeciesId) -> Option<Page> {
    let s = archive.get(id)?;
    let named = |other: SpeciesId| {
        archive
            .get(other)
            .map(|o| (other, o.name.full()))
            .unwrap_or((other, format!("species {other}")))
    };
    // Normalised against the species' own peak rather than the world's, so a species that
    // never got big still shows the shape of its rise and fall instead of a flat line.
    let peak = s
        .curve
        .points()
        .iter()
        .map(|p| p.population)
        .max()
        .unwrap_or(0);
    let curve = s
        .curve
        .points()
        .iter()
        .map(|p| (p.tick, p.population as f32 / peak.max(1) as f32))
        .collect();

    Some(Page {
        id: s.id,
        name: s.name.full(),
        abbreviated: s.name.abbreviated(),
        description: s.describe(archive),
        parent: s.parent.map(named),
        children: s.children.iter().map(|c| named(*c)).collect(),
        founded_tick: s.founded_tick,
        extinct_tick: s.extinct_tick,
        extinction: s.extinction.map(|e| e.describe(archive)),
        population: s.population,
        peak_population: s.peak_population,
        peak_tick: s.peak_tick,
        births: s.births,
        deaths: s.deaths,
        depth: s.depth,
        curve,
        curve_peak: peak,
        founder_genome: s.founder_genome.bytes().to_vec(),
        fingerprint: s.founder_fingerprint,
    })
}

/// One entry on the scrubbable world timeline.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TimelineEntry {
    pub tick: u64,
    pub headline: String,
    pub species: SpeciesId,
    pub species_name: String,
    pub what: Occurrence,
    /// Position along the timeline, `0..=1000` — permille rather than a float, so a timeline
    /// with one event and a timeline with a thousand are laid out the same way.
    pub at: u32,
}

/// A parameter change, on the same axis as everything the world did to itself (M10.4).
///
/// The hand reaching into the world is part of the world's history. "The population crashed"
/// and "you tripled the repair cost ninety ticks earlier" belong on one axis or neither is
/// legible.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Meddle {
    pub tick: u64,
    pub at: u32,
    /// What changed, as a short phrase. Worked out by the caller, which is the only place that
    /// knows what the parameters are called.
    pub summary: String,
}

/// The annotated world timeline (SPEC §14).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Timeline {
    pub entries: Vec<TimelineEntry>,
    /// Parameter changes, kept apart from `entries` because they are a different kind of fact:
    /// one is something that happened, the other is something somebody did.
    pub meddles: Vec<Meddle>,
    pub span: u64,
}

impl Timeline {
    /// The entry nearest a position along the axis, for scrubbing.
    #[must_use]
    pub fn nearest(&self, at: u32) -> Option<&TimelineEntry> {
        self.entries.iter().min_by_key(|e| e.at.abs_diff(at))
    }
}

/// Lay the event log out along a timeline running from tick zero to `now`.
#[must_use]
pub fn timeline(archive: &Phylogeny, events: &[Event], now: u64) -> Timeline {
    timeline_with(archive, events, &[], now)
}

/// The same, with parameter changes on the axis too.
#[must_use]
pub fn timeline_with(
    archive: &Phylogeny,
    events: &[Event],
    meddles: &[(u64, String)],
    now: u64,
) -> Timeline {
    let span = now.max(1);
    let place = |tick: u64| ((tick.min(span) * 1000) / span) as u32;
    Timeline {
        meddles: meddles
            .iter()
            .map(|(tick, summary)| Meddle {
                tick: *tick,
                at: place(*tick),
                summary: summary.clone(),
            })
            .collect(),
        entries: events
            .iter()
            .map(|e| TimelineEntry {
                tick: e.tick,
                headline: e.what.headline(),
                species: e.species,
                species_name: archive
                    .get(e.species)
                    .map_or_else(|| format!("species {}", e.species), |s| s.name.full()),
                what: e.what,
                at: place(e.tick),
            })
            .collect(),
        span,
    }
}

/// Species worth showing first: the biggest, whether or not they are still alive.
///
/// A wiki that listed species in id order would open on whichever lineage happened to fork
/// first, which is almost never the one anybody wants to read about.
#[must_use]
pub fn notable(archive: &Phylogeny, limit: usize) -> Vec<SpeciesId> {
    let mut all: Vec<(&mm_core::phylogeny::Species, u64)> = archive
        .iter()
        // Living species outrank dead ones of the same size, so the front page is about what
        // is happening rather than what happened.
        .map(|s| {
            let weight = u64::from(s.peak_population) * if s.population > 0 { 2 } else { 1 };
            (s, weight)
        })
        .collect();
    all.sort_by_key(|(s, weight)| (std::cmp::Reverse(*weight), s.id));
    all.into_iter().take(limit).map(|(s, _)| s.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_core::genome::Genome;
    use mm_core::names::Traits;
    use mm_core::organelle::{Organelle, OrganelleType};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn traits() -> Traits {
        Traits::of(&[Organelle::finished(OrganelleType::Chloroplast, 40)], 200)
    }

    fn drifted(base: &[u8], want: u32) -> Vec<u8> {
        let mut g = base.to_vec();
        let base_fp = mm_core::genome::simhash(base);
        for i in 0..g.len() {
            if mm_core::genome::fingerprint_distance(base_fp, mm_core::genome::simhash(&g)) >= want
            {
                break;
            }
            g[i] = g[i].wrapping_add(97).wrapping_mul(3);
        }
        g
    }

    /// A root with two children and one grandchild.
    #[test]
    fn a_guild_is_read_off_the_loadout_and_a_mixotroph_is_both() {
        use mm_core::names::Traits;
        use mm_core::OrganelleType;
        let with = |kinds: &[OrganelleType]| {
            let mut t = Traits::default();
            for k in kinds {
                t.counts[*k as usize] = 1;
            }
            Guild::of(&t)
        };

        assert!(with(&[OrganelleType::Chloroplast]).producer);
        assert!(with(&[OrganelleType::Spike]).predator);
        assert!(with(&[OrganelleType::Lysosome]).scavenger);
        assert!(with(&[OrganelleType::Mitochondrion]).is_osmotroph());

        // The point of it being a set. A cell that photosynthesises *and* hunts is both, and
        // rounding it to whichever the code checked first would be the cell-type enum arriving
        // through the display layer.
        let both = with(&[OrganelleType::Chloroplast, OrganelleType::Spike]);
        assert!(both.producer && both.predator);
        assert!(!both.is_osmotroph());
        assert_eq!(both.label(), "producer + predator");
    }

    #[test]
    fn a_mixotrophs_colour_sits_between_the_ones_it_mixes() {
        let producer = Guild {
            producer: true,
            ..Guild::default()
        };
        let predator = Guild {
            predator: true,
            ..Guild::default()
        };
        let both = Guild {
            producer: true,
            predator: true,
            ..Guild::default()
        };
        for k in 0..3 {
            let (lo, hi) = (
                producer.rgb()[k].min(predator.rgb()[k]),
                producer.rgb()[k].max(predator.rgb()[k]),
            );
            assert!(
                both.rgb()[k] >= lo && both.rgb()[k] <= hi,
                "channel {k} of the blend is outside what it blends"
            );
        }
        // And every guild is distinguishable from every other, or the colour says nothing.
        let all = [producer, predator, both, Guild::default()];
        for a in 0..all.len() {
            for b in (a + 1)..all.len() {
                assert_ne!(all[a].rgb(), all[b].rgb(), "{a} and {b} look the same");
            }
        }
    }

    #[test]
    fn a_living_branch_reaches_the_present_and_an_extinct_one_stops() {
        // `small_archive` ends on a census that omits `b`, so one species is already extinct
        // at tick 400 and the others are alive — which is exactly the mix this needs.
        let p = small_archive();
        let plot = plot(&layout(&p), 1_000, 0);
        assert!(plot.branches.iter().any(|b| b.alive));
        assert!(
            plot.branches.iter().any(|b| !b.alive),
            "nothing in the fixture died, so this proves nothing"
        );
        for branch in &plot.branches {
            if branch.alive {
                assert!(
                    (branch.x1 - 1.0).abs() < 1e-6,
                    "a living species stopped short of the present"
                );
            } else {
                assert!(
                    branch.x1 < 1.0,
                    "an extinct species ran to the right-hand edge"
                );
            }
            assert!(branch.x0 <= branch.x1, "a species ended before it began");
            assert!((0.0..=1.0).contains(&branch.y));
        }
    }

    #[test]
    fn every_fork_joins_two_rows_that_are_actually_drawn() {
        // A fork to a pruned parent would draw a line to a row holding something else, which
        // is worse than drawing nothing: it asserts a descent that is not there.
        let p = small_archive();
        let tree = layout(&p);
        let rows: Vec<f32> = tree
            .nodes
            .iter()
            .map(|n| n.row as f32 / tree.rows.max(1) as f32)
            .collect();
        for floor in [0u32, 1, 2, 100] {
            let plot = plot(&tree, 1_000, floor);
            for fork in &plot.forks {
                assert!(
                    rows.iter().any(|r| (r - fork.y_parent).abs() < 1e-6),
                    "a fork at floor {floor} points at no row"
                );
                assert!(
                    plot.branches
                        .iter()
                        .any(|b| (b.y - fork.y_child).abs() < 1e-6),
                    "a fork at floor {floor} comes from a branch that was pruned"
                );
                assert!(
                    plot.branches
                        .iter()
                        .any(|b| (b.y - fork.y_parent).abs() < 1e-6),
                    "a fork at floor {floor} joins a parent that was pruned"
                );
            }
        }
    }

    #[test]
    fn the_floor_prunes_the_small_and_keeps_the_large() {
        let p = small_archive();
        let tree = layout(&p);
        let all = plot(&tree, 1_000, 0).branches.len();
        let pruned = plot(&tree, 1_000, u32::MAX).branches.len();
        assert_eq!(all, tree.nodes.len());
        assert_eq!(pruned, 0, "an infinite floor kept something");
    }

    #[test]
    fn weight_favours_what_was_ever_large_rather_than_what_is_large_now() {
        // A species that dominated the world for a long time and then died is the most
        // important thing on the chart. Scaling by live population would draw it as nothing.
        let mut p = small_archive();
        let ids: Vec<SpeciesId> = p.iter().map(|s| s.id).collect();
        let (giant, survivor) = (ids[0], ids[1]);

        // It was enormous. Then it was gone and something small was all that was left.
        p.census(&std::iter::once((giant, 5_000u32)).collect(), 500);
        p.census(&std::iter::once((survivor, 10u32)).collect(), 600);

        let plot = plot(&layout(&p), 1_000, 0);
        let dead = plot
            .branches
            .iter()
            .find(|b| b.id == giant)
            .expect("the dead giant");
        let alive = plot
            .branches
            .iter()
            .find(|b| b.id == survivor)
            .expect("the survivor");
        assert!(!dead.alive && alive.alive);
        assert!(
            dead.weight > alive.weight,
            "a former giant was drawn thinner than a survivor that was never large: \
             {} against {}",
            dead.weight,
            alive.weight
        );
    }

    #[test]
    fn parameter_changes_land_on_the_same_axis_as_events() {
        let p = small_archive();
        let meddles = vec![
            (250u64, "division energy: 20480 -> 61440".to_string()),
            (750, "point: 64 -> 8".to_string()),
        ];
        let t = timeline_with(&p, &[], &meddles, 1_000);
        assert_eq!(t.meddles.len(), 2);
        assert_eq!(t.meddles[0].at, 250);
        assert_eq!(t.meddles[1].at, 750);
        // Kept apart from the events, because one is something that happened and the other is
        // something somebody did, and the pane draws them differently for that reason.
        assert!(t.entries.is_empty());
    }

    #[test]
    fn scrubbing_finds_the_nearest_event_and_not_merely_the_first() {
        let p = small_archive();
        let species = p.iter().map(|s| s.id).next().expect("a species");
        let at = |tick: u64| mm_core::events::Event {
            tick,
            what: mm_core::events::Occurrence::ALL[0],
            species,
            x: 0,
            y: 0,
        };
        let t = timeline(&p, &[at(100), at(500), at(900)], 1_000);

        assert_eq!(t.nearest(0).map(|e| e.tick), Some(100));
        assert_eq!(t.nearest(480).map(|e| e.tick), Some(500));
        assert_eq!(t.nearest(1_000).map(|e| e.tick), Some(900));

        // With nothing on it there is nothing to find, which must be `None` rather than a
        // panic — an empty world is the first thing anybody sees.
        assert!(timeline(&p, &[], 1_000).nearest(500).is_none());
    }

    fn small_archive() -> Phylogeny {
        let mut p = Phylogeny::new();
        let base = vec![7u8; 200];
        let root = p.found(&Arc::new(Genome::new(base.clone()).unwrap()), traits(), 0);
        p.record_birth(root);

        let a_bytes = drifted(&base, p.speciation_threshold + 6);
        let a = p.on_birth(
            root,
            &Arc::new(Genome::new(a_bytes.clone()).unwrap()),
            traits(),
            100,
        );
        p.record_birth(a);

        // A second child of the root, far from both the root and from `a`.
        let mut b_bytes = base.clone();
        for (i, byte) in b_bytes.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(31).wrapping_add(13);
        }
        let b = p.on_birth(
            root,
            &Arc::new(Genome::new(b_bytes).unwrap()),
            traits(),
            200,
        );
        p.record_birth(b);

        let c_bytes = drifted(&a_bytes, p.speciation_threshold + 6);
        let c = p.on_birth(a, &Arc::new(Genome::new(c_bytes).unwrap()), traits(), 300);
        p.record_birth(c);

        let mut counts = BTreeMap::new();
        counts.insert(root, 100u32);
        counts.insert(a, 50u32);
        counts.insert(c, 25u32);
        p.census(&counts, 400);
        p
    }

    #[test]
    fn every_species_gets_exactly_one_row() {
        let p = small_archive();
        let tree = layout(&p);
        assert_eq!(tree.nodes.len(), p.len());
        let rows: std::collections::BTreeSet<u32> = tree.nodes.iter().map(|n| n.row).collect();
        assert_eq!(rows.len(), tree.nodes.len(), "two species share a row");
        assert_eq!(tree.rows, tree.nodes.len() as u32);
    }

    #[test]
    fn a_child_sits_below_its_parent_and_deeper_in() {
        let p = small_archive();
        let tree = layout(&p);
        let by_id: BTreeMap<SpeciesId, &TreeNode> = tree.nodes.iter().map(|n| (n.id, n)).collect();
        for node in &tree.nodes {
            let Some(parent) = node.parent.and_then(|p| by_id.get(&p)) else {
                continue;
            };
            assert!(
                node.row > parent.row,
                "species {} is drawn above its parent {}",
                node.id,
                parent.id
            );
            assert!(
                node.depth > parent.depth,
                "species {} is not deeper than its parent",
                node.id
            );
        }
    }

    #[test]
    fn a_subtree_is_contiguous() {
        // What makes the tree readable: a lineage and its descendants occupy a solid block of
        // rows rather than being interleaved with unrelated branches.
        let p = small_archive();
        let tree = layout(&p);
        let by_id: BTreeMap<SpeciesId, &TreeNode> = tree.nodes.iter().map(|n| (n.id, n)).collect();
        for node in &tree.nodes {
            let mut descendants: Vec<u32> = tree
                .nodes
                .iter()
                .filter(|other| {
                    let mut at = other.parent;
                    while let Some(id) = at {
                        if id == node.id {
                            return true;
                        }
                        at = by_id.get(&id).and_then(|n| n.parent);
                    }
                    false
                })
                .map(|n| n.row)
                .collect();
            if descendants.is_empty() {
                continue;
            }
            descendants.push(node.row);
            descendants.sort_unstable();
            let span = descendants.last().unwrap() - descendants.first().unwrap();
            assert_eq!(
                span as usize + 1,
                descendants.len(),
                "species {}'s subtree is split across other branches",
                node.id
            );
        }
    }

    #[test]
    fn a_deep_lineage_does_not_blow_the_stack() {
        // A long thin chain is the shape a real run produces, and it is the one a recursive
        // layout would fall over on.
        let mut p = Phylogeny::new();
        let mut bytes = vec![7u8; 200];
        let mut species = p.found(&Arc::new(Genome::new(bytes.clone()).unwrap()), traits(), 0);
        for step in 0..3_000u64 {
            bytes = drifted(&bytes, p.speciation_threshold + 6);
            species = p.on_birth(
                species,
                &Arc::new(Genome::new(bytes.clone()).unwrap()),
                traits(),
                step,
            );
        }
        let tree = layout(&p);
        assert_eq!(tree.nodes.len(), p.len());
        assert!(
            tree.max_depth > 100,
            "the chain did not get deep: {}",
            tree.max_depth
        );
    }

    #[test]
    fn a_page_describes_the_species_and_carries_its_genome() {
        let p = small_archive();
        let id = p.iter().next().expect("a species").id;
        let root = page(&p, id).expect("a page");
        assert!(
            root.description.contains(&root.name),
            "{}",
            root.description
        );
        assert!(
            !root.founder_genome.is_empty(),
            "no genome to load into the editor"
        );
        assert!(
            !root.children.is_empty(),
            "the root has children and the page shows none"
        );
        assert!(root.curve.iter().all(|(_, v)| (0.0..=1.0).contains(v)));
        assert_eq!(page(&p, 9_999), None);
    }

    #[test]
    fn the_timeline_places_events_along_its_span() {
        let p = small_archive();
        let events = vec![
            Event {
                tick: 0,
                what: Occurrence::EndogenousReplication,
                species: 0,
                x: 1,
                y: 2,
            },
            Event {
                tick: 500,
                what: Occurrence::Motility,
                species: 0,
                x: 3,
                y: 4,
            },
            Event {
                tick: 1000,
                what: Occurrence::MassExtinction,
                species: 0,
                x: 5,
                y: 6,
            },
        ];
        let t = timeline(&p, &events, 1000);
        assert_eq!(t.entries[0].at, 0);
        assert_eq!(t.entries[1].at, 500);
        assert_eq!(t.entries[2].at, 1000);
        assert!(t.entries.iter().all(|e| e.at <= 1000));
        assert!(!t.entries[0].species_name.is_empty());
        assert!(!t.entries[0].headline.is_empty());
    }

    #[test]
    fn an_empty_timeline_does_not_divide_by_zero() {
        let p = Phylogeny::new();
        let t = timeline(&p, &[], 0);
        assert!(t.entries.is_empty());
        assert_eq!(t.span, 1);
    }

    #[test]
    fn the_front_page_leads_with_what_is_happening() {
        let p = small_archive();
        let notable = notable(&p, 3);
        assert_eq!(notable.len(), 3);
        // The root has the largest population and should lead.
        let first = p.get(notable[0]).expect("species");
        assert!(
            first.population > 0,
            "the front page opened on a species that is not alive"
        );
    }
}
