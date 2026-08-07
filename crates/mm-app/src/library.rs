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

use mm_core::{Scenario, ScenarioError};

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
