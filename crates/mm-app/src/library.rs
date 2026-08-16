//! Scenario files: finding them, loading them, writing them back (`docs/UI.md` §4).
//!
//! # Why this is a module and not four lines in a menu
//!
//! `docs/UI.md` §4 says there are exactly three kinds of file and the relationship between them
//! is simple: a `.ron` is a `Scenario` and no state, a `.mmslide` is a scenario plus the world
//! that grew from it, and settings never reach the simulation. Until now the front end could
//! read or write none of them — every one of File ▸ New/Open/Save and Slide ▸ Library/Open/Save
//! was a disabled button, and the nine authored scenarios in `scenarios/` could be run by
//! `mm-cli` and by nothing that draws a picture.
//!
//! What made that awkward to fix was never serialisation. `Scenario::from_ron` and
//! `Scenario::to_ron` have been in `mm-core` since M10.2, ISA check included. It was that the
//! interesting parts — where the library lives when the binary is not the one `cargo` just
//! built, and what to say when a file is not what it claims — are decisions worth testing, and
//! nothing inside `main.rs` can be tested at all.
//!
//! So the rule here is the same one `slide.rs` and `ui.rs` follow: no Bevy, no egui, no
//! `World`. `main.rs` calls these and shows what comes back.

use std::path::{Path, PathBuf};

use mm_core::light::{CurrentField, LightRegime};
use mm_core::{Inhabitant, Scenario, ScenarioError, Seeding};

/// What `New scenario…` will build (`docs/UI.md` §9.6).
///
/// The sheet used to offer a size and nothing else, and describe what you would get in a table of
/// five hardcoded strings — one of which said `light  Uniform(intensity: 0)` while the code built
/// the slide at full daylight. Both halves of that were the same mistake: the dialog was
/// *describing* a constant rather than *being* the decision, so there was nothing to keep the
/// description honest.
///
/// It is the decision now, and deliberately not all of it. Light is one uniform intensity here
/// where the build window's `world` view has all six regimes, and the chemistry is the four a
/// slide cannot do without where that view reaches all sixteen. A starting point that can be got
/// wrong and corrected is worth having; a second full editor for the same fields would be a
/// second thing to keep in step with the first.
///
/// Here rather than in `main.rs` for the reason the module header gives: nothing in `main.rs` can
/// be tested, and "the controls produce the scenario they describe" is exactly the claim that
/// wants a test.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NewWorld {
    /// The edge, in squares. The slide is square.
    pub size: u32,
    /// Uniform daylight, `Q10`. 1024 is full.
    pub light: i32,
    /// `(chemical, per square)` for the six a slide cannot do without: carbon, carbon dioxide
    /// and the oxidant — a body and what a chloroplast runs on — plus the three minerals nothing
    /// in the engine produces.
    ///
    /// Silicon is here for a different reason from the other three, and the reason is a bug it
    /// is fixing: it is not something every cell needs, it is something *no* cell could obtain.
    /// See [`NewWorld::default`].
    pub chemistry: [(usize, i32); 9],
    /// Who lives here, as a name the scenario can write down and [`mm_asm::locate`] can find.
    pub genome: String,
    /// Zero is a legal answer, and is the one you want when the point is to draw the world
    /// first.
    pub founders: u32,
}

/// Carbon: what a cell builds its body out of, and so the one a slide is most often short of.
pub const CARBON: usize = 4;
/// Carbon dioxide, and the oxidant that goes with it. What a chloroplast runs on.
pub const CARBON_DIOXIDE: usize = 11;
/// The oxidant.
pub const OXIDANT: usize = 14;
/// Silicon: what a shell is made of.
pub const SILICON: usize = 7;
/// Nitrogen: what protein is made of, so what the enzymatic machinery costs.
pub const NITROGEN: usize = 5;
/// Phosphorus: what a nucleus costs, and the one chemical that does not move.
pub const PHOSPHORUS: usize = 6;
/// Dinitrogen: the inert pool, and the only thing a diazosome is for.
pub const DINITROGEN: usize = mm_core::chem::DINITROGEN;
/// Calcium: the fourth mineral, and half of a calcite test.
pub const CALCIUM: usize = mm_core::chem::CALCIUM;
/// Carbonate: the buffer, and the other half.
pub const CARBONATE: usize = mm_core::chem::CARBONATE;

