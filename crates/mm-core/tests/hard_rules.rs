//! CI enforcement for the hard rules in `CLAUDE.md`.
//!
//! Each rule here is one whose violation would be invisible until much later: a float that
//! makes a run irreproducible across machines, a `bevy_*` import that chains the simulation
//! to a frame budget, an `unwrap` on a path a genome can reach. They are cheap to check by
//! reading the source, and expensive to discover any other way.
//!
//! These are lint tests, not proofs. `tests/totality_fuzz.rs` is what actually establishes
//! that no genome can panic; this file is what stops the obvious ways of breaking it from
//! being merged.

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_root() -> PathBuf {
    crate_root().join("../..")
}

/// Every `.rs` file under `src/`, as `(display path, contents)`.
///
/// Deliberately `src/` only: `tests/` and `benches/` are allowed floats and `Instant`,
/// because measuring a thing is not the same as simulating it.
fn simulation_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                out.push((path.display().to_string(), text));
            }
        }
    }
    walk(&crate_root().join("src"), &mut out);
    assert!(!out.is_empty(), "found no sources to lint");
    out
}

/// Strip `//` and `/* */` comments and string literals, so that a rule named in prose does
/// not trip the check that enforces it.
fn code_only(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_block = false;
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
                out.push('\n');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                in_block = true;
            }
            '"' => {
                in_string = true;
                out.push('"');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Line numbers where `needle` appears as a whole word, ignoring `#[cfg(test)]` modules.
fn find_outside_tests(code: &str, needle: &str) -> Vec<usize> {
    let mut hits = Vec::new();
    let mut depth: i32 = 0;
    let mut test_mod_depth: Option<i32> = None;
    let mut pending_test_attr = false;

    for (i, line) in code.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.contains("cfg(test)") {
            pending_test_attr = true;
        }
        // Attributes name lints after the very constructs they forbid, so `#![deny(
        // clippy::panic)]` must not read as a use of `panic`.
        let is_attribute = trimmed.starts_with("#[")
            || trimmed.starts_with("#![")
            || trimmed.starts_with("clippy::");
        let in_test = test_mod_depth.is_some();
        if !in_test && !is_attribute && contains_word(line, needle) {
            hits.push(i + 1);
        }
        let opens = line.matches('{').count() as i32;
        let closes = line.matches('}').count() as i32;
        if pending_test_attr && opens > 0 {
            test_mod_depth = Some(depth);
            pending_test_attr = false;
        }
        depth += opens - closes;
        if let Some(d) = test_mod_depth {
            if depth <= d {
                test_mod_depth = None;
            }
        }
    }
    hits
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = haystack.get(from..).and_then(|s| s.find(needle)) {
        let start = from + rel;
        let end = start + needle.len();
        let before_ok = start == 0
            || !bytes
                .get(start - 1)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
        let after_ok = !bytes
            .get(end)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

/// Hard rule 2 / invariant I2 — no floats in `mm-core`.
///
/// Floating point is not reproducible across compilers, architectures or optimisation
/// levels. One `f32` in the simulation and I1 is gone, quietly, on somebody else's machine.
#[test]
fn no_floats_in_the_simulation() {
    for (path, src) in simulation_sources() {
        let code = code_only(&src);
        for ty in ["f32", "f64"] {
            let hits = find_outside_tests(&code, ty);
            assert!(
                hits.is_empty(),
                "{path} uses `{ty}` at line(s) {hits:?}; \
                 mm-core is integer and fixed-point only (I2)"
            );
        }
        for name in ["sqrt", "powf", "to_bits", "from_bits"] {
            assert!(
                !contains_word(&code, name),
                "{path} mentions `{name}`, which suggests floating point crept in"
            );
        }
    }
}

/// Hard rule 5 — no sequential RNG, no wall clock.
///
/// Randomness is `hash(seed, tick, cell_id, purpose)`. A stateful generator or a clock would
/// make results depend on scheduling order, which breaks I1 and I6 together and would make
/// the deferred networking permanently impossible.
#[test]
fn no_wall_clock_and_no_global_rng() {
    for (path, src) in simulation_sources() {
        let code = code_only(&src);
        for banned in ["Instant", "SystemTime", "thread_rng", "rand", "getrandom"] {
            let hits = find_outside_tests(&code, banned);
            assert!(
                hits.is_empty(),
                "{path} uses `{banned}` at line(s) {hits:?}; randomness is \
                 hash(seed, tick, cell_id, purpose) and there is no clock (SPEC §11)"
            );
        }
    }
}

/// Hard rule 3 — nothing on a path a genome can reach may panic.
///
/// Every byte sequence must be a legal program. A cell that can fault is a cell every later
/// system has to have an opinion about; a cell that cannot is one nobody needs to think
/// about again.
#[test]
fn no_panicking_constructs_on_genome_reachable_paths() {
    for (path, src) in simulation_sources() {
        let code = code_only(&src);
        for banned in [
            "unwrap",
            "expect",
            "panic",
            "unreachable",
            "todo",
            "unimplemented",
        ] {
            let hits = find_outside_tests(&code, banned);
            assert!(
                hits.is_empty(),
                "{path} uses `{banned}` at line(s) {hits:?}; \
                 the worst a program may do is waste energy (I3)"
            );
        }
        assert!(
            !contains_word(&code, "unsafe"),
            "{path} uses `unsafe`; mm-core is `#![forbid(unsafe_code)]`"
        );
    }
}

/// Hard rule 6 — no iteration-order dependence.
///
/// `HashMap` iteration order varies run to run. Anything that reaches simulation state
/// through it destroys I1 in a way that reproduces only sometimes, which is the worst kind
/// of bug this project can have.
#[test]
fn no_hash_ordered_collections_in_the_simulation() {
    for (path, src) in simulation_sources() {
        let code = code_only(&src);
        for banned in ["HashMap", "HashSet"] {
            let hits = find_outside_tests(&code, banned);
            assert!(
                hits.is_empty(),
                "{path} uses `{banned}` at line(s) {hits:?}; \
                 use BTreeMap/BTreeSet or sort by stable id (I6)"
            );
        }
    }
}

/// Hard rule 1 — no Bevy in `mm-core`, checked against the resolved dependency graph rather
/// than against the manifest, so a transitive pull-in is caught too.
#[test]
fn no_bevy_in_the_dependency_graph() {
    // The resolved graph, not the manifest, so a transitive pull-in is caught too.
    // Dev-dependencies are excluded on purpose: a benchmark harness is not the simulation.
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "tree",
            "--package",
            "mm-core",
            "--edges",
            "normal,build",
            "--prefix",
            "none",
        ])
        .current_dir(workspace_root())
        .output();
    let Ok(output) = output else {
        eprintln!("skipping: `cargo tree` could not be run");
        return;
    };
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tree = String::from_utf8_lossy(&output.stdout);
    let offenders: Vec<&str> = tree
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("bevy"))
        .collect();
    assert!(
        offenders.is_empty(),
        "mm-core depends on {offenders:?}; the simulation must build with Bevy absent so \
         that a headless run is never hostage to a frame budget"
    );

    // And nothing else has crept in either. `mm-core` carries the invariants that are
    // hardest to keep and easiest to lose to somebody else's code — no floats in simulation
    // state, no clock, no global RNG, no hash-ordered iteration — so its dependency list is
    // reviewed rather than merely bounded. Anything not on this list is a decision, not an
    // accident, and should be added here deliberately.
    //
    // What is here and why:
    //   rayon  — the fluid solver's parallelism (M1). Its scheduling is unobservable by
    //            construction; see `fluid`'s module docs.
    //   serde  — scenario files (SPEC §16). Derive only; no runtime behaviour.
    //   ron    — the scenario file format itself.
    // Everything below those is their own transitive closure.
    const REVIEWED_ROOTS: &[&str] = &["mm-core", "rayon", "serde", "ron"];
    const REVIEWED_TRANSITIVE: &[&str] = &[
        "rayon-core",
        "crossbeam-deque",
        "crossbeam-epoch",
        "crossbeam-utils",
        "either",
        "serde_derive",
        "serde_core",
        "proc-macro2",
        "quote",
        "syn",
        "unicode-ident",
        "base64",
        "bitflags",
        "once_cell",
        "typeid",
        "unicode-ident",
    ];

    let mut unexpected: Vec<String> = Vec::new();
    for line in tree.lines() {
        let name = line.split_whitespace().next().unwrap_or("");
        if name.is_empty() {
            continue;
        }
        if REVIEWED_ROOTS.contains(&name) || REVIEWED_TRANSITIVE.contains(&name) {
            continue;
        }
        unexpected.push(name.to_string());
    }
    unexpected.sort();
    unexpected.dedup();
    assert!(
        unexpected.is_empty(),
        "mm-core has picked up unreviewed dependencies: {unexpected:?}\n\
         Add them to REVIEWED_TRANSITIVE only after checking they cannot introduce floats \
         into simulation state, a clock, a global RNG, or hash-ordered iteration.\n{tree}"
    );
}

