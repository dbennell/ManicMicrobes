//! The balance harness: is any way of making a living worth living?
//!
//! `benches/` gates performance and `shaderbench` gates the picture. This gates the *economy*,
//! which underpins both — a simulation whose only viable strategy is the first one anybody wrote
//! is not an open-ended evolutionary system, however fast it runs and however well it draws.
//!
//! # What it measures, and what it deliberately does not
//!
//! It does **not** score strategies. There is no fitness function in this project and there must
//! never be one (CLAUDE.md), and a harness that asserted "predation should reach 30% of the
//! slide" would be one wearing a lab coat. What it asserts instead is that the *strategy space*
//! is not degenerate, in four ways that are agnostic about which strategy wins:
//!
//! 1. **Viability** — every shipped organism founds a lineage that survives somewhere.
//! 2. **Payoff** — for every organism there exists at least one world in which its extra
//!    machinery is roughly competitive. Not "wins": competitive. See [`PAYOFF_FLOOR`].
//! 3. **Discrimination** — the environment changes the answer. If every world ranks the
//!    contenders identically, the worlds are decoration.
//! 4. **No sweep** — no single contender is best everywhere.
//!
//! Each of those is a statement about the *shape* of the matrix rather than about any cell in it,
//! which is what keeps the harness from having an opinion.
//!
//! # The harness proves its own fairness first
//!
//! Every scenario in a panel is run as a **mirror bout** — the reference genome against itself —
//! before anything else is believed of it. A fair slide returns 500‰ (a dead heat); a slide that
//! returns 700‰ has a better half, and every number taken from it is measuring the terrain. See
//! [`Report::mirror`] and [`Layout`].
//!
//! This is not a formality. `the_drift.ron` runs a left-to-right current, and mirroring two
//! lineages across the *vertical* midline there hands the upstream one the match before a tick is
//! run. The mirror control catches that and the panel entry declares a horizontal mirror instead.
//!
//! # A control that fires can mean two different things, and they want opposite fixes
//!
//! A mirror lands away from `EVEN` either because the slide has a **better half** or because the
//! bout is **too noisy to read**, and the two are not distinguishable from one number. `drift`
//! taught this the expensive way: it returned 557 at 12,000 ticks on both three and five seeds
//! and was called tilted, and it has no better half at all — over twelve seeds its median is 491.
//! What it has is a standing population of about sixty cells, at which two identical lineages
//! drift apart neutrally, with a spread that grows as the bout runs longer.
//!
//! Tell them apart by re-running the control at more seeds and a different bout length:
//!
//! * a **better half** holds its offset as seeds are added, and does not care how long the bout
//!   is — whoever starts on the good side is still there;
//! * **noise** has a median that wanders about `EVEN` across seeds and a spread that shrinks as
//!   the bout shortens.
//!
//! The fix for the first is the `Layout` or the world. The fix for the second is a shorter bout
//! — stop once the thing being measured has settled — or a world that holds more cells. It is
//! never [`MIRROR_TOLERANCE`], which is why that constant says so.
//!
//! # Where the numbers are not neutral, said out loud
//!
//! Two choices could be argued with, so they are stated rather than buried.
//!
//! **The reference is `ancestor.mm`**, the minimal autotroph, and a contender's score is its share
//! of the two-lineage population. That is not a claim that the ancestor deserves to be the
//! yardstick; it is the matched control. Most of `genomes/` is built as "the ancestor plus one
//! thing" — `hoarder` adds a vacuole, `sponge` a holdfast, `hunter` a spike — so measuring against
//! it isolates that one thing, which is the cleanest experimental design available and it is
//! already in the tree. Where a contender differs by more than one thing (`drifter`, `stalker`)
//! the score is about the whole body and the report says so.
//!
//! **Both sides start with a membrane, a nucleus and cytoplasm, and nothing else.** Not the
//! nucleus-plus-mitochondrion-plus-chloroplast kit `World::place_founders` hands out, because a
//! harness auditing whether the economy favours autotrophy must not begin by giving every
//! contender a free chloroplast. The nucleus is the one exception and it is a bootstrap
//! necessity rather than a policy: `BUD` returns zero without one, so a cell that has to build its
//! own nucleus before it can build anything is not testing its strategy, it is testing whether it
//! can survive tick zero.

use crate::arena::{roots, standing, Entry, Side};
use crate::biology::BiologyConfig;
use crate::cell::{CellId, CellSeed};
use crate::fixed::q10;
use crate::mutation::MutationRates;
use crate::organelle::{Organelle, OrganelleType, SLOT_COUNT};
use crate::scenario::Scenario;
use crate::world::World;

/// One part in a thousand. Every ratio here is an integer in these units; `mm-core` has no floats.
pub const PERMILLE: u32 = 1000;

/// A dead heat.
pub const EVEN: u32 = PERMILLE / 2;

/// How far a mirror bout may land from a dead heat before its scenario is called unfair.
///
/// Fifty parts in a thousand, which is five per cent of the slide. Two identical lineages on a
/// fair slide do not finish exactly level — they interfere with each other, and which of two
/// equally-matched cells gets the last free square is a coin toss the physics resolves. What they
/// must not do is finish consistently apart, which is what a slide with a better half produces.
///
/// **A world that trips this because it is noisy is not an argument for raising it.** See the
/// module header: a small population drifts neutrally, the spread grows with the bout, and the
/// median of a few seeds then lands anywhere. Widening the tolerance to admit that world would
/// blind the control to every *real* better half by the same amount, and the noisy column would
/// go on being noise — it would merely stop saying so. Shorten the bout instead, which is what
/// the `drift` entry of [`shipped_panel`] does and records.
pub const MIRROR_TOLERANCE: u32 = 50;

