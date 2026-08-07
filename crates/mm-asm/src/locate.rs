//! Where the shipped `genomes/` and `scenarios/` directories are at run time.
//!
//! Here, in the crate both binaries depend on, because both need the same answer and two answers
//! that drift apart is how `mm-cli` and the microscope come to disagree about which ancestor is
//! the ancestor.
//!
//! **The bug this exists to fix.** Every caller used to resolve these through
//! `CARGO_MANIFEST_DIR`, which is baked at compile time and points at the machine that did the
//! compiling. That is correct in development and worthless in a release: a downloaded build went
//! looking for its genomes in `/home/runner/work/ManicMicrobes/...`, a path belonging to a CI
//! runner that no longer exists, while the `genomes/` folder shipped in the same archive sat
//! unread beside the binary. It failed quietly — an empty slide and a line on stderr — which is
//! why it survived a green build. Only running a distributed artefact somewhere other than where
//! it was built shows it.
//!
//! So: ask the filesystem, in the order that answers a real layout.

use std::path::{Path, PathBuf};

/// Every directory a shipped data folder might sit under, most specific first.
///
/// 1. The working directory, which is the repository during development and the unpacked folder
///    for anyone who cd'd into it.
/// 2. Beside the executable — the tarball and the zip both put `genomes/` next to the binary,
///    and it is where the AppImage keeps them too.
/// 3. `../Resources` from the executable, which is where a macOS bundle keeps everything that is
///    not the binary: the executable lives in `Contents/MacOS`, so this lands in
///    `Contents/Resources`.
/// 4. The source tree, from `CARGO_MANIFEST_DIR`. Last rather than first, and kept because
///    `cargo run` from anywhere in the workspace should still find them — that is the whole of
///    what it was ever good for.
#[must_use]
pub fn search_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from(".")];

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
            roots.push(dir.join("../Resources"));
        }
    }

    roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    roots
}

/// The first `<root>/<name>` that exists and is a directory, or `None`.
#[must_use]
pub fn dir(name: &str) -> Option<PathBuf> {
    search_roots()
        .into_iter()
        .map(|root| root.join(name))
        .find(|path| path.is_dir())
}

/// The first `<root>/<dir>/<file>` that exists, or `None`.
///
/// Searched per file rather than per directory on purpose: a `genomes/` beside the binary that
/// happens not to contain the one being asked for should not stop the source tree being tried.
#[must_use]
pub fn file(dir: &str, file: &str) -> Option<PathBuf> {
    search_roots()
        .into_iter()
        .map(|root| root.join(dir).join(file))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_working_directory_comes_first_and_the_source_tree_last() {
        let roots = search_roots();
        assert_eq!(roots.first().unwrap(), Path::new("."));
        assert!(
            roots.last().unwrap().join("Cargo.toml").is_file(),
            "the last root should be the workspace: {:?}",
            roots.last()
        );
    }

    /// The tests run from the workspace, so this is really asserting that the source-tree root
    /// works — but it is also the assertion that the whole thing resolves at all.
    #[test]
    fn the_ancestor_is_found() {
        let path = file("genomes", "ancestor.mm").expect("ancestor.mm is in the repository");
        assert!(std::fs::read_to_string(path).is_ok());
    }

    #[test]
    fn a_genome_that_does_not_exist_is_none() {
        assert!(file("genomes", "no_such_genome.mm").is_none());
    }

    #[test]
    fn the_genome_directory_is_found() {
        assert!(dir("genomes").is_some());
        assert!(dir("scenarios").is_some());
    }
}
