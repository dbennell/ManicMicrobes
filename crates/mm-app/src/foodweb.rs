//! Trophic analysis and the food-web display (M8, SPEC §10.5).
//!
//! Same shape as [`crate::wiki`]: everything is copied out of the world, and the layout is
//! decided here rather than in the renderer so that it can be tested on a machine with no
//! display.
//!
//! # What a food web can honestly say
//!
//! There is no `Predator` type and no `eats` relation in the engine. A trophic level is an
//! inference from an organelle loadout — a chloroplast makes a producer, a lysosome a
//! scavenger, a spike a predator — exactly as skin, muscle and neuron are inferences in
//! `CLAUDE.md`'s design rules. A cell can be two of those at once and often is, so the guilds
//! are drawn as overlapping, not as a partition.
//!
//! The edges are the routes matter can actually take, and there are only five of them because
//! the engine only implements five. The one that is conspicuously *missing* is the direct
//! predator-eats-prey edge: a spike does damage, damage kills, death makes carrion, and
//! whoever digests that carrion is whoever has a lysosome — which may well not be the cell
//! that did the killing. That is worth drawing rather than papering over, because it is the
//! reason a predator lineage tends to acquire a lysosome and a genome nobody wrote gets to
//! discover that hunting only pays if you can eat what you kill.
//!
//! Weights are measured over a window of ticks, not modelled. An edge the engine has but that
//! carried nothing this window is drawn at zero rather than dropped, so an empty niche looks
//! like an empty niche.

use mm_core::ecology::TrophicMix;
use mm_core::world::TickReport;

/// A node in the web: either something matter comes from, or a guild that takes it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Node {
    /// The light field. The only place energy enters the world.
    Light,
    /// Substrate and waste dissolved in the water.
    Dissolved,
    /// What the dead leave behind.
    Carrion,

    /// Cells carrying a chloroplast.
    Producers,
    /// Cells carrying neither chloroplast nor lysosome: they live on what is dissolved.
    Osmotrophs,
    /// Cells carrying a lysosome.
    Scavengers,
    /// Cells carrying a spike.
    Predators,
}

impl Node {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Node::Light => "light",
            Node::Dissolved => "dissolved matter",
            Node::Carrion => "carrion",
            Node::Producers => "producers",
            Node::Osmotrophs => "osmotrophs",
            Node::Scavengers => "scavengers",
            Node::Predators => "predators",
        }
    }

    /// Which row it sits on: sources at the bottom, then who lives off them, with carrion at
    /// the top because everything ends there.
    ///
    /// That puts every death edge pointing upward and leaves exactly one edge running back
    /// down the picture — carrion to scavengers. Which is the right picture: a food web with
    /// recycling in it is a cycle, and the single downward arrow is where the cycle closes.
    /// Drawing carrion in the middle instead would scatter the same cycle over two arrows and
    /// make it look like an error.
    #[must_use]
    pub fn level(self) -> u32 {
        match self {
            Node::Light | Node::Dissolved => 0,
            Node::Producers | Node::Osmotrophs => 1,
            Node::Scavengers | Node::Predators => 2,
            Node::Carrion => 3,
        }
    }

    /// Whether this is a pool of matter rather than a population of cells.
    #[must_use]
    pub fn is_source(self) -> bool {
        matches!(self, Node::Light | Node::Dissolved | Node::Carrion)
    }

    /// Every node, in level order.
    pub const ALL: [Node; 7] = [
        Node::Light,
        Node::Dissolved,
        Node::Producers,
        Node::Osmotrophs,
        Node::Carrion,
        Node::Scavengers,
        Node::Predators,
    ];
}

/// How much an edge's weight is worth trusting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Basis {
    /// The engine measured exactly this flow, along exactly this edge.
    Measured,
    /// The engine measured the total but not who it belonged to, so it is shared out by
    /// population. Drawn differently, because it is arithmetic rather than observation.
    SharedByPopulation,
}

/// A route matter takes, and how much went along it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Edge {
    pub from: Node,
    pub to: Node,
    /// `Q10` matter over the window. Zero is a real answer: the route exists and carried
    /// nothing.
    pub weight: i64,
    pub basis: Basis,
    /// What the edge is, in the register of the wiki rather than of a log file.
    pub note: &'static str,
}

impl Edge {
    /// Whether this is matter falling out of the living world rather than being eaten.
    #[must_use]
    pub fn is_death(&self) -> bool {
        self.to == Node::Carrion
    }