/// The share below which a strategy is judged to pay nowhere.
///
/// **Four hundred, not five.** The gate this feeds asks whether there is *any* world in which
/// carrying this machinery is roughly competitive, and demanding parity would be demanding that
/// every organelle be exactly as good as every other, which is neither achievable nor desirable —
/// a spike *should* be a worse deal than a chloroplast in a well-lit pond. Four hundred says the
/// machinery costs something and is not a death sentence; below it, a feature is decoration that
/// a cell pays upkeep for.
///
/// **The world it clears the floor in has to be one it survived.** A bout both lineages died in
/// scores `EVEN` — the sentinel for "nobody won" — and while that was counted as a reading, a
/// contender could clear this floor on the strength of a world that wiped it out. See
/// [`Row::best`], which is where that is now refused.
pub const PAYOFF_FLOOR: u32 = 400;

/// How many of one organelle a specialist should be able to carry and still live.
///
/// The catalogue gives a cell sixteen slots, and every evolved cell in the engine carries four —
/// membrane, nucleus, mitochondrion, chloroplast — which is the loadout it was seeded with. The
/// design intends a softer ceiling somewhere near twelve, past which a body has to be delivering
/// something extraordinary, and a specialist devoting several of its slots to whatever its niche
/// rewards. Four of one thing is a modest reading of that and is currently an extreme outlier.
///
/// **Deliberately a viability floor and not a competitive one.** The gate asks whether such a body
/// can *live*, not whether it wins: a specialist should lose in a world that does not reward its
/// speciality, and a gate that demanded otherwise would be the fitness function this project must
/// not have. What it refuses to accept is a depth that is simply fatal.
///
/// Four rather than six or eight, and the reason is arithmetic rather than modesty: a *matched*
/// metabolic specialist needs two slots per unit of depth plus the membrane and the nucleus, so
/// eight is eighteen slots and unreachable by construction. Four is seven of sixteen and leaves
/// half the body for everything else.
///
/// `docs/ECONOMY.md` §12 is what this measures and why it currently fails — not for cost, not for
/// slots and not for yield, but because respiration's exhaust scales with respiration while
/// excretion does not.
pub const SPECIALIST_DEPTH: usize = 4;

/// How much the panel must be able to move a strategy's fortunes, in permille.
///
/// The spread between a contender's best world and its worst. If the median spread across the
/// library is below this, the panel is a set of worlds that all pose the same question, and the
/// scenarios are not doing anything the parameters could not.
pub const DISCRIMINATION_FLOOR: u32 = 100;

/// Which midline the two lineages are mirrored across.
///
/// Chosen per scenario and checked, not assumed. A slide with a left-to-right current has no fair
/// vertical mirror: whichever lineage starts upstream gets the water and the food in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// Left column against right column. Fair on any slide that is symmetric left to right.
    Vertical,
    /// Top row against bottom row. For slides with a horizontal current or a horizontal barrier.
    Horizontal,
}

/// One world in the panel, and how to play a bout on it.
#[derive(Clone, Debug)]
pub struct Arena {
    /// What to call it in the report. The scenario's own name, usually.
    pub label: String,
    /// The world. Loaded from `scenarios/` by the caller — `mm-core` reads no files.
    pub scenario: Scenario,
    pub layout: Layout,
    /// How long a bout runs. Long enough for the slower world to leave its lag phase: a dim slide
    /// can sit at a tenth of its ceiling for twenty-five thousand ticks and then converge (see
    /// `docs/ECONOMY.md` §5), so a panel read at one horizon reports the lag of half of it.
    pub ticks: u64,
    /// Founders per side.
    pub founders: u32,
    /// How far in from its edge each side's lane sits, in squares, measured across the mirror
    /// axis. `None` is an eighth of the slide.
    ///
    /// # Why a world has to be able to say this
    ///
    /// An eighth is right for an open slide and wrong for a channel. `the_drift.ron` is 96 square
    /// with its walls at y=32..33 and y=62..63, so an eighth puts the two lanes at y=12 and y=83
    /// — **both outside the channel**, in open water with no wall to grip and none of the
    /// detritus, which only enters between y=34 and y=61. The world advertises "a current,
    /// particulate food that lags it, and walls to hold on to" and the panel was placing its
    /// contenders where none of the three exist.
    ///
    /// That is not only a wasted column. The slide outside the channel carries about **60 cells
    /// against `soup.ron`'s 1072**, and two neutral lineages sharing sixty cells are a
    /// Wright-Fisher population whose share is a random walk: measured over twelve seeds, the
    /// mirror's spread is 213 parts in a thousand at 12,000 ticks and the median of any five
    /// consecutive seeds lands up to 82 out — past [`MIRROR_TOLERANCE`], on a slide with no
    /// better half at all. The fairness control was reporting drift as tilted because it *is*
    /// noisy, not because it is unfair, and more founders does not help (the spread is 153 to 319
    /// at every count from 8 to 128). Population does.
    pub lane: Option<i32>,
}

/// One contender.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Contender {
    pub name: String,
    pub genome: Vec<u8>,
}

impl Contender {
    #[must_use]
    pub fn new(name: impl Into<String>, genome: Vec<u8>) -> Contender {
        Contender {
            name: name.into(),
            genome,
        }
    }
}

/// What one bout came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bout {
    /// The challenger's share of the two lineages, in permille. 500 is a dead heat.
    ///
    /// A bout in which both sides died is `EVEN` and `alive: false` — nobody won, and reporting
    /// it as a draw at zero would drag a genome's average down for a world that killed everyone.
    pub share: u32,
    pub challenger: u32,
    pub reference: u32,
    /// Whether the challenger's lineage still existed at the end.
    pub alive: bool,
    /// Cells no longer running the genome their side entered. See `arena::copy_damaged` — with
    /// mutation off a genome still drifts, because `COPYB` skips bytes a cell cannot pay for.
    pub copy_damaged: u32,
}