impl Default for NewWorld {
    fn default() -> Self {
        NewWorld {
            size: 256,
            light: mm_core::Q10_ONE,
            // The fresh slide's levels, and — since `petri_of` is written in terms of this — the
            // only definition of them. The number being changed in the sheet therefore has
            // something to be a change *from*.
            //
            // **Forty a square, not the four hundred `scenarios/soup.ron` still records.** That
            // number was never measured, it was picked; `docs/CHEMISTRY.md` §6 puts the knee at
            // roughly four to ten a square, two orders of magnitude below it. Four hundred was
            // not a well-fed world so much as one with most of its matter parked in the water
            // where nothing competed for it.
            //
            // Every level above forty **rings** — 400, 100, 60 and 50 all overshoot at about tick
            // 15,000 and starve back, differing only in period, and 100's period is long enough
            // that at 60,000 ticks it reads as perfectly flat and is not. Forty is settled and
            // earned: 200,000 ticks on three seeds, `--check` clean, 23,342 / 22,888 / 23,711,
            // drifting under a quarter of a percent across the last fifty thousand ticks of each.
            // What makes that a plateau rather than another phase sample is where the peak sits —
            // 130,000 to 150,000, so a hundred thousand ticks of flat lie behind the final
            // reading, where the 100 world had none behind its apparent one.
            //
            // The reason is not the expected one. **Forty never overshoots**: it reaches capacity
            // from below, so it never makes the death pulse that puts an unusual amount of matter
            // into the decay chain at once, and so has no trough to climb out of. It also carries
            // about 1,500 lineages where the 100 world carried 664, because a slide that troughs
            // loses lineages in every trough. §7 has both tables.
            //
            // `soup.ron` and the rest of `scenarios/` stay at four hundred: the soup is the
            // control condition every other world is a variation on, and moving it would move
            // every comparison made against it.
            //
            // **Silicon is here because without it a shell cannot be built at all.** ISA 7 gave
            // the shell a recipe — `build_trace[7]`, the only non-zero entry in the catalogue —
            // and `biology::resolve` refuses a build whose ingredients are absent. Nothing
            // produces silicon, so a slide that does not seed it is a slide where armour is
            // unreachable however a lineage evolves. `scenarios/the_scattering.ron` was the only
            // world in the repo that seeded any.
            //
            // Twenty, on two arguments that happen to agree. It is what `the_scattering` seeds,
            // which is the only figure in the repo; and the shell's own recipe asks for `q10(6)`
            // silicon against `q10(13)` carbon, so twenty against carbon's forty is roughly the
            // proportion the organelle consumes them in. That makes silicon comfortably
            // available rather than contested, which is the right way round for now: the point
            // of this number is to make armour *possible*, and how scarce it should be is a
            // question for the sweep `docs/CHEMISTRY.md` §8 asks for. §6's lesson is that
            // guessing a level is how the soup ended up at four hundred.
            chemistry: [
                (CARBON, mm_core::fixed::q10(40)),
                (CARBON_DIOXIDE, mm_core::fixed::q10(40)),
                (OXIDANT, mm_core::fixed::q10(40)),
                // Twenty, against silicon's ceiling of forty — **below the line on purpose**,
                // and the reasoning is worth having because both directions are wrong in
                // different ways.
                //
                // *Above* it and the world precipitates: a flowing slide concentrates silica
                // wherever the current converges, deposition ratchets, and `the_drift` paved a
                // fifth of itself in two thousand ticks. *At* it is metastable for the same
                // reason — no headroom, so any convergence tips it over. Below it the water is
                // mildly corrosive to silica, which is what undersaturated water is, and rock
                // dissolves into it slowly.
                //
                // The consequence is real and is not a fault: a reef in this water **wears from
                // its rim inwards**. Measured on a seven-by-seven blob, the middle keeps every
                // unit it was laid with — it has no open neighbour to dissolve into — while the
                // rim goes to nothing over about eight thousand ticks. That is a reef, and a wall
                // that is meant to be permanent is `Barrier`, which holds no mineral and has
                // nothing to give up. See `World::rock_dose` for the other half of this.
                (SILICON, mm_core::fixed::q10(20)),
                // The carbonate system (`docs/CHEMISTRY.md` §11).
                //
                // **Carbonate is matched to the dissolved CO₂ above rather than chosen**, and
                // that is the whole of the number. pH is derived from the ratio of the two
                // (`chem::ph_of`), so equal pools read as exactly neutral — which makes a fresh
                // slide start at seven and every move away from it something the cells did. A
                // figure picked independently would set the slide's resting pH to whatever the
                // ratio happened to be, and every reading afterwards would be measured from a
                // baseline nobody chose.
                //
                // Calcium is then **not** free either, and this is the number that trips people
                // up. The pair precipitates on the *product* of the two, so once carbonate is
                // pinned by the pH anchor, calcium is what decides whether the slide sits above
                // or below `minerals.calcite_saturation`. Twice the amount that puts
                // `sqrt(calcium x carbonate)` exactly on the line — near enough to equilibrium
                // that the pH decides which way it goes, which is the whole point of the
                // coupling, and far enough over that a lit mat has something to lay down.
                //
                // Unlike the three minerals above these are not Redfield quantities. Nothing is
                // built out of them except a calcite test; what they are mostly for is to be
                // water chemistry.
                (CARBONATE, mm_core::fixed::q10(40)),
                (CALCIUM, mm_core::fixed::q10(34)),
                // Nitrogen and phosphorus, at the Redfield proportion of the carbon above.
                //
                // Organisms hold C : N : P at roughly 106 : 16 : 1 because that is what the
                // machinery is made of, and `organelle::nitrogen_trace` writes those proportions
                // into the catalogue — so seeding them in the same proportion is the only figure
                // that is not a guess. Against carbon's forty that is six of nitrogen and about
                // four tenths of phosphorus.
                //
                // Phosphorus is rounded up to a whole unit rather than left at the strict 0.38,
                // and deliberately so: it is the one chemical that **does not move**, so a cell
                // can only ever use what is standing on its own square, and a first landing that
                // made it binding before anybody had swept it would be §6's mistake told the
                // other way round. Scarce enough to matter is a question for the sweep; this is
                // the level that lets the mechanism exist.
                (NITROGEN, mm_core::fixed::q10(6)),
                (PHOSPHORUS, mm_core::fixed::q10(1)),
                // The inert reservoir, four times the bioavailable pool. A young world is
                // mostly locked up and a diazosome is the only key — but not so locked that a
                // slide with no diazotroph on it cannot start, which is the bootstrapping trap
                // this level exists to stay out of: nothing in `genomes/` grows one, so a first
                // landing that made bioavailable nitrogen scarce would kill the library rather
                // than reward fixing.
                (DINITROGEN, mm_core::fixed::q10(24)),
            ],
            genome: "ancestor.mm".to_string(),
            founders: 0,
        }
    }
}