    /// Whether this is the edge that closes the loop — the only one that runs back down the
    /// picture, and the one whose absence means the world is silting up.
    #[must_use]
    pub fn is_recycling(&self) -> bool {
        self.from == Node::Carrion
    }
}

/// Everything the panel draws.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FoodWeb {
    pub nodes: Vec<Occupancy>,
    pub edges: Vec<Edge>,
    pub mix: TrophicMix,
    /// Ticks the weights were accumulated over.
    pub window_ticks: u64,
}

/// A node with its population.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Occupancy {
    pub node: Node,
    /// Cells in this guild; zero for a source.
    pub count: u32,
    /// Share of the living population, per mille. Guilds overlap, so these do not sum to 1000.
    pub share: u32,
}

/// Flows accumulated over a window of ticks.
///
/// Kept separately from the world because it is a moving average for a panel, not simulation
/// state: nothing reads it back, and a run that never opens the panel must produce identical
/// results to one that does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Flows {
    pub ticks: u64,
    /// Matter fixed by photosynthesis, `Q10`.
    pub photosynthesised: i64,
    /// Matter respired, `Q10`.
    pub respired: i64,
    /// Structural mass the dead left as carrion, `Q10`.
    pub to_carrion: i64,
    /// Carrion digested back into substrate, `Q10`.
    pub scavenged: i64,
    /// Damage dealt by spikes, `Q10`.
    pub wounding: i64,
}

impl Flows {
    /// Fold one tick's report in.
    pub fn accumulate(&mut self, report: &TickReport) {
        self.ticks = self.ticks.saturating_add(1);
        self.photosynthesised = self.photosynthesised.saturating_add(report.metabolism.fixed);
        self.respired = self.respired.saturating_add(report.metabolism.burned);
        self.to_carrion = self.to_carrion.saturating_add(report.biology.to_carrion);
        self.scavenged = self.scavenged.saturating_add(report.ecology.scavenged);
        self.wounding = self.wounding.saturating_add(report.ecology.damage_dealt);
    }

    /// Start a fresh window, keeping nothing. Called when the panel's averaging period rolls
    /// over, so what is drawn is recent rather than a run-long average that never moves.
    pub fn reset(&mut self) {
        *self = Flows::default();
    }
}

/// Build the web from a census and a window of measured flow.
#[must_use]
pub fn web(mix: TrophicMix, flows: &Flows) -> FoodWeb {
    let share = |n: u32| -> u32 {
        if mix.total == 0 {
            0
        } else {
            ((n as u64 * 1000) / mix.total as u64) as u32
        }
    };
    let count = |node: Node| -> u32 {
        match node {
            Node::Producers => mix.producers,
            Node::Osmotrophs => mix.osmotrophs,
            Node::Scavengers => mix.scavengers,
            Node::Predators => mix.predators,
            _ => 0,
        }
    };

    let nodes = Node::ALL
        .iter()
        .map(|node| Occupancy {
            node: *node,
            count: count(*node),
            share: share(count(*node)),
        })
        .collect();

    // Respiration and death are measured world-wide but not per guild, so both are shared out
    // by head count and labelled as such. The denominator is the sum of the guild counts
    // rather than the population, because guilds overlap — a cell with a chloroplast and a
    // lysosome is in two of them — and dividing by the population would leave the parts
    // adding up to more than the whole. This way they add up exactly, at the price of a cell
    // in two guilds having its respiration split between them, which is the honest reading of
    // an attribution nobody measured.
    let guild_total = (mix.producers as i64)
        .saturating_add(mix.osmotrophs as i64)
        .saturating_add(mix.scavengers as i64)
        .saturating_add(mix.predators as i64);
    let split = move |total: i64, n: u32| -> i64 {
        if guild_total == 0 {
            0
        } else {
            total.saturating_mul(n as i64) / guild_total
        }
    };

    const GUILDS: [Node; 4] = [
        Node::Producers,
        Node::Osmotrophs,
        Node::Scavengers,
        Node::Predators,
    ];

    // The two guild-exact edges first: only a chloroplast photosynthesises and only a lysosome
    // digests, so these are observations rather than arithmetic.
    let mut edges = vec![
        Edge {
            from: Node::Light,
            to: Node::Producers,
            weight: flows.photosynthesised,
            basis: Basis::Measured,
            note: "photosynthesis: the only way matter re-enters the food web",
        },
        Edge {
            from: Node::Carrion,
            to: Node::Scavengers,
            weight: flows.scavenged,
            basis: Basis::Measured,
            note: "lysosome digestion: the only way carrion returns to the living",
        },
    ];
    for guild in GUILDS {
        edges.push(Edge {
            from: Node::Dissolved,
            to: guild,
            weight: split(flows.respired, count(guild)),
            basis: Basis::SharedByPopulation,
            note: "respiration of what is dissolved in the water",
        });
    }
    for guild in GUILDS {
        edges.push(Edge {
            from: guild,
            to: Node::Carrion,
            weight: split(flows.to_carrion, count(guild)),
            basis: Basis::SharedByPopulation,
            note: "what the dead leave behind, however they died",
        });
    }
    // Predation has no edge of its own into a predator, and that absence is the point: a spike
    // does damage, damage kills, and the kill becomes carrion like any other death. Only a
    // lysosome can eat it. So a hunter with no lysosome spends its life feeding scavengers,
    // and the pressure to acquire one is something the physics applies rather than something
    // anybody wrote down.
    edges.push(Edge {
        from: Node::Predators,
        to: Node::Carrion,
        weight: flows.wounding,
        basis: Basis::Measured,
        note: "spike damage — which reaches the killer only by way of the carrion pool",
    });

    FoodWeb {
        nodes,
        edges,
        mix,
        window_ticks: flows.ticks,
    }
}

