//! Rayon's pool, pinned to the cores worth having.
//!
//! [`mm_core::threads`] works out *which* cores those are and can build a pool with the right
//! number of workers, but it cannot pin them: pinning is `sched_setaffinity`, `mm-core` is
//! `#![forbid(unsafe_code)]`, and that rule is worth more than the four percent pinning adds.
//! This is the other half, in the crate that is allowed to make the call.
//!
//! Measured on an i7-12700K at fifty thousand cells — eight performance cores with hyperthreading
//! on CPUs 0–15, four efficiency cores on 16–19:
//!
//! | pool | ms/tick |
//! |---|---|
//! | all 20 threads, unpinned (rayon's default) | 32.10 |
//! | 16 threads, unpinned | 29.70 |
//! | **16 threads, pinned here** | **28.31** |
//!
//! Without a mask the scheduler is free to migrate a worker onto an efficiency core whatever the
//! pool size, and on a busy desktop it does. Every parallel phase ends at a barrier, so one
//! worker on a slower core holds up the phase.
//!
//! None of this can change what the simulation computes: hard rule 6 requires that no outcome
//! depend on scheduling or thread count, and the determinism test holds the world to it.

/// Build rayon's global pool with one worker per performance core, each pinned to it.
///
/// Call once, early, before anything has used rayon. Safe to call twice and safe to call late —
/// a global pool can only be built once and every later attempt does nothing at all. Honours
/// `RAYON_NUM_THREADS`, so an experiment that asks for a particular pool still gets it.
///
/// Returns the number of workers when this call is what built the pool.
pub fn use_performance_cores() -> Option<usize> {
    if std::env::var_os("RAYON_NUM_THREADS").is_some() {
        return None;
    }
    let cores = mm_core::threads::performance_cores()?;
    let n = cores.len();
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        // In the worker, because that is the only place that knows which thread it is:
        // `sched_setaffinity` with pid 0 applies to the caller.
        .start_handler(move |_| pin_to(&cores))
        .build_global()
        .ok()
        .map(|()| n)
}

/// Confine the calling thread to `cpus`.
///
/// Best-effort in the strictest sense. A failure means the thread runs where it would have run
/// anyway, which is the behaviour this is trying to improve on and never a reason to stop.
#[cfg(target_os = "linux")]
fn pin_to(cpus: &[usize]) {
    // SAFETY: `set` is zeroed before use and written only through `CPU_SET`, which is bounded
    // above by the `CPU_SETSIZE` check; `sched_setaffinity` receives that set and its true size,
    // and pid 0 means the calling thread. Nothing is shared and nothing outlives the call.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut set);
        for &cpu in cpus {
            if cpu < libc::CPU_SETSIZE as usize {
                libc::CPU_SET(cpu, &mut set);
            }
        }
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
    }
}

#[cfg(not(target_os = "linux"))]
fn pin_to(_cpus: &[usize]) {}

#[cfg(test)]
mod tests {
    use rayon::prelude::*;

    #[test]
    fn the_pool_still_works_however_this_went() {
        let _ = super::use_performance_cores();
        let _ = super::use_performance_cores();
        let total: usize = (0..10_000usize).into_par_iter().sum();
        assert_eq!(total, 49_995_000);
    }
}
