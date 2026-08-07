//! Manic Microbes — headless runner, parameter sweeps and metric export.
//!
//! The reason `mm-core` has no Bevy in it is so that this exists: the simulation can run at a
//! thousand times realtime for parameter sweeps, and the renderer can never hold it hostage
//! to a frame budget (SPEC §1).
//!
//! ```text
//! mm-cli run   scenarios/soup.ron --ticks 1000000 --metrics out.ndjson
//! mm-cli sweep scenarios/soup.ron --param mutation --range 1..64
//! mm-cli hash  scenarios/soup.ron --ticks 100000
//! ```
//!
//! Arguments are parsed by hand rather than with a crate. The surface is three subcommands
//! and a dozen flags, the diagnostics are better when they can talk about scenarios and ticks
//! rather than about arguments, and a tool whose whole purpose is reproducibility is a poor
//! place to take on a dependency that does not earn itself.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mm_core::biology::BiologyConfig;
use mm_core::metrics::Sample;
use mm_core::mutation::RATE_SCALE;
use mm_core::{MutationRates, Scenario, Snapshot, World};

const USAGE: &str = "\
Manic Microbes — headless runner.

USAGE:
    mm-cli run   <scenario.ron> [options]     run a simulation
    mm-cli sweep <scenario.ron> [options]     run it once per parameter value
    mm-cli hash  <scenario.ron> [options]     print the state hash, for determinism checks
    mm-cli match <left.mm> <right.mm> [opts]  play an arena match and report it

OPTIONS:
    --ticks <n>            how long to run                     [default: 100000]
    --seed <n>             override the scenario's seed
    --genome <file.mm>     ancestor to seed the slide with
    --population <n>       how many ancestors to seed          [default: 16]
    --metrics <file>       write NDJSON metrics here
    --every <n>            ticks between metric samples        [default: 1000]
    --quiet                do not print progress to the terminal
    --archive <file>       write the species archive as NDJSON when the run ends
    --prune-every <n>      drop uninteresting extinct branches this often  [default: 0, off]
    --prune-keep <n>       peak population a dead species needs to survive pruning [default: 32]
    --save <file>          write a snapshot when the run ends
    --load <file>          resume from a snapshot instead of a scenario
    --check                verify the invariants at every sample, and fail if one breaks

MATCH OPTIONS:
    --ticks <n>            tick limit for the match             [default: 20000]
    --seed <n>             the match seed
    --population <n>       cells per side                       [default: 8]

SWEEP OPTIONS:
    --param <name>         one of: mutation, duplication, fluid, light
    --range <lo>..<hi>     inclusive range of values to try
    --steps <n>            how many values across that range   [default: 8]

Exit status is non-zero if a run breaks an invariant under --check, so this is usable
directly from a CI job.
";