impl NewWorld {
    /// The recipe this describes.
    ///
    /// Nothing is placed here. `World::new` seeds the chemistry from the `seeding` list, and the
    /// front end's `seed_into` places the inhabitants it names — the same path a scenario opened
    /// from the library takes, so a slide built from the sheet and one opened from a file are
    /// populated by one piece of code rather than two.
    #[must_use]
    pub fn scenario(&self) -> Scenario {
        Scenario {
            name: "untitled".to_string(),
            seed: 1,
            width: self.size,
            height: self.size,
            light: LightRegime::Uniform {
                intensity: self.light,
            },
            current: CurrentField::Still,
            // A chemical set to nothing is left out rather than written as zero: a recipe that
            // lists what it does not contain says less than one that does not mention it.
            seeding: self
                .chemistry
                .iter()
                .filter(|(_, per_square)| *per_square > 0)
                .map(|(chemical, per_square)| Seeding::Uniform {
                    chemical: *chemical,
                    per_square: *per_square,
                })
                .collect(),
            inhabitants: if self.founders == 0 || self.genome.trim().is_empty() {
                Vec::new()
            } else {
                vec![Inhabitant {
                    genome: self.genome.trim().to_string(),
                    count: self.founders,
                    // Spread, because founders piled on one square are a pile and not a
                    // population. The seed tool is how you say *where*.
                    place: mm_core::Placement::Spread,
                }]
            },
            ..Scenario::default()
        }
    }
}

/// `270 squares` is an edge and it reads as an area. Say which.
///
/// Both `New…` sheets offer one number, because the slide is square, and suffix it ` squares` —
/// so a slide of 72,900 squares announces itself as 270 of them, wrong by the side length.
/// Nothing downstream was affected; the label was, and it is the first number anybody setting up
/// a world reads.
#[must_use]
pub fn size_reading(size: u32) -> String {
    let digits = (u64::from(size) * u64::from(size)).to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{size} × {size} — {grouped} squares in all")
}

/// Where the shipped scenarios are, tried in order.
///
/// This used to say that a binary somebody installs "will find neither and gets an empty library
/// rather than an error, which is the right failure". That was written when nobody could install
/// one. With releases it stopped being a graceful degradation and became an empty library for
/// everybody who did not build the thing themselves — so the roots come from
/// [`mm_asm::locate`], which also looks beside the executable and inside a macOS bundle.
#[must_use]
pub fn search_paths() -> Vec<PathBuf> {
    mm_asm::locate::search_roots()
        .into_iter()
        .map(|root| root.join("scenarios"))
        .collect()
}

/// One scenario the library found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// What to show: the file stem, so `the_long_dusk.ron` reads as `the long dusk`.
    pub label: String,
    pub path: PathBuf,
}

/// Every `.ron` in the first search path that has any, sorted by name.
///
/// Sorted because a directory listing is in whatever order the filesystem felt like, and a menu
/// whose items move between runs is a menu you cannot learn. The first directory that exists
/// wins outright rather than the results being merged, so a working directory with its own
/// `scenarios/` shadows the built-in set instead of doubling it.
#[must_use]
pub fn scenarios() -> Vec<Entry> {
    for dir in search_paths() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut found: Vec<Entry> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "ron"))
            .filter_map(|path| {
                let stem = path.file_stem()?.to_str()?.to_string();
                Some(Entry {
                    label: stem.replace('_', " "),
                    path,
                })
            })
            .collect();
        if found.is_empty() {
            continue;
        }
        found.sort_by(|a, b| a.label.cmp(&b.label));
        return found;
    }
    Vec::new()
}

/// Every `.mm` in `genomes/`, sorted, as file names.
///
/// The seeding tool takes a genome by *name* — `Inhabitant.genome` is a path the scenario writes
/// down and the caller resolves, because `mm-core` has no filesystem — and until now the only way
/// to supply one was to type it into a box hinting `ancestor.mm`. Eighteen files ship in
/// `genomes/` and the interface named exactly one of them, so the other seventeen were reachable
/// only by somebody who had gone and listed the directory themselves.
///
/// File names rather than [`Entry`], because a genome's name is what has to reach the scenario
/// verbatim: `ancestor.mm` prettified to `ancestor` is a string the assembler cannot find again.
/// The same first-directory-wins rule as [`scenarios`], for the same reason.
#[must_use]
pub fn genomes() -> Vec<String> {
    for root in mm_asm::locate::search_roots() {
        let Ok(read) = std::fs::read_dir(root.join("genomes")) else {
            continue;
        };
        let mut found: Vec<String> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "mm"))
            .filter_map(|p| Some(p.file_name()?.to_str()?.to_string()))
            .collect();
        if found.is_empty() {
            continue;
        }
        found.sort();
        return found;
    }
    Vec::new()
}

/// The directory the scenarios came from, or `None` when none of them had any.
///
/// So the sheet can name the place it actually read rather than the place it used to be the only
/// candidate for. When this was one hard-coded `./scenarios`, a header saying so was true; with
/// four roots it is a guess, and an empty library that blames the working directory sends
/// somebody looking in the wrong place.
#[must_use]
pub fn source_dir() -> Option<PathBuf> {
    search_paths().into_iter().find(|dir| {
        std::fs::read_dir(dir).is_ok_and(|mut read| {
            read.any(|e| {
                e.is_ok_and(|e| e.path().extension().is_some_and(|ext| ext == "ron"))
            })
        })
    })
}

/// Every directory that was tried, for an error message that can be acted on.
#[must_use]
pub fn searched() -> Vec<PathBuf> {
    search_paths()
}

/// What went wrong, in a sentence a person can act on.
///
/// Its own type rather than `String` so the caller can tell "no such file" from "that is not a
/// scenario" — the first is a typo and the second means the file is something else, and a menu
/// that reported both as "could not open" would leave you guessing which.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FileError {
    Io(String),
    Scenario(String),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::Io(m) | FileError::Scenario(m) => write!(f, "{m}"),
        }
    }
}

fn describe(path: &Path, e: &ScenarioError) -> FileError {
    let name = path.display();
    FileError::Scenario(match e {
        ScenarioError::IsaMismatch { scenario, engine } => format!(
            "{name} was written for ISA version {scenario} and this build speaks {engine}. \
             Its genomes would mean something different — see SPEC §16."
        ),
        ScenarioError::Parse(m) => format!("{name} is not a scenario: {m}"),
        ScenarioError::Substrate(m) => format!("{name} describes a slide that cannot exist: {m:?}"),
        // Its own message already names the path, and the path is the whole diagnosis.
        ScenarioError::BadPath(_) => format!("{name}: {e}"),
    })
}