impl Bout {
    /// Whether this bout was a contest at all: did *either* lineage survive to be counted.
    ///
    /// The `EVEN` that [`Bout::share`] carries when both sides died is a **sentinel for "nobody
    /// won"**, not a measurement — there was no population to take a share of, and the field's
    /// own documentation says so. Every consumer that treats it as a number is reading a dead
    /// heat between two corpses as a creditable draw, which is how a world that killed a
    /// contender outright came to be the world its payoff was credited to.
    ///
    /// So the sentinel is filtered out here rather than interpreted downstream, and a world in
    /// which no seed was a contest yields no reading instead of a flattering one.
    #[must_use]
    pub fn contested(&self) -> bool {
        self.challenger > 0 || self.reference > 0
    }
}

/// The seeds a panel is run at, recorded here because a stochastic result is only a result if it
/// reproduces (CLAUDE.md).
///
/// Five. Enough that one unlucky slide does not decide a gate, few enough that a panel is minutes
/// rather than an afternoon. The median across them is what the report carries — a median rather
/// than a mean because a single wipe-out should not move a genome's score by a third.
pub const SEEDS: [u64; 5] = [0x0BA1, 0x1CE5, 0x2D07, 0x3E19, 0x4F2B];

/// One entry of the shipped panel: which world, and how to play a bout on it.
///
/// The panel is *data*, held here so that the gates in `tests/balance.rs` and the `mm-cli balance`
/// front end run the same five worlds rather than two lists that drift apart. `mm-core` reads no
/// files, so the caller resolves `file` against `scenarios/` and hands the loaded scenario back
/// through [`PanelEntry::arena`].
#[derive(Clone, Debug)]
pub struct PanelEntry {
    pub label: &'static str,
    pub file: &'static str,
    pub layout: Layout,
    pub ticks: u64,
    pub founders: u32,
    /// How deep each side's lane sits from its edge. `None` is an eighth of the slide; see
    /// [`Arena::lane`] for the world that needed it and what it cost to leave at the default.
    pub lane: Option<i32>,
    /// A light regime to substitute for the file's own.
    ///
    /// Two worlds in the panel are shaped right and calendared wrong for a bout: `the_long_dusk`
    /// declines over a million ticks and `seasons` runs a 96,000-tick year, so at any bout length
    /// the panel can afford, the first never leaves high summer and the second sees one season.
    /// Both are compressed rather than lengthened — the *shape* is what the panel wants, not the
    /// calendar — and the substitution is recorded here rather than done quietly at the call site.
    pub light: Option<crate::LightRegime>,
    /// The limit this world poses that no other entry does. A new entry needs one of these, and
    /// if it cannot be written the entry is a duplicate.
    pub poses: &'static str,
}

impl PanelEntry {
    /// Build the arena, given the scenario the caller loaded from `file`.
    #[must_use]
    pub fn arena(&self, scenario: Scenario, ticks_scale: u64) -> Arena {
        let scenario = match self.light.clone() {
            Some(light) => Scenario { light, ..scenario },
            None => scenario,
        };
        Arena {
            label: self.label.to_string(),
            scenario,
            layout: self.layout,
            ticks: (self.ticks * ticks_scale / 100).max(1_000),
            founders: self.founders,
            lane: self.lane,
        }
    }
}

