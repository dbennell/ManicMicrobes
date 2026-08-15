//! Which cores the simulation runs on.
//!
//! Nothing here can change what the simulation computes. Hard rule 6 requires that no outcome
//! depend on rayon scheduling or thread count, and the determinism acceptance test holds the
//! world to it at half a million ticks; how many workers there are, and which cores they sit on,
//! is a weaker statement than that. This is a scheduling hint and nothing else.
//!
//! It is in `mm-core` rather than in the front end because the benchmarks, the headless runner
//! and the microscope all have to be measuring the same machine. A gate taken under a different
//! thread pool than the application uses is a measurement of something else.
//!
//! # Why it is worth a module
//!
//! A modern desktop processor is not made of one kind of core. Measured on an i7-12700K — eight
//! performance cores with hyperthreading on CPUs 0–15 at 4.9–5.0 GHz, four efficiency cores on
//! 16–19 at 3.8 GHz — at fifty thousand cells:
//!
//! | pool | ms/tick |
//! |---|---|
//! | all 20 threads, unpinned (rayon's default) | 32.10 |
//! | **16 threads, unpinned — what [`use_performance_cores`] does** | **29.70** |
//! | 16 threads, pinned to the performance cores | 28.31 |
//!
//! Every parallel phase in a tick ends at a barrier, so a phase finishes when its *slowest*
//! worker does — and a worker on a core a fifth slower holds the whole phase there. Rayon splits
//! the work evenly because it has no way to know the difference.
//!
//! Capping the count recovers most of that. Pinning recovers the rest, and cannot be done here:
//! it needs `sched_setaffinity`, `mm-core` is `#![forbid(unsafe_code)]`, and that rule is worth
//! more than four percent. [`performance_cores`] is public so that a front end, which is under no
//! such rule, can build its own pool and pin to the same set — `mm_app::threads` does.
//!
//! # How the two kinds are told apart
//!
//! By asking the kernel, and only falling back to inferring it from the clock. A hybrid part
//! publishes one PMU device per core type, and their `cpus` files are the answer outright:
//!
//! ```text
//! /sys/devices/cpu_core/cpus   0-15     the performance cores, hyperthreads included
//! /sys/devices/cpu_atom/cpus   16-19    the efficiency cores
//! ```
//!
//! That is what [`performance_cores`] reads first. The frequency comparison it used to lead with
//! is still there behind it, because it is the only thing that works on a machine that does not
//! publish the PMU split — but it is a proxy, and it fails in both directions: it gives up
//! entirely where `cpufreq` is absent, and on a part that bins two favoured cores well above
//! their siblings it can decide the machine has two performance cores. Neither can happen to a
//! question about which cores are `cpu_core`.
//!
//! # Is there anything worth giving the efficiency cores?
//!
//! Not in here. A tick is a chain of parallel phases and every one of them ends at a barrier, so
//! a phase costs what its *slowest* worker costs, and a worker on a core a fifth slower holds the
//! whole phase there. That is why the table above goes the way it does, and it applies to every
//! phase in `World::step` without exception.
//!
//! Leaving them out is not leaving them idle. The four cores this pool declines are the ones the
//! render thread, egui and Bevy's own task pools then have to themselves, and that is where the
//! work with no barrier on it actually lives. The candidates worth moving there deliberately are
//! all in the front end and none is in the tick: the food web, which is presentation-only and
//! rebuilt for every published frame; the metrics and archive writing in `mm-cli`; and Bevy's IO
//! pool. Being one frame stale costs nothing for any of them, which is exactly what makes them
//! suitable and the tick unsuitable.