/// Read a scenario from a `.ron`.
///
/// # Errors
///
/// If the file cannot be read, or is not a scenario this build can run.
pub fn load(path: &Path) -> Result<Scenario, FileError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| FileError::Io(format!("cannot read {}: {e}", path.display())))?;
    // Through the ruleset library, not `Scenario::from_ron` — the difference is only visible for
    // a file that names a ruleset, and there it is the difference between running the economy the
    // file asked for and silently running the default one. See `mm_core::ruleset`.
    rulesets(path).load_scenario(&text).map_err(|e| match e {
        // A file that will not parse is not a scenario, and says so in those words — the
        // resolver's own message would say "ruleset does not parse", which is true and unhelpful
        // when what the reader has is a file of rubbish.
        mm_core::ruleset::RulesetError::Parse(m) => describe(path, &ScenarioError::Parse(m)),
        // Naming a ruleset that is missing, circular or full of typos is a fault in the scenario
        // and not in the reader's disk, so it is reported as one.
        other => FileError::Scenario(format!("{}: {other}", path.display())),
    })
}

/// Every ruleset that applies to a scenario at `path`: a `rulesets/` beside its directory, then
/// one in the working directory.
///
/// Missing is not an error — a tree with no `rulesets/` is one where no scenario names a set, and
/// a scenario that names one that is not there fails at resolution with the name in the message.
fn rulesets(path: &Path) -> mm_core::ruleset::RulesetLibrary {
    let mut library = mm_core::ruleset::RulesetLibrary::new();
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(parent) = path.parent().and_then(Path::parent) {
        roots.push(parent.join("rulesets"));
    }
    // Then wherever the app's own library lives. **Not just the working directory**, which is
    // what this did — and what made a scenario saved anywhere but inside the project tree a file
    // that would not reopen. That did not show while every saved scenario was written complete
    // and inherited nothing; a file that names a ruleset needs the set to be findable from
    // wherever it was put, and the installed `rulesets/` is where it is.
    roots.extend(ruleset_paths());
    for root in roots {
        read_rulesets_into(&mut library, &root);
        if !library.is_empty() {
            break;
        }
    }
    library
}

/// Where rulesets live, in the same first-directory-wins order as [`search_paths`].
#[must_use]
pub fn ruleset_paths() -> Vec<PathBuf> {
    mm_asm::locate::search_roots()
        .into_iter()
        .map(|root| root.join("rulesets"))
        .collect()
}

/// Every named ruleset the app can see, resolved.
///
/// The front end had no way to reach these at all: [`rulesets`] was built at load, used once and
/// dropped, so the interface could run the economy a scenario named and never say which one it
/// was, what else was available, or what any of them changed. That is most of what the rules page
/// exists to answer.
///
/// Resolved here rather than in the caller because resolution can fail — a cycle, a typo in a
/// path — and a set that will not resolve is one the editor must not offer as a baseline. Those
/// are dropped, with their names kept in the second return value so the interface can say a file
/// was refused rather than silently listing one fewer.
#[must_use]
pub fn ruleset_choices() -> (Vec<Choice>, Vec<String>) {
    let library = ruleset_library();
    let mut good = Vec::new();
    let mut bad = Vec::new();
    for name in library.names() {
        let Some(set) = library.get(name) else {
            continue;
        };
        match library.rules(name) {
            Ok(rules) => good.push(Choice {
                name: name.to_string(),
                title: set.name.clone(),
                notes: set.notes.clone(),
                of: set.of.clone(),
                changes: set.set.len(),
                rules,
            }),
            Err(e) => bad.push(format!("{name}: {e}")),
        }
    }
    (good, bad)
}

/// One named ruleset, resolved, as the parameter editor needs it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Choice {
    /// The file stem, which is what a scenario's `ruleset:` field names.
    pub name: String,
    /// What the document calls itself, for reading.
    pub title: String,
    pub notes: String,
    /// The set it is a diff against, or empty for the engine's own numbers.
    pub of: String,
    /// How many parameters it names in its own right, before inheritance.
    pub changes: usize,
    /// What it comes to, with its `of` chain applied.
    pub rules: mm_core::ruleset::Rules,
}

/// The whole ruleset library, from wherever it is.
///
/// The same first-directory-wins rule as [`scenarios`]. [`rulesets`] is the per-scenario version
/// and looks beside the file being opened; this one answers "what sets exist" with no scenario in
/// hand, which is what the parameter editor is asking.
#[must_use]
pub fn ruleset_library() -> mm_core::ruleset::RulesetLibrary {
    let mut library = mm_core::ruleset::RulesetLibrary::new();
    for root in ruleset_paths() {
        read_rulesets_into(&mut library, &root);
        if !library.is_empty() {
            break;
        }
    }
    library
}

/// Every `.ron` in `root`, inserted by file stem. Missing or unreadable is not an error.
fn read_rulesets_into(library: &mut mm_core::ruleset::RulesetLibrary, root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    // Sorted: the order a filesystem hands files back is not a thing a run may depend on.
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ron"))
        .collect();
    files.sort();
    for file in files {
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if let Ok(text) = std::fs::read_to_string(&file) {
            let _ = library.insert(&stem, &text);
        }
    }
}