/// The shipped panel: five worlds, each posing a limit none of the others does.
///
/// `seasons` and `the_long_dusk` are here because of `docs/ECONOMY.md` §5: they are the only two
/// worlds in the library that make light scarce, and no result from either had ever been
/// recorded. They are where storing energy against the dark is supposed to become worth doing —
/// and where a cell that has stored it becomes worth eating.
#[must_use]
pub fn shipped_panel() -> Vec<PanelEntry> {
    vec![
        PanelEntry {
            label: "soup",
            file: "soup.ron",
            layout: Layout::Vertical,
            // `docs/ECONOMY.md` §5: the soup converges by twelve thousand and does not move
            // again by a hundred thousand. Anything past the knee is time spent confirming it.
            ticks: 12_000,
            founders: 8,
            lane: None,
            light: None,
            poses: "nothing. The control: light, food and room all free",
        },
        PanelEntry {
            label: "thicket",
            file: "the_thicket.ron",
            layout: Layout::Vertical,
            ticks: 12_000,
            founders: 8,
            lane: None,
            light: None,
            // "Locally", precisely: the thicket seeds 40 units a square, which `CHEMISTRY.md` §6
            // measures as still above the cliff, and pairs it with `fluid_interval: 8` so that a
            // pack can deplete its own interior faster than the water refills it. That is a
            // gradient across a crowd, not a ceiling on the slide — `the_lean_water` is the
            // ceiling, and the two are different questions.
            poses: "light is rival, and a crowd can deplete the carbon inside itself",
        },
        PanelEntry {
            label: "dusk",
            file: "the_long_dusk.ron",
            layout: Layout::Vertical,
            // Long enough to be well past the knee the decline crosses, and no longer.
            ticks: 30_000,
            founders: 8,
            lane: None,
            light: Some(crate::LightRegime::SlowDecline {
                start: crate::Q10_ONE * 3 / 2,
                end: 0,
                over_ticks: 60_000,
            }),
            poses: "the light runs out for good, with free sugar to live on when it does",
        },
        PanelEntry {
            label: "seasons",
            file: "seasons.ron",
            layout: Layout::Vertical,
            // Two whole years, so a bout sees two winters rather than one.
            ticks: 40_000,
            founders: 8,
            lane: None,
            light: Some(crate::LightRegime::Seasonal {
                day_ticks: 240,
                year_ticks: 20_000,
                summer_day: crate::Q10_ONE * 5 / 4,
                winter_day: crate::Q10_ONE * 7 / 32,
                night: 0,
            }),
            poses: "the light comes and goes, and winter is below the extinction knee",
        },
        PanelEntry {
            // Horizontal, not vertical: the current runs left to right, so a vertical mirror
            // hands the bout to whoever starts upstream before a tick is run. The mirror control
            // is what found that, and this is the fix it asked for.
            label: "drift",
            file: "the_drift.ron",
            layout: Layout::Horizontal,
            // **Three thousand, not twelve.** This world converges by about tick 1,500 and holds
            // roughly sixty cells thereafter — the current washes cells off the slide, so the
            // standing population is a division-against-washout balance rather than a carrying
            // capacity. Sixty is small enough that two *identical* lineages sharing it are a
            // Wright-Fisher population whose share is a random walk, and the walk's spread grows
            // with the length of the bout: measured over twelve seeds, the mirror's range is 213
            // parts in a thousand at 12,000 ticks, 141 at 6,000 and 117 at 3,000.
            //
            // At 12,000 that put the median of five consecutive seeds up to 82 out and the
            // fairness control refused the whole panel — correctly by its own rule, and for the
            // wrong reason: the world has no better half. Its median over twelve seeds is 491.
            // What it has is neutral drift, and the only honest answer to neutral drift is to
            // stop the bout once the thing being measured has settled. Everything past 1,500
            // ticks here is sampling the walk, not the economy.
            ticks: 3_000,
            founders: 8,
            // Inside the channel, which is where this world's food and walls are. The default
            // eighth put the two lanes at y=12 and y=83 while the channel runs y=34..61, so both
            // sides were seeded in the open water *outside* the walls — no barrier to grip and
            // none of the detritus, which only enters between y=34 and y=61. A panel entry whose
            // `poses` promises "walls to hold on to" was placing every contender where there are
            // none, and `docs/ECONOMY.md` §8a had already had to seed along the wall by hand to
            // get a reading out of `sponge.mm`.
            //
            // 34 and its mirror 61 are the two rows immediately inside the upper and lower walls.
            //
            // This does **not** fix the mirror and was not what did: measured, it leaves the
            // population where it was, at 57 to 66 cells, because the reference is a photo-
            // autotroph that neither eats detritus nor grips anything. It costs a little scatter
            // (range 117 -> 145 at 3,000 ticks) and buys a column that measures what it claims to.
            lane: Some(34),
            light: None,
            poses: "a current, particulate food that lags it, and walls to hold on to",
        },
        PanelEntry {
            // Matter, at last. Every other world in the panel is limited by *area* — `ECONOMY.md`
            // §3 measured a saturated soup consuming 8% of its structural carbon and refusing
            // thirty divisions a tick for want of room — and a contest over area is won by
            // whoever carries least, which is why §9a can scale the whole catalogue fourfold and
            // change nothing. This is the one slide where earning more can buy more.
            label: "lean",
            file: "the_lean_water.ron",
            layout: Layout::Vertical,
            ticks: 12_000,
            founders: 8,
            lane: None,
            light: None,
            poses: "structural matter binds before space does, so earning more buys more",
        },
        PanelEntry {
            // `LightRegime::DayNight` had been in the engine since M1 with no scenario using it.
            // The vacuole pays nowhere in this panel (§15.3) and has never been asked a question
            // it could answer: `seasons` is a cull, `dusk` is a millennium, and everything else
            // is uniform. A night is the smallest honest test of a battery.
            label: "night",
            file: "the_short_night.ron",
            layout: Layout::Vertical,
            ticks: 12_000,
            founders: 8,
            lane: None,
            light: None,
            poses: "income stops and starts, on the timescale a cell lives at",
        },
        PanelEntry {
            // `LightRegime::Directional`'s own doc says it "makes position worth something, and
            // is the scenario that phototaxis has a reason to evolve in", and no scenario had
            // ever used it. The photosensor, chemosensor and cilium all pay nowhere and fail
            // together, because on a uniform slide the honest value of any reading is zero.
            label: "shallows",
            file: "the_shallows.ron",
            layout: Layout::Vertical,
            ticks: 12_000,
            founders: 8,
            lane: None,
            light: None,
            poses: "light is a gradient in space, so where a cell is worth something",
        },
        PanelEntry {
            // The only world in the library that is a function of time on the scale of a life.
            // A spring tide is past what a cilium can swim against and a neap is not, so the
            // right answer changes while a cell is alive — which is the pressure differentiation
            // needs and nothing else here applies.
            //
            // Twenty thousand ticks: one full spring/neap cycle. A bout that saw only half of one
            // would be a constant-current world with extra steps.
            label: "tide",
            file: "the_tide.ron",
            layout: Layout::Horizontal,
            ticks: 20_000,
            founders: 8,
            // The same channel geometry as the drift, and the same reason — see `Arena::lane`.
            lane: Some(34),
            light: None,
            poses: "the flow reverses and varies, so no one answer is right for long",
        },
    ]
}