/// The CPUs running at or near the highest clock this machine offers.
///
/// Read from `cpufreq`, which is where the kernel publishes it, and by a *proportion* of the
/// maximum rather than by equality — the performance cores of a 12700K do not all carry the same
/// number, since two of the eight boost to 5.0 GHz and the rest to 4.9, so an exact match would
/// pick two cores out of sixteen. The efficiency cores sit far enough below that the gap is
/// unmistakable: 3.8 against 4.9 is a fifth, and the threshold here is a tenth.
///
/// `None` when there is nothing to choose between — every core the same speed, or a machine that
/// does not publish this, which includes every non-Linux one. In both cases rayon's default is
/// already the right answer and the caller should leave it alone.
#[must_use]
pub fn performance_cores() -> Option<Vec<usize>> {
    // What the kernel says, before what the clock implies.
    if let Some(cpus) = hybrid_performance_cpus() {
        return Some(cpus);
    }
    let speeds = max_frequencies()?;
    let top = speeds.iter().map(|(_, k)| *k).max()?;
    let cut = top - top / 10;
    let fast: Vec<usize> = speeds
        .iter()
        .filter(|(_, k)| *k >= cut)
        .map(|(c, _)| *c)
        .collect();
    // Every core is a fast core, so there is no choice to make.
    if fast.len() == speeds.len() || fast.len() < 2 {
        return None;
    }
    Some(fast)
}

/// The performance CPUs as the kernel itself reports them, rather than as a clock implies.
///
/// On a hybrid processor the perf subsystem publishes one PMU device per kind of core, and the
/// `cpus` file under each is the authoritative list — on a 12700K, `/sys/devices/cpu_core/cpus`
/// reads `0-15` and `/sys/devices/cpu_atom/cpus` reads `16-19`. That is a statement about the
/// silicon. [`max_frequencies`] is an inference from a number that may not be published at all
/// (no `cpufreq` driver, a virtual machine, a locked-down kernel), and it is the reason
/// `performance_cores` used to give up and hand back the whole machine on those.
///
/// It also removes a way the heuristic can be wrong in the *other* direction. Several Intel parts
/// bin two favoured cores a few hundred megahertz above their siblings — Turbo Boost Max 3.0 —
/// and on a part where that gap exceeded a tenth, the frequency rule would conclude the machine
/// had two performance cores and build a pool of two. Asking which cores are `cpu_core` cannot
/// make that mistake, because it never looks at speed.
///
/// `None` on every non-hybrid machine and every non-Linux one, where there is nothing to choose
/// between and rayon's default is already right.
#[cfg(target_os = "linux")]
fn hybrid_performance_cpus() -> Option<Vec<usize>> {
    // Both files, deliberately. `cpu_core` is present on some non-hybrid parts as well, where it
    // is simply "the CPU PMU" and lists every processor there is; it is only evidence of a split
    // when there is something on the other side of it.
    let fast = parse_cpu_list(&std::fs::read_to_string("/sys/devices/cpu_core/cpus").ok()?)?;
    let slow = parse_cpu_list(&std::fs::read_to_string("/sys/devices/cpu_atom/cpus").ok()?)?;
    if fast.len() < 2 || slow.is_empty() {
        return None;
    }
    Some(fast)
}

#[cfg(not(target_os = "linux"))]
fn hybrid_performance_cpus() -> Option<Vec<usize>> {
    None
}