/// Write a named ruleset out, into the first `rulesets/` directory that exists.
///
/// The other half of "a ruleset is a diff, not a document": `mm_core::params::diff` computes what
/// a world changed, and this puts it somewhere every other world can name. Until now a ruleset
/// could only be written in a text editor, which meant that the numbers arrived at by moving
/// sliders in the parameter editor had to be copied out by hand — and a number copied by hand is
/// a number that can be copied wrong.
///
/// # Errors
///
/// If the name is empty or not a legal file stem, or the file cannot be written.
pub fn save_ruleset(
    dir: &Path,
    name: &str,
    of: &str,
    notes: &str,
    changes: &std::collections::BTreeMap<String, mm_core::params::Value>,
) -> Result<PathBuf, FileError> {
    let stem = name.trim().replace(' ', "_");
    // Refused rather than sanitised further: a name with a slash in it is somebody meaning a
    // path, and quietly writing somewhere else is worse than saying no.
    if stem.is_empty() || stem.contains(['/', '\\']) {
        return Err(FileError::Io(format!(
            "`{name}` is not a name a ruleset file can have"
        )));
    }
    let path = dir.join(format!("{stem}.ron"));
    let text = mm_core::ruleset::Ruleset::from_diff(name.trim(), of, notes, changes)
        .to_ron()
        .map_err(|e| FileError::Scenario(format!("cannot write this ruleset out: {e}")))?;
    std::fs::create_dir_all(dir)
        .map_err(|e| FileError::Io(format!("cannot make {}: {e}", dir.display())))?;
    std::fs::write(&path, text)
        .map_err(|e| FileError::Io(format!("cannot write {}: {e}", path.display())))?;
    Ok(path)
}

/// Where a new ruleset goes: the first `rulesets/` that exists, or one beside the working
/// directory if none does.
#[must_use]
pub fn ruleset_dir() -> PathBuf {
    ruleset_paths()
        .into_iter()
        .find(|d| d.is_dir())
        .unwrap_or_else(|| PathBuf::from("rulesets"))
}

/// The text [`save`] will write — so the preview and the file cannot disagree.
///
/// `delta` chooses between the two forms `mm_core::scenario` documents at length:
///
/// * **the delta** — only what this world changes, against the engine's defaults with whatever
///   ruleset it names resolved into them. Fifteen lines instead of four hundred and thirty-six,
///   and the form every hand-written file in `scenarios/` is already in.
/// * **complete** — every field. What a `.mmslide` embeds, and what to use for a file that must
///   still mean the same thing after somebody edits a ruleset.
///
/// # Errors
///
/// If the scenario cannot be serialised, or names a ruleset this library does not have — which is
/// refused rather than written against the engine's defaults instead, because a file saved
/// against the wrong baseline is a file that reopens as a different world.
pub fn scenario_ron(scenario: &Scenario, delta: bool) -> Result<String, FileError> {
    if !delta {
        return scenario
            .to_ron()
            .map_err(|e| FileError::Scenario(format!("cannot write this scenario out: {e}")));
    }
    let base = ruleset_library()
        .baseline(&scenario.ruleset)
        .map_err(|e| FileError::Scenario(format!("cannot write a delta: {e}")))?;
    scenario
        .to_ron_sparse(&base)
        .map_err(|e| FileError::Scenario(format!("cannot write this scenario out: {e}")))
}

/// Write a scenario out as a `.ron`.
///
/// Adds the extension when it is missing, because a file named `vent` that is RON inside is a
/// file the library will not list and the loader will not offer.
///
/// # Errors
///
/// If the scenario cannot be serialised, or the file cannot be written.
pub fn save(path: &Path, scenario: &Scenario, delta: bool) -> Result<PathBuf, FileError> {
    let path = with_extension(path, "ron");
    let text = scenario_ron(scenario, delta)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| FileError::Io(format!("cannot make {}: {e}", parent.display())))?;
        }
    }
    std::fs::write(&path, text)
        .map_err(|e| FileError::Io(format!("cannot write {}: {e}", path.display())))?;
    Ok(path)
}

