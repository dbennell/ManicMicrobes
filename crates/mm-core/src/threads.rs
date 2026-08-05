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
        let all = max_frequencies().expect("frequencies, since a fast set was found");
        assert!(fast.len() >= 2, "a pool of one is not worth building");
        assert!(fast.len() < all.len(), "a subset, or there was no choice to make");
        assert!(
            fast.iter().all(|c| *c < all.len()),
            "a cpu index outside the machine"
        );
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