/// The lints above pass. These check they would not pass on a violation — a guard that
/// cannot fail is worse than no guard, because it reads like coverage.
#[test]
fn the_lints_detect_what_they_claim_to() {
    // Whole-word matching, so `rand_ctr` is not `rand` and `unsafe_code` is not `unsafe`.
    assert!(contains_word("let x: f32 = 1;", "f32"));
    assert!(!contains_word("let f32x = 1;", "f32"));
    assert!(!contains_word("self.rand_ctr += 1;", "rand"));
    assert!(contains_word("use rand;", "rand"));
    assert!(!contains_word("#![forbid(unsafe_code)]", "unsafe"));
    assert!(contains_word("unsafe { }", "unsafe"));

    // Comments and string literals are stripped, so prose about a rule does not trip it.
    let src = "// this mentions f32\n/* and f64 */\nlet s = \"f32\";\nlet a: i16 = 0;";
    let code = code_only(src);
    assert!(!contains_word(&code, "f32"), "{code:?}");
    assert!(!contains_word(&code, "f64"), "{code:?}");
    assert!(contains_word(&code, "i16"));

    // A violation in real code is found, with its line number...
    let bad = "fn a() -> f32 { 0.0 }\nfn b() {}\n";
    assert_eq!(find_outside_tests(bad, "f32"), vec![1]);

    // ...and one inside a `#[cfg(test)]` module is not, since tests may panic freely.
    let with_tests = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() { panic!() }\n}\n";
    assert!(find_outside_tests(with_tests, "panic").is_empty());

    // But code after the test module closes is checked again.
    let after = "#[cfg(test)]\nmod tests {\n    fn t() { panic!() }\n}\nfn c() { panic!() }\n";
    assert_eq!(find_outside_tests(after, "panic"), vec![5]);
}

