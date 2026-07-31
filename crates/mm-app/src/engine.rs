//! The simulation, on its own thread (M10.1).
//!
//! # Why
//!
//! Until now `advance_simulation` and `redraw` were chained in one Bevy schedule, so at 1× the
//! world advanced exactly once per frame and **the tick rate was the frame rate**. Three things
//! follow from that, and all three are bad:
//!
//! * M4's third acceptance test — "dropping the render to 5fps does not change tick output" —
//!   could not fail, because there was no separate tick output to change.
//! * Any frame-budget measurement was really the tick cost and the render cost added together,
//!   so neither half of the working target in `docs/MILESTONES.md` could be measured at all.
//! * A heavy frame slowed the world down, which is the wrong way round for a simulator: the
//!   world is the thing, and the picture of it is what should be dropped when there is not
//!   enough time for both.
//!
//! # What is guaranteed
//!
//! The world is still advanced by **a whole number of ticks and nothing else**. Wall-clock time
//! decides *when* ticks happen and *how many per second*, never what a tick does — exactly as
//! before, where the frame rate played the same role. `Slide::advance` takes a count, there is
//! no delta time anywhere below this module, and the world at tick `n` is identical whatever
//! rate it was run at or how the ticks were grouped. That is asserted directly in the tests
//! here, against a world advanced in one go.
//!
//! `Instant` appears in this file, which `mm-core` forbids itself (hard rule 5). The rule is
//! about the simulation: no tick may depend on elapsed time. Pacing a thread that calls
//! `advance(n)` is not a tick depending on elapsed time, any more than a frame rate was.
//!
//! # Shape
//!
//! One mutex around the [`Slide`], taken and released **once per tick**, so the front end never
//! waits longer than a single tick to reach the world. At 50,000 cells a tick is milliseconds
//! and an uncontended lock is nanoseconds, so the overhead does not appear in any measurement;
//! at a thousand ticks a second on a small world it is still nothing.
//!
//! Frames are handed over separately, through a slot the render thread empties and the
//! simulation thread refills. Nothing is cloned and the common path — draw the last frame —
//! never takes the world's lock at all.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::foodweb::FoodWeb;
use crate::inspector::Inspection;
use crate::slide::{Frame, MetricHistory, Slide};

/// What "1×" means: sixty ticks a second.
///
/// The rate the microscope ran at when the world was advanced once per frame at sixty frames a
/// second. Keeping the number the same means a scenario that looked right before still looks
/// right, and that the speed control means what it used to mean.
pub const REALTIME: u32 = 60;

/// Sentinel rate: run the world as fast as the machine will go (SPEC §14).
const UNLIMITED: u32 = u32::MAX;

/// How long the simulation thread waits when it has nothing owed.
///
/// Short enough that resuming feels immediate, long enough that a paused world does not spin a
/// core.
const IDLE: Duration = Duration::from_micros(500);

/// The most catching-up a stalled thread will do at once, as a fraction of a second's ticks.
///
/// Without a cap, a window dragged across a desktop for five seconds comes back and runs five
/// seconds of world in one burst. The world would be *correct* — it is still whole ticks — but
/// the jump is not what anybody asked for.
const MAX_BACKLOG_SECONDS: f64 = 0.25;

/// How fast the world runs. Not how it runs: nothing here reaches a tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rate {
    /// Stopped. A paused world is still inspectable, and still publishes frames, because a
    /// tool can change it while it is not running.
    Paused,
    /// Ticks per second. [`REALTIME`] is 1×.
    PerSecond(u32),
    /// As fast as the machine will go.
    Unlimited,
}

impl Rate {
    /// `n` times realtime.
    #[must_use]
    pub fn times(n: u32) -> Rate {
        match n {
            0 => Rate::Paused,
            n => Rate::PerSecond(n.saturating_mul(REALTIME)),
        }
    }

    fn encode(self) -> u32 {
        match self {
            Rate::Paused => 0,
            Rate::Unlimited => UNLIMITED,
            // A requested rate of zero is a pause however it was spelled, and `UNLIMITED` is
            // the sentinel, so a literal `PerSecond(u32::MAX)` clamps rather than aliasing it.
            Rate::PerSecond(n) => n.min(UNLIMITED - 1),
        }
    }

    fn decode(raw: u32) -> Rate {
        match raw {
            0 => Rate::Paused,
            UNLIMITED => Rate::Unlimited,
            n => Rate::PerSecond(n),
        }
    }

    #[must_use]
    pub fn is_running(self) -> bool {
        self != Rate::Paused
    }
}