fn main() -> ExitCode {
    // Before anything touches rayon, because a global pool can only be built once. On a
    // processor with more than one kind of core this is worth about a tenth of a tick; on one
    // without, it does nothing. See `mm_core::threads`.
    //
    // The count only, not the affinity: pinning needs `sched_setaffinity` and lives in
    // `mm_app::threads`, which the microscope calls and this does not depend on.
    mm_core::threads::use_performance_cores();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mm-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Everything the three subcommands share.
struct Options {
    scenario: PathBuf,
    ticks: u64,
    seed: Option<u64>,
    genome: Option<PathBuf>,
    population: u32,
    metrics: Option<PathBuf>,
    every: u64,
    quiet: bool,
    save: Option<PathBuf>,
    archive: Option<PathBuf>,
    prune_every: u64,
    prune_keep: u32,
    load: Option<PathBuf>,
    check: bool,
    param: Option<String>,
    range: Option<(i64, i64)>,
    steps: u32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            scenario: PathBuf::new(),
            ticks: 100_000,
            seed: None,
            genome: None,
            population: 16,
            metrics: None,
            every: 1_000,
            quiet: false,
            save: None,
            archive: None,
            prune_every: 0,
            prune_keep: 32,
            load: None,
            check: false,
            param: None,
            range: None,
            steps: 8,
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let Some(command) = args.first() else {
        print!("{USAGE}");
        return Ok(());
    };
    if command == "--help" || command == "-h" || command == "help" {
        print!("{USAGE}");
        return Ok(());
    }

    // `match` takes two genome paths rather than a scenario, so it is dispatched before the
    // scenario is demanded.
    if command == "match" {
        return cmd_match(&args[1..]);
    }

    let mut opts = parse(&args[1..])?;
    if opts.scenario.as_os_str().is_empty() && opts.load.is_none() {
        return Err("no scenario given; try `mm-cli --help`".to_string());
    }

    match command.as_str() {
        "run" => cmd_run(&opts),
        "hash" => cmd_hash(&opts),
        "sweep" => cmd_sweep(&mut opts),
        other => Err(format!("unknown command `{other}`; try `mm-cli --help`")),
    }
}

/// Play an arena match between two genomes and print the report (M6).
///
/// Takes `.mm` assembly or a shareable `.mmg` genome file, deciding by content rather than by
/// extension — a file that says what it is should be believed over a name anybody can change.
fn cmd_match(args: &[String]) -> Result<(), String> {
    let mut paths: Vec<&String> = Vec::new();
    let mut rules = mm_core::arena::MatchRules::default();
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        if let Some(flag) = arg.strip_prefix("--") {
            let value = args
                .get(i + 1)
                .ok_or_else(|| format!("--{flag} needs a value"))?;
            match flag {
                "ticks" => {
                    rules.tick_limit = value
                        .parse()
                        .map_err(|_| "--ticks wants a number".to_string())?
                }
                "seed" => {
                    rules.seed = value
                        .parse()
                        .map_err(|_| "--seed wants a number".to_string())?
                }
                "population" => {
                    rules.cells_per_side = value
                        .parse()
                        .map_err(|_| "--population wants a number".to_string())?
                }
                other => return Err(format!("unknown option `--{other}`")),
            }
            i += 2;
        } else {
            paths.push(arg);
            i += 1;
        }
    }
    if paths.len() != 2 {
        return Err("match takes exactly two genomes".to_string());
    }

    let load = |path: &str| -> Result<mm_core::arena::Entry, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
        let name = std::path::Path::new(path)
            .file_stem()
            .map_or_else(|| path.to_string(), |s| s.to_string_lossy().to_string());
        // A shareable genome file first, because it carries an ISA stamp that must be
        // honoured. Falling back to assembly when it is not one.
        match mm_core::genome_file::GenomeFile::from_text(&text) {
            Ok(file) => Ok(mm_core::arena::Entry::new(
                if file.name.is_empty() {
                    name
                } else {
                    file.name
                },
                file.bytes,
            )),
            Err(mm_core::genome_file::GenomeFileError::NotAGenomeFile) => {
                let bytes = mm_asm::assemble(&text)
                    .map_err(|e| format!("{path} does not assemble:\n{e}"))?
                    .bytes;
                Ok(mm_core::arena::Entry::new(name, bytes))
            }
            // Anything else — a wrong ISA above all — is refused rather than worked around.
            Err(e) => Err(format!("{path}: {e}")),
        }
    };

    let left = load(paths[0])?;
    let right = load(paths[1])?;
    let report = mm_core::arena::play(&rules, &left, &right).map_err(|e| e.to_string())?;
    println!("{}", report.summary());
    if report.copy_damaged > 0 {
        // Real information about the match: with mutation off a cell can still produce a
        // damaged daughter by running short of energy mid-copy, and a match where most of a
        // side is damaged was not really won by the genome its author wrote.
        println!(
            "  {} cells finished on a copy-damaged genome",
            report.copy_damaged
        );
    }
    println!("\n  tick      {:>8} {:>8}", left.name, right.name);
    for s in &report.standings {
        println!("  {:>8}  {:>8} {:>8}", s.tick, s.left, s.right);
    }
    Ok(())
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        // A bare word is the scenario path; everything else is a flag.
        if !arg.starts_with("--") {
            if o.scenario.as_os_str().is_empty() {
                o.scenario = PathBuf::from(arg);
                i += 1;
                continue;
            }
            return Err(format!("unexpected argument `{arg}`"));
        }
        let mut value = || -> Result<String, String> {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--ticks" => o.ticks = parse_u64(&value()?, "--ticks")?,
            "--seed" => o.seed = Some(parse_u64(&value()?, "--seed")?),
            "--genome" => o.genome = Some(PathBuf::from(value()?)),
            "--population" => o.population = parse_u64(&value()?, "--population")? as u32,
            "--metrics" => o.metrics = Some(PathBuf::from(value()?)),
            "--every" => o.every = parse_u64(&value()?, "--every")?.max(1),
            "--quiet" => o.quiet = true,
            "--save" => o.save = Some(PathBuf::from(value()?)),
            "--archive" => o.archive = Some(PathBuf::from(value()?)),
            "--prune-every" => {
                o.prune_every = value()?
                    .parse()
                    .map_err(|_| "--prune-every wants a number".to_string())?
            }
            "--prune-keep" => {
                o.prune_keep = value()?
                    .parse()
                    .map_err(|_| "--prune-keep wants a number".to_string())?
            }
            "--load" => o.load = Some(PathBuf::from(value()?)),
            "--check" => o.check = true,
            "--param" => o.param = Some(value()?),
            "--steps" => o.steps = parse_u64(&value()?, "--steps")?.max(1) as u32,
            "--range" => {
                let v = value()?;
                let (lo, hi) = v
                    .split_once("..")
                    .ok_or_else(|| format!("--range wants `lo..hi`, got `{v}`"))?;
                o.range = Some((
                    parse_i64(lo, "--range low")?,
                    parse_i64(hi, "--range high")?,
                ));
            }
            "--help" | "-h" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }
    Ok(o)
}