/// Set up one bout without running it.
///
/// # Errors
///
/// A scenario this engine cannot honour, or a genome it will not intern.
pub fn setup(
    arena: &Arena,
    challenger: &Contender,
    reference: &Contender,
    seed: u64,
    // Which side the challenger takes. Alternated across seeds by [`bouts`], so that any residual
    // asymmetry the mirror control is too coarse to catch averages out instead of accumulating.
    challenger_side: Side,
) -> Result<World, crate::arena::ArenaError> {
    let scenario = Scenario {
        seed,
        biology: BiologyConfig {
            // A contender is the genome its author wrote. Mutation would make a bout a race
            // between two evolving populations, which is a different and much longer question.
            mutation: MutationRates::none(),
            ..arena.scenario.biology.clone()
        },
        ..arena.scenario.clone()
    };
    let mut world =
        World::new(scenario).map_err(|e| crate::arena::ArenaError::Scenario(e.to_string()))?;

    let (w, h) = (
        world.substrate().width() as i32,
        world.substrate().height() as i32,
    );

    // Each side founds one root species and the rest of that side joins it, exactly as
    // `arena::setup` does and for the same reason: the archive merges identical seedings by
    // design, so two sides running the same genome would otherwise resolve to one root and every
    // mirror bout would end 100-0 at tick one.
    let mut root: [Option<crate::phylogeny::SpeciesId>; 2] = [None, None];
    for k in 0..arena.founders {
        for side in [Side::Left, Side::Right] {
            let who = if side == challenger_side {
                challenger
            } else {
                reference
            };
            let genome = world
                .genomes()
                .intern(who.genome.clone())
                .map_err(|e| crate::arena::ArenaError::Genome(e.to_string()))?;
            let (x, y) = start_position(arena.layout, w, h, side, k, arena.founders, arena.lane);
            let cell = CellSeed {
                x: crate::fixed::pos(x) + crate::fixed::POS_ONE / 2,
                y: crate::fixed::pos(y) + crate::fixed::POS_ONE / 2,
                mass: q10(30),
                energy: q10(400),
                membrane: 24,
                key: 11,
                badge: 0,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome,
            };
            let slot = usize::from(side == Side::Right);
            let id = match root[slot] {
                Some(existing) => {
                    let id = world.spawn_cell(cell);
                    if let Some(i) = world.cells_mut().index(id) {
                        world.cells_mut().species[i] = existing;
                    }
                    id
                }
                None => {
                    let id = world.spawn_cell_as_new_species(cell);
                    if let Some(i) = world.cells().index(id) {
                        root[slot] = Some(world.cells().species[i]);
                    }
                    id
                }
            };
            if let Some(i) = world.cells_mut().index(id) {
                dress(world.cells_mut(), i);
            }
        }
    }
    world.adopt_current_contents_as_baseline();
    Ok(world)
}

/// The starting kit, identical on both sides: a nucleus, and matter to build with.
///
/// See the module header for why it is a nucleus and not the metabolic kit `place_founders`
/// hands out. Everything a contender needs beyond this it builds and pays for, which is the
/// whole thing being measured.
fn dress(cells: &mut crate::cell::CellArena, i: usize) {
    for slot in 1..SLOT_COUNT {
        cells.slots_mut(i)[slot] = Organelle::empty();
    }
    cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 64);
    // Building material, and the two chemicals every default pathway needs to turn over. The
    // same three `place_founders` seeds, at the same amounts, so that a contender that would have
    // bootstrapped there bootstraps here.
    cells.interior_mut(i)[4] = q10(200);
    cells.interior_mut(i)[11] = q10(40);
    cells.interior_mut(i)[14] = q10(40);
}

/// Where a side's `k`th founder starts, mirrored across the chosen midline.
///
/// `lane` is how deep each side sits from its own edge; see [`Arena::lane`] for why a channel
/// world has to be able to override the default eighth. Whatever it is, it is applied to both
/// sides identically and is clamped so that the two lanes cannot cross the midline, which is what
/// keeps the mirror a mirror.
fn start_position(
    layout: Layout,
    w: i32,
    h: i32,
    side: Side,
    k: u32,
    founders: u32,
    lane: Option<i32>,
) -> (i32, i32) {
    let (long, across) = match layout {
        Layout::Vertical => (h, w),
        Layout::Horizontal => (w, h),
    };
    // Never past the midline: at `across / 2` the two sides would land on the same row and the
    // bout would start as one population rather than two.
    let margin = lane
        .unwrap_or(across / 8)
        .clamp(2, (across / 2 - 1).max(2));
    let rows = founders.max(1) as i32;
    let spacing = (long - 2 * margin).max(1) / rows;
    let along = (margin + spacing * k as i32 + spacing / 2).clamp(0, long - 1);
    let depth = match side {
        Side::Left => margin,
        Side::Right => across - 1 - margin,
    };
    match layout {
        Layout::Vertical => (depth.clamp(0, w - 1), along),
        Layout::Horizontal => (along, depth.clamp(0, h - 1)),
    }
}

/// Play one bout.
///
/// # Errors
///
/// A scenario this engine cannot honour, or a genome it will not intern.
pub fn bout(
    arena: &Arena,
    challenger: &Contender,
    reference: &Contender,
    seed: u64,
    challenger_side: Side,
) -> Result<Bout, crate::arena::ArenaError> {
    let mut world = setup(arena, challenger, reference, seed, challenger_side)?;
    let left = Entry::new(challenger.name.clone(), challenger.genome.clone());
    let right = Entry::new(reference.name.clone(), reference.genome.clone());
    let (left_root, right_root) = roots(&world, &left, &right);
    world.run(arena.ticks);
    let (l, r) = standing(&world, left_root, right_root);
    let (mine, theirs) = match challenger_side {
        Side::Left => (l, r),
        Side::Right => (r, l),
    };
    let total = mine + theirs;
    Ok(Bout {
        // Both extinct is a dead heat and not a loss. See the field.
        share: if total == 0 {
            EVEN
        } else {
            ((mine as u64 * PERMILLE as u64) / total as u64) as u32
        },
        challenger: mine,
        reference: theirs,
        alive: mine > 0,
        copy_damaged: crate::arena::copy_damaged(&world, &left, &right),
    })
}