/// The path with `ext` on the end, unless it already has it.
///
/// Case-insensitively, so `SOUP.RON` is not given a second extension.
#[must_use]
pub fn with_extension(path: &Path, ext: &str) -> PathBuf {
    match path.extension().and_then(|e| e.to_str()) {
        Some(have) if have.eq_ignore_ascii_case(ext) => path.to_path_buf(),
        _ => {
            let mut s = path.as_os_str().to_os_string();
            s.push(".");
            s.push(ext);
            PathBuf::from(s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_finds_the_shipped_scenarios() {
        let found = scenarios();
        assert!(
            found.len() >= 8,
            "the library found {} scenarios; there are nine in the repository",
            found.len()
        );
        assert!(
            found.iter().any(|e| e.label == "soup"),
            "soup is missing: {:?}",
            found.iter().map(|e| &e.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_library_is_sorted_and_readable() {
        let found = scenarios();
        let labels: Vec<&str> = found.iter().map(|e| e.label.as_str()).collect();
        let mut sorted = labels.clone();
        sorted.sort_unstable();
        assert_eq!(labels, sorted, "a menu whose items move cannot be learned");
        assert!(
            !labels.iter().any(|l| l.contains('_')),
            "underscores reached the menu: {labels:?}"
        );
    }

    /// The picker lists names, and a name has to be one the assembler can find again.
    #[test]
    fn the_genome_list_is_names_that_can_be_seeded() {
        let found = genomes();
        assert!(
            found.len() >= 8,
            "the library found {} genomes; there are eighteen in the repository",
            found.len()
        );
        assert!(
            found.contains(&"ancestor.mm".to_string()),
            "the default founder is missing: {found:?}"
        );
        let mut sorted = found.clone();
        sorted.sort();
        assert_eq!(found, sorted, "a picker whose items move cannot be learned");
        for name in &found {
            assert!(
                name.ends_with(".mm"),
                "{name} is not a name the seeding tool could hand to the assembler"
            );
            let Some(path) = mm_asm::locate::file("genomes", name) else {
                panic!("{name} is offered by the picker and cannot be found again");
            };
            // And found is not enough: the seed tool assembles what it is handed, and a name
            // that does not assemble reaches the user as "did not assemble" from a list the
            // program itself wrote. If a broken `.mm` lands in `genomes/`, it should fail here
            // rather than in somebody's hand.
            let src = std::fs::read_to_string(&path).expect("the picker offered an unreadable file");
            assert!(
                mm_asm::assemble(&src).is_ok(),
                "{name} is offered by the picker and does not assemble"
            );
        }
    }

    /// The claim the sheet makes: the controls are the world you get.
    ///
    /// The version this replaces described itself in five hardcoded strings, one of which said
    /// the slide would be dark while the code built it at full daylight. Nothing could have
    /// caught that, because there was nothing to compare the description against.
    #[test]
    fn the_new_scenario_sheet_builds_the_world_it_describes() {
        let want = NewWorld {
            size: 270,
            light: mm_core::Q10_ONE * 4 / 5,
            chemistry: [
                (CARBON, mm_core::fixed::q10(40)),
                (CARBON_DIOXIDE, mm_core::fixed::q10(400)),
                (OXIDANT, mm_core::fixed::q10(400)),
                // Zero, so this test also covers a chemical being dropped from the recipe while
                // the others are kept — the case the test below checks in isolation.
                (SILICON, 0),
                (NITROGEN, 0),
                (PHOSPHORUS, 0),
                (DINITROGEN, 0),
                (CARBONATE, 0),
                (CALCIUM, 0),
            ],
            genome: "predator.mm".to_string(),
            founders: 6,
        };
        let s = want.scenario();
        assert_eq!((s.width, s.height), (270, 270));
        assert_eq!(s.light, LightRegime::Uniform { intensity: 819 });
        assert_eq!(s.current, CurrentField::Still);
        assert_eq!(
            s.seeding,
            vec![
                Seeding::Uniform {
                    chemical: CARBON,
                    per_square: mm_core::fixed::q10(40)
                },
                Seeding::Uniform {
                    chemical: CARBON_DIOXIDE,
                    per_square: mm_core::fixed::q10(400)
                },
                Seeding::Uniform {
                    chemical: OXIDANT,
                    per_square: mm_core::fixed::q10(400)
                },
            ]
        );
        assert_eq!(
            s.inhabitants,
            vec![Inhabitant {
                genome: "predator.mm".to_string(),
                count: 6,
                place: mm_core::Placement::Spread
            }]
        );
        assert!(s.barriers.is_empty(), "nothing is drawn on it yet");
    }

    /// **The default world must seed every ingredient the catalogue can ask for.**
    ///
    /// Written after ISA 7 shipped an organelle nobody could build. The shell's recipe is
    /// `build_trace[7] = q10(6)` silicon, `biology::resolve` refuses a build whose ingredients
    /// are absent, and nothing in the engine produces silicon — so on every slide that did not
    /// seed it, a lineage that evolved armour paid for a `BUILD` that could never complete. One
    /// scenario in the whole repo seeded any.
    ///
    /// A test about *silicon* would have been the wrong test. The fault is not that this
    /// ingredient was missed, it is that adding a recipe and seeding the world it needs are two
    /// edits in two crates with nothing tying them together — so this is quantified over the
    /// catalogue rather than written against a list. Fill another `build_trace` entry and this
    /// fails until the fresh slide can supply it.
    ///
    /// It cannot see `petri_of`, which lives in `main.rs` and so is untestable; that is why
    /// `petri_of` is written in terms of this type rather than repeating its own literals.
    #[test]
    fn the_default_world_seeds_every_ingredient_the_catalogue_needs() {
        let catalogue = mm_core::OrganelleCatalogue::default();
        let recipe = NewWorld::default().scenario().seeding;
        // Every entry has to be one this test understands, or it could pass by ignoring the one
        // that mattered. A non-uniform seeding is a legal thing for the sheet to grow, and if it
        // does, the per-square reasoning below needs rethinking rather than extending.
        assert!(
            recipe
                .iter()
                .all(|s| matches!(s, Seeding::Uniform { .. })),
            "the default world seeds something other than a uniform level; this test reads \
             per-square availability and cannot speak for a gradient or a patch"
        );
        let seeded: std::collections::BTreeMap<usize, i32> = recipe
            .iter()
            .filter_map(|s| match s {
                Seeding::Uniform {
                    chemical,
                    per_square,
                } => Some((*chemical, *per_square)),
                _ => None,
            })
            .collect();

        for kind in mm_core::OrganelleType::all() {
            let spec = catalogue.spec(*kind);
            for c in 0..mm_core::chem::CHEM_COUNT {
                // At `param` 0, which is the cheapest any of them gets. An ingredient needed at
                // all is an ingredient that has to be there.
                let needed = spec.trace_cost(c, 0);
                if needed <= 0 {
                    continue;
                }
                let held = seeded.get(&c).copied().unwrap_or(0);
                assert!(
                    held >= needed,
                    "a {} needs {needed} of chemical {c} to build and the fresh slide seeds \
                     {held}: nothing in the engine produces it, so that organelle can never be \
                     built on the world the microscope opens on",
                    kind.name()
                );
            }
        }
    }

    /// A slide with nothing in the water is a legal and useful thing to ask for, and it has to
    /// come out as a recipe that *says nothing* rather than one listing three zeroes.
    #[test]
    fn a_chemical_set_to_nothing_is_left_out_of_the_recipe() {
        let s = NewWorld {
            chemistry: [
                (CARBON, 0),
                (CARBON_DIOXIDE, 0),
                (OXIDANT, 0),
                (SILICON, 0),
                (NITROGEN, 0),
                (PHOSPHORUS, 0),
                (DINITROGEN, 0),
                (CARBONATE, 0),
                (CALCIUM, 0),
            ],
            ..NewWorld::default()
        }
        .scenario();
        assert!(s.seeding.is_empty());
    }

    /// Zero founders is the empty dish the sheet is for, and it must not name a genome nobody
    /// asked to be placed.
    #[test]
    fn nobody_home_is_no_inhabitants_and_not_a_count_of_zero() {
        for want in [
            NewWorld {
                founders: 0,
                ..NewWorld::default()
            },
            NewWorld {
                founders: 8,
                genome: "   ".to_string(),
                ..NewWorld::default()
            },
        ] {
            assert!(want.scenario().inhabitants.is_empty());
        }
    }

    /// The whole point of the reading: the number on the dial is an edge.
    #[test]
    fn the_size_reading_says_edge_and_area_and_not_one_pretending_to_be_the_other() {
        assert_eq!(size_reading(270), "270 × 270 — 72,900 squares in all");
        assert_eq!(size_reading(16), "16 × 16 — 256 squares in all");
        assert_eq!(size_reading(1024), "1024 × 1024 — 1,048,576 squares in all");
    }

    /// Whatever the sheet builds has to be a scenario the engine will actually accept, and one
    /// that survives being written down — it is the thing Save writes.
    #[test]
    fn a_sheet_built_world_loads_and_round_trips() {
        let want = NewWorld {
            size: 32,
            light: 819,
            founders: 3,
            ..NewWorld::default()
        };
        let s = want.scenario();
        mm_core::World::new(s.clone()).expect("the sheet built a world that cannot exist");
        let back = Scenario::from_ron(&s.to_ron().unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn every_shipped_scenario_actually_loads() {
        // The library listing a file it cannot open is worse than not listing it, and this is
        // the only place that would notice a scenario going stale against the engine — an ISA
        // bump, a renamed field, a slide size the substrate refuses.
        for entry in scenarios() {
            if let Err(e) = load(&entry.path) {
                panic!("{} is in the library and does not load: {e}", entry.label);
            }
        }
    }

    #[test]
    fn a_scenario_survives_the_round_trip_through_a_file() {
        let dir = std::env::temp_dir().join("mm-library-test");
        let _ = std::fs::remove_dir_all(&dir);
        let original = load(&scenarios()[0].path).expect("a shipped scenario");
        // No extension given, so `save` has to supply one.
        let written = save(&dir.join("nested/round-trip"), &original, false).expect("write");
        assert_eq!(written.extension().and_then(|e| e.to_str()), Some("ron"));
        let back = load(&written).expect("read it back");
        assert_eq!(back, original, "a scenario changed by being written down");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_shipped_scenario_survives_being_saved_as_a_delta() {
        // The whole library through the new default, because this is the operation that used to
        // be lossy in the other direction: open a file, save it, and get four hundred and
        // thirty-six lines that no longer inherit anything. Every world has to come back the
        // same world, ruleset and all.
        let dir = std::env::temp_dir().join("mm-library-delta-test");
        let _ = std::fs::remove_dir_all(&dir);
        for entry in scenarios() {
            let original = load(&entry.path).expect("a shipped scenario");
            let written =
                save(&dir.join(&entry.label), &original, true).expect("write the delta");
            let mut back = load(&written).expect("read it back");

            // `set` is what the file said, kept after it has been applied — provenance, the way
            // `ruleset` is. A world that wrote its changes as paths carries them; the one it was
            // written from said the same thing in an inline block or not at all. What has to
            // match is everything the simulation reads, which is everything else.
            assert_eq!(
                mm_core::ruleset::Rules::of(&back),
                mm_core::ruleset::Rules::of(&original),
                "{}: the rules changed by being saved as a delta",
                entry.label
            );
            back.set = original.set.clone();
            assert_eq!(
                back, original,
                "{} changed by being saved as a delta",
                entry.label
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_delta_is_a_fraction_of_the_complete_form() {
        // The number that is the point of the exercise. Not a fixed threshold on either — the
        // library will grow — but a saved scenario that is not dramatically shorter than the
        // engine's own numbers written out is one where this has quietly stopped working.
        let scenario = load(&scenarios()[0].path).expect("a shipped scenario");
        let delta = scenario_ron(&scenario, true).expect("delta");
        let complete = scenario_ron(&scenario, false).expect("complete");
        assert!(
            delta.lines().count() * 8 < complete.lines().count(),
            "the delta is {} lines against the complete form's {}",
            delta.lines().count(),
            complete.lines().count()
        );
    }

    #[test]
    fn a_worlds_parameters_can_be_kept_as_a_ruleset_and_read_back() {
        // The other direction of the same idea: numbers arrived at by dragging values in the
        // editor become a file any world can name. Until this existed they had to be copied out
        // by hand, and a number copied by hand is a number that can be copied wrong.
        let dir = std::env::temp_dir().join("mm-ruleset-save-test");
        let _ = std::fs::remove_dir_all(&dir);

        let base = mm_core::ruleset::Rules::default();
        let mut moved = base.clone();
        moved.biology.metabolism.rates.light_occlusion = 128;
        moved.biology.division_energy = 4_096;
        let changes = mm_core::params::diff(&base, &moved);

        let path = save_ruleset(&dir, "lean light", "default", "measured", &changes)
            .expect("write the ruleset");
        assert_eq!(
            path.file_name().and_then(|s| s.to_str()),
            Some("lean_light.ron"),
            "a name with a space in it should become a file stem without one"
        );

        let mut library = mm_core::ruleset::RulesetLibrary::new();
        library
            .insert("default", "( name: \"default\", set: {} )")
            .expect("default");
        library
            .insert("lean_light", &std::fs::read_to_string(&path).expect("read"))
            .expect("the file it just wrote should parse");
        assert_eq!(library.rules("lean_light").expect("resolve"), moved);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_ruleset_name_that_is_really_a_path_is_refused() {
        let dir = std::env::temp_dir().join("mm-ruleset-save-bad");
        for name in ["", "  ", "../escape", "sub/dir"] {
            assert!(
                save_ruleset(&dir, name, "", "", &Default::default()).is_err(),
                "`{name}` was accepted as a ruleset name"
            );
        }
    }

    #[test]
    fn the_shipped_rulesets_all_resolve() {
        // The parameter editor offers these as baselines to compare against, so one that will
        // not resolve is one the interface has to say it refused rather than quietly omit.
        let (good, refused) = ruleset_choices();
        assert!(refused.is_empty(), "rulesets that would not resolve: {refused:?}");
        assert!(
            good.iter().any(|c| c.name == "default"),
            "the engine's own numbers are not in the library: {:?}",
            good.iter().map(|c| &c.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_missing_file_and_a_file_that_is_not_a_scenario_read_differently() {
        let missing = load(Path::new("no/such/scenario.ron")).unwrap_err();
        assert!(matches!(missing, FileError::Io(_)), "{missing:?}");

        let dir = std::env::temp_dir().join("mm-library-test-bad");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("rubbish.ron");
        std::fs::write(&path, "this is not ron at all").expect("write");
        let bad = load(&path).unwrap_err();
        assert!(matches!(bad, FileError::Scenario(_)), "{bad:?}");
        assert!(
            bad.to_string().contains("is not a scenario"),
            "unhelpful: {bad}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_extension_is_added_once_and_not_twice() {
        assert_eq!(
            with_extension(Path::new("a/b"), "ron"),
            PathBuf::from("a/b.ron")
        );
        assert_eq!(
            with_extension(Path::new("a/b.ron"), "ron"),
            PathBuf::from("a/b.ron")
        );
        assert_eq!(
            with_extension(Path::new("a/b.RON"), "ron"),
            PathBuf::from("a/b.RON")
        );
    }
}

/// One line of the RON preview, and whether it stands for lines that are not being shown.
pub struct RonLine {
    pub text: String,
    pub folded: bool,
}

/// The RON with its chemical table folded to one line, unless asked for.
///
/// Four hundred of an empty scenario's four hundred and twenty lines are `chemicals`, and they
/// are the same in every scenario on the shelf. A preview whose job is "did the wall I just drew
/// reach the file" is no use if the answer is four hundred lines below the fold.
///
/// Brace depth rather than a RON parser, because this is a *preview* and the thing being
/// previewed is text. The summary line says how many lines it stands for, so nobody can read the
/// fold as the file being shorter than it is.
#[must_use]
pub fn fold_ron(text: &str, show_chemicals: bool) -> Vec<RonLine> {
    let mut out = Vec::new();
    let mut skipping = false;
    let mut depth = 0i32;
    let mut hidden = 0usize;
    for line in text.lines() {
        if !show_chemicals && !skipping && line.trim_start().starts_with("chemicals:") {
            skipping = true;
            depth = 0;
            hidden = 0;
        }
        if skipping {
            hidden += 1;
            depth += line.chars().filter(|c| *c == '[' || *c == '(').count() as i32;
            depth -= line.chars().filter(|c| *c == ']' || *c == ')').count() as i32;
            if depth <= 0 {
                let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                out.push(RonLine {
                    text: format!("{indent}chemicals: [ … {hidden} lines ],"),
                    folded: true,
                });
                skipping = false;
            }
            continue;
        }
        out.push(RonLine {
            text: line.to_string(),
            folded: false,
        });
    }
    // A table whose brackets never balance would otherwise swallow every line after it and
    // leave the preview showing half a file with nothing saying so. This is not a shape
    // `to_ron` produces — but a preview that can silently drop the end of the thing it is
    // previewing is the one failure a preview must not have.
    if skipping {
        out.push(RonLine {
            text: format!("chemicals: [ … {hidden} lines, unterminated ],"),
            folded: true,
        });
    }
    out
}


#[cfg(test)]
mod fold_tests {
    use super::*;

    const SAMPLE: &str = "(\n    name: \"x\",\n    chemicals: [\n        (\n            name: \"a\",\n        ),\n        (\n            name: \"b\",\n        ),\n    ],\n    barriers: [],\n)";

    #[test]
    fn the_chemical_table_folds_to_one_line_that_says_what_it_hides() {
        let folded = fold_ron(SAMPLE, false);
        let text: Vec<&str> = folded.iter().map(|l| l.text.as_str()).collect();
        assert!(
            text.iter().any(|l| l.contains("chemicals: [ … 8 lines ]")),
            "{text:?}"
        );
        // And the fold is marked, so the preview can draw it as the summary it is.
        assert_eq!(folded.iter().filter(|l| l.folded).count(), 1);
    }

    #[test]
    fn what_the_author_edits_survives_the_fold() {
        // The whole point: `barriers` is what the wall tool writes, and it is *after* the
        // chemical table in the file. A fold that swallowed it would be worse than no fold.
        let text: Vec<String> = fold_ron(SAMPLE, false).into_iter().map(|l| l.text).collect();
        assert!(text.iter().any(|l| l.contains("barriers: []")), "{text:?}");
        assert!(text.iter().any(|l| l.contains("name: \"x\"")), "{text:?}");
        assert_eq!(text.last().map(String::as_str), Some(")"));
    }

    #[test]
    fn asking_for_the_chemicals_gives_every_line_back_unchanged() {
        let shown: Vec<String> = fold_ron(SAMPLE, true).into_iter().map(|l| l.text).collect();
        assert_eq!(shown, SAMPLE.lines().collect::<Vec<_>>());
        assert!(fold_ron(SAMPLE, true).iter().all(|l| !l.folded));
    }

    #[test]
    fn a_file_with_no_chemical_table_is_left_alone() {
        let plain = "(\n    name: \"x\",\n)";
        let shown: Vec<String> = fold_ron(plain, false).into_iter().map(|l| l.text).collect();
        assert_eq!(shown, plain.lines().collect::<Vec<_>>());
    }

    #[test]
    fn an_unterminated_table_does_not_swallow_the_rest_of_the_file() {
        // Depth never returns to zero, so nothing after it is emitted — which would be a
        // preview quietly showing half a file. The summary is emitted at the end instead.
        let truncated = "(\n    chemicals: [\n        (\n";
        let shown = fold_ron(truncated, false);
        assert!(
            shown.iter().any(|l| l.folded),
            "a table that never closes produced no summary at all: {:?}",
            shown.iter().map(|l| &l.text).collect::<Vec<_>>()
        );
    }
}