fn parse_u64(s: &str, what: &str) -> Result<u64, String> {
    s.replace('_', "")
        .parse()
        .map_err(|_| format!("{what}: `{s}` is not a number"))
}

fn parse_i64(s: &str, what: &str) -> Result<i64, String> {
    s.replace('_', "")
        .parse()
        .map_err(|_| format!("{what}: `{s}` is not a number"))
}

/// Build the world a run starts from.
fn build(opts: &Options) -> Result<World, String> {
    if let Some(path) = &opts.load {
        let bytes = std::fs::read(path)
            .map_err(|e| format!("cannot read snapshot {}: {e}", path.display()))?;
        return Snapshot::read(&bytes).map_err(|e| format!("{}: {e}", path.display()));
    }

    let text = std::fs::read_to_string(&opts.scenario)
        .map_err(|e| format!("cannot read {}: {e}", opts.scenario.display()))?;
    let mut scenario =
        Scenario::from_ron(&text).map_err(|e| format!("{}: {e}", opts.scenario.display()))?;
    if let Some(seed) = opts.seed {
        scenario.seed = seed;
    }
    let mut world = World::new(scenario).map_err(|e| e.to_string())?;

    if let Some(genome_path) = &opts.genome {
        seed_population(&mut world, genome_path, opts.population)?;
    } else {
        seed_inhabitants(&mut world, &genome_root())?;
    }
    Ok(world)
}

/// Assemble a genome and put `n` copies of it on the slide.
///
/// Placement, the starting loadout and the ledger rebaseline all live in
/// `World::place_founders` — this is the half that needs a filesystem and an assembler, which
/// is the half `mm-core` will not have.
fn seed_population(world: &mut World, genome_path: &Path, n: u32) -> Result<(), String> {
    let src = std::fs::read_to_string(genome_path)
        .map_err(|e| format!("cannot read {}: {e}", genome_path.display()))?;
    let assembled = mm_asm::assemble(&src)
        .map_err(|e| format!("{} does not assemble:\n{e}", genome_path.display()))?;
    world.place_founders(&assembled.bytes, n);
    Ok(())
}

/// Put the scenario's own inhabitants on the slide.
///
/// What a scenario says about who lives on it, honoured. `--genome` overrides it entirely
/// rather than adding to it, because the point of passing a genome on the command line is
/// usually to ask what *that* one does here.
fn seed_inhabitants(world: &mut World, root: &Path) -> Result<(), String> {
    let wanted = world.scenario().inhabitants.clone();
    for who in &wanted {
        let path = root.join(&who.genome);
        seed_population(world, &path, who.count)?;
    }
    Ok(())
}

/// Where a scenario's genome names resolve to.
///
/// Found rather than baked: this was `CARGO_MANIFEST_DIR` and so pointed at the machine that
/// compiled the binary, which meant a released `mm-cli` could not resolve the genome names in
/// the scenarios shipped beside it. Falls back to the source-tree path when nothing exists, so
/// the error a caller gets still names a directory rather than nothing.
fn genome_root() -> PathBuf {
    mm_asm::locate::dir("genomes")
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../genomes"))
}