impl FoodWeb {
    /// The largest edge weight, for scaling the drawing. At least 1, so nothing divides by it
    /// and gets a surprise.
    #[must_use]
    pub fn peak(&self) -> i64 {
        self.edges.iter().map(|e| e.weight).max().unwrap_or(0).max(1)
    }

    /// A one-line reading of the web, for the panel's header.
    ///
    /// Descriptive, not evaluative: nothing here scores an ecosystem, because scoring one is a
    /// short walk from selecting for one.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.mix.total == 0 {
            return "nothing alive".to_string();
        }
        let mut levels = 1;
        if self.mix.scavengers > 0 {
            levels += 1;
        }
        if self.mix.predators > 0 {
            levels += 1;
        }
        let closed = self.edges.iter().any(|e| e.to == Node::Scavengers && e.weight > 0);
        format!(
            "{} cells across {levels} trophic {}; the carrion loop is {}",
            self.mix.total,
            if levels == 1 { "level" } else { "levels" },
            if closed { "closing" } else { "open" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mix(producers: u32, osmotrophs: u32, scavengers: u32, predators: u32) -> TrophicMix {
        TrophicMix {
            producers,
            osmotrophs,
            scavengers,
            predators,
            total: producers + osmotrophs,
        }
    }

    #[test]
    fn an_empty_world_produces_a_web_rather_than_a_panic() {
        let w = web(TrophicMix::default(), &Flows::default());
        assert_eq!(w.nodes.len(), Node::ALL.len());
        assert!(w.edges.iter().all(|e| e.weight == 0));
        assert_eq!(w.peak(), 1, "scaling must never divide by zero");
        assert_eq!(w.summary(), "nothing alive");
    }

    #[test]
    fn every_node_appears_exactly_once_and_levels_go_upward() {
        let w = web(mix(4, 2, 1, 1), &Flows::default());
        for node in Node::ALL {
            assert_eq!(
                w.nodes.iter().filter(|o| o.node == node).count(),
                1,
                "{node:?} is not in the web exactly once"
            );
        }
        // A food web that recycles is a cycle, so "every edge runs up" cannot hold. What must
        // hold is that the cycle closes in exactly one place, so the picture reads as a loop
        // rather than as a tangle.
        let downhill: Vec<&Edge> = w
            .edges
            .iter()
            .filter(|e| e.to.level() <= e.from.level())
            .collect();
        assert_eq!(downhill.len(), 1, "the web has {} back-edges", downhill.len());
        assert!(
            downhill[0].is_recycling(),
            "the one back-edge is {:?} -> {:?}, not the carrion loop",
            downhill[0].from,
            downhill[0].to
        );
        assert!(w.edges.iter().all(|e| e.from != e.to), "an edge loops on itself");
    }

    #[test]
    fn the_shared_out_flows_add_up_to_what_was_measured() {
        // Guilds overlap, so this is the property that stops the arithmetic inventing or
        // losing matter: whatever basis the split uses, the parts must sum to the whole.
        let flows = Flows {
            respired: 1_000,
            to_carrion: 700,
            ..Flows::default()
        };
        // A population where two of the four guilds overlap: 3 producers of whom 1 scavenges.
        let m = TrophicMix {
            producers: 3,
            osmotrophs: 1,
            scavengers: 1,
            predators: 2,
            total: 4,
        };
        let w = web(m, &flows);
        let sum = |f: fn(&Edge) -> bool| w.edges.iter().filter(|e| f(e)).map(|e| e.weight).sum::<i64>();
        let respired: i64 = sum(|e| e.from == Node::Dissolved);
        let died: i64 = sum(|e| e.is_death() && e.basis == Basis::SharedByPopulation);
        // Integer division loses at most one unit per guild.
        assert!(
            (flows.respired - respired) < 4 && respired <= flows.respired,
            "respiration split to {respired} of {}",
            flows.respired
        );
        assert!(
            (flows.to_carrion - died) < 4 && died <= flows.to_carrion,
            "deaths split to {died} of {}",
            flows.to_carrion
        );
    }

    #[test]
    fn there_is_no_direct_predator_eats_prey_edge() {
        // The design decision, asserted rather than left in a comment: predation goes through
        // carrion, because that is the only path the engine implements. If someone adds a
        // direct edge, they have either added a mechanism or drawn a lie.
        let w = web(mix(4, 2, 1, 1), &Flows::default());
        assert!(
            !w.edges
                .iter()
                .any(|e| e.from == Node::Predators && !e.to.is_source()),
            "the web claims predators eat something directly; the engine has no such path"
        );
        assert!(
            w.edges
                .iter()
                .any(|e| e.from == Node::Predators && e.to == Node::Carrion),
            "predation does not reach the web at all"
        );
    }

    #[test]
    fn respiration_is_shared_out_and_labelled_as_shared() {
        let flows = Flows {
            respired: 900,
            ..Flows::default()
        };
        // Three cells across three guilds, so the split is a clean third each.
        let w = web(mix(1, 1, 1, 0), &flows);
        let to = |n: Node| w.edges.iter().find(|e| e.to == n && e.from == Node::Dissolved);
        assert_eq!(to(Node::Producers).map(|e| e.weight), Some(300));
        assert_eq!(to(Node::Osmotrophs).map(|e| e.weight), Some(300));
        assert_eq!(to(Node::Scavengers).map(|e| e.weight), Some(300));
        assert_eq!(to(Node::Predators).map(|e| e.weight), Some(0));
        assert!(w
            .edges
            .iter()
            .filter(|e| e.from == Node::Dissolved)
            .all(|e| e.basis == Basis::SharedByPopulation));
        // And the guild-exact ones are not quietly labelled the same way. Only a chloroplast
        // photosynthesises and only a lysosome digests, so those two are observations.
        assert!(w
            .edges
            .iter()
            .filter(|e| e.from == Node::Light || e.is_recycling())
            .all(|e| e.basis == Basis::Measured));
    }

    #[test]
    fn the_summary_counts_levels_from_what_is_actually_there() {
        assert!(web(mix(4, 0, 0, 0), &Flows::default())
            .summary()
            .contains("1 trophic level"));
        assert!(web(mix(4, 0, 2, 0), &Flows::default())
            .summary()
            .contains("2 trophic levels"));
        assert!(web(mix(4, 0, 2, 1), &Flows::default())
            .summary()
            .contains("3 trophic levels"));
    }

    #[test]
    fn the_carrion_loop_is_open_until_something_digests() {
        let open = web(mix(4, 0, 1, 0), &Flows::default());
        assert!(open.summary().ends_with("open"));
        let closing = web(
            mix(4, 0, 1, 0),
            &Flows {
                scavenged: 10,
                ..Flows::default()
            },
        );
        assert!(closing.summary().ends_with("closing"));
    }

    #[test]
    fn a_window_accumulates_and_resets() {
        let mut flows = Flows::default();
        let mut report = TickReport::default();
        report.metabolism.fixed = 5;
        report.ecology.scavenged = 2;
        flows.accumulate(&report);
        flows.accumulate(&report);
        assert_eq!((flows.ticks, flows.photosynthesised, flows.scavenged), (2, 10, 4));
        flows.reset();
        assert_eq!(flows, Flows::default());
    }
}