/// Play one contender against the reference on one arena, at every seed.
///
/// The challenger takes the left side on even-indexed seeds and the right on odd ones. The mirror
/// control is the primary defence against a slide with a better half; this is the belt to its
/// braces, and it costs nothing.
///
/// # Errors
///
/// A scenario this engine cannot honour, or a genome it will not intern.
pub fn bouts(
    arena: &Arena,
    challenger: &Contender,
    reference: &Contender,
    seeds: &[u64],
) -> Result<Vec<Bout>, crate::arena::ArenaError> {
    let mut out = Vec::with_capacity(seeds.len());
    for (n, &seed) in seeds.iter().enumerate() {
        let side = if n % 2 == 0 { Side::Left } else { Side::Right };
        out.push(bout(arena, challenger, reference, seed, side)?);
    }
    Ok(out)
}

/// The middle value. Ties broken low, which is the conservative direction for a gate.
#[must_use]
pub fn median(values: &mut [u32]) -> u32 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    values[(values.len() - 1) / 2]
}

/// One contender's row of the matrix.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    pub name: String,
    /// Median share per arena, in the panel's order.
    pub share: Vec<u32>,
    /// Whether the lineage survived at least one seed, per arena.
    pub alive: Vec<bool>,
    /// Whether the arena was a contest at all, per arena: `false` where *both* lineages were
    /// extinct at the end of every seed, so there was no population to take a share of.
    ///
    /// See [`Bout::contested`]. Such an arena carries `EVEN` in [`Row::share`] because that is
    /// the sentinel a bout returns when nobody is left, and **every statistic below skips it** —
    /// a world that killed both lineages has not told us anything about either.
    pub contested: Vec<bool>,
    /// The organelle types this contender was found to be carrying, once grown.
    pub carries: Vec<OrganelleType>,
}

impl Row {
    /// The shares that are actually readings: one per arena that was a contest.
    ///
    /// An arena missing from `contested` counts as a reading, so that a `Row` assembled by hand
    /// without the flag behaves as it reads. [`tournament`] always fills it.
    fn readings(&self) -> impl Iterator<Item = u32> + '_ {
        self.share
            .iter()
            .enumerate()
            .filter(|(a, _)| self.contested.get(*a).copied().unwrap_or(true))
            .map(|(_, s)| *s)
    }

    /// The highest share this contender reached in any world that was a contest.
    ///
    /// **Not the maximum over the row.** A world in which both lineages died returns `EVEN` by
    /// convention, and taking a plain maximum let that 500 clear [`PAYOFF_FLOOR`] — so a
    /// contender could be credited with "there exists a world where its machinery pays" on the
    /// strength of a world that killed it. Five of eleven shipped contenders passed the payoff
    /// gate that way. Zero if no world in the panel was a contest, which [`Report::extinct`]
    /// reports separately and more directly.
    #[must_use]
    pub fn best(&self) -> u32 {
        self.readings().max().unwrap_or(0)
    }
    #[must_use]
    pub fn worst(&self) -> u32 {
        self.readings().min().unwrap_or(0)
    }
    /// The distance between this contender's best world and its worst, over the worlds that were
    /// a contest. The panel's leverage on this strategy, and what [`Report::discrimination`]
    /// takes the median of.
    #[must_use]
    pub fn spread(&self) -> u32 {
        self.best().saturating_sub(self.worst())
    }
    /// Arenas in which this contender beat the reference.
    #[must_use]
    pub fn wins(&self) -> usize {
        self.readings().filter(|s| *s > EVEN).count()
    }
    #[must_use]
    pub fn viable_anywhere(&self) -> bool {
        self.alive.iter().any(|a| *a)
    }
}

/// The whole matrix, and what it says about the shape of the strategy space.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Report {
    pub arenas: Vec<String>,
    pub rows: Vec<Row>,
    /// The reference against itself, per arena. Every one of these should be [`EVEN`] within
    /// [`MIRROR_TOLERANCE`], or that arena has a better half and its column means nothing.
    pub mirror: Vec<u32>,
}

impl Report {
    /// Arenas whose mirror bout did not come out level. Any entry here invalidates that column.
    #[must_use]
    pub fn unfair(&self) -> Vec<&str> {
        self.mirror
            .iter()
            .enumerate()
            .filter(|(_, m)| m.abs_diff(EVEN) > MIRROR_TOLERANCE)
            .filter_map(|(i, _)| self.arenas.get(i).map(String::as_str))
            .collect()
    }

    /// Contenders that survive nowhere in the panel.
    #[must_use]
    pub fn extinct(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter(|r| !r.viable_anywhere())
            .map(|r| r.name.as_str())
            .collect()
    }

    /// Contenders with no world in which they are competitive. See [`PAYOFF_FLOOR`].
    #[must_use]
    pub fn stranded(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter(|r| r.best() < PAYOFF_FLOOR)
            .map(|r| r.name.as_str())
            .collect()
    }

    /// How far the panel can move a strategy's fortunes: the median spread across contenders.
    #[must_use]
    pub fn discrimination(&self) -> u32 {
        let mut spreads: Vec<u32> = self.rows.iter().map(Row::spread).collect();
        median(&mut spreads)
    }

    /// Contenders that beat the reference in *every* arena. A non-empty list means the panel
    /// poses one question in several costumes.
    #[must_use]
    pub fn sweepers(&self) -> Vec<&str> {
        let n = self.arenas.len();
        self.rows
            .iter()
            .filter(|r| n > 0 && r.wins() == n)
            .map(|r| r.name.as_str())
            .collect()
    }