/// Parse a kernel cpulist — comma-separated indices and inclusive ranges, as `0-7,16,18-19`.
///
/// `None` rather than a partial answer on anything unexpected, because the caller's fallback is
/// a working heuristic and half a CPU list is worse than none.
fn parse_cpu_list(text: &str) -> Option<Vec<usize>> {
    let mut out: Vec<usize> = Vec::new();
    for part in text.trim().split(',').filter(|p| !p.trim().is_empty()) {
        match part.split_once('-') {
            Some((lo, hi)) => {
                let lo: usize = lo.trim().parse().ok()?;
                let hi: usize = hi.trim().parse().ok()?;
                // A reversed or absurd range is a file this code does not understand.
                if hi < lo || hi.checked_sub(lo)? > 4096 {
                    return None;
                }
                out.extend(lo..=hi);
            }
            None => out.push(part.trim().parse().ok()?),
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(target_os = "linux")]
fn max_frequencies() -> Option<Vec<(usize, u64)>> {
    let mut speeds: Vec<(usize, u64)> = Vec::new();
    for cpu in 0..1024usize {
        let path = format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/cpuinfo_max_freq");
        let Ok(text) = std::fs::read_to_string(&path) else {
            break;
        };
        speeds.push((cpu, text.trim().parse().ok()?));
    }
    if speeds.is_empty() {
        return None;
    }
    Some(speeds)
}

#[cfg(not(target_os = "linux"))]
fn max_frequencies() -> Option<Vec<(usize, u64)>> {
    None
}

/// Build rayon's global pool with one worker per performance core.
///
/// Call once, early, from a binary or a benchmark. Safe to call more than once and safe to call
/// after rayon has already been used: a global pool can only be built once, and every later
/// attempt simply does nothing. The simulation is correct on whatever pool it ends up with, so
/// there is no failure here worth reporting to anybody.
///
/// Does nothing when `RAYON_NUM_THREADS` is set, so an experiment that asks for a particular pool
/// still gets it — which is what the thread-scaling probe in `tests/tick_cost.rs` relies on.
///
/// Returns the number of workers if this call is what built the pool.
pub fn use_performance_cores() -> Option<usize> {
    if std::env::var_os("RAYON_NUM_THREADS").is_some() {
        return None;
    }
    let n = performance_cores()?.len();
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build_global()
        .ok()
        .map(|()| n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rayon::prelude::*;

    #[test]
    fn the_fast_set_is_a_real_subset() {
        let Some(fast) = performance_cores() else {
            return; // a uniform machine, or one that does not say; nothing to check
        };
        assert!(fast.len() >= 2, "a pool of one is not worth building");
        // Against the machine's own processor count rather than against `max_frequencies`, which
        // is now only one of the two ways the set can be arrived at and is absent on a hybrid
        // machine with no `cpufreq` driver.
        let total = std::thread::available_parallelism().map_or(usize::MAX, std::num::NonZero::get);
        assert!(fast.len() < total, "a subset, or there was no choice to make");
        assert!(
            fast.iter().all(|c| *c < total),
            "a cpu index outside the machine"
        );
        let mut sorted = fast.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), fast.len(), "a cpu listed twice");
    }

    /// The cpulist grammar, including the shapes this machine does not happen to produce.
    ///
    /// `/sys/devices/cpu_core/cpus` reads `0-15` here, so every other spelling the kernel is
    /// entitled to use — a bare index, several ranges, a mixture — is untested by running on it.
    #[test]
    fn cpu_lists_parse_the_way_the_kernel_writes_them() {
        assert_eq!(parse_cpu_list("0-15"), Some((0..=15).collect()));
        assert_eq!(parse_cpu_list("16-19\n"), Some((16..=19).collect()));
        assert_eq!(parse_cpu_list("3"), Some(vec![3]));
        assert_eq!(parse_cpu_list("0-3,8,12-13"), Some(vec![0, 1, 2, 3, 8, 12, 13]));
        // Nothing, and nothing that can be trusted: the caller has a working fallback and would
        // rather take it than act on half an answer.
        assert_eq!(parse_cpu_list(""), None);
        assert_eq!(parse_cpu_list("\n"), None);
        assert_eq!(parse_cpu_list("7-3"), None, "a reversed range");
        assert_eq!(parse_cpu_list("0-15,frog"), None);
        assert_eq!(parse_cpu_list("0-999999"), None, "wider than any machine");
    }

    #[test]
    fn asking_twice_is_harmless_and_leaves_rayon_usable() {
        // Whichever call wins — or neither, if something has already used rayon — none may panic
        // and none may leave the pool broken. The simulation is correct on any pool; this is
        // only ever a hint.
        let _ = use_performance_cores();
        let _ = use_performance_cores();
        let total: usize = (0..1000usize).into_par_iter().sum();
        assert_eq!(total, 499_500);
    }
}