fn cmd_run(opts: &Options) -> Result<(), String> {
    let mut world = build(opts)?;

    let mut out: Option<Box<dyn Write>> = match &opts.metrics {
        Some(path) => {
            let f = std::fs::File::create(path)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            Some(Box::new(std::io::BufWriter::new(f)))
        }
        None => None,
    };

    if !opts.quiet {
        println!("{}", Sample::header());
    }

    let mut previous: Option<Sample> = None;
    let mut emit = |world: &World, previous: &mut Option<Sample>| -> Result<(), String> {
        let sample = Sample::take(world, previous.as_ref());
        if let Some(w) = out.as_mut() {
            writeln!(w, "{}", sample.to_json())
                .map_err(|e| format!("cannot write metrics: {e}"))?;
        }
        if !opts.quiet {
            println!("{}", sample.to_row());
        }
        *previous = Some(sample);
        Ok(())
    };

    emit(&world, &mut previous)?;
    for tick in 0..opts.ticks {
        world.step();
        if (tick + 1) % opts.every == 0 {
            emit(&world, &mut previous)?;
            if opts.check {
                world
                    .check_invariants()
                    .map_err(|e| format!("invariant broken at tick {}: {e}", world.tick_count()))?;
            }
        }
        // Pruning on a schedule (SPEC §10.3). Off by default: a sweep that wants every dead
        // end has as good a claim as a long run that needs the archive bounded, and silently
        // discarding branches would make two runs of the same scenario disagree about their
        // own history for reasons the operator never asked for.
        if opts.prune_every > 0 && (tick + 1) % opts.prune_every == 0 {
            let dropped = world.prune_archive(opts.prune_keep);
            if dropped > 0 && !opts.quiet {
                eprintln!(
                    "tick {}: pruned {dropped} extinct branches, {} species remain",
                    world.tick_count(),
                    world.archive().len()
                );
            }
        }
    }
    // Only if the loop has not just done it. A run whose length is a multiple of `--every`
    // ends on a sampled tick, and emitting again there wrote a duplicate row differenced
    // against itself — so every aligned run finished with a line reading zero dissipation, zero
    // influx, zero births, which is the most misleading thing a metrics file can end with.
    if opts.ticks == 0 || !opts.ticks.is_multiple_of(opts.every.max(1)) {
        emit(&world, &mut previous)?;
    }

    if let Some(w) = out.as_mut() {
        w.flush()
            .map_err(|e| format!("cannot flush metrics: {e}"))?;
    }
    if let Some(path) = &opts.archive {
        let text = mm_core::phylogeny::export::archive_ndjson(world.archive(), world.events());
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        if !opts.quiet {
            eprintln!(
                "wrote {} species and {} events to {}",
                world.archive().len(),
                world.events().events().len(),
                path.display()
            );
        }
    }
    if !opts.quiet {
        report_story(&world);
    }
    if let Some(path) = &opts.save {
        let bytes = Snapshot::write(&world).map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        if !opts.quiet {
            eprintln!("saved {} ", path.display());
        }
    }
    Ok(())
}

/// Print what happened, in the register of a newspaper rather than a log file.
///
/// This is the point of M5. A run that ends with a population count has told you a number; a
/// run that ends with "first motility at tick 12,400, *Cilius rapidus* took the slide at
/// 41,000, three species extinct" has told you what happened.
fn report_story(world: &World) {
    let archive = world.archive();
    let events = world.events();
    if archive.is_empty() {
        return;
    }

    println!();
    println!(
        "{} species over the run, {} still alive, {} pruned",
        archive.len(),
        archive.living(),
        archive.pruned()
    );

    if !events.is_empty() {
        println!("\nthe newspaper:");
        for e in events.events() {
            let who = archive
                .get(e.species)
                .map_or_else(|| format!("species {}", e.species), |s| s.name.full());
            println!("  tick {:>10}  {} — {who}", e.tick, e.what.headline());
        }
    }

    // The five biggest species by peak, which is the closest thing to "who mattered".
    let mut by_peak: Vec<_> = archive.iter().collect();
    by_peak.sort_by_key(|s| std::cmp::Reverse(s.peak_population));
    println!("\nthe cast:");
    for s in by_peak.iter().take(5) {
        println!("  {}", s.describe(archive));
    }
}