/// Everything the front end needs each frame, gathered on the simulation thread.
///
/// The point of the bundle is that **the panels drawn every frame never take the world's lock**.
/// A lock taken once per frame would wait up to a whole tick — thirty milliseconds at fifty
/// thousand cells, which is a dropped frame every frame — because the simulation thread holds
/// it for the duration of each tick.
///
/// The panels *not* covered here — wiki, editor, debugger, and the tools — do take the lock.
/// They are opened deliberately to look at one thing, and a stutter while one is open is an
/// honest price. If that stops being true, publishing their data too is what to do about it,
/// and M10.4 owns the ecology half of it.
#[derive(Clone, Debug)]
pub struct Published {
    pub frame: Frame,
    /// The selected cell, read where the world is rather than where the panel is.
    pub inspection: Option<Inspection>,
    /// The selected cell's species name. Needs the archive, so it cannot be worked out on the
    /// render thread without the very lock this exists to avoid.
    pub species: String,
    pub history: MetricHistory,
    pub web: FoodWeb,
    /// The objective's settings as the frame was built under them, so the renderer draws the
    /// vignette this frame was measured for rather than the one that has since been toggled.
    pub optics: crate::optics::Optics,
}

/// State shared between the two threads.
struct Shared {
    slide: Mutex<Slide>,
    /// The newest bundle, waiting to be collected. The renderer empties it; the simulation
    /// refills it. `None` means "the renderer has taken the last one and wants another", which
    /// is what paces frame building to the frame rate without either side knowing the other's.
    posted: Mutex<Option<Published>>,
    /// Which cell the front end has selected, so the simulation thread knows whose reading to
    /// publish. A mutex rather than an atomic because a `CellId` is a generational pair, and
    /// packing it into a `u64` would be a second encoding to keep honest.
    selected: Mutex<Option<mm_core::CellId>>,
    rate: AtomicU32,
    /// One-shot ticks owed, honoured whatever the rate is — including paused, which is the
    /// point of them.
    steps: AtomicU64,
    stop: AtomicBool,
    /// Ticks actually achieved in the last second. The readout, and the simulation half of the
    /// working target.
    measured: AtomicU32,

    // --- presentation: set by the renderer, applied by the simulation thread ---
    //
    // These belong to `Slide` because `frame()` reads them and `frame()` runs over there. Going
    // through the lock to set them would make every change wait for a tick, and the zoom is set
    // on every frame the wheel moves. Atomics instead, applied under the lock the simulation
    // thread is already taking to build the frame.
    /// Pixels per substrate square, as `f32` bits.
    zoom: AtomicU32,
    /// One bit per chemical.
    overlays: AtomicU32,
    optics: AtomicBool,
}

/// Lock without letting one thread's panic wedge the other.
///
/// A poisoned mutex means some other thread panicked while holding it. The front end recovering
/// and showing the world as it stands is better than a second panic on top of the first, and
/// there is nothing in `Slide` that a panic could leave half-written — `advance` either
/// completed a tick or did not.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A way to reach the world that does not borrow the [`Engine`].
///
/// Panels need to hold the world's lock *and* write to the rest of the front-end's state at the
/// same time — a tool applies to the world and then records what it did. If taking the lock
/// borrowed the engine, and the engine lived in the same resource as everything else, that
/// would be two borrows of one struct and none of it would compile. Cloning an `Arc` costs
/// nothing and sidesteps the whole question.
#[derive(Clone)]
pub struct Handle(Arc<Shared>);

impl Handle {
    /// Reach the world. Blocks the simulation thread for as long as the guard is held, so hold
    /// it for the length of a panel and not the length of a frame.
    pub fn slide(&self) -> MutexGuard<'_, Slide> {
        lock(&self.0.slide)
    }
}

/// The simulation thread, and the controls for it.
pub struct Engine {
    shared: Arc<Shared>,
    /// `None` once joined. Only [`Engine::stop`] and `Drop` take it.
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Start the world running on its own thread.
    #[must_use]
    pub fn start(slide: Slide, rate: Rate) -> Engine {
        // The presentation atomics start from whatever the slide already says, so starting the
        // thread cannot silently reset the overlays a scenario chose.
        let overlays = slide.overlay_mask();
        let optics = slide.optics.enabled;
        let shared = Arc::new(Shared {
            slide: Mutex::new(slide),
            posted: Mutex::new(None),
            selected: Mutex::new(None),
            rate: AtomicU32::new(rate.encode()),
            steps: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            measured: AtomicU32::new(0),
            zoom: AtomicU32::new(1.0f32.to_bits()),
            overlays: AtomicU32::new(overlays),
            optics: AtomicBool::new(optics),
        });
        let worker = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("mm-simulation".to_string())
            .spawn(move || run(&worker))
            .ok();
        Engine { shared, thread }
    }

