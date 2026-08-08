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
/// where the build window's `world` view has all six regimes, and the chemistry is the three a
/// cell cannot do without where that view reaches all sixteen. A starting point that can be got
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
    /// `(chemical, per square)` for carbon, carbon dioxide and the oxidant — what a body is
    /// built out of, and what a chloroplast runs on.
    pub chemistry: [(usize, i32); 3],
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

impl Default for NewWorld {
    fn default() -> Self {
        NewWorld {
            size: 256,
            light: mm_core::Q10_ONE,
            // The soup's levels, which is the control condition every other scenario is a
            // variation on — so the number being changed has something to be a change *from*.
            chemistry: [
                (CARBON, mm_core::fixed::q10(400)),
                (CARBON_DIOXIDE, mm_core::fixed::q10(400)),
                (OXIDANT, mm_core::fixed::q10(400)),
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
                    at: None,
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
    Scenario::from_ron(&text).map_err(|e| describe(path, &e))
}

/// Write a scenario out as a `.ron`.
///
/// Adds the extension when it is missing, because a file named `vent` that is RON inside is a
/// file the library will not list and the loader will not offer.
///
/// # Errors
///
/// If the scenario cannot be serialised, or the file cannot be written.
pub fn save(path: &Path, scenario: &Scenario) -> Result<PathBuf, FileError> {
    let path = with_extension(path, "ron");
    let text = scenario
        .to_ron()
        .map_err(|e| FileError::Scenario(format!("cannot write this scenario out: {e:?}")))?;
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
            assert!(
                mm_asm::locate::file("genomes", name).is_some(),
                "{name} is offered by the picker and cannot be found again"
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
                at: None
            }]
        );
        assert!(s.barriers.is_empty(), "nothing is drawn on it yet");
    }

    /// A slide with nothing in the water is a legal and useful thing to ask for, and it has to
    /// come out as a recipe that *says nothing* rather than one listing three zeroes.
    #[test]
    fn a_chemical_set_to_nothing_is_left_out_of_the_recipe() {
        let s = NewWorld {
            chemistry: [(CARBON, 0), (CARBON_DIOXIDE, 0), (OXIDANT, 0)],
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
        let written = save(&dir.join("nested/round-trip"), &original).expect("write");
        assert_eq!(written.extension().and_then(|e| e.to_str()), Some("ron"));
        let back = load(&written).expect("read it back");
        assert_eq!(back, original, "a scenario changed by being written down");
        let _ = std::fs::remove_dir_all(&dir);
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