/// Print the state hash. Two machines running this on the same scenario must agree (I1).
fn cmd_hash(opts: &Options) -> Result<(), String> {
    let mut world = build(opts)?;
    world.run(opts.ticks);
    if opts.check {
        world
            .check_invariants()
            .map_err(|e| format!("invariant broken: {e}"))?;
    }
    println!("{:016x}", world.state_hash());
    Ok(())
}

/// Which scenario knob a sweep varies.
///
/// Deliberately a small named set rather than a path into the scenario: a sweep's output is
/// only comparable if everyone means the same thing by "the mutation rate", and a
/// stringly-typed field path would let two runs disagree about that silently.
fn apply_param(
    world_scenario: &mut Scenario,
    biology: &mut BiologyConfig,
    param: &str,
    value: i64,
) -> Result<String, String> {
    match param {
        "mutation" => {
            // Chances in RATE_SCALE for every operator at once: the knob a sweep over
            // "mutation rate" means.
            let v = value.clamp(0, RATE_SCALE as i64) as u32;
            biology.mutation = MutationRates {
                point: v,
                insertion: v / 4,
                deletion: v / 4,
                duplication: v / 3,
                inversion: v / 8,
                translocation: v / 8,
                ..MutationRates::default()
            };
            Ok(format!("mutation={v}"))
        }
        "duplication" => {
            biology.mutation.duplication = value.clamp(0, RATE_SCALE as i64) as u32;
            Ok(format!("duplication={}", biology.mutation.duplication))
        }
        "fluid" => {
            world_scenario.fluid_interval = value.clamp(1, 1 << 20) as u32;
            Ok(format!("fluid_interval={}", world_scenario.fluid_interval))
        }
        "light" => {
            world_scenario.light = mm_core::LightRegime::Uniform {
                intensity: value.clamp(0, i32::MAX as i64) as i32,
            };
            Ok(format!("light={value}"))
        }
        other => Err(format!(
            "unknown --param `{other}`; try mutation, duplication, fluid or light"
        )),
    }
}

fn cmd_sweep(opts: &mut Options) -> Result<(), String> {
    let param = opts
        .param
        .clone()
        .ok_or_else(|| "sweep needs --param".to_string())?;
    let (lo, hi) = opts
        .range
        .ok_or_else(|| "sweep needs --range lo..hi".to_string())?;
    if hi < lo {
        return Err(format!("--range {lo}..{hi} runs backwards"));
    }

    let text = std::fs::read_to_string(&opts.scenario)
        .map_err(|e| format!("cannot read {}: {e}", opts.scenario.display()))?;
    let base = Scenario::from_ron(&text).map_err(|e| e.to_string())?;

    println!(
        "{:<28} {:>10} {:>8} {:>8} {:>12} {:>18}",
        "parameter", "ticks", "final", "peak", "dissipation", "hash"
    );

    let steps = opts.steps.max(1);
    for step in 0..steps {
        // Inclusive of both ends, and exact at both: a sweep whose endpoints were off by a
        // rounding error would be quietly not the sweep that was asked for.
        let value = if steps == 1 {
            lo
        } else {
            lo + (hi - lo) * step as i64 / (steps - 1) as i64
        };

        let mut scenario = base.clone();
        if let Some(seed) = opts.seed {
            scenario.seed = seed;
        }
        let mut biology = BiologyConfig::default();
        let label = apply_param(&mut scenario, &mut biology, &param, value)?;

        let mut world = World::new(scenario).map_err(|e| e.to_string())?;
        world.set_biology(biology);
        if let Some(genome) = &opts.genome {
            seed_population(&mut world, genome, opts.population)?;
        } else {
            seed_inhabitants(&mut world, &genome_root())?;
        }

        let mut peak = 0u64;
        let start_dissipated = world.ledger().energy_out();
        for _ in 0..opts.ticks {
            world.step();
            peak = peak.max(world.cells().len() as u64);
        }
        if opts.check {
            world
                .check_invariants()
                .map_err(|e| format!("{label}: invariant broken: {e}"))?;
        }

        println!(
            "{:<28} {:>10} {:>8} {:>8} {:>12} {:>18x}",
            label,
            opts.ticks,
            world.cells().len(),
            peak,
            world.ledger().energy_out() - start_dissipated,
            world.state_hash()
        );
    }
    Ok(())
}