/// The per-cell fixed-state budget of SPEC §6.1 is 512 bytes. The VM is the bulk of it, and
/// the rest — position, mass, energy, organelle slots, the internal chemical vector — lands
/// on top from M2 onward. Worth knowing now, while there is still room to choose.
#[test]
fn vm_state_fits_the_per_cell_budget() {
    let size = std::mem::size_of::<mm_core::Vm>();
    assert!(
        size <= 320,
        "VM state is {size} bytes; SPEC §6.1 budgets 512 for the whole cell and the \
         organelles, chemistry and physics still have to fit"
    );
}

/// Every parameter in the biology config reaches the state hash.
///
/// `BiologyConfig::hash_state` is written out field by field, and its own note says why: "adding a
/// parameter and forgetting to hash it is a visible omission here rather than an invisible one
/// everywhere." It was not visible enough. `light_occlusion` and `rigidity_gain` were added at
/// SPEC §17.8 and §17.7, both change what the simulation does, and neither reached the hash for
/// two milestones — so `mm-cli hash` could not tell `the_thicket.ron`'s economy from `soup.ron`'s,
/// and a determinism check would have passed straight across the difference.
///
/// This enumerates the config through `params::fields`, which walks the *serialised* form, so a
/// parameter that exists is a parameter this test knows about. Nobody has to remember.
#[test]
fn every_parameter_reaches_the_state_hash() {
    use mm_core::params::{self, Value};
    use mm_core::state_hash::{StateHash, StateHasher};

    let hash_of = |config: &mm_core::BiologyConfig| -> u64 {
        let mut h = StateHasher::new();
        config.hash_state(&mut h);
        h.finish()
    };

    let base = mm_core::BiologyConfig::default();
    let baseline = hash_of(&base);
    let mut missed: Vec<String> = Vec::new();

    for (path, value) in params::fields(&base) {
        // A value that is certainly different and certainly still fits an `i32`.
        let changed = match value {
            Value::Bool(b) => Value::Bool(!b),
            Value::Int(v) if v > 0 => Value::Int(v - 1),
            Value::Int(v) => Value::Int(v + 1),
        };
        let Some(moved) = params::set(&base, &path, changed) else {
            // A field that will not take the value is not this test's business; `params`'s own
            // tests cover the setter.
            continue;
        };
        if moved == base {
            continue;
        }
        if hash_of(&moved) == baseline {
            missed.push(path);
        }
    }

    assert!(
        missed.is_empty(),
        "these parameters change the configuration and not its state hash, so two worlds that \
         differ only in them are indistinguishable to every determinism check in the tree:\n  {}",
        missed.join("\n  ")
    );
}