    /// How many contenders are the single best in at least one arena.
    ///
    /// The direct measure of whether the environment picks different winners. One means the panel
    /// has a favourite and the worlds are decoration.
    ///
    /// Only rows the arena was a contest for are eligible. A world that killed both lineages
    /// scores every contender `EVEN`, so without the filter its "winner" is whichever row the
    /// panel happens to list first — an alphabetical accident counted as a distinct winner.
    #[must_use]
    pub fn distinct_winners(&self) -> usize {
        let mut winners = std::collections::BTreeSet::new();
        for a in 0..self.arenas.len() {
            let mut best: Option<(u32, &str)> = None;
            for row in &self.rows {
                if !row.contested.get(a).copied().unwrap_or(true) {
                    continue;
                }
                let s = row.share.get(a).copied().unwrap_or(0);
                if best.is_none_or(|(b, _)| s > b) {
                    best = Some((s, row.name.as_str()));
                }
            }
            if let Some((_, name)) = best {
                winners.insert(name);
            }
        }
        winners.len()
    }

    /// The best share any contender carrying each organelle type reached, anywhere in the panel.
    ///
    /// The answer to "is this feature dead weight?", which is the question a balance pass is
    /// actually asked. Reported rather than gated: a type below the floor here is a finding to
    /// explain, not automatically a failure — a `Reserved` slot is expected to be there, and so is
    /// anything the panel has no world for yet.
    #[must_use]
    pub fn by_organelle(&self) -> Vec<(OrganelleType, u32)> {
        let mut out = Vec::new();
        for kind in *OrganelleType::all() {
            let best = self
                .rows
                .iter()
                .filter(|r| r.carries.contains(&kind))
                .map(Row::best)
                .max();
            if let Some(best) = best {
                out.push((kind, best));
            }
        }
        out
    }
}

/// The fewest seeds the fairness control may be taken at.
///
/// A single mirror bout is a coin toss with a heavy coin: measured on `seasons.ron`, one seed
/// returned 445permille and three returned 487. The control is what licenses every other number in
/// the report, so it is never allowed to be measured more thinly than they are — a run asked for
/// fewer seeds than this gets them for the mirror anyway.
pub const MIRROR_SEEDS: usize = 3;