    #[must_use]
    pub fn handle(&self) -> Handle {
        Handle(Arc::clone(&self.shared))
    }

    /// Take the newest bundle, if the simulation has published one since the last call.
    ///
    /// `None` means nothing new — the renderer should keep drawing what it has, which is what
    /// makes a render slower than the simulation cost the simulation nothing.
    #[must_use]
    pub fn collect(&self) -> Option<Published> {
        lock(&self.shared.posted).take()
    }

    /// Tell the simulation thread whose reading to publish.
    pub fn select(&self, cell: Option<mm_core::CellId>) {
        *lock(&self.shared.selected) = cell;
    }

    /// Pixels per substrate square, which chooses the level-of-detail tier.
    pub fn set_zoom(&self, pixels_per_square: f32) {
        self.shared
            .zoom
            .store(pixels_per_square.to_bits(), Ordering::Relaxed);
    }

    pub fn toggle_overlay(&self, chemical: usize) {
        if chemical < 32 {
            self.shared
                .overlays
                .fetch_xor(1u32 << chemical, Ordering::Relaxed);
        }
    }

    #[must_use]
    pub fn overlay_enabled(&self, chemical: usize) -> bool {
        chemical < 32 && self.shared.overlays.load(Ordering::Relaxed) & (1u32 << chemical) != 0
    }

    pub fn set_optics(&self, on: bool) {
        self.shared.optics.store(on, Ordering::Relaxed);
    }

    #[must_use]
    pub fn optics_enabled(&self) -> bool {
        self.shared.optics.load(Ordering::Relaxed)
    }

    pub fn set_rate(&self, rate: Rate) {
        self.shared.rate.store(rate.encode(), Ordering::Relaxed);
    }

    #[must_use]
    pub fn rate(&self) -> Rate {
        Rate::decode(self.shared.rate.load(Ordering::Relaxed))
    }

    /// Advance exactly one tick, whatever the rate is. The thing that makes a paused world
    /// inspectable.
    pub fn step(&self) {
        self.shared.steps.fetch_add(1, Ordering::Relaxed);
    }

    /// Ticks achieved per second, measured. The simulation half of the working target, and the
    /// number that used to be indistinguishable from the frame rate.
    #[must_use]
    pub fn ticks_per_second(&self) -> u32 {
        self.shared.measured.load(Ordering::Relaxed)
    }

