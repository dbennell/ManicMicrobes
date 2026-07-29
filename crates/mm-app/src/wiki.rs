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

/// The annotated world timeline (SPEC §14).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Timeline {
    pub entries: Vec<TimelineEntry>,
    pub span: u64,
}

/// Lay the event log out along a timeline running from tick zero to `now`.
#[must_use]
pub fn timeline(archive: &Phylogeny, events: &[Event], now: u64) -> Timeline {
    let span = now.max(1);
    Timeline {
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
                at: ((e.tick.min(span) * 1000) / span) as u32,
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