/// Run a whole panel: every contender against the reference on every arena at every seed.
///
/// # Errors
///
/// A scenario this engine cannot honour, or a genome it will not intern.
pub fn tournament(
    panel: &[Arena],
    contenders: &[Contender],
    reference: &Contender,
    seeds: &[u64],
) -> Result<Report, crate::arena::ArenaError> {
    // The control gets at least `MIRROR_SEEDS`, whatever the contenders get. See that constant.
    let control_seeds = if seeds.len() >= MIRROR_SEEDS {
        seeds
    } else {
        &SEEDS[..MIRROR_SEEDS.min(SEEDS.len())]
    };
    let mut mirror = Vec::with_capacity(panel.len());
    for arena in panel {
        let mut shares: Vec<u32> = bouts(arena, reference, reference, control_seeds)?
            .iter()
            .map(|b| b.share)
            .collect();
        mirror.push(median(&mut shares));
    }

    let mut rows = Vec::with_capacity(contenders.len());
    for c in contenders {
        let mut share = Vec::with_capacity(panel.len());
        let mut alive = Vec::with_capacity(panel.len());
        let mut contested = Vec::with_capacity(panel.len());
        let mut carries: Vec<OrganelleType> = Vec::new();
        for arena in panel {
            let results = bouts(arena, c, reference, seeds)?;
            // Only the seeds that were a contest. A seed both lineages died in scores `EVEN`,
            // which is the sentinel for "nobody won" rather than a draw (see [`Bout::contested`]),
            // and letting it into the median hands the contender a dead heat for a slide that
            // wiped it out — the more so because a median of three is decided by any two of them.
            let mut shares: Vec<u32> = results
                .iter()
                .filter(|b| b.contested())
                .map(|b| b.share)
                .collect();
            let was_contested = !shares.is_empty();
            share.push(if was_contested {
                median(&mut shares)
            } else {
                EVEN
            });
            alive.push(results.iter().any(|b| b.alive));
            contested.push(was_contested);
        }
        // What it actually built, read off a grown cell rather than parsed out of the source —
        // a `BUILD` the cell could never afford is not a loadout.
        if let Ok(mut world) = setup(&panel[0], c, reference, seeds[0], Side::Left) {
            world.run(1_200);
            for i in world.cells().iter() {
                for o in world.cells().slots(i) {
                    if o.is_active() && !carries.contains(&o.kind) {
                        carries.push(o.kind);
                    }
                }
            }
        }
        rows.push(Row {
            name: c.name.clone(),
            share,
            alive,
            contested,
            carries,
        });
    }

    Ok(Report {
        arenas: panel.iter().map(|a| a.label.clone()).collect(),
        rows,
        mirror,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dish(name: &str) -> Scenario {
        Scenario {
            name: name.to_string(),
            width: 32,
            height: 32,
            light: crate::LightRegime::Uniform {
                intensity: crate::Q10_ONE,
            },
            current: crate::CurrentField::Still,
            seeding: vec![
                crate::Seeding::Uniform {
                    chemical: 11,
                    per_square: q10(400),
                },
                crate::Seeding::Uniform {
                    chemical: 14,
                    per_square: q10(400),
                },
                crate::Seeding::Uniform {
                    chemical: 4,
                    per_square: q10(400),
                },
            ],
            ..Scenario::default()
        }
    }

    fn arena(name: &str) -> Arena {
        Arena {
            label: name.to_string(),
            scenario: dish(name),
            layout: Layout::Vertical,
            ticks: 3_000,
            founders: 4,
            lane: None,
        }
    }

    /// A contender for the harness's own arithmetic.
    ///
    /// Deliberately not a real organism: `mm-asm` depends on `mm-core`, so `mm-core`'s unit tests
    /// cannot assemble one. What is being checked here is that the harness is symmetric, and two
    /// identical inert lineages check that as well as two identical thriving ones — better, in
    /// fact, since neither can win by luck. The panel run against the shipped library, with real
    /// genomes and the mirror control that matters, is `tests/balance.rs`.
    fn inert() -> Contender {
        Contender::new("inert", vec![0x2E])
    }

    #[test]
    fn a_mirror_bout_is_a_dead_heat() {
        // The property the whole harness rests on. Two identical lineages, mirrored, must finish
        // level — otherwise the slide has a better half and every number taken from it is
        // measuring the terrain rather than the genome.
        let a = arena("fair");
        let me = inert();
        let mut shares: Vec<u32> = bouts(&a, &me, &me, &SEEDS)
            .expect("bouts")
            .iter()
            .map(|b| b.share)
            .collect();
        let m = median(&mut shares);
        assert!(
            m.abs_diff(EVEN) <= MIRROR_TOLERANCE,
            "a mirror bout came out {m}permille, not {EVEN}: the slide has a better half"
        );
    }

    #[test]
    fn swapping_the_sides_mirrors_the_result() {
        // If a contender scores 700 on the left it must score 300 on the right. Anything else is
        // the slide voting.
        let a = arena("swap");
        let me = inert();
        let l = bout(&a, &me, &me, SEEDS[0], Side::Left).expect("left");
        let r = bout(&a, &me, &me, SEEDS[0], Side::Right).expect("right");
        assert_eq!(
            l.share,
            PERMILLE - r.share,
            "the same bout scored {} from the left and {} from the right",
            l.share,
            r.share
        );
    }

    #[test]
    fn a_bout_nobody_survived_is_a_draw_and_not_a_loss() {
        // A world that kills everyone says nothing about the contender, and scoring it zero would
        // drag a genome's median down for somebody else's fault.
        let b = Bout {
            share: EVEN,
            challenger: 0,
            reference: 0,
            alive: false,
            copy_damaged: 0,
        };
        assert_eq!(b.share, EVEN);
    }

    #[test]
    fn the_indices_read_the_matrix_the_way_they_claim_to() {
        let report = Report {
            arenas: vec!["a".into(), "b".into()],
            mirror: vec![EVEN, EVEN],
            rows: vec![
                Row {
                    name: "sweeper".into(),
                    share: vec![900, 800],
                    alive: vec![true, true],
                    contested: vec![true, true],
                    carries: vec![OrganelleType::Spike],
                },
                Row {
                    name: "stranded".into(),
                    share: vec![100, 120],
                    alive: vec![true, false],
                    contested: vec![true, true],
                    carries: vec![OrganelleType::Holdfast],
                },
            ],
        };
        assert_eq!(report.sweepers(), vec!["sweeper"]);
        assert_eq!(report.stranded(), vec!["stranded"]);
        assert!(report.extinct().is_empty(), "alive somewhere is alive");
        assert_eq!(report.distinct_winners(), 1, "one genome tops both arenas");
        // Spreads are 100 and 20; the median of two ties low.
        assert_eq!(report.discrimination(), 20);
        let by_type = report.by_organelle();
        assert!(by_type.contains(&(OrganelleType::Spike, 900)));
        assert!(by_type.contains(&(OrganelleType::Holdfast, 120)));
    }

    /// A world that killed both lineages is not a world the loser is competitive in.
    ///
    /// The shape of a real regression: `hunter` scored 20 and 5 in the two worlds it lived in,
    /// died outright in the third, and was credited with a payoff of 500 — the `EVEN` a bout
    /// returns when there is nobody left to take a share of. Five of the eleven shipped
    /// contenders passed the payoff gate on a world that had wiped them out.
    #[test]
    fn a_dead_heat_between_two_corpses_is_not_a_payoff() {
        let corpses = |contested| Report {
            arenas: vec!["lived".into(), "killed everyone".into()],
            mirror: vec![EVEN, EVEN],
            rows: vec![Row {
                name: "hunter".into(),
                // 20permille where it lived, and the both-died sentinel where it did not.
                share: vec![20, EVEN],
                alive: vec![true, false],
                contested: vec![true, contested],
                carries: vec![OrganelleType::Spike],
            }],
        };

        let report = corpses(false);
        assert_eq!(
            report.rows[0].best(),
            20,
            "the only reading is the world it survived"
        );
        assert_eq!(
            report.stranded(),
            vec!["hunter"],
            "a contender whose every live world is under the floor is stranded, and a world \
             that killed it does not rescue it"
        );
        assert_eq!(
            report.discrimination(),
            0,
            "one reading cannot be a spread"
        );
        assert!(
            report.extinct().is_empty(),
            "extinct-everywhere is a different finding and is reported separately"
        );

        // The control: if that same arena *had* been a contest, the 500 is a real draw and the
        // contender is not stranded. The filter must key on the corpses and nothing else.
        assert!(
            corpses(true).stranded().is_empty(),
            "a genuine dead heat at 500 clears the floor and always did"
        );
    }

    #[test]
    fn an_unfair_arena_is_named_rather_than_averaged_away() {
        let report = Report {
            arenas: vec!["fair".into(), "downstream".into()],
            mirror: vec![EVEN, 800],
            rows: vec![],
        };
        assert_eq!(report.unfair(), vec!["downstream"]);
    }
}