    /// Stop the thread and wait for it. Idempotent.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Block until the world reaches `tick`, or `timeout` elapses. `true` if it got there.
    ///
    /// For tests, and for the run-to-tick the debugger will want. Polls rather than signals
    /// because the wait is measured in whole ticks and the thing being waited on is the world's
    /// own counter, not an event.
    pub fn wait_for_tick(&self, tick: u64, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.handle().slide().world().tick_count() >= tick {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(IDLE);
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The simulation thread's whole life.
fn run(shared: &Shared) {
    // Fractional ticks owed, carried between iterations so that a rate which is not a whole
    // number of ticks per iteration still averages out rather than rounding to zero forever.
    let mut owed = 0.0f64;
    let mut last = Instant::now();
    let mut sampled_at = Instant::now();
    let mut since_sample: u64 = 0;

    while !shared.stop.load(Ordering::Acquire) {
        let now = Instant::now();
        let elapsed = now.duration_since(last).as_secs_f64();
        last = now;

        let rate = Rate::decode(shared.rate.load(Ordering::Relaxed));
        // Taken, not read: a step is owed exactly once.
        let mut want = shared.steps.swap(0, Ordering::Relaxed);
        match rate {
            Rate::Paused => {}
            Rate::Unlimited => want = want.saturating_add(1),
            Rate::PerSecond(n) => {
                owed += elapsed * f64::from(n);
                owed = owed.min(f64::from(n) * MAX_BACKLOG_SECONDS);
                let whole = owed.floor();
                owed -= whole;
                want = want.saturating_add(whole as u64);
            }
        }

        let mut done = 0u64;
        while done < want && !shared.stop.load(Ordering::Relaxed) {
            // One tick per lock. The front end waits at most one tick to reach the world, and
            // the lock itself does not show up next to the cost of a tick.
            lock(&shared.slide).advance(1);
            done += 1;
        }
        since_sample += done;

        // Build a bundle only when the renderer has taken the last one. Two things fall out:
        // it is paced by the render rate without either side knowing the other's, and a
        // renderer slower than the world simply misses frames instead of holding it up.
        if lock(&shared.posted).is_none() {
            let selected = *lock(&shared.selected);
            let published = {
                let mut slide = lock(&shared.slide);
                // The renderer's presentation choices, applied under the lock that is being
                // taken anyway, so setting them never had to wait for a tick.
                slide.set_zoom(f32::from_bits(shared.zoom.load(Ordering::Relaxed)));
                slide.set_overlay_mask(shared.overlays.load(Ordering::Relaxed));
                slide.optics.enabled = shared.optics.load(Ordering::Relaxed);

                let inspection = selected.and_then(|id| slide.inspect(id));
                Published {
                    frame: slide.frame(),
                    species: inspection
                        .as_ref()
                        .map_or_else(String::new, |c| slide.species_name(c.species)),
                    inspection,
                    history: slide.history().clone(),
                    web: slide.food_web(),
                    optics: slide.optics,
                }
            };
            *lock(&shared.posted) = Some(published);
        }

        let sample_age = now.duration_since(sampled_at);
        if sample_age >= Duration::from_secs(1) {
            let per_second = since_sample as f64 / sample_age.as_secs_f64();
            shared.measured.store(per_second as u32, Ordering::Relaxed);
            since_sample = 0;
            sampled_at = now;
        }

        // Nothing owed: yield rather than spin. Unlimited never idles, which is the point.
        if done == 0 && rate != Rate::Unlimited {
            thread::sleep(IDLE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_core::{LightRegime, Scenario, Seeding};

    fn scenario() -> Scenario {
        Scenario {
            name: "engine".to_string(),
            seed: 7,
            width: 24,
            height: 24,
            light: LightRegime::Uniform {
                intensity: mm_core::Q10_ONE,
            },
            seeding: vec![Seeding::Uniform {
                chemical: 11,
                per_square: mm_core::fixed::q10(80),
            }],
            ..Scenario::default()
        }
    }

    /// What the world looks like at `ticks`, advanced in one go with no thread involved.
    fn headless(ticks: u64) -> u64 {
        let mut slide = Slide::new(scenario()).unwrap();
        slide.advance(ticks);
        slide.world().state_hash()
    }

    const PATIENCE: Duration = Duration::from_secs(20);

    #[test]
    fn a_world_run_on_a_thread_is_the_world_run_headless() {
        // The whole guarantee, stated directly: moving the simulation off the render thread
        // must not change what the simulation does. If this fails, nothing else here matters.
        let engine = Engine::start(Slide::new(scenario()).unwrap(), Rate::Unlimited);
        assert!(engine.wait_for_tick(500, PATIENCE), "never reached 500");
        engine.set_rate(Rate::Paused);
        // The thread may have been mid-batch when the rate changed, so compare against
        // whatever tick it actually stopped on rather than against 500.
        let held = engine.handle();
        let (tick, hash) = {
            let slide = held.slide();
            (slide.world().tick_count(), slide.world().state_hash())
        };
        assert!(tick >= 500);
        assert_eq!(hash, headless(tick), "the thread changed the world");
    }

    #[test]
    fn how_the_ticks_were_grouped_does_not_change_the_world() {
        // M4 acceptance 3, with teeth. This used to be asserted about frames, which was a
        // claim about a loop in the same thread; it is now a claim about a rate.
        let slow = Engine::start(Slide::new(scenario()).unwrap(), Rate::PerSecond(400));
        assert!(slow.wait_for_tick(200, PATIENCE), "slow never got there");
        slow.set_rate(Rate::Paused);
        let held = slow.handle();
        let (tick, hash) = {
            let slide = held.slide();
            (slide.world().tick_count(), slide.world().state_hash())
        };
        assert_eq!(hash, headless(tick));
    }

    #[test]
    fn collecting_frames_cannot_change_the_world() {
        // The render side of the same guarantee: a renderer that collects greedily and one that
        // never collects at all must leave the same world behind. `collect` is the only thing
        // the render thread does to the engine.
        let greedy = Engine::start(Slide::new(scenario()).unwrap(), Rate::Unlimited);
        for _ in 0..2_000 {
            let _ = greedy.collect();
        }
        assert!(greedy.wait_for_tick(300, PATIENCE));
        greedy.set_rate(Rate::Paused);
        let held = greedy.handle();
        let (tick, hash) = {
            let slide = held.slide();
            (slide.world().tick_count(), slide.world().state_hash())
        };
        assert_eq!(hash, headless(tick), "collecting frames reached the world");
    }

    #[test]
    fn the_presentation_controls_cannot_reach_the_world() {
        // Zoom, overlays, optics and selection are set through atomics and applied by the
        // simulation thread. That is a path from the renderer into the same struct the world
        // lives in, which is exactly the sort of thing M4 exists to forbid — so it is checked
        // rather than argued.
        let engine = Engine::start(Slide::new(scenario()).unwrap(), Rate::Paused);
        engine.step();
        assert!(engine.wait_for_tick(1, PATIENCE));
        let held = engine.handle();
        let before = held.slide().world().state_hash();

        for i in 0..16 {
            engine.toggle_overlay(i);
        }
        engine.set_zoom(64.0);
        engine.set_optics(false);
        engine.select(None);
        // Force several bundles to be built, since applying the controls happens there.
        for _ in 0..20 {
            let _ = engine.collect();
            thread::sleep(Duration::from_millis(2));
        }

        assert_eq!(
            held.slide().world().state_hash(),
            before,
            "a presentation control reached the world"
        );
        assert_eq!(held.slide().world().tick_count(), 1);
    }

    #[test]
    fn paused_means_paused() {
        let engine = Engine::start(Slide::new(scenario()).unwrap(), Rate::Paused);
        // Long enough that a thread which was going to run would have run.
        thread::sleep(Duration::from_millis(120));
        assert_eq!(engine.handle().slide().world().tick_count(), 0);
        // And it is still publishing, because a tool can change a stopped world and the picture
        // has to follow.
        assert!(engine.collect().is_some(), "a paused world went dark");
    }

    #[test]
    fn a_step_is_one_step_and_not_a_resume() {
        let engine = Engine::start(Slide::new(scenario()).unwrap(), Rate::Paused);
        engine.step();
        assert!(engine.wait_for_tick(1, PATIENCE));
        thread::sleep(Duration::from_millis(120));
        assert_eq!(
            engine.handle().slide().world().tick_count(),
            1,
            "a step resumed the world"
        );
    }

    #[test]
    fn steps_are_honoured_exactly_once() {
        let engine = Engine::start(Slide::new(scenario()).unwrap(), Rate::Paused);
        for _ in 0..5 {
            engine.step();
        }
        assert!(engine.wait_for_tick(5, PATIENCE));
        thread::sleep(Duration::from_millis(120));
        assert_eq!(engine.handle().slide().world().tick_count(), 5);
    }

    #[test]
    fn stopping_twice_is_allowed() {
        // `Drop` calls `stop`, so anything that stops explicitly stops twice. Joining a taken
        // handle would panic.
        let mut engine = Engine::start(Slide::new(scenario()).unwrap(), Rate::Paused);
        engine.stop();
        engine.stop();
    }

    #[test]
    fn a_rate_round_trips_through_its_encoding() {
        for rate in [
            Rate::Paused,
            Rate::Unlimited,
            Rate::PerSecond(1),
            Rate::PerSecond(60),
            Rate::PerSecond(15_360),
        ] {
            assert_eq!(Rate::decode(rate.encode()), rate);
        }
        // Zero ticks a second is a pause however it was spelled, and the sentinel cannot be
        // reached by asking for a very large rate.
        assert_eq!(Rate::decode(Rate::PerSecond(0).encode()), Rate::Paused);
        assert_eq!(
            Rate::decode(Rate::PerSecond(u32::MAX).encode()),
            Rate::PerSecond(UNLIMITED - 1)
        );
        assert_eq!(Rate::times(0), Rate::Paused);
        assert_eq!(Rate::times(8), Rate::PerSecond(8 * REALTIME));
    }

    #[test]
    fn a_backlog_does_not_become_a_burst() {
        // A window dragged around for a while must not come back and run the missing seconds
        // all at once. The cap is a quarter of a second's worth.
        let engine = Engine::start(Slide::new(scenario()).unwrap(), Rate::Paused);
        // Hold the world still for well over the cap while the rate says it should be running.
        let held = engine.handle();
        let guard = held.slide();
        engine.set_rate(Rate::PerSecond(1_000));
        thread::sleep(Duration::from_millis(600));
        drop(guard);
        thread::sleep(Duration::from_millis(50));
        engine.set_rate(Rate::Paused);
        let ticks = engine.handle().slide().world().tick_count();
        // 600ms at 1,000/s would be 600 ticks without the cap; the cap allows 250 plus whatever
        // the 50ms of genuine running produced.
        assert!(ticks < 450, "caught up with a burst of {ticks} ticks");
    }
}
