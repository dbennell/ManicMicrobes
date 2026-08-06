//! The microscope (M4).
//!
//! ```text
//! cargo run -p mm-app --features render --release
//! ```
//!
//! # What this is
//!
//! The slide is presented as a plate under a microscope: chemical fields as false-colour
//! overlays in each chemical's own colour, light as a warm luminance layer, cells as points
//! that resolve into organelles as you zoom in, a circular vignette and dust on the objective.
//!
//! # The shell (M10.1, `docs/UI.md`)
//!
//! A menu bar across the top, a rail either side, a tabbed drawer along the bottom, and the
//! slide in whatever is left. Every shortcut below also appears against its menu item; there
//! is deliberately no binding that cannot be discovered from the menus.
//!
//! ```text
//!   File  Slide  Simulation  View  Tools  Help     ⏸ ▶ ½× 1× 8× max ⏭
//!  ┌────────────┬────────────────────────────┬──────────────────────┐
//!  │  cell      │        THE SLIDE           │  metrics             │
//!  │            │                            │  legend              │
//!  ├────────────┴────────────────────────────┴──────────────────────┤
//!  │  genome │ ecology │ editor │ debugger                            │
//!  ├─────────────────────────────────────────────────────────────────┤
//!  │  Cryptous mixtus · tick 1 204 887 · 48 213 cells       102×     │
//!  └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! | | |
//! |---|---|
//! | drag | pan the slide |
//! | wheel | zoom about the pointer, whole-slide to single cell |
//! | left click | select a cell |
//! | right click | apply the current tool |
//! | `space` | pause / resume, at the speed you were watching at |
//! | `.` | step one tick |
//! | `0` `` ` `` `-` `=` `backspace` | speed: paused, ½×, 1×, 8×, as fast as it will go |
//! | `1`–`9` | toggle that chemical's overlay |
//! | `i` `p` `l` | cell, metrics, legend |
//! | `g` `w` `e` `d` | drawer: genome, ecology, editor, debugger |
//! | `f` | the ecology pane, on the food web |
//! | `c` | rounded cells |
//! | `t` | track the selected cell |
//! | `home` | reset the camera |
//! | `F1`–`F5` | tool: select, move, remove, wall, erase |
//! | `o` | optics on/off |
//! | `r` | wipe and reseed the slide |
//!
//! Which of those the pointer and the keyboard reach is decided in [`mm_app::ui`], once a
//! frame, rather than by each reader of the mouse asking egui separately — which is what let a
//! scroll on the genome listing zoom the microscope, and what let typing `p` into the editor
//! open the metrics rail.
//!
//! # What it is not allowed to be
//!
//! It never touches the world. Everything on screen comes from a [`Frame`] — a snapshot with
//! no reference back — and the simulation is advanced by a whole number of ticks per frame,
//! never by a delta time. A world watched at sixty frames a second is bit-identical to one run
//! headless, and that is enforced by the shape of [`Slide`] rather than by care. The tests
//! that check it live in `slide.rs` and run without a graphics stack.
//!
//! # Status
//!
//! Everything drawn here is derived from data that is tested without a graphics stack
//! (`optics.rs`, `inspector.rs`, `slide.rs`, `ui.rs`), which is where the guarantees live. The
//! layout constants are not tested and are not meant to be; they are first guesses adjusted by
//! looking.
//!
//! The vignette, depth-of-field and chromatic aberration are applied per-sprite from the
//! parameters in [`mm_app::optics`] rather than as a full-screen post-process pass. That is a
//! deliberate simplification: it needs no custom render graph node, it is exactly right for
//! the vignette and the aberration, and it approximates defocus by size and alpha rather than
//! by convolution. A real separable blur belongs in the post-process pass this leaves room
//! for.

use bevy::diagnostic::{Diagnostic, DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::image::ImageSampler;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
// 0.17 split the renderer into `bevy_mesh`, `bevy_shader`, `bevy_camera` and
// `bevy_sprite_render`. Nothing below changed but where it lives.
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::Mesh2d;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite_render::MeshMaterial2d;
use bevy::window::PrimaryWindow;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};

use mm_app::art;
use mm_app::cellmesh;
use mm_app::cellpipe;
use mm_app::debugger::{Breakpoint, Breakpoints, Sandbox};
use mm_app::editor::Editor;
use mm_app::engine::{Engine, Published, Rate};
use mm_app::inspector::Inspection;
use mm_app::library;
use mm_app::params;
use mm_app::skin;
use mm_app::slide::{self, Frame, Lod, Slide};
use mm_app::theme::{self, Mood, Role};
use mm_app::tools::{self, ToolEvent};
use mm_app::ui::{self, Dock, Ecology, Focus, Panel, Panels, Rect, Target};
use mm_app::wiki;
use mm_core::biology::BiologyConfig;
use mm_core::cell::CellSeed;
use mm_core::fixed::{pos, q10};
use mm_core::light::CurrentField;
use mm_core::metrics::Sample;
use mm_core::{CellId, LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding};

/// Render one frame to a PNG and carry on, when asked by the environment.
///
/// The only way a change to the renderer can be checked at all. Everything else in this crate
/// is tested without a graphics stack — that is the point of the wall in `slide.rs` — which
/// leaves the actual pixels verified by looking, and looking needs something to look at. A
/// desktop capture is no good under Wayland and asks for a permission nobody wants to grant to
/// a simulation; this asks the renderer that drew the frame.
///
/// ```text
/// MM_SHOT=/tmp/slide.png MM_SHOT_AFTER=900 MM_SHOT_ZOOM=14 cargo run -p mm-app --features render --release
/// ```
///
/// `MM_SHOT_TICK` is the one to reach for: it photographs a *state*, so two runs of different
/// speed produce comparable pictures. `MM_SHOT_AFTER` is in frames and is what there was
/// before, which is a proxy for wall-clock and therefore for how fast the build happened to be; `MM_SHOT_ZOOM` and `MM_SHOT_FLAT` set the camera and the cell style up front,
/// because a run that exits after one photograph has nobody to drive it.
///
/// **The frame rate in a `MM_SHOT` run means nothing.** Bevy's default `WinitSettings` waits a
/// second between redraws while the window is unfocused, and a run driven from a script never
/// has focus — so the status bar reads exactly `1 fps` however fast the renderer is. Measured
/// rather than assumed: 120 frames in 120.34 seconds, which is 0.997, and no workload lands
/// within a third of a percent of exactly one. The simulation is unaffected and keeps its own
/// rate, so `MM_SHOT_TICK` still photographs the state it says it does. To time the renderer,
/// focus the window and read it there.
///
/// **It photographs the panels too**, and used to not. This note said the opposite for several
/// versions — Bevy 0.15 took the screenshot before the egui pass, so what came out was the slide
/// and none of the interface over it — and somewhere between then and 0.19 that stopped being
/// true. Believing the stale note cost a review of a pane that could have been looked at
/// directly, so: `MM_SHOT_VIEW=ecology:budget` photographs the budget pane, and the whole window
/// is in the file.
///
/// The file is named for `MM_SHOT` with a frame number appended, as `slide_000.png`. A single
/// shot still gets a number, so a series and a lone frame are named the same way and one
/// analysis script reads both.
fn numbered_path(path: &str, n: u32) -> String {
    match path.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}_{n:03}.{ext}"),
        None => format!("{path}_{n:03}"),
    }
}

fn screenshot(
    mut commands: Commands,
    mut sim: ResMut<SlideRes>,
    mut view: ResMut<View>,
    mut exit: MessageWriter<AppExit>,
    mut frames: Local<u32>,
    mut done: Local<Option<u32>>,
    mut shot: Local<u32>,
    // Whether `MM_SHOT_VIEW` has been applied. Once only — `arrange` toggles rather than sets
    // for some of what it touches, so running it twice would switch things back off.
    mut arranged: Local<bool>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    *frames += 1;
    if *frames == 1 {
        // The bench replaces the world, so it has to happen before the run rather than one
        // frame before the photograph like the panels do — the point of it is what the cells
        // settle into, and at tick zero they have not settled into anything.
        if std::env::var("MM_SHOT_BENCH").is_ok() {
            sim.bench();
        }
        // Centre on whatever slide is actually loaded, always — not only after the bench has
        // replaced it. The camera opens on the middle of the *default* slide, so `MM_SLIDE` or
        // `MM_OPEN` with anything smaller photographed a patch of empty water off one side of
        // it, which reads as "the feature draws nothing" rather than as "the camera is
        // elsewhere".
        //
        // From the world rather than from `latest`: the engine publishes frames on its own
        // schedule, so the newest one still describes the slide that was just replaced.
        let (w, h) = {
            let held = sim.engine.handle();
            let slide = held.slide();
            let s = slide.world().substrate();
            (s.width(), s.height())
        };
        view.centre = Vec2::new(w as f32 / 2.0, h as f32 / 2.0);
        if let Ok(zoom) = std::env::var("MM_SHOT_ZOOM") {
            view.zoom = zoom.parse().unwrap_or(view.zoom);
        }
        if std::env::var("MM_SHOT_FLAT").is_ok() {
            view.rounded = false;
        }
    }
    // Arranged one frame before the photograph rather than at startup, so a panel that needs a
    // selection has a populated world to select from.
    // `MM_SHOT_TICK` waits for a state; `MM_SHOT_AFTER` waits for a number of frames.
    //
    // Prefer the tick. Frames are a proxy for wall-clock, and wall-clock buys wildly different
    // amounts of simulation depending on how fast the thing is running — the same
    // `MM_SHOT_AFTER=2600` photographed twenty thousand cells before a renderer change and a
    // hundred and eight thousand after it. Two screenshots taken that way are not comparable,
    // which turns a measurement into an anecdote, and it did so repeatedly before anyone noticed
    // the units.
    let want_tick: Option<u64> = std::env::var("MM_SHOT_TICK")
        .ok()
        .and_then(|v| v.parse().ok());
    let after: u32 = std::env::var("MM_SHOT_AFTER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    // Arranged ahead of the photograph, on whichever trigger is in force.
    //
    // This used to key off the frame counter alone, so `MM_SHOT_VIEW` silently did nothing
    // whenever `MM_SHOT_TICK` was the trigger — the arrangement landed at frame 600 and the
    // photograph had been taken hundreds of frames earlier. Nothing reported it; the picture
    // just came out in the default state, which looks exactly like a panel that failed to draw.
    //
    // `ARRANGE_LEAD` frames of grace rather than one, because some of what `arrange` switches
    // on is not visible in the very next frame: a sprite pool grows through `commands`, which
    // are applied after the system that queued them, so the flow overlay's arrows exist one
    // frame later and carry a size and a colour the frame after that.
    let arrange_at = match want_tick {
        Some(want) => sim.engine.tick_count() + u64::from(ARRANGE_LEAD) >= want,
        None => u64::from(*frames + ARRANGE_LEAD) >= u64::from(after),
    };
    if arrange_at && !*arranged {
        *arranged = true;
        if let Ok(spec) = std::env::var("MM_SHOT_VIEW") {
            arrange(&spec, &mut sim, &mut view);
        }
    }
    let Ok(path) = std::env::var("MM_SHOT") else {
        return;
    };
    // How many consecutive frames to photograph. One is a picture; a series is a measurement.
    //
    // A still frame cannot show instability, and describing one in prose is not analysis — a
    // change that altered nothing at all was twice reported here as making things visibly worse,
    // because the pictures were being read rather than compared. A series can be differenced,
    // which turns "it flickers" into a number.
    let series: u32 = std::env::var("MM_SHOT_SERIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .max(1);
    // Taken, and now waiting to be written. The save is asynchronous — the observer fires when
    // the image has come back off the GPU — so quitting the same frame would race it and
    // produce no file. A few frames' grace, then out.
    if let Some(taken_at) = *done {
        if *shot < series {
            // Still collecting the series: one frame apart, so what shows up between them is a
            // tick's worth of movement and nothing else.
            let n = *shot;
            *shot += 1;
            let numbered = numbered_path(&path, n);
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(numbered));
            return;
        }
        if frames.saturating_sub(taken_at) > 10 {
            exit.write(AppExit::Success);
        }
        return;
    }
    // A tick to wait for beats a frame count, so it wins when both are set.
    if let Some(want) = want_tick {
        if sim.engine.tick_count() < want {
            return;
        }
    } else if *frames < after {
        return;
    }
    // An entity with an observer since 0.15, rather than a manager resource. Failure is the
    // observer's problem and is reported by it: a missing directory should not take down a run
    // that is otherwise doing its job.
    let first = numbered_path(&path, 0);
    *shot = 1;
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(first));
    // `MM_SHOT` is a batch tool: it exists so a change can be photographed from a script, and
    // a window that sits open afterwards waiting to be killed by a `timeout` is a window
    // somebody has to close. It quits itself once the file is on disk.
    *done = Some(*frames);
}

/// Replace the running world with a scenario from a file.
///
/// The whole of "open a scenario", in one place so that the library menu and the path field
/// cannot drift apart. A failure leaves the slide exactly as it was — the world is only touched
/// once the file has parsed and `World::new` has accepted it, so a typo costs you nothing.
///
/// The camera follows, for the same reason `New slide` moves it: opening a sixteen-square vent
/// while parked over the middle of a five-hundred-square soup leaves you staring at open water
/// with no clue that anything happened.
fn open_scenario(sim: &mut SlideRes, view: &mut View, path: &std::path::Path) {
    let scenario = match library::load(path) {
        Ok(s) => s,
        Err(e) => {
            view.file_note = Some(Err(e.to_string()));
            return;
        }
    };
    let (name, size) = (scenario.name.clone(), scenario.width.max(scenario.height));
    let world = match mm_core::World::new(scenario) {
        Ok(w) => w,
        Err(e) => {
            view.file_note = Some(Err(format!("{} will not start: {e:?}", path.display())));
            return;
        }
    };
    {
        let held = sim.engine.handle();
        held.slide().set_world(world);
    }
    // Seed it. A scenario that names its own inhabitants gets those and the New-slide founder
    // count is ignored, because a slide written around a strategy knows better than a spinner
    // does who belongs on it. One that names nobody still gets ancestors, because the library
    // handing you a beautifully authored empty dish is the behaviour this replaced.
    let seeded = {
        let held = sim.engine.handle();
        let mut slide = held.slide();
        seed_into(&mut slide, view.new_founders)
    };

    // Everything that pointed into the old world. A selection is a slot in an arena that no
    // longer exists, and a breakpoint is an offset into a genome nobody is running.
    sim.selected = None;
    sim.engine.select(None);
    sim.sandbox = None;
    sim.breakpoints.rearm();
    sim.draft = None;
    view.centre = Vec2::splat(size as f32 / 2.0);
    view.zoom = (BASE_SCALE * 6.0 / size as f32).clamp(0.05, 40.0);
    view.file_path = path.display().to_string();
    view.file_note = Some(Ok(if seeded > 0 {
        format!("opened {name}, seeded {seeded}")
    } else {
        format!("opened {name}")
    }));
}

/// Put the interface into a named state, for a screenshot of it.
///
/// A comma-separated list of panels, so one run photographs one arrangement:
/// `MM_SHOT_VIEW=cell,genome` or `MM_SHOT_VIEW=ecology:web`. Reviewing a layout means looking
/// at it in each of the states it has, and a screenshot tool that can only photograph the
/// state it starts in can photograph one of them.
fn arrange(spec: &str, sim: &mut SlideRes, view: &mut View) {
    // Something to inspect. The first cell on the slide is as good as any and is the one a
    // person would click.
    let first = sim.latest.frame.cells.first().map(|dot| dot.id);
    for part in spec.split(',') {
        let (name, sub) = part.split_once(':').unwrap_or((part, ""));
        match name.trim() {
            "cell" => {
                sim.select(first);
                view.panels.set(Panel::Cell, true);
            }
            "genome" => {
                sim.select(first);
                view.panels.set(Panel::Genome, true);
            }
            "toolbox" => view.panels.set(Panel::Toolbox, true),
            "ecology" => {
                view.panels.set(Panel::Ecology, true);
                view.ecology = match sub {
                    "web" => Ecology::Web,
                    "timeline" => Ecology::Timeline,
                    "interventions" => Ecology::Interventions,
                    "budget" => Ecology::Budget,
                    _ => Ecology::Tree,
                };
            }
            "editor" => view.panels.set(Panel::Editor, true),
            "debugger" => view.panels.set(Panel::Debugger, true),
            "params" => {
                view.panels.set(Panel::Parameters, true);
                // `params:metabolism` names the page, the way `ecology:web` names the view.
                // Without it a screenshot of the editor is always whichever page was last
                // looked at, which is not a thing a script can rely on.
                if let Some(group) = params::Group::ALL.iter().find(|g| g.title() == sub) {
                    view.params_page = ParamPage::Group(*group);
                } else if !sub.is_empty() {
                    view.params_page = match sub {
                        "environment" => ParamPage::Environment,
                        "pathways" => ParamPage::Pathways,
                        "catalogue" => ParamPage::Catalogue,
                        other => {
                            eprintln!("MM_SHOT_VIEW: no such parameter page `{other}`");
                            view.params_page
                        }
                    };
                }
            }
            "interventions" => {
                view.panels.set(Panel::Ecology, true);
                view.ecology = Ecology::Interventions;
            }
            "bare" => {
                view.panels.metrics = false;
                view.panels.legend = false;
            }
            "nocells" => view.organelles = false,
            // The flow overlay is off by default, so a screenshot of it has to ask.
            "flow" => sim.engine.set_flow(true),
            // Every overlay at once, for photographing the "all" state.
            "alloverlays" => sim
                .engine
                .set_overlays(ui::all_overlays(sim.chem_names.len())),
            // Open a scenario by library label, so a screenshot can photograph one.
            "open" => {
                let want = sub.replace('_', " ");
                if let Some(e) = library::scenarios().into_iter().find(|e| e.label == want) {
                    open_scenario(sim, view, &e.path);
                }
            }
            // For asking whether the *picture* is stable: with the world stopped, two frames
            // that differ differ because of the renderer and nothing else.
            "pause" => {
                sim.engine.set_rate(Rate::Paused);
                view.paused = true;
            }
            other => eprintln!("MM_SHOT_VIEW: no such panel `{other}`"),
        }
    }
}

/// Frames of grace between arranging the interface and photographing it.
///
/// Three, because a sprite pool takes two to come up: `commands.spawn` is applied after the
/// system that queued it, so an overlay switched on this frame has entities next frame and a
/// size and colour on the one after. Photographed any sooner and the overlay is missing from
/// the picture for a reason that has nothing to do with whether it works.
const ARRANGE_LEAD: u32 = 3;

/// Pixels per substrate square at zoom 1.
const BASE_SCALE: f32 = 8.0;

/// Roughly how far apart flow arrows land, in screen pixels.
///
/// The lattice is chosen in screen space rather than in substrate squares so the field reads
/// the same at every zoom. At whole-slide magnification a substrate-spaced lattice is a solid
/// hedge; at full magnification it is one arrow somewhere off the edge of the window.
const ARROW_PITCH: f32 = 46.0;

/// Shortest and longest an arrow is drawn, as a fraction of [`ARROW_PITCH`].
///
/// Length reads *speed relative to the solver's ceiling* rather than a literal distance the
/// water covers in some number of steps. The literal version was tried first and is unreadable:
/// at a gentle eighth of a square per step and six pixels to the square, eight steps of travel
/// is six pixels, so the whole field came out as a lattice of identical dashes with no length
/// to compare and no direction to see. Scaling against `FLOW_FULL` instead means the longest
/// arrow on screen is water at the engine's maximum and the shortest is barely moving, which is
/// the comparison somebody looking at a flow field actually wants to make.
const ARROW_SHORT: f32 = 0.35;
const ARROW_LONG: f32 = 1.05;

/// Speed below which no arrow is drawn, in squares per fluid step.
///
/// Still water gets nothing rather than a dot, which is what makes a channel legible: the
/// arrows are where the water is going, so a bare region means it is not going anywhere.
const FLOW_FLOOR: f32 = 0.002;

/// Speed at which an arrow is drawn at full brightness, in squares per fluid step.
///
/// A quarter of a square per step is `fluid::MAX_VELOCITY`, the fastest the solver allows, so
/// the brightest arrow on screen is water at the engine's own ceiling.
const FLOW_FULL: f32 = 0.25;

/// Every tool and its key, in one list so the menu and the toolbox cannot disagree.
const TOOLS: [(Tool, &str); 10] = [
    (Tool::Select, "F1"),
    (Tool::Move, "F2"),
    (Tool::Remove, "F3"),
    (Tool::DrawBarrier, "F4"),
    (Tool::EraseBarrier, "F5"),
    (Tool::Paint, "F6"),
    (Tool::Unpaint, "F7"),
    (Tool::Source, "F8"),
    (Tool::Drain, "F9"),
    (Tool::PlaceCell, "F10"),
];

/// Roughly how far apart the specks of suspended particulate are drawn, in pixels.
///
/// Sparser than it could be, on purpose. A dense stipple reads as fog — a property of the
/// water — where a sparse one reads as things *in* the water, which is what detritus is.
const SPECK_PITCH: f32 = 27.0;

/// The specks' colour, matching `detritus` in the chemical table.
///
/// A constant rather than a lookup because the stipple is drawn whether or not the chemical
/// overlay that would show that colour is switched on.
const SPECK_RGB: [f32; 3] = [0.745, 0.667, 0.510];

/// Most specks drawn per lattice block along one axis, so the square of it per block.
///
/// A ceiling on the work rather than a look: eight a side is sixty-four specks in one block of
/// water, which only happens at magnifications where a block fills a good part of the window.
const SPECK_MAX_SIDE: u64 = 8;

fn main() {
    // Before anything touches rayon, because a global pool can only be built once. Worth about
    // a tenth of a tick at fifty thousand cells on a processor with more than one kind of core,
    // and nothing at all on one without — see `mm_app::threads`.
    mm_app::threads::use_performance_cores();

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Manic Microbes".to_string(),
            ..default()
        }),
        ..default()
    }));
    // After `DefaultPlugins`, not before: the macro writes straight into `Assets<Shader>`, and
    // `AssetPlugin` is what puts that resource in the world. Before it, this is a panic on the
    // first line of `main` with a message about a missing resource rather than about a shader.
    cellpipe::plugin(&mut app);
    app.add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(EguiPlugin::default())
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.03)))
        .insert_resource(SlideRes::new())
        .insert_resource(View::default())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                handle_input,
                screenshot,
                // Ordered so that a frame always shows the tick that has just finished,
                // rather than one caught halfway through being computed.
                collect_simulation,
                redraw,
            )
                .chain(),
        )
        // The interface lives in its own schedule since bevy_egui 0.34, because multi-pass mode
        // may run it more than once a frame — a `Grid` needs the widths of columns it has not
        // laid out yet, and gets them by being asked twice. Drawing egui from `Update` instead
        // panics with "no fonts available until first call to Context::run", which is a true
        // statement about a pass that has not begun and says nothing about fonts.
        //
        // It runs after `Update`, so `panels` still sees the frame `collect_simulation`
        // published, and `view.viewport` is still read a frame later by `handle_input`, which
        // it always was.
        .add_systems(EguiPrimaryContextPass, panels)
        .run();
}

/// The simulation, as a Bevy resource.
///
/// Bevy owns the *box*, not the contents: nothing in this file can reach `World` except through
/// [`Engine`], and the world is not even on this thread.
///
/// [`Self::latest`] is what the per-frame panels read. It is a copy, gathered on the simulation
/// thread, and reading it takes no lock at all — see [`mm_app::engine::Published`] for why that
/// matters and which panels are exempt.
#[derive(Resource)]
struct SlideRes {
    engine: Engine,
    /// The last bundle collected. Held rather than re-fetched so a frame the simulation has not
    /// finished yet redraws the previous one instead of blinking.
    latest: Published,
    /// Chemical names, cached at load. They come from the scenario and never change, and
    /// fetching them each frame would be a lock for a constant.
    chem_names: Vec<String>,
    /// Each chemical's colour, so the budget's bars match the overlay legend.
    chem_colours: Vec<[u8; 3]>,
    /// The cell the inspector is pointed at, if any.
    selected: Option<CellId>,
    /// The genome editor (M6).
    editor: Editor,
    /// Breakpoints over the live world, and the sandbox for instruction stepping.
    breakpoints: Breakpoints,
    sandbox: Option<Sandbox>,
    /// What the last tool did, for the status line.
    last_tool: Option<ToolEvent>,
    /// Where a genome exported from the editor was written.
    last_export: Option<String>,
    /// The selected cell's genome, disassembled once and kept until the genome changes.
    listing: mm_app::inspector::Listing,
    /// Which cell the editor's buffer was loaded from, so "apply to this cell" cannot write a
    /// genome into a cell it was never taken from.
    editing: Option<CellId>,
    /// The ecology pane's data, gathered under one lock and reused for a while (M10.4).
    ///
    /// The archive is far too large to publish with every frame, so this pane is one of the
    /// few that reaches into the world — and since M10.1 reaching in makes the *simulation*
    /// stand aside, so doing it once a frame with the pane left open would tax the world for
    /// the whole time somebody is reading about it. A tree of what has already happened does
    /// not need to be a frame old; a couple of seconds is imperceptible and costs the
    /// simulation nothing measurable.
    ecology: Option<EcologyView>,
    /// The parameter editor's working copy, and the two things it is compared against.
    ///
    /// Edits are applied on a button rather than on a keystroke, because every apply is an
    /// intervention that goes on the record and one per keystroke would be a useless record.
    draft: Option<Draft>,
}

/// The ecology pane's copy of the world's history.
struct EcologyView {
    /// The tick it was gathered at, so it can be refreshed when it is stale enough to matter.
    at: u64,
    /// Which species the page describes, so choosing another refreshes immediately rather
    /// than in two seconds' time.
    showing: Option<mm_core::phylogeny::SpeciesId>,
    tree: mm_app::wiki::Tree,
    timeline: mm_app::wiki::Timeline,
    page: Option<mm_app::wiki::Page>,
    species: usize,
    living: usize,
}

/// How many ticks the ecology pane's data may be behind the world.
///
/// Two seconds at 1x. A chart of what has already happened does not become wrong because a
/// hundred more ticks have passed, and the alternative is holding the world still while
/// somebody reads it.
const ECOLOGY_STALE_AFTER: u64 = 120;

/// The world's environment: what falls on it and what moves through it.
///
/// Held apart from [`BiologyConfig`] because that is what the *living* half runs on and these
/// are `Scenario` fields — the slide's weather rather than its physiology. `World::set_light`
/// and `World::set_current` have existed since M8 and had no caller in `mm-app` at all, so
/// every one of the six light regimes and five current fields was reachable only by hand-
/// writing a `.ron` and running `mm-cli`, which cannot draw a picture.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Environment {
    light: mm_core::light::LightRegime,
    current: mm_core::light::CurrentField,
}

/// The parameter editor's state while it is open.
struct Draft {
    /// What will be applied.
    editing: BiologyConfig,
    /// What the world is running on now, so "changed" can be shown and reverted.
    live: BiologyConfig,
    /// What the scenario says, so a value that has drifted from the file can be marked.
    founding: BiologyConfig,
    /// The environment being edited, and what is in force.
    ///
    /// Drafted rather than applied on each keystroke for a reason that is not only consistency
    /// with the rest of the pane: `set_current` invalidates the whole prescribed velocity
    /// field, so a strength dragged through a slider would rebuild it on every frame of the
    /// drag.
    env: Environment,
    env_live: Environment,
}

impl SlideRes {
    fn new() -> SlideRes {
        // A living slide, not `Scenario::stress`.
        //
        // The viewer used to boot into the stress scenario, which is a physics workload with
        // nothing alive on it — so a microscope built to show cells opened on a slide that had
        // none, and the first thing anyone saw was "0 cells". Whatever else this is, it is
        // supposed to be the thing people want to look at.
        //
        // Loading a *chosen* scenario is M6's slide save/load and is still not here. This is
        // the built-in default: the same petri dish M2's tests use, seeded with the ancestor
        // from `genomes/`, which divides and fills it within a few thousand ticks.
        //
        // `MM_OPEN` takes a different path from the built-in dish, and it has to: `seed_ancestors`
        // *replaces* the world it is handed with a fresh `petri_of`, which is right when the
        // point is a clean slide of a given size and silently discards an authored one — walls,
        // flow, chemistry and all — when it is not. An opened scenario keeps its world and is
        // only given inhabitants, because that is the one thing a `.ron` cannot describe.
        let opened = std::env::var("MM_OPEN").is_ok();
        let mut slide = Slide::new(petri()).expect("opening scenario");
        if opened {
            seed_into(&mut slide, 16);
        } else {
            seed_ancestors(&mut slide, slide_size(), 16);
        }
        let chem_names = slide.chemical_names();
        let chem_colours = slide.chemical_colours();
        let latest = Published {
            frame: slide.frame(),
            selection: None,
            inspection: None,
            species: String::new(),
            history: slide.history().clone(),
            web: slide.food_web(),
            interventions: Vec::new(),
            founding: slide.world().scenario().biology.clone(),
            optics: slide.optics,
        };
        SlideRes {
            engine: Engine::start(slide, Rate::times(1)),
            latest,
            chem_names,
            chem_colours,
            selected: None,
            editor: Editor::new(),
            breakpoints: Breakpoints::new(),
            sandbox: None,
            last_tool: None,
            last_export: None,
            listing: mm_app::inspector::Listing::default(),
            editing: None,
            ecology: None,
            draft: None,
        }
    }

    /// The reading for the cell that is actually selected, if one has arrived yet.
    ///
    /// Not `latest.inspection` directly. That is whatever the simulation thread last published,
    /// and for a frame or two after a click it describes the *previous* selection — so a panel
    /// reading it raw shows the cell you just stopped looking at, and then blinks.
    fn reading(&self) -> Option<&Inspection> {
        if self.latest.selection == self.selected {
            self.latest.inspection.as_ref()
        } else {
            None
        }
    }

    /// Point the inspector at a cell, or at nothing.
    ///
    /// Three things move together and always have to: what the front end thinks is selected,
    /// what the simulation thread is publishing a reading of, and the sandbox — which was a
    /// copy of a *different* cell and would be shown under the new one's name.
    fn select(&mut self, cell: Option<CellId>) {
        self.selected = cell;
        self.engine.select(cell);
        self.sandbox = None;
    }

    /// Wipe the slide and start the ancestor over. Bound to `r`.
    /// Swap the slide for the packing bench. See [`seed_packing`].
    fn bench(&mut self) {
        {
            let held = self.engine.handle();
            seed_packing(&mut held.slide());
        }
        self.selected = None;
        self.engine.select(None);
        self.sandbox = None;
        self.breakpoints.rearm();
    }

    fn reseed(&mut self) {
        self.new_slide(slide_size(), 16);
    }

    /// Throw the slide away and start again at a given size with a given number of founders.
    ///
    /// The size is the reason this exists. It was a constant, then an environment variable, and
    /// neither is any use to someone who wants to watch one cell fill a small dish and then try
    /// it again bigger — which is the experiment the whole thing is for.
    fn new_slide(&mut self, size: u32, founders: u32) {
        let held = self.engine.handle();
        seed_ancestors(&mut held.slide(), size, founders);
        self.selected = None;
        self.engine.select(None);
        self.sandbox = None;
        self.breakpoints.rearm();
    }

    /// An empty slide, stopped, for building a scenario on.
    ///
    /// Not `new_slide` with a founder count of zero. That gives a petri dish — lit, and seeded
    /// with the three chemicals a cell needs — which is a fine place to *run* something and the
    /// wrong place to *author* one: everything you then paint is on top of a uniform wash you
    /// did not ask for and cannot see, and the saved scenario carries it.
    ///
    /// Stopped, because authoring a slide that is running means placing a cell into a current
    /// and watching it leave. The tools all work on a stopped world — that is what the pause
    /// was built to allow.
    fn new_scenario(&mut self, size: u32) {
        let blank = Scenario {
            name: "untitled".to_string(),
            seed: 1,
            width: size,
            height: size,
            light: LightRegime::Uniform {
                intensity: mm_core::Q10_ONE,
            },
            current: CurrentField::Still,
            ..Scenario::default()
        };
        {
            let held = self.engine.handle();
            let mut slide = held.slide();
            match mm_core::World::new(blank) {
                Ok(w) => slide.set_world(w),
                Err(e) => {
                    eprintln!("cannot make a {size}-square slide: {e:?}");
                    return;
                }
            }
        }
        self.engine.set_rate(Rate::Paused);
        self.selected = None;
        self.engine.select(None);
        self.sandbox = None;
        self.breakpoints.rearm();
    }
}

/// A bench for looking at nothing but how cell volumes behave.
///
/// Every question about how a crowd is drawn — do neighbours share a wall, does the packing
/// hold still, does a seam land where the eye expects — is a question about geometry, and on a
/// living slide it is impossible to ask. Cells are dividing, dying, changing size, poisoning
/// each other and swimming, so anything you notice might be the renderer or might be the
/// world, and the two cannot be told apart by looking.
///
/// So this is a slide with the biology switched off. No division, no upkeep, no ageing, no
/// mutation, no metabolism: the cells cannot change and cannot go away. A convergent current
/// holds them pressed into the middle at a steady pressure, which is the one thing the bench
/// does want, and then nothing else happens at all. Whatever moves after that is volumes
/// resolving against each other, and whatever the picture does is the renderer's doing.
///
/// Deliberately reachable from the menu rather than hidden behind a build flag. It is a
/// measuring instrument, and an instrument you have to recompile to pick up is one nobody
/// picks up.
fn seed_packing(slide: &mut Slide) {
    let scenario = Scenario {
        name: "packing bench".to_string(),
        seed: 7,
        // `MM_BENCH_SLIDE=<n>` shrinks it until the pack is pressed against the walls, which is
        // the other thing a live slide has and this bench does not. A colony floating in open
        // water can spread until it is comfortable; one against a boundary cannot, and
        // `step_physics` clamps a cell at the edge — so separation pushes it in and the clamp
        // puts it back, every tick, for as long as the crowd leans on the wall.
        width: bench_slide(),
        height: bench_slide(),
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        // Everything drawn *gently* towards the middle, so the crowd stays a crowd without
        // anything having to want it to. Gently is the operative word: strong enough and the
        // bench stops being a packing and becomes a crush, which shows how the renderer fails
        // rather than how it behaves.
        // Firm rather than gentle. It was turned down when a crush was indistinguishable from a
        // packing, because nothing bounded how far cells could sink into one another and the
        // bench only showed the renderer failing. SPEC §6.4 bounds it now, so leaning hard on
        // the crowd is the interesting case again: a dense pack is the thing being built, and a
        // gentle current only ever produces the loose edge of one.
        current: mm_core::light::CurrentField::Still,
        // `Scenario::gravity` is the better mechanism for this and is deliberately *not* used
        // yet: a single cell falls towards the middle under it correctly (there is a test), but
        // a crowd of 220 evacuates the centre and packs against the walls, which is not
        // understood and is not something to leave a bench standing on. The current has the
        // known flaw described in `Scenario::gravity` — it cannot be damped — but it packs.
        // `MM_BENCH_GRAVITY=0` takes it off, which separates "cells fight the wall" from "cells
        // are being *pushed* into the wall and fight it". A live slide has no gravity.
        gravity: std::env::var("MM_BENCH_GRAVITY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2),
        // No thermal motion. The bench's premise is that whatever moves is volumes resolving
        // against each other, and the default 24 breaks it: every cell wanders about a
        // sixteenth of a square per tick for no reason to do with packing, which in a sheet of
        // cells with sharp shared walls is every boundary in the picture redrawing every frame.
        // Biology was zeroed here when the bench was built; this was missed because it lives in
        // the physics rather than in a rate.
        //
        // `MM_BENCH_JITTER=<n>` puts it back, which is the experiment that separates "the
        // packing maths is wrong" from "the packing maths is fine and cannot keep up with a
        // population that never stops moving". The bench and a live slide of the same density
        // differ in exactly two things — this, and whether biology runs — and the bench already
        // carries the spread of sizes, so size is not one of them.
        jitter: std::env::var("MM_BENCH_JITTER")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        seeding: vec![],
        ..Scenario::default()
    };
    slide.set_world(mm_core::World::new(scenario).expect("packing scenario"));

    // Nothing lives, nothing dies, nothing grows. Every rate that could change a cell is zero,
    // so the population is a constant and the only thing left in motion is geometry.
    //
    // Each piece can be put back one at a time, which is the point: the bench tiles perfectly
    // and a live slide of the same density does not, and the two differ only in this. Turning
    // them on one by one and measuring each says which one it is, rather than which one sounds
    // most likely — and eight plausible answers have already been wrong.
    //
    //   MM_BENCH_GROWTH   cells grow towards their membrane target again
    //   MM_BENCH_DEATH    upkeep and wear, so a cell can starve
    //   MM_BENCH_DIVIDE   the ancestor instead of an inert `HALT`, so cells bud
    let on = |name: &str| std::env::var(name).is_ok();
    let mut biology = BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    };
    if !on("MM_BENCH_DEATH") {
        biology.metabolism.rates.background_damage = 0;
        biology.metabolism.rates.metabolic_floor = 0;
    }
    if !on("MM_BENCH_GROWTH") {
        biology.metabolism.rates.growth_rate = 0;
    }
    biology.ecology.crowding_damage = 0;
    biology.ecology.spike_damage = 0;
    slide.world_mut().set_biology(biology);

    let world = slide.world_mut();
    // One `HALT`. Not the ancestor with its organelles left off — an actual genome that does
    // nothing, so there is no chance of a cell here reaching into the world and no need to
    // wonder whether it did.
    let seed_genome = if on("MM_BENCH_DIVIDE") {
        ancestor_genome().unwrap_or_else(|| vec![mm_core::Op::Halt.canonical_byte()])
    } else {
        vec![mm_core::Op::Halt.canonical_byte()]
    };
    let Ok(inert) = world.genomes().intern(seed_genome) else {
        return;
    };
    // A spread of sizes, because a packing of identical circles is a lattice and tells you
    // nothing about the case that actually looks wrong.
    for k in 0..220u32 {
        let genome = std::sync::Arc::clone(&inert);
        // Seeded already overlapping, and left alone. The bench used to start the cells apart
        // and squeeze them together with a convergent current, which worked but meant the
        // picture was never still: the current adds its drift straight to the position step
        // every tick and the contact solver takes it straight back out, so a jammed crowd
        // oscillates for as long as the current runs — measured at a sixteenth of a square per
        // cell per tick, with `cells.vx` reading exactly zero throughout.
        //
        // With an area-preserving core there is no need for the squeeze. Start them inside one
        // another and the solver's own expansion packs them, then stops. Whatever moves after
        // that really is volumes resolving against each other, which is what the bench was for.
        // Centred on whatever slide it was given, so shrinking the slide presses the same pack
        // against the walls rather than seeding it off the edge.
        let across = 15;
        let span = mm_core::fixed::POS_ONE * 5 / 4;
        let start = (pos(bench_slide() as i32) - (across - 1) * span) / 2;
        let x = start + (k % across as u32) as i32 * span;
        let y = start + (k / across as u32) as i32 * span;
        let size = 18 + (k * 7 % 26) as i32;
        let id = world.spawn_cell(CellSeed {
            x,
            y,
            mass: q10(size),
            // Enough that nothing starves in the time anybody watches.
            energy: q10(1_000_000),
            membrane: 24,
            key: 11,
            badge: 0,
            species: 0,
            parent: CellId::NONE,
            birth_tick: 0,
            genome,
        });
        if let Some(i) = world.cells_mut().index(id) {
            // A membrane and nothing else. No nucleus, so it cannot divide; no chloroplast, so
            // it has nothing to do; no spike, so it cannot touch anybody. A cell here is a
            // volume and that is the whole of it.
            let cells = world.cells_mut();
            cells.slots_mut(i)[0] =
                Organelle::finished(OrganelleType::Membrane, 24 + (k % 5) as u8 * 40);
        }
    }
    world.adopt_current_contents_as_baseline();
}

/// Replace whatever is on the slide with a fresh petri dish and sixteen ancestors.
fn seed_ancestors(slide: &mut Slide, size: u32, founders: u32) {
    slide.set_world(mm_core::World::new(petri_of(size)).expect("default scenario"));
    slide.world_mut().set_biology(BiologyConfig {
        mutation: MutationRates::default(),
        ..BiologyConfig::default()
    });
    seed_into(slide, founders);
}

/// Put founders on whatever world is already on the slide.
///
/// Split out of [`seed_ancestors`] for the scenario library. Placement, the starting loadout and
/// the ledger rebaseline are `World::place_founders` — both front ends kept their own copy of
/// that and the copies had drifted apart. What is left here is the part needing a filesystem and
/// an assembler, and the choice of *who*: the scenario's own inhabitants when it names any, and
/// the ancestor when it does not, because a slide that opens empty is not one anybody wants.
fn seed_into(slide: &mut Slide, founders: u32) -> u32 {
    let wanted = slide.world().scenario().inhabitants.clone();
    if wanted.is_empty() {
        return match ancestor_genome() {
            Some(bytes) => slide.world_mut().place_founders(&bytes, founders),
            None => 0,
        };
    }
    let mut placed = 0;
    for who in &wanted {
        match genome_bytes(&who.genome) {
            Some(bytes) => placed += slide.world_mut().place_founders(&bytes, who.count),
            None => eprintln!("scenario asks for {}, which did not assemble", who.genome),
        }
    }
    placed
}

/// How wide and tall the default slide is, in substrate squares.
///
/// The microscope opened on 96 for its first ten milestones, then on 512 — the slide the fluid
/// and population benchmarks measure, so that the gate and the thing people actually run were
/// the same world. This is a quarter of that area, and the trade is deliberate: the simulation,
/// not the renderer, is the limit at these sizes, and 512 opens slowly enough on an ordinary
/// machine that the first minute of the application is spent watching it rather than using it.
/// The gates still measure 512; what they measure is now a larger slide than the default rather
/// than the default, and that is worth knowing when reading them.
///
/// A population that fills its slide in a thousand ticks has nothing left to do but subdivide,
/// and what is interesting here needs somewhere for a lineage to *go* that is not already
/// occupied: a frontier to spread into, a corner to be isolated in, room for two strategies to
/// be tried at once without immediately meeting. Sixteen founders still grow as sixteen
/// separate colonies here for a good while before their frontiers touch.
///
/// Area is matter as well as room, because seeding is per square — carrying capacity scales
/// with the slide rather than the same crowd thinning out over it.
///
/// See `MM_SLIDE` below for trying another without a rebuild, and the Slide menu for changing it
/// without leaving the application. The slide has been a scenario field all along — `width` and
/// `height` in every `.ron` — and this is only what the app opens on when nobody has said
/// otherwise.
const DEFAULT_SLIDE: u32 = 256;

/// The slide to open on: `MM_OPEN=scenarios/the_drift.ron` for one off the shelf, or a bare
/// dish at `MM_SLIDE` squares.
///
/// `MM_OPEN` exists because the File menu is the only other way in, and a scenario that has to
/// be opened by hand cannot be photographed by `MM_SHOT` or opened twice the same way.
fn petri() -> Scenario {
    match std::env::var("MM_OPEN") {
        Ok(path) => match library::load(std::path::Path::new(&path)) {
            Ok(scenario) => {
                eprintln!("opened {path}: {}", scenario.name);
                return scenario;
            }
            // Complain and open the default rather than refuse to start, the same way a
            // missing `ancestor.mm` does: a typo in a path should not cost the microscope.
            Err(e) => eprintln!("cannot open {path}: {e:?}"),
        },
        Err(_) => {}
    }
    petri_of(slide_size())
}

/// How big the packing bench's slide is. See the note where it is used.
fn bench_slide() -> u32 {
    std::env::var("MM_BENCH_SLIDE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(48)
        .clamp(16, 512)
}

fn slide_size() -> u32 {
    std::env::var("MM_SLIDE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(DEFAULT_SLIDE)
        .clamp(16, 1024)
}

/// The default slide: light, food, no flow. The habitat the ancestor was written for.
///
/// Sized by the caller, because the slide is a thing the user can change now — see the Slide
/// menu. `petri()` is this at whatever [`slide_size`] says.
fn petri_of(size: u32) -> Scenario {
    Scenario {
        name: "petri".to_string(),
        seed: 1,
        width: size,
        height: size,
        light: LightRegime::Uniform {
            intensity: mm_core::Q10_ONE,
        },
        current: CurrentField::Still,
        seeding: vec![
            Seeding::Uniform {
                chemical: 11,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 14,
                per_square: q10(400),
            },
            Seeding::Uniform {
                chemical: 4,
                per_square: q10(400),
            },
        ],
        ..Scenario::default()
    }
}

/// Assemble `genomes/ancestor.mm`, or `None` if it cannot be found or does not assemble.
///
/// Returns rather than panics: a missing genome file should open an empty slide with a
/// complaint on stderr, not refuse to start the microscope.
fn ancestor_genome() -> Option<Vec<u8>> {
    genome_bytes("ancestor.mm")
}

/// Assemble a genome from `genomes/`, or `None` with a complaint on stderr.
fn genome_bytes(name: &str) -> Option<Vec<u8>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../genomes")
        .join(name);
    let src = match std::fs::read_to_string(&path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("cannot read {}: {e}", path.display());
            return None;
        }
    };
    match mm_asm::assemble(&src) {
        Ok(out) => Some(out.bytes),
        Err(e) => {
            eprintln!("{} does not assemble: {e}", path.display());
            None
        }
    }
}

/// Where the camera is looking, and which panels are open. Purely presentational — none of it
/// reaches the world.
#[derive(Resource)]
struct View {
    centre: Vec2,
    zoom: f32,
    /// What the next slide will be, as the Slide menu is currently set. Kept on `View` rather
    /// than in the menu closure because a menu is rebuilt every frame and would forget.
    new_size: u32,
    new_founders: u32,
    paused: bool,
    /// Which panels are showing. One place rather than eight booleans, so the View menu and
    /// the keyboard are generated from the same list and cannot drift apart.
    panels: Panels,
    /// The rectangle the slide is drawn into: whatever egui left over after the rails, the
    /// drawer and the bars. Recorded at the end of `panels` and used by `handle_input` on the
    /// next frame — one frame stale, which is invisible for a rectangle that only changes when
    /// a panel is resized, and much simpler than running the layout twice.
    viewport: Rect,
    /// Who owns the pointer for the duration of the current drag.
    focus: Focus,
    /// Keep the camera centred on the selected cell.
    follow: bool,
    /// Draw cells as shaded, irregular blobs rather than as flat squares (M10.5).
    ///
    /// Presentation only, like everything else in this struct — it changes which tile of a
    /// baked atlas each sprite samples and nothing else. Off is the M2 look, which is still the
    /// right one for a screenshot meant to show data.
    rounded: bool,
    /// Draw the organelles inside each cell.
    ///
    /// On, because they are most of what a cell *is*. Off is for looking at the cells
    /// themselves: at any density a crowd is mostly organelles by area, and the shape of the
    /// crowd — who is squashed against whom, and how hard — is entirely underneath them.
    organelles: bool,
    /// Show the genome with its templates resolved rather than as `%` bits (M10.3b).
    ///
    /// On by default. The `%` form is not a mistake — it is the source, and it is the only
    /// thing that round-trips — but it answers "what bytes are these", and the question the
    /// pane is usually open for is "what does this cell do".
    genome_reading: bool,
    /// Keep the listing scrolled to the instruction pointer.
    genome_follow_ip: bool,
    /// The last `ip` the listing was scrolled to, so it scrolls when the pointer *moves*
    /// rather than on every frame the marker happens to be visible. Scrolling every frame
    /// pins the view to the pointer and there is no way to read the rest of the genome —
    /// worst when paused, where the pointer is not even moving and the scrollbar still will
    /// not stay where it is put.
    genome_scrolled_to: Option<u16>,
    /// Pixels the pointer has moved since the left button went down, so a click can be told
    /// from a drag. Not a preference — transient input state that happens to live here.
    drag_distance: f32,
    /// Which laboratory tool the mouse is holding.
    tool: Tool,
    /// Who owns the right button for the duration of a tool drag, latched like [`Focus`] does
    /// for the left. Separate from `focus` because both buttons can be down at once and a pan
    /// that wandered over a rail must not also cancel the wall being drawn.
    tool_focus: ui::Focus,
    /// How wide a barrier stroke is, in squares. Covers drawing and erasing alike — an eraser
    /// narrower than the wall it is trying to remove is a tool that cannot undo its own work.
    brush: u32,
    /// The last square a barrier stroke painted, or `None` when no stroke is in progress.
    ///
    /// The pointer is sampled once a frame, so a hand moving at any speed skips squares between
    /// samples. Keeping where the stroke was lets the next sample fill the gap — see
    /// [`ui::line_squares`].
    paint_from: Option<(i32, i32)>,
    /// Which chemical the brush and the flux tools are loaded with.
    load: usize,
    /// How much the brush puts in one square per stamp, and what a new source puts in per step.
    dose: i32,
    /// What fraction a new drain takes per step, `Q10`.
    drain_rate: i32,
    /// Where a flux rectangle was started, while the button is still down.
    flux_from: Option<(i32, i32)>,
    /// Which genome the cell-placing tool drops, by file name in `genomes/`.
    place_genome: String,
    /// How many it drops at once.
    place_count: u32,
    /// Which species page is open.
    species: Option<mm_core::phylogeny::SpeciesId>,
    /// Which of the ecology pane's three views is showing (M10.4).
    ecology: Ecology,
    /// Which page of the parameter editor is showing (M10.6).
    ///
    /// On `View` rather than on `Draft` because the draft is dropped whenever the tab is not
    /// the one on show — which is right for the *edits*, and wrong for where you were looking:
    /// coming back to the metabolism page you left is not an edit, it is not losing your place.
    params_page: ParamPage,
    /// Hide species whose peak population never reached this. A long run makes thousands of
    /// them, most one cell that divided twice.
    tree_floor: u32,
    /// Where the timeline's cursor is, in permille, or `None` when nobody has scrubbed.
    scrub: Option<u32>,
    /// The path in the open/save fields. One field for both, because you are almost always
    /// saving next to what you opened.
    file_path: String,
    /// What the last file operation said, kept until the next one so a menu that closed on
    /// the click still gets to report what happened.
    file_note: Option<Result<String, String>>,
}

/// Which page of the parameter editor is showing.
///
/// A rail of pages rather than a stack of collapsing headers. Fifty-one fields under six
/// headers meant that finding one was expand, scan, collapse, expand — and that the column
/// header scrolled away with the group it belonged to, so three screens down the value column
/// was a column of numbers with nothing saying what they were.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ParamPage {
    /// The light falling on the slide and the water moving under it.
    Environment,
    Group(params::Group),
    /// Which reactions this world offers.
    Pathways,
    /// What each organelle costs to build and to keep.
    Catalogue,
}

impl ParamPage {
    /// The rail, in order.
    fn all() -> Vec<ParamPage> {
        let mut out = vec![ParamPage::Environment];
        out.extend(params::Group::ALL.map(ParamPage::Group));
        out.push(ParamPage::Pathways);
        out.push(ParamPage::Catalogue);
        out
    }

    fn title(self) -> &'static str {
        match self {
            ParamPage::Environment => "environment",
            ParamPage::Group(g) => g.title(),
            ParamPage::Pathways => "pathways",
            ParamPage::Catalogue => "catalogue",
        }
    }
}

impl Default for View {
    fn default() -> Self {
        View {
            // What the slide opened on, so "New slide" starts from where you are rather than
            // from a number you have to discover.
            new_size: slide_size(),
            new_founders: 16,
            // The middle of whatever slide the app opened on. It was a constant of 48, which
            // was the middle of a 96-square slide and has been the middle of nothing since.
            centre: Vec2::splat(slide_size() as f32 / 2.0),
            zoom: 1.0,
            paused: false,
            panels: Panels::default(),
            viewport: Rect::default(),
            focus: Focus::default(),
            follow: false,
            rounded: true,
            organelles: true,
            genome_reading: true,
            genome_follow_ip: true,
            genome_scrolled_to: None,
            drag_distance: 0.0,
            tool: Tool::Select,
            tool_focus: ui::Focus::default(),
            brush: ui::BRUSH_DEFAULT,
            paint_from: None,
            // Carbon: the thing a cell builds itself out of, and so the one a slide is most
            // often short of.
            load: 4,
            dose: mm_core::fixed::q10(100),
            drain_rate: mm_core::Q10_ONE / 8,
            flux_from: None,
            place_genome: "ancestor.mm".to_string(),
            place_count: 1,
            species: None,
            ecology: Ecology::Tree,
            // Metabolism, because it is the group with sixteen fields and the one being tuned.
            params_page: ParamPage::Group(params::Group::Metabolism),
            tree_floor: 2,
            scrub: None,
            file_path: String::new(),
            file_note: None,
        }
    }
}

/// What a click does (M6's laboratory tools).
///
/// `Select` is the default and is the only one that cannot change the world — everything else
/// writes, which is why the tool is explicit rather than implied by a modifier key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Select,
    Move,
    Remove,
    DrawBarrier,
    EraseBarrier,
    /// Paint chemistry into the water. Matter from outside the world, through the ledger.
    Paint,
    /// Take it out again. Matter *leaving*, not matter ceasing to exist.
    Unpaint,
    /// Drag a rectangle that supplies chemistry every step.
    Source,
    /// Drag a rectangle that lets it off the slide every step.
    Drain,
    /// Drop founders of a chosen genome where you point.
    PlaceCell,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Move => "move",
            Tool::Remove => "remove",
            Tool::DrawBarrier => "wall",
            Tool::EraseBarrier => "erase",
            Tool::Paint => "paint",
            Tool::Unpaint => "unpaint",
            Tool::Source => "source",
            Tool::Drain => "drain",
            Tool::PlaceCell => "seed",
        }
    }

    /// Whether this tool works by dragging a rectangle rather than by clicking or stroking.
    fn is_rect(self) -> bool {
        matches!(self, Tool::Source | Tool::Drain)
    }
}

#[derive(Component)]
struct MoteSprite(usize);

/// A junction, drawn as a thin sprite stretched between two cells (M7).
#[derive(Component)]
struct JunctionSprite(usize);

/// One arrow of the flow overlay: a shaft stretched along the local velocity.
/// One speck of suspended particulate. Presentation only — see [`art::speck`] for why these
/// are not particles and must never become clickable.
#[derive(Component)]
struct Suspended(usize);

#[derive(Component)]
struct FlowArrow(usize);

/// One edge of a source or drain rectangle, four to a flux.
#[derive(Component)]
struct FluxMark(usize);

/// The entity the whole population is drawn as at [`Lod::Packed`] and above.
///
/// One of a pair. Both exist for the life of the application and exactly one is ever visible —
/// see [`DotMesh`] and the tier switch in [`redraw`]. Two entities rather than one mesh that
/// changes its attributes, because a `Material2d` pipeline is specialised against a vertex layout
/// and swapping the layout under it means discarding and recompiling the pipeline on the frame a
/// person crosses the tier, which is a hitch exactly where they are already moving.
#[derive(Component)]
struct CellMesh;

/// The entity the whole population is drawn as below [`Lod::Packed`], over a quarter of the data.
///
/// See `cellpipe::DotMaterial`. The other half of the pair with [`CellMesh`]; the one that is not
/// being drawn keeps last frame's vertices, hidden, and costs nothing until the tier changes back.
#[derive(Component)]
struct DotMesh;

/// The baked cell atlas, uploaded once (M10.5).
///
/// Held rather than looked up because every cell sprite needs both handles every frame, and an
/// asset lookup per sprite per frame at fifty thousand cells is not a lookup, it is a workload.
#[derive(Resource)]
struct CellArt {
    image: Handle<Image>,
    layout: Handle<TextureAtlasLayout>,
    /// The chemical field and the light, as one texture rewritten each frame (M10.5).
    ///
    /// This replaced a sprite entity per grid square — 262,144 of them at 512×512, each showing
    /// a single texel, every one extracted and prepared by the renderer every frame. It was the
    /// largest cost in the renderer by a wide margin and it bought nothing that a texture does
    /// not.
    field: Handle<Image>,
    /// The barrier mask, as its own texture drawn over the field.
    ///
    /// Separate from `field` because it needs a different sampler, and that is the whole
    /// reason it exists — see [`art::paint_barriers`]. Chemistry is a continuous quantity
    /// sampled on a grid and interpolates faithfully; a wall is blocked or not, and linear
    /// filtering across its edge draws half a wall, which is a thing the world does not have.
    barriers: Handle<Image>,
    /// What the field and barrier textures are currently sized for, so a scenario with a
    /// different grid reallocates rather than painting into the wrong shape.
    field_size: (u32, u32),
    /// The population's vertex buffers, reused every frame so a steady world allocates nothing.
    cells: cellmesh::Buffers,
}

/// The single quad the chemical field is drawn on.
#[derive(Component)]
struct FieldQuad;

/// The quad the barriers are drawn on, over the field and under the junctions and cells.
#[derive(Component)]
struct BarrierQuad;

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cell_materials: ResMut<Assets<cellpipe::CellMaterial>>,
    mut dot_materials: ResMut<Assets<cellpipe::DotMaterial>>,
) {
    // Order -1 so the slide is composited *before* anything egui draws. bevy_egui attaches its
    // context to a camera, and with both at the default order the tie-break is spawn order —
    // which put the interface under the slide and drew cells through the parameter window.
    commands.spawn((
        Camera2d,
        Camera {
            order: -1,
            ..default()
        },
    ));

    // See `mm_app::art` for what is in it and why it is baked rather than shaded per pixel.
    let side = art::TILE as u32;
    let mut image = Image::new(
        Extent3d {
            width: art::atlas_width() as u32,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        art::atlas(),
        // Srgb, because the luminance ramp was baked in the space a person looks at. Linear
        // would render every cell noticeably darker for no reason anybody could name.
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Linear, so a blob drawn forty pixels across from a sixty-four pixel tile has a smooth
    // edge rather than a staircase. The whole point of the exercise is the edge.
    image.sampler = ImageSampler::linear();

    // One texel, until the first frame says how big the grid is.
    let mut field = Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        // Both worlds, and this is not belt and braces. `RENDER_WORLD` alone drops the CPU-side
        // pixels once the texture is uploaded, so `image.data` is `None` from the second frame
        // — and this image is repainted every frame. Getting it wrong cost an afternoon: the
        // field went black, the cells stopped moving and the dust vanished, because the `else`
        // that handled `None` returned out of the whole of `redraw`.
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    // Linear, deliberately. A diffusion field is a continuous quantity sampled on a grid, so
    // interpolating between two measured squares is a more faithful picture of it than hard
    // blocks are — the blockiness was an artefact of how it was drawn, not a property of the
    // data. Nothing is invented: the legend still reports the true peak, and no texel is shown
    // a value that was not measured somewhere adjacent.
    field.sampler = ImageSampler::linear();

    // One mesh for the whole population, rewritten each frame. Spawned empty; `redraw` fills it.
    let cells = cellpipe::empty_mesh();
    commands.spawn((
        CellMesh,
        // The vertices are rewritten every frame in screen space, so the bounding box Bevy
        // computes once from the first frame's positions is wrong by the second. Left to itself
        // the whole population would be frustum-culled the moment the camera moved, which looks
        // exactly like every cell dying at once.
        NoFrustumCulling,
        Mesh2d(meshes.add(cells)),
        MeshMaterial2d(cell_materials.add(cellpipe::CellMaterial {})),
        // Above the chemical field, below the organelles.
        Transform::from_xyz(0.0, 0.0, 1.0),
    ));

    // And the same population below `Lod::Packed`, at a quarter of the vertex data. Spawned here
    // rather than when the tier is first crossed, so that crossing it is a change of `Visibility`
    // and not a pipeline compile — see `DotMesh`. At the same z, because only one is ever visible.
    let dots = cellpipe::dot_mesh();
    commands.spawn((
        DotMesh,
        NoFrustumCulling,
        Mesh2d(meshes.add(dots)),
        MeshMaterial2d(dot_materials.add(cellpipe::DotMaterial {})),
        Transform::from_xyz(0.0, 0.0, 1.0),
    ));

    // The same shape as the field, and deliberately not the same sampler.
    let mut barriers = Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    // **Nearest**, and this is the point of the second texture. A barrier is a binary property
    // of a square, so there is no value between blocked and open for a sampler to find — and
    // linear filtering invents one, which at high magnification smears a one-square wall into a
    // soft band several pixels wider than the square it stands on. See `art::paint_barriers`.
    barriers.sampler = ImageSampler::nearest();

    let field = images.add(field);
    let barriers = images.add(barriers);
    commands.spawn((
        FieldQuad,
        Sprite {
            image: field.clone(),
            color: Color::NONE,
            custom_size: Some(Vec2::splat(1.0)),
            ..default()
        },
    ));
    commands.spawn((
        BarrierQuad,
        Sprite {
            image: barriers.clone(),
            color: Color::NONE,
            custom_size: Some(Vec2::splat(1.0)),
            ..default()
        },
    ));

    commands.insert_resource(CellArt {
        image: images.add(image),
        layout: layouts.add(TextureAtlasLayout::from_grid(
            UVec2::splat(side),
            art::TILES as u32,
            1,
            None,
            None,
        )),
        field,
        barriers,
        field_size: (1, 1),
        cells: cellmesh::Buffers::default(),
    });
}

#[allow(clippy::too_many_arguments)]
fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut view: ResMut<View>,
    mut sim: ResMut<SlideRes>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut contexts: EguiContexts,
) {
    // Who owns the pointer and the keyboard this frame, decided once (M10.1). This used to be
    // a single `is_pointer_over_area` consulted by one of the four things that read the mouse,
    // which is why scrolling the genome listing zoomed the microscope.
    //
    // `wants_pointer_input` and `is_pointer_over_area` are both asked because they answer
    // different questions: the first is "egui is using the pointer right now" — mid-drag on a
    // scrollbar, say — and the second is "the pointer is over something egui drew". A drag
    // that leaves a slider still belongs to the slider, and a pointer resting on a panel
    // belongs to the panel even though nothing is being dragged.
    // Fallible since 0.16: the context belongs to a window entity, and a system can run
    // before there is one. Nothing to route on a frame with no window, so nothing happens.
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let (wants_pointer, wants_keyboard) = (
        ctx.egui_wants_pointer_input() || ctx.is_pointer_over_egui(),
        ctx.egui_wants_keyboard_input(),
    );
    let pointer = window
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .map(|p| (p.x, p.y));
    let live = ui::route(pointer, view.viewport, wants_pointer);

    // The left button latches its owner for the duration of the drag, so a pan that wanders
    // over a rail does not drop the plate halfway through.
    if buttons.just_pressed(MouseButton::Left) {
        view.drag_distance = 0.0;
        view.focus.press(live);
    }
    // The right button latches too, so a wall being dragged towards the edge of the plate is
    // not abandoned the moment the pointer touches a rail. Same rule as the pan, and the same
    // reason: losing the gesture halfway is worse than the thing it protects against.
    if buttons.just_pressed(MouseButton::Right) {
        view.tool_focus.press(live);
    }
    if buttons.just_released(MouseButton::Right) {
        view.tool_focus.release();
        // A stroke ends when the button comes up, so the next one starts a fresh line rather
        // than drawing a wall from wherever the last one happened to stop.
        view.paint_from = None;
    }
    let target = view.focus.resolve(live);

    // Typing in the editor must not toggle panels. `p` opened the metrics rail from inside a
    // source buffer and `2` toggled a chemical overlay, which made the editor unusable for the
    // one thing it is for.
    if !wants_keyboard {
        keyboard(&keys, &mut view, &mut sim);
    }

    handle_mouse(
        &buttons,
        &mut motion,
        &mut wheel,
        &mut view,
        &mut sim,
        &window,
        target,
        live,
        pointer,
    );
}

/// Everything bound to a key, once it is established that the keyboard is ours.
fn keyboard(keys: &ButtonInput<KeyCode>, view: &mut View, sim: &mut SlideRes) {
    if keys.just_pressed(KeyCode::Space) {
        // Resuming goes back to the speed you were watching at rather than to 1×. See
        // `Engine::toggle_pause`: looking closely at something means pausing over and over,
        // and every one of those used to cost you the speed you had chosen.
        view.paused = !sim.engine.toggle_pause().is_running();
    }
    if keys.just_pressed(KeyCode::Period) {
        // Step one tick, whatever the speed. A paused world you cannot advance is a
        // screenshot.
        sim.engine.step();
    }
    // Speed control, including "run as fast as the machine will go" (SPEC §14): the render
    // detaches from the tick rate rather than the tick rate bending to the render.
    //
    // `` ` `` for the slow stop: the ramp `0 - = ⌫` runs along the right of the number row and
    // there is no free key to the left of `0` on it — `1`–`9` are the overlays and taking one
    // would cost a chemical its key. The row's other end is the only unclaimed key on the row,
    // and it is at least the end of it that reads as "less".
    for (key, rate) in [
        (KeyCode::Digit0, Rate::Paused),
        (KeyCode::Backquote, Rate::half()),
        (KeyCode::Minus, Rate::times(1)),
        (KeyCode::Equal, Rate::times(8)),
        (KeyCode::Backspace, Rate::Unlimited),
    ] {
        if keys.just_pressed(key) {
            sim.engine.set_rate(rate);
            view.paused = rate == Rate::Paused;
        }
    }
    // Chemical overlays, individually toggleable.
    for (i, key) in [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ]
    .into_iter()
    .enumerate()
    {
        if keys.just_pressed(key) {
            sim.engine.toggle_overlay(i);
        }
    }
    // Panels, from the one list the View menu is also built from.
    for panel in Panel::ALL {
        if keys.just_pressed(panel_key(panel)) {
            view.panels.toggle(panel);
        }
    }
    if keys.just_pressed(KeyCode::KeyO) {
        sim.engine.set_optics(!sim.engine.optics_enabled());
    }
    if keys.just_pressed(KeyCode::KeyV) {
        sim.engine.set_flow(!sim.engine.flow_enabled());
    }
    // Step one overlay at a time, on its own. The gesture for comparing two chemical fields:
    // same place, same zoom, one then the other, with nothing covering the plate. Keys 1-9
    // toggle and only reach nine of sixteen; these reach all of them and hold the picture to
    // one at a time, which is what "compare" needs.
    for (key, step) in [(KeyCode::BracketRight, 1i32), (KeyCode::BracketLeft, -1)] {
        if keys.just_pressed(key) {
            let n = sim.chem_names.len();
            sim.engine
                .set_overlays(ui::step_solo(sim.engine.overlays(), n, step));
        }
    }
    if keys.just_pressed(KeyCode::KeyC) {
        view.rounded = !view.rounded;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        view.organelles = !view.organelles;
    }
    // `f` was the food web's own panel before M10.4 merged it into the ecology pane. Kept, and
    // now meaning "the ecology pane, on that view", which is what it always meant.
    if keys.just_pressed(KeyCode::KeyF) {
        view.panels.set(Panel::Ecology, true);
        view.ecology = Ecology::Web;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        view.follow = !view.follow;
    }
    if keys.just_pressed(KeyCode::Home) {
        view.centre = Vec2::new(
            sim.latest.frame.width as f32 / 2.0,
            sim.latest.frame.height as f32 / 2.0,
        );
        view.zoom = 1.0;
    }
    for (key, tool) in [
        (KeyCode::F1, Tool::Select),
        (KeyCode::F2, Tool::Move),
        (KeyCode::F3, Tool::Remove),
        (KeyCode::F4, Tool::DrawBarrier),
        (KeyCode::F5, Tool::EraseBarrier),
        (KeyCode::F6, Tool::Paint),
        (KeyCode::F7, Tool::Unpaint),
        (KeyCode::F8, Tool::Source),
        (KeyCode::F9, Tool::Drain),
        (KeyCode::F10, Tool::PlaceCell),
    ] {
        if keys.just_pressed(key) {
            view.tool = tool;
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        // Wipe and start the ancestor over. The nearest thing to a tool the microscope has
        // until M6 brings tweezers and slide loading.
        sim.reseed();
    }
}

/// The Bevy key that toggles each panel.
///
/// Must agree with [`Panel::key`], which is the same binding as the string the menu prints.
/// Two matches rather than one because `KeyCode` is a Bevy type and `ui.rs` is deliberately
/// free of Bevy; `ui`'s own test asserts the letters are at least unique.
fn panel_key(panel: Panel) -> KeyCode {
    match panel {
        Panel::Cell => KeyCode::KeyI,
        Panel::Metrics => KeyCode::KeyP,
        Panel::Legend => KeyCode::KeyL,
        Panel::Genome => KeyCode::KeyG,
        Panel::Ecology => KeyCode::KeyW,
        Panel::Parameters => KeyCode::Comma,
        Panel::Editor => KeyCode::KeyE,
        Panel::Debugger => KeyCode::KeyD,
        Panel::Toolbox => KeyCode::KeyT,
    }
}

/// Everything bound to the mouse, once it is established which region owns it.
#[allow(clippy::too_many_arguments)]
fn handle_mouse(
    buttons: &ButtonInput<MouseButton>,
    motion: &mut MessageReader<MouseMotion>,
    wheel: &mut MessageReader<MouseWheel>,
    view: &mut View,
    sim: &mut SlideRes,
    window: &Query<&Window, With<PrimaryWindow>>,
    target: Target,
    // Who is under the pointer *now*, before the left button's latch is applied. The tool
    // drag has its own latch and needs the unlatched answer to resolve against.
    live_target: Target,
    pointer: Option<(f32, f32)>,
) {
    // The events are drained whoever owns them — an unread `EventReader` accumulates, and a
    // wheel event ignored this frame must not arrive next frame as a jump.
    for ev in wheel.read() {
        if target != Target::Slide {
            continue;
        }
        let before = BASE_SCALE * view.zoom;
        view.zoom = (view.zoom * (1.0 + ev.y * 0.1)).clamp(0.15, 40.0);
        let after = BASE_SCALE * view.zoom;
        // Zoom about the pointer, not about the middle of the viewport: what is under the
        // cursor stays under the cursor, which is the difference between zooming a microscope
        // and operating a slider. Possible for the first time now that the slide has a
        // rectangle of its own to measure the offset from.
        if let Some((px, py)) = pointer {
            let (cx, cy) = view.viewport.centre();
            let moved = ui::zoom_about(
                (view.centre.x, view.centre.y),
                (px - cx, py - cy),
                before,
                after,
            );
            view.centre = Vec2::new(moved.0, moved.1);
        }
    }
    let scale = BASE_SCALE * view.zoom;
    sim.engine.set_zoom(scale);
    // And where it is looking, so the frame builder can skip the expensive per-cell work for
    // cells nobody can see. Half-extents in substrate squares: what fits on screen at this zoom.
    {
        let win = window
            .single()
            .map(|w| (w.width(), w.height()))
            .unwrap_or((1280.0, 720.0));
        sim.engine.set_camera(
            view.centre.x,
            view.centre.y,
            win.0 / (2.0 * scale.max(0.001)),
            win.1 / (2.0 * scale.max(0.001)),
        );
    }

    if buttons.pressed(MouseButton::Left) && target == Target::Slide {
        for ev in motion.read() {
            // Both axes drag the slide with the pointer: push the mouse down and the slide
            // comes down with it, as though you had a finger on the plate. The vertical used
            // to be inverted — the double negative between "screen y is up, slide y is down"
            // and "the camera moves opposite to the content" had been applied once too often,
            // which is exactly the sort of thing that reads as correct on paper and wrong in
            // the hand.
            view.centre -= ev.delta / scale;
            view.drag_distance += ev.delta.length();
        }
    } else {
        motion.clear();
    }

    // Left click selects; left *drag* pans. The two share a button because dragging the plate
    // around is what a hand does to a microscope, and clicking the thing you want to look at
    // is what a hand does to everything else. They are told apart by how far the pointer moved
    // between press and release — under a few pixels was a click, however long it was held.
    //
    // Selection used to be on the right button, where the header table had been claiming the
    // left one since M4. Anybody following the documentation got a pan and concluded there was
    // no inspector.
    if buttons.just_released(MouseButton::Left) {
        if target == Target::Slide && view.drag_distance < 4.0 {
            if let Some((slide_x, slide_y)) = pointer_on_slide(window, view, scale) {
                // Picking a cell is the one thing a click does that has to ask the world, and
                // it happens once per click rather than once per frame.
                let held = sim.engine.handle();
                let hit = held.slide().cell_at(slide_x, slide_y, 3.0);
                if hit.is_some() {
                    sim.select(hit);
                    view.panels.set(Panel::Cell, true);
                }
            }
        }
        // Whoever the drag belonged to, it is over.
        view.focus.release();
    }

    // The barrier tools paint for as long as the button is down, filling the gap between one
    // frame's sample and the next. Everything else fires once on the press.
    //
    // Drawing a wall was one square per click, which meant the dividing wall in
    // `archipelago.ron` — about a hundred and fifty squares — was a hundred and fifty separate
    // right-clicks, each taking the simulation lock and each re-running the ring eviction that
    // pushes the square's chemistry outwards. It was the single reason the tool was unusable.
    let painting = matches!(
        view.tool,
        Tool::DrawBarrier | Tool::EraseBarrier | Tool::Paint | Tool::Unpaint
    );
    let tool_target = view.tool_focus.resolve(live_target);
    if painting && buttons.pressed(MouseButton::Right) && tool_target == Target::Slide {
        if let Some((slide_x, slide_y)) = pointer_on_slide(window, view, scale) {
            let to = (slide_x.floor() as i32, slide_y.floor() as i32);
            // From wherever the stroke was, or from here if it has only just begun.
            let from = view.paint_from.unwrap_or(to);
            if view.paint_from != Some(to) {
                let chemistry = matches!(view.tool, Tool::Paint | Tool::Unpaint);
                let draw = view.tool == Tool::DrawBarrier;
                // The whole stroke segment, brush and all, gathered before the lock is taken:
                // `World::set_barriers` rebuilds the fluid's edge masks once for the batch, and
                // that rebuild walks every square on the slide. Per square it would be a
                // quarter of a million operations each at 512×512, eighty times over for one
                // stamp of a ten-wide brush.
                let mut squares: Vec<(u32, u32)> = Vec::new();
                for centre in ui::line_squares(from, to) {
                    for (x, y) in ui::brush_squares(centre, view.brush) {
                        if x >= 0 && y >= 0 {
                            squares.push((x as u32, y as u32));
                        }
                    }
                }
                if !squares.is_empty() && chemistry {
                    // Through `World::inject`/`extract`, never `substrate_mut().add_chem`:
                    // matter from outside the world has to reach the ledger, and a metabolic
                    // substrate brings its energy with it.
                    let add = view.tool == Tool::Paint;
                    let (mut moved, c, dose) = (0i64, view.load, view.dose);
                    let held = sim.engine.handle();
                    let mut slide = held.slide();
                    let world = slide.world_mut();
                    for (x, y) in &squares {
                        moved += i64::from(if add {
                            world.inject(c, *x as i32, *y as i32, dose)
                        } else {
                            world.extract(c, *x as i32, *y as i32, dose)
                        });
                    }
                    let name = sim
                        .chem_names
                        .get(c)
                        .cloned()
                        .unwrap_or_else(|| c.to_string());
                    sim.last_tool = Some(tools::ToolEvent::Refused(format!(
                        "{} {moved} {name}",
                        if add { "added" } else { "removed" }
                    )));
                } else if !squares.is_empty() {
                    let held = sim.engine.handle();
                    held.slide().world_mut().set_barriers(&squares, draw);
                    sim.last_tool = Some(if draw {
                        tools::ToolEvent::BarrierDrawn {
                            x: to.0.max(0) as u32,
                            y: to.1.max(0) as u32,
                        }
                    } else {
                        tools::ToolEvent::BarrierErased {
                            x: to.0.max(0) as u32,
                            y: to.1.max(0) as u32,
                        }
                    });
                }
                view.paint_from = Some(to);
            }
        }
    }

    // Right-click applies the current tool. `Select` is the default and is the only one that
    // cannot change the world; the rest write, which is why the tool is chosen explicitly.
    if !painting && buttons.just_pressed(MouseButton::Right) && target == Target::Slide {
        if let Some((slide_x, slide_y)) = pointer_on_slide(window, view, scale) {
            let square = (slide_x.floor() as i32, slide_y.floor() as i32);
            let held = sim.engine.handle();
            match view.tool {
                Tool::Select => {
                    let hit = held.slide().cell_at(slide_x, slide_y, 3.0);
                    sim.select(hit);
                    view.panels.set(Panel::Cell, hit.is_some());
                }
                Tool::Move => {
                    if let Some(cell) = sim.selected {
                        let event =
                            tools::relocate(held.slide().world_mut(), cell, square.0, square.1);
                        sim.last_tool = Some(event);
                    }
                }
                Tool::Remove => {
                    let hit = held.slide().cell_at(slide_x, slide_y, 3.0);
                    if let Some(cell) = hit {
                        let event = tools::remove(held.slide().world_mut(), cell);
                        sim.last_tool = Some(event);
                        if sim.selected == Some(cell) {
                            sim.select(None);
                        }
                    }
                }
                Tool::PlaceCell => {
                    // Founders of a named genome, where you point. The genome is a *name*
                    // because that is what a scenario can write down — a cell dropped from a
                    // file the scenario can also name is a cell the saved slide will have.
                    match genome_bytes(&view.place_genome) {
                        Some(bytes) => {
                            let n = view.place_count.max(1);
                            let at = (square.0.max(0) as u32, square.1.max(0) as u32);
                            let mut slide = held.slide();
                            let world = slide.world_mut();
                            let placed = world.place_founders_at(&bytes, n, Some(at));
                            world.note_inhabitant(&view.place_genome, placed, at);
                            sim.last_tool = Some(tools::ToolEvent::Refused(format!(
                                "seeded {placed} × {}",
                                view.place_genome
                            )));
                        }
                        None => {
                            sim.last_tool = Some(tools::ToolEvent::Refused(format!(
                                "{} did not assemble",
                                view.place_genome
                            )))
                        }
                    }
                }
                // Painted above, for as long as the button is held, rather than once here.
                Tool::DrawBarrier | Tool::EraseBarrier | Tool::Paint | Tool::Unpaint => {}
                // Dragged: the press only marks the corner. See below.
                Tool::Source | Tool::Drain => {}
            }
        }
    }

    // The rectangle tools. A source is an *area* rather than a point, so it is dragged: the
    // press marks one corner, the release the other, and nothing is committed until the button
    // comes up — which means a drag that started by accident can be abandoned by dragging back
    // onto its own corner.
    if view.tool.is_rect() && tool_target == Target::Slide {
        if buttons.just_pressed(MouseButton::Right) {
            view.flux_from = pointer_on_slide(window, view, scale)
                .map(|(x, y)| (x.floor() as i32, y.floor() as i32));
        }
        if buttons.just_released(MouseButton::Right) {
            if let (Some(from), Some((sx, sy))) =
                (view.flux_from.take(), pointer_on_slide(window, view, scale))
            {
                let to = (sx.floor() as i32, sy.floor() as i32);
                let (x0, y0) = (from.0.min(to.0).max(0), from.1.min(to.1).max(0));
                let (x1, y1) = (from.0.max(to.0).max(0), from.1.max(to.1).max(0));
                let (w, h) = ((x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32);
                let f = if view.tool == Tool::Source {
                    mm_core::Flux::Source {
                        chemical: view.load,
                        x: x0 as u32,
                        y: y0 as u32,
                        width: w,
                        height: h,
                        per_tick: view.dose,
                    }
                } else {
                    mm_core::Flux::Drain {
                        chemical: view.load,
                        x: x0 as u32,
                        y: y0 as u32,
                        width: w,
                        height: h,
                        rate: view.drain_rate,
                    }
                };
                let held = sim.engine.handle();
                held.slide().world_mut().add_flux(f);
                let name = sim
                    .chem_names
                    .get(view.load)
                    .cloned()
                    .unwrap_or_else(|| view.load.to_string());
                sim.last_tool = Some(tools::ToolEvent::Refused(format!(
                    "{} of {name}, {w}×{h}",
                    view.tool.name()
                )));
            }
        }
    }
}

/// Collect whatever the simulation has finished, and follow the tracked cell.
///
/// Advancing the world is no longer this thread's job (M10.1). This takes the newest bundle if
/// there is one and otherwise leaves the last one in place — a render slower than the
/// simulation misses frames rather than holding it up, and a render faster than the simulation
/// redraws what it has rather than blinking.
fn collect_simulation(mut sim: ResMut<SlideRes>, mut view: ResMut<View>) {
    if let Some(published) = sim.engine.collect() {
        sim.latest = published;
    }

    // Breakpoints act on the viewer, not on the world: when one holds, the world stops being
    // asked to advance. Pausing provably does not change a world (`engine.rs`), so a breakpoint
    // cannot either — and there is no stop-in-the-middle-of-a-tick, because a tick is the
    // simulation's atom.
    //
    // `is_empty` first, and it is not a micro-optimisation. Checking a breakpoint needs the
    // world, and asking for the world makes the simulation thread stand aside for as long as
    // the answer takes. Doing that every frame for a set that is empty in every session where
    // nobody has opened the debugger would tax the simulation for nothing.
    if !sim.breakpoints.is_empty() && sim.breakpoints.tripped().is_none() {
        // The breakpoint set is taken out for the duration so that checking it — which needs
        // `&mut` for the tripped marker — can hold `&World` at the same time. Both live in the
        // same resource; neither can reach the other.
        let mut points = std::mem::take(&mut sim.breakpoints);
        let held = sim.engine.handle();
        let hit = points.check(held.slide().world());
        sim.breakpoints = points;
        if hit {
            sim.engine.set_rate(Rate::Paused);
            view.paused = true;
        }
    }

    // The decision itself is in `inspector::tracking`, where it can be tested without a
    // graphics stack. This applies it and nothing more.
    let track = mm_app::inspector::tracking(
        sim.latest.inspection.as_ref(),
        view.follow,
        sim.selected,
        sim.latest.selection,
    );
    match track {
        // Set rather than eased: the camera has to be exactly on the cell or a fast one
        // outruns the lerp and drifts to the edge of the view, and a microscope stage does not
        // have momentum anyway.
        mm_app::inspector::Track::MoveTo(x, y) => view.centre = Vec2::new(x, y),
        mm_app::inspector::Track::Stay => {}
        mm_app::inspector::Track::Lost => {
            sim.select(None);
            view.follow = false;
        }
    }
}

/// How much of its own colour a cell at the far end of the depth of field gives up to the slide.
///
/// The one knob for how strongly depth reads. At 1 a distant cell would be indistinguishable
/// from the background; the gap below 1 is what keeps it a visible cell.
const HAZE_MAX: f32 = 0.75;

/// How far into the slide a cell at `depth` has faded, `0..=1`.
///
/// One function rather than the arithmetic twice, because a cell and its organelles have to
/// agree. They did not before — organelles took the vignette and no depth of field at all — and
/// the result was a distant cell drawn as a dark body with a crisp bright nucleus in it.
///
/// A selected cell never hazes. You went looking for it, and the microscope's job at that point
/// is to show it to you rather than to be tasteful about where it is sitting.
fn haze_of(optics: &mm_app::optics::Optics, depth: f32, selected: bool) -> f32 {
    if selected {
        return 0.0;
    }
    HAZE_MAX * (optics.blur(depth) / optics.max_blur.max(f32::EPSILON)).clamp(0.0, 1.0)
}

/// A colour `t` of the way towards the slide behind it.
///
/// `t` of 0 is the thing itself and 1 is the bare slide. Capped well below 1 by the caller, so
/// even the most defocused cell keeps a trace of its own colour — a cell that faded *completely*
/// into the background would be a cell that vanished, and the population count would stop
/// matching what you can see.
fn haze_into(rgb: [f32; 3], slide: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        rgb[0] + (slide[0] - rgb[0]) * t,
        rgb[1] + (slide[1] - rgb[1]) * t,
        rgb[2] + (slide[2] - rgb[2]) * t,
    ]
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn redraw(
    mut commands: Commands,
    sim: Res<SlideRes>,
    view: Res<View>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut art_handles: ResMut<CellArt>,
    mut field_quad: Query<
        (&mut Sprite, &mut Transform),
        (
            With<FieldQuad>,
            Without<BarrierQuad>,
            Without<MoteSprite>,
            Without<JunctionSprite>,
            Without<FlowArrow>,
            Without<Suspended>,
            Without<FluxMark>,
        ),
    >,
    mut barrier_quad: Query<
        (&mut Sprite, &mut Transform),
        (
            With<BarrierQuad>,
            Without<FieldQuad>,
            Without<MoteSprite>,
            Without<JunctionSprite>,
            Without<FlowArrow>,
            Without<Suspended>,
            Without<FluxMark>,
        ),
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cell_mesh: Query<(&Mesh2d, &mut Visibility), (With<CellMesh>, Without<DotMesh>)>,
    mut dot_mesh: Query<(&Mesh2d, &mut Visibility), (With<DotMesh>, Without<CellMesh>)>,
    mut motes: Query<
        (&MoteSprite, &mut Sprite, &mut Transform),
        (
            Without<FieldQuad>,
            Without<BarrierQuad>,
            Without<JunctionSprite>,
            Without<FlowArrow>,
            Without<Suspended>,
            Without<FluxMark>,
        ),
    >,
    mut junctions: Query<
        (&JunctionSprite, &mut Sprite, &mut Transform),
        (
            Without<FieldQuad>,
            Without<BarrierQuad>,
            Without<MoteSprite>,
            Without<FlowArrow>,
            Without<Suspended>,
            Without<FluxMark>,
        ),
    >,
    mut arrows: Query<
        (&FlowArrow, &mut Sprite, &mut Transform),
        (
            Without<FieldQuad>,
            Without<BarrierQuad>,
            Without<MoteSprite>,
            Without<JunctionSprite>,
            Without<Suspended>,
            Without<FluxMark>,
        ),
    >,
    mut specks: Query<
        (&Suspended, &mut Sprite, &mut Transform),
        (
            Without<FieldQuad>,
            Without<BarrierQuad>,
            Without<MoteSprite>,
            Without<JunctionSprite>,
            Without<FlowArrow>,
            Without<FluxMark>,
        ),
    >,
    mut flux_marks: Query<
        (&FluxMark, &mut Sprite, &mut Transform),
        (
            Without<FieldQuad>,
            Without<BarrierQuad>,
            Without<MoteSprite>,
            Without<JunctionSprite>,
            Without<FlowArrow>,
            Without<Suspended>,
        ),
    >,
) {
    let frame = &sim.latest.frame;
    let optics = &sim.latest.optics;
    let scale = BASE_SCALE * view.zoom;
    let Ok(window) = window.single() else {
        return;
    };
    let size = window.size();

    // The slide is centred on the viewport, not on the window. With a rail open those are not
    // the same place, and without this the plate sits visibly off to one side and zooming about
    // the pointer walks it further off with every scroll.
    //
    // Bevy's world origin is the middle of the window with y up; the viewport rectangle is in
    // cursor pixels with y down. This is the one conversion between them.
    let viewport = if view.viewport.is_empty() {
        Rect::new(0.0, 0.0, size.x, size.y)
    } else {
        view.viewport
    };
    let (vx, vy) = viewport.centre();
    let origin = Vec2::new(vx - size.x / 2.0, size.y / 2.0 - vy);
    // Half-diagonal of the *viewport*, for the field radius the vignette and the aberration are
    // measured in — so the circular field is centred on the slide rather than on the window.
    let half_diagonal = (viewport.width().powi(2) + viewport.height().powi(2)).sqrt() / 2.0;
    // What is actually on screen, in the same space `to_screen` produces, with a margin.
    //
    // Everything not inside this used to be built into the mesh anyway. At whole-slide zoom that
    // is harmless — every cell is on screen. Zoomed in on a full slide it is most of the work in
    // the frame: at 420x on seventy-two thousand cells you can see perhaps five hundred of them,
    // and the renderer was uploading a quad for every one of the rest, plus a quad per organelle
    // inside each, which is well over a million vertices of geometry that lands nowhere.
    //
    // The margin is generous on purpose. A cell is culled on its centre, so it has to cover the
    // largest a cell is ever *drawn* or one whose middle is just off screen would lose the part
    // of it that is not.
    let cull = {
        let m = (BASE_SCALE * view.zoom * slide::PACKING * 4.0).max(64.0);
        Rect::new(
            origin.x - size.x / 2.0 - m,
            origin.y - size.y / 2.0 - m,
            origin.x + size.x / 2.0 + m,
            origin.y + size.y / 2.0 + m,
        )
    };

    let to_screen = |x: f32, y: f32| -> Vec3 {
        Vec3::new(
            (x - view.centre.x) * scale + origin.x,
            // Screen y is up, slide y is down.
            -(y - view.centre.y) * scale + origin.y,
            0.0,
        )
    };
    // How far off the centre of the field a point is, as a fraction of the half-diagonal.
    let field_radius = |p: Vec3| -> f32 {
        ((p.truncate() - origin).length() / half_diagonal.max(1.0)).clamp(0.0, 1.0)
    };

    let plane = frame.width as usize * frame.height as usize;

    // What an out-of-focus cell fades into, filled in by `paint_field` below. The fallback is
    // the clear colour, which is genuinely what is behind the cells when there is no field to
    // paint.
    let mut haze_rgb = [0.02f32, 0.02, 0.03];

    // The chemical fields and the light, as one texture on one quad (M10.5).
    //
    // This was a sprite entity per grid square: 262,144 of them at 512×512, each showing a
    // single texel, each with a `Transform` the renderer extracted and prepared every frame. It
    // was the largest single cost in the renderer. The arithmetic per square is unchanged and
    // now lives in `art::paint_field`, where it is tested; what has gone is the entities.
    if plane > 0 {
        let size = (frame.width.max(1), frame.height.max(1));
        if art_handles.field_size != size {
            // A scenario with a different grid. Reallocated rather than painted into the wrong
            // shape, which would smear the world diagonally and look like a physics bug.
            for handle in [art_handles.field.clone(), art_handles.barriers.clone()] {
                if let Some(mut image) = images.get_mut(&handle) {
                    image.resize(Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    });
                }
            }
            art_handles.field_size = size;
        }
        if let Some(mut image) = images.get_mut(&art_handles.field) {
            let layers: Vec<(&[f32], [f32; 3])> = frame
                .overlays
                .iter()
                .map(|l| (l.field.as_slice(), l.rgb))
                .collect();
            // `if let`, not `let else`: a field with no pixels is a field that cannot be
            // painted, not a reason to stop drawing the cells.
            if let Some(pixels) = image.data.as_mut() {
                haze_rgb = art::paint_field(
                    pixels,
                    frame.width as usize,
                    frame.height as usize,
                    &frame.light,
                    &layers,
                    // The vignette, which used to be applied per sprite and now has to be painted
                    // in. Asked per square, in square coordinates, because where a square lands on
                    // screen is the camera's business and not the painter's.
                    &|x, y| optics.vignette(field_radius(to_screen(x, y))),
                );
            }
        }
        // The walls, into their own nearest-sampled texture. Skipped entirely on a slide with
        // no barriers, where the mask is empty and the quad is switched off below.
        if !frame.barriers.is_empty() {
            if let Some(mut image) = images.get_mut(&art_handles.barriers) {
                if let Some(pixels) = image.data.as_mut() {
                    art::paint_barriers(
                        pixels,
                        frame.width as usize,
                        frame.height as usize,
                        &frame.barriers,
                        &|x, y| optics.vignette(field_radius(to_screen(x, y))),
                    );
                }
            }
        }
    }
    for (mut sprite, mut transform) in &mut field_quad {
        // Stretched over the whole grid, so a texel covers exactly the square it describes.
        let a = to_screen(0.0, 0.0);
        let b = to_screen(frame.width as f32, frame.height as f32);
        sprite.color = if plane > 0 { Color::WHITE } else { Color::NONE };
        sprite.custom_size = Some(Vec2::new((b.x - a.x).abs(), (a.y - b.y).abs()));
        transform.translation = ((a + b) / 2.0).with_z(0.0);
    }
    for (mut sprite, mut transform) in &mut barrier_quad {
        let a = to_screen(0.0, 0.0);
        let b = to_screen(frame.width as f32, frame.height as f32);
        // Switched off rather than drawn transparent when there is nothing to draw, so a slide
        // with no barriers costs no blend and no sample.
        sprite.color = if frame.barriers.is_empty() {
            Color::NONE
        } else {
            Color::WHITE
        };
        sprite.custom_size = Some(Vec2::new((b.x - a.x).abs(), (a.y - b.y).abs()));
        // Above the field, below the junctions at 0.5 and the cells above them: a wall is part
        // of the slide, and everything alive sits on top of it.
        transform.translation = ((a + b) / 2.0).with_z(0.25);
    }

    // The flow overlay: an arrow per lattice point, over the field and under everything alive.
    //
    // Drawn on a *screen*-spaced lattice rather than a substrate-spaced one, so the field reads
    // the same at every zoom: `Frame::flow` is sampled every `FLOW_STRIDE` squares and this
    // takes every nth of those, choosing n so the arrows land roughly `ARROW_PITCH` pixels
    // apart. Without that, whole-slide zoom is a solid hedge of arrows and full magnification
    // is one arrow somewhere off screen.
    // Rebuilt each frame; a few hundred entries at most, because the lattice is chosen in
    // screen space and the window is only so big.
    let mut arrow_slots: Vec<(Vec3, [f32; 2], f32)> = Vec::new();
    //
    // Gated on `flow_shown` and not merely on the field being there: the field is also gathered
    // for the particulate, which needs the velocity to drift the specks along, so on any slide
    // with detritus on it `frame.flow` is populated whether or not the overlay was asked for.
    // Drawing off the field alone had the arrows permanently on and the menu item inert.
    let arrow_count = if !frame.flow_shown || frame.flow.is_empty() || frame.flow_cols == 0 {
        0
    } else {
        let cols = frame.flow_cols as usize;
        let rows = frame.flow.len() / cols.max(1);
        // Squares per arrow, then how many of the sampled lattice that skips.
        let square_px = scale.max(0.0001);
        let skip = ((ARROW_PITCH / (square_px * slide::FLOW_STRIDE as f32)).ceil() as usize).max(1);
        let mut n = 0usize;
        for row in (0..rows).step_by(skip) {
            for col in (0..cols).step_by(skip) {
                let Some(v) = frame.flow.get(row * cols + col) else {
                    continue;
                };
                let speed = (v[0] * v[0] + v[1] * v[1]).sqrt();
                // Still water gets no arrow at all, which is what makes a channel legible:
                // the arrows *are* where the water is going, so an empty region means still.
                if speed < FLOW_FLOOR {
                    continue;
                }
                let mid = slide::FLOW_STRIDE as f32 / 2.0;
                let at = to_screen(
                    col as f32 * slide::FLOW_STRIDE as f32 + mid,
                    row as f32 * slide::FLOW_STRIDE as f32 + mid,
                );
                if at.x < cull.min_x || at.x > cull.max_x || at.y < cull.min_y || at.y > cull.max_y
                {
                    continue;
                }
                arrow_slots.push((at, *v, speed));
                n += 1;
            }
        }
        n
    };
    // Two sprites per arrow — a shaft and a head — from one pool, even indices being shafts.
    // One pool rather than two so the whole overlay is a single marker component and a single
    // query; two would mean another `Without` on every other sprite query in this system.
    for i in arrows.iter().count()..arrow_count * 2 {
        commands.spawn((
            FlowArrow(i),
            Sprite {
                color: Color::NONE,
                custom_size: Some(Vec2::splat(1.0)),
                ..default()
            },
        ));
    }
    for (marker, mut sprite, mut transform) in &mut arrows {
        let head = marker.0 % 2 == 1;
        let Some((at, v, speed)) = arrow_slots.get(marker.0 / 2).copied() else {
            // Surplus from a frame that had more arrows than this one. Hidden rather than
            // despawned, the way the junction pool does it: a pool that shrinks and regrows
            // costs a spawn every time the camera moves.
            sprite.color = Color::NONE;
            continue;
        };
        let share = (speed / FLOW_FULL).clamp(0.0, 1.0);
        let length = ARROW_PITCH * (ARROW_SHORT + (ARROW_LONG - ARROW_SHORT) * share);
        let dim = optics.vignette(field_radius(at));
        // Cool, and pale enough to read over both the mauve field and the cells without
        // competing with them: an overlay that shouts is one you turn off and never turn on.
        let bright = (0.45 + 0.55 * share) * dim;
        sprite.color = Color::srgba(0.60 * bright, 0.84 * bright, 1.0 * bright, 0.85);
        // Anchored at the sample point and extending *downstream*, rather than centred on it,
        // so which way the water is going is readable from a still picture. A lattice of
        // centred dashes is a texture; offset ones with a head are a direction.
        let dir = Vec2::new(v[0], -v[1]).normalize_or_zero();
        let along = if head { length } else { length / 2.0 };
        if head {
            sprite.custom_size = Some(Vec2::splat(3.5));
        } else {
            sprite.custom_size = Some(Vec2::new(length, 1.5));
        }
        transform.translation = (at + (dir * along).extend(0.0)).with_z(0.3);
        transform.rotation = Quat::from_rotation_z(dir.y.atan2(dir.x));
    }

    // The particulate itself: a stipple of specks carried by the water, over the field and
    // under everything solid.
    //
    // Not an overlay and not switchable, because it is not an instrument reading. The detritus
    // is *there*, the way cells are there, and a slide with particulate flowing through it that
    // looked empty would be lying about what is on it. The switchable thing is the chemical
    // overlay that washes each square in colour — that one is a reading, and it answers "how
    // much", which is a question a stipple should not be asked.
    let mut speck_slots: Vec<(Vec3, f32)> = Vec::new();
    if !frame.detritus.is_empty() && frame.flow_cols > 0 {
        let cols = frame.flow_cols as usize;
        let rows = frame.detritus.len() / cols.max(1);
        let stride = slide::FLOW_STRIDE as f32;
        let square_px = scale.max(0.0001);
        // The arrows' trick: a substrate lattice thinned in screen space, so the stipple stays
        // about as dense at every magnification. Substrate-spaced rather than screen-spaced so
        // that a speck keeps its place on the slide when the view is panned across it.
        //
        // Thinning alone is not enough, and the first version of this had only that. `skip` can
        // make the lattice coarser and never finer, so the lattice itself is a *floor* on the
        // density: at high magnification a block is wider than the pitch and one speck in it is
        // all there is, which had the stipple thinning out as the view zoomed in — precisely
        // backwards, since magnifying should reveal more of the water and not less of it. So
        // the two halves are here: `skip` drops blocks when they crowd, `per_side` fills them
        // when they spread. Only one of the pair is ever above 1.
        let block_px = square_px * stride;
        let skip = ((SPECK_PITCH / block_px).ceil() as usize).max(1);
        let per_side = ((block_px / SPECK_PITCH).round() as u64).clamp(1, SPECK_MAX_SIDE);
        let mid = stride / 2.0;
        for row in (0..rows).step_by(skip) {
            for col in (0..cols).step_by(skip) {
                let i = row * cols + col;
                let conc = frame.detritus.get(i).copied().unwrap_or(0.0);
                // The water's velocity, geared down to the speed the particulate in it
                // actually travels: detritus lags the current, and specks drawn at the water's
                // speed would say it does not.
                let water = frame.flow.get(i).copied().unwrap_or([0.0, 0.0]);
                let vel = [
                    water[0] * frame.detritus_drift,
                    water[1] * frame.detritus_drift,
                ];
                for k in 0..per_side * per_side {
                    // Distinct per speck *and* stable as `per_side` changes, so that zooming in
                    // adds specks to the ones already on screen rather than dealing a new hand.
                    let index = i as u64 * SPECK_MAX_SIDE * SPECK_MAX_SIDE + k;
                    let Some(sp) = art::speck(index, frame.tick, vel, stride, conc) else {
                        continue;
                    };
                    let at = to_screen(
                        col as f32 * stride + mid + sp.dx,
                        row as f32 * stride + mid + sp.dy,
                    );
                    if at.x < cull.min_x
                        || at.x > cull.max_x
                        || at.y < cull.min_y
                        || at.y > cull.max_y
                    {
                        continue;
                    }
                    speck_slots.push((at, sp.alpha * optics.vignette(field_radius(at))));
                }
            }
        }
    }
    for i in specks.iter().count()..speck_slots.len() {
        commands.spawn((
            Suspended(i),
            Sprite {
                color: Color::NONE,
                custom_size: Some(Vec2::splat(1.0)),
                ..default()
            },
        ));
    }
    // Grains grow with magnification, because unlike an arrow a speck is meant to read as
    // something in the water rather than as a mark on the picture of it.
    let speck_px = (scale * 0.5).clamp(1.5, 5.0);
    for (marker, mut sprite, mut transform) in &mut specks {
        let Some((at, alpha)) = speck_slots.get(marker.0).copied() else {
            // Surplus from a busier frame, hidden rather than despawned, like the arrows.
            sprite.color = Color::NONE;
            continue;
        };
        sprite.color = Color::srgba(SPECK_RGB[0], SPECK_RGB[1], SPECK_RGB[2], alpha * 0.85);
        sprite.custom_size = Some(Vec2::splat(speck_px));
        // Above the field and below the walls at 0.25: the particulate is in the water, and
        // the water is under everything solid.
        transform.translation = at.with_z(0.2);
    }

    // Sources and drains, outlined where they are.
    //
    // An outline rather than a wash, and the distinction is the whole point: a filled rectangle
    // in the chemical's colour is indistinguishable from the chemical overlay reading high
    // there, which is exactly the confusion to avoid — a source is a *cause* and the overlay is
    // the *effect*, and a source that has not run yet must still be visible.
    //
    // Four edge sprites each, so the water inside is unobscured.
    let mut edges: Vec<(Vec3, Vec2, [f32; 3], f32)> = Vec::new();
    for f in &frame.flux {
        let (c, x, y, w, h, source) = match f {
            mm_core::Flux::Source {
                chemical,
                x,
                y,
                width,
                height,
                ..
            } => (*chemical, *x, *y, *width, *height, true),
            mm_core::Flux::Drain {
                chemical,
                x,
                y,
                width,
                height,
                ..
            } => (*chemical, *x, *y, *width, *height, false),
        };
        let a = to_screen(x as f32, y as f32);
        let b = to_screen((x + w) as f32, (y + h) as f32);
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        if x1 < cull.min_x || x0 > cull.max_x || y1 < cull.min_y || y0 > cull.max_y {
            continue;
        }
        let rgb = sim.chem_colours.get(c).copied().unwrap_or([200, 200, 200]);
        // A source is drawn in its chemical's own colour; a drain in the same hue held back, so
        // the pair reads as "this one gives, this one takes" without needing a legend.
        let dim = if source { 1.0 } else { 0.45 };
        let tint = [
            rgb[0] as f32 / 255.0 * dim,
            rgb[1] as f32 / 255.0 * dim,
            rgb[2] as f32 / 255.0 * dim,
        ];
        let thick = if source { 2.0 } else { 1.5 };
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (width, height) = (x1 - x0, y1 - y0);
        for (at, size) in [
            (Vec3::new(cx, y0, 0.0), Vec2::new(width, thick)),
            (Vec3::new(cx, y1, 0.0), Vec2::new(width, thick)),
            (Vec3::new(x0, cy, 0.0), Vec2::new(thick, height)),
            (Vec3::new(x1, cy, 0.0), Vec2::new(thick, height)),
        ] {
            edges.push((at, size, tint, if source { 0.9 } else { 0.7 }));
        }
    }
    for i in flux_marks.iter().count()..edges.len() {
        commands.spawn((
            FluxMark(i),
            Sprite {
                color: Color::NONE,
                custom_size: Some(Vec2::splat(1.0)),
                ..default()
            },
        ));
    }
    for (marker, mut sprite, mut transform) in &mut flux_marks {
        let Some((at, size, rgb, alpha)) = edges.get(marker.0).copied() else {
            sprite.color = Color::NONE;
            continue;
        };
        sprite.color = Color::srgba(rgb[0], rgb[1], rgb[2], alpha);
        sprite.custom_size = Some(size);
        // Above the walls, because a source drawn over a barrier is a source you can see is
        // pointed at one.
        transform.translation = at.with_z(0.28);
    }

    // Cells: the whole population as one mesh, one material, one draw call (M10.5).
    //
    // This was a sprite entity per cell — fifty thousand `Transform`s and `Sprite`s at the
    // target scale, extracted and prepared every frame to draw quads that differ only in where
    // they are and what colour they are. What it buys beyond the entities is the shape: the
    // fragment shader evaluates a signed-distance field per pixel per cell, so every cell has
    // its own outline, it stays crisp at any magnification, and a failing membrane roughens it.
    // The baked atlas could do none of that; it is still what organelles and dust wear.
    //
    // Below `Lod::Packed` this goes through the narrow layout instead — a quarter of the vertex
    // data and a fragment shader with the twelve seams taken out of it, for a picture that is the
    // same one. See `cellpipe::DotMaterial`, and the note above `DotVertex` in `cell.wgsl` for
    // why "the same" is arithmetic rather than judgement.
    let detail = if frame.lod.resolves_packing() {
        cellmesh::Detail::Seamed
    } else {
        cellmesh::Detail::Plain
    };
    cellmesh::build(&mut art_handles.cells, &frame.cells, detail, |dot| {
        let at = to_screen(dot.x, dot.y);
        // Off screen: no quad, no organelles, nothing uploaded.
        if at.x < cull.min_x || at.x > cull.max_x || at.y < cull.min_y || at.y > cull.max_y {
            return None;
        }
        let dim = optics.vignette(field_radius(at));
        // Depth of field. The size-and-alpha approximation is still here, but the *edge* is
        // genuinely softer now — `softness` widens the smoothstep in the shader, which is the
        // one part of the microscope look a texture could not fake.
        let blur = optics.blur(dot.depth);
        let selected = sim.selected == Some(dot.id);
        // Defocus fades a cell *into the slide*. It used to multiply the colour by as little as
        // 0.25, and that is not what being out of focus does to something — an object off the
        // focal plane loses contrast against the field it sits in, it does not lose brightness.
        //
        // Multiplying invented a population. `cell_colour` already spans about 0.30 to 0.50
        // between a bare cell and one carrying a full loadout, so a four-fold darkening on top
        // of it put genetically identical sisters seven times apart in brightness purely on a
        // hash of their ids — and a near-black cell in a crowd of pale ones does not read as
        // "further away", it reads as a different kind of cell. Which is the one thing the
        // picture must not say, because what a cell is made of is the only thing colour here is
        // allowed to mean.
        //
        // Fading cannot say that: the far end of the ramp is the slide itself, so a defocused
        // cell is always somewhere between its own colour and the background it is dissolving
        // into, and never a colour no cell could have.
        let haze = haze_of(optics, dot.depth, selected);
        let [r, g, b] = haze_into(dot.rgb, haze_rgb, haze);
        // The vignette still multiplies, because that one *is* a brightness: less light reaches
        // the edge of the field.
        let tint = if selected { 1.0 } else { dim };
        // Newly divided cells swell into place over their first few ticks rather than
        // appearing at full size.
        //
        // A daughter arrives with its adult radius already — mass is conserved and split at
        // the moment of division, so there is no growing-up for the simulation to represent —
        // and popping into existence at full width is the single most jarring thing a crowd
        // does. It also shoves its neighbours' seams sideways in one frame, which reads as the
        // whole neighbourhood flinching.
        //
        // Presentation only, and deliberately short: eight ticks, about an eighth of a second
        // at 1x. Long enough to be a movement rather than a jump, short enough that a cell is
        // never drawn much smaller than it really is — which would be lying about how crowded
        // the slide is.
        let newborn = (dot.age as f32 / 8.0).clamp(0.0, 1.0);
        // Eased, so it arrives rather than stops.
        let swell = 0.35 + 0.65 * newborn * (2.0 - newborn);
        // The cell at the size the simulation says, and *not* inflated by the blur.
        //
        // Inflating it was inherited from the sprite era, where a defocused cell was drawn
        // bigger and fainter because a texture cannot be blurred. The shader blurs the outline
        // properly now, so the only thing the inflation still did was leave a small blob inside
        // a large quad with the fade filling the gap — which at whole-slide zoom drew a square
        // halo round every cell and made the population look like squares.
        //
        // A selected cell gets a floor big enough to find in a crowd of six hundred, which is
        // the crowd you are in when you have lost it.
        // Drawn a fifth larger than the simulation's radius, and cut back by the seams where
        // a neighbour is in the way — see `slide::PACKING`. Cells rest at exactly touching,
        // and touching circles leave a hole between every three of them.
        let body = (dot.radius * 2.0 * scale * slide::PACKING * dot.area_swell * swell)
            .max(if selected { 12.0 } else { 1.5 });
        Some(cellmesh::Placed {
            x: at.x,
            y: at.y,
            // The field fills part of its quad — the margin is what the outline wobbles into
            // and what the fade needs — so the quad is grossed up to make the *body* the size
            // asked for.
            half: body / (2.0 * cellmesh::FIELD_FILL),
            // Opaque. A cell out of focus was being drawn at a quarter alpha *as well as*
            // dimmed by `tint`, so depth of field was charged twice and the second charge cost
            // the picture its solidity: in a clump you saw through the front cell into the one
            // behind, and a mass of cells read as one pane of stained glass.
            //
            // Nothing is lost by it. Defocus is still carried by `haze`, which fades the cell
            // into the slide, and by `softness`, which genuinely blurs the outline — and blur is
            // the part a sprite could never do and the reason the shader exists. What
            // transparency added was the ability to see through a solid object.
            //
            // Fading is also why this can stay opaque at all: it reaches the same place
            // transparency would — the cell approaching the colour behind it — without ever
            // letting you see the cell that is genuinely behind this one.
            //
            // Safe because the seams partition the overlap exactly: two cells pressed together
            // cut each other along the same plane from either side, so opaque bodies tile with
            // no region drawn twice.
            rgba: [r * tint, g * tint, b * tint, 1.0],
            shape: cellmesh::Shape {
                seed: cellmesh::seed_of(dot.id.ordering_key()),
                // In the field's own units, where 1 is the cell's radius, so a defocused cell
                // is soft by the same *fraction* however big it is drawn. Bounded well below
                // the radius: past about a quarter the fade reaches the quad's corners and the
                // cell stops being a cell.
                softness: (blur / body.max(1.0)).clamp(0.0, 0.25),
                integrity: 1.0,
                rounded: if view.rounded { 1.0 } else { 0.0 },
            },
            squash: squash_of(dot),
            // `body` above is already the swollen size. This is how much of it is swell, so the
            // shader can hand it back along the shared walls — see the taper in `cell.wgsl`.
            // The newborn swell is deliberately not included: that one is a cell arriving, and
            // it should shrink the whole outline, seams and all.
            swell: dot.area_swell,
        })
    });
    // Organelles, into the same buffers and therefore the same draw call, at the tiers that
    // resolve them. They were sprites wearing the baked atlas, which at high magnification made
    // them the one soft thing in a sharp picture — the tile is 64 pixels and a cell at 1400×
    // is not.
    if frame.lod.resolves_organelles() && view.organelles {
        for dot in &frame.cells {
            let dim = optics.vignette(field_radius(to_screen(dot.x, dot.y)));
            // Hazed with the cell that contains them, which they were not before: organelles
            // took the vignette but no depth of field at all, so a defocused cell was drawn as a
            // dark body with a crisp bright nucleus sitting in it. Whatever depth is doing to a
            // cell it must do to the cell's contents, or the contents read as floating in front.
            let haze = haze_of(optics, dot.depth, sim.selected == Some(dot.id));
            for (nth, o) in dot.organelles.iter().enumerate() {
                let at = to_screen(dot.x + o.dx, dot.y + o.dy);
                let sep = optics.separation(field_radius(at));
                let size = (o.radius * 2.0 * scale).max(1.0) + sep;
                let [r, g, b] = haze_into(o.rgb, haze_rgb, haze);
                let tint = dim * o.built;
                art_handles.cells.push(cellmesh::Placed {
                    x: at.x,
                    y: at.y,
                    half: size / (2.0 * cellmesh::FIELD_FILL),
                    rgba: [r * tint, g * tint, b * tint, 1.0],
                    shape: cellmesh::Shape {
                        // Its own outline, from its slot and its cell — so the inside of a cell
                        // is not four copies of one pebble, and two cells' nuclei differ.
                        seed: cellmesh::seed_of(
                            dot.id
                                .ordering_key()
                                .wrapping_mul(31)
                                .wrapping_add(nth as u64 + 1),
                        ),
                        softness: 0.0,
                        integrity: 1.0,
                        rounded: if view.rounded { 1.0 } else { 0.0 },
                    },
                    // Organelles are not squashed. They sit inside a cell that may be, and a
                    // nucleus flattening against the neighbouring *cell* would be drawing a
                    // constraint the simulation does not have.
                    squash: Default::default(),
                    // And not swollen, so there is nothing to hand back.
                    swell: 1.0,
                });
            }
        }
    }

    // Into whichever of the two meshes this tier draws through, and the other one goes dark.
    //
    // Exactly one entity each, and that now has to be true rather than merely happening to be:
    // `upload` swaps the vertices across instead of copying them, so a second entity reaching
    // this would be handed the frame *before* last. `setup` spawns one of each, and these are
    // the lines that have to change if that ever stops being true.
    let seamed = detail == cellmesh::Detail::Seamed;
    // Asked before the swap, because after it the buffers hold the last frame.
    //
    // An empty mesh is a validation error in some backends and a wasted draw in the rest. A slide
    // with nothing alive on it simply does not draw the layer, and nor does the tier that is not
    // in force.
    let drawn = art_handles.cells.cells() > 0;
    if let Ok((mesh_handle, mut visibility)) = cell_mesh.single_mut() {
        *visibility = if seamed && drawn {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if seamed {
            if let Some(mut mesh) = meshes.get_mut(&mesh_handle.0) {
                cellpipe::upload(&mut mesh, &mut art_handles.cells);
            }
        }
    }
    if let Ok((mesh_handle, mut visibility)) = dot_mesh.single_mut() {
        *visibility = if !seamed && drawn {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if !seamed {
            if let Some(mut mesh) = meshes.get_mut(&mesh_handle.0) {
                cellpipe::upload(&mut mesh, &mut art_handles.cells);
            }
        }
    }

    // Junctions. A stretched, rotated sprite per link: hard ones solid because they are
    // structure, soft ones faint because they are a channel rather than a body.
    let junction_pool = junctions.iter().count();
    for i in junction_pool..frame.junctions.len() {
        commands.spawn((
            JunctionSprite(i),
            Sprite {
                color: Color::NONE,
                custom_size: Some(Vec2::splat(1.0)),
                ..default()
            },
        ));
    }
    for (marker, mut sprite, mut transform) in &mut junctions {
        let Some(link) = frame.junctions.get(marker.0) else {
            sprite.color = Color::NONE;
            continue;
        };
        let a = to_screen(link.from.0, link.from.1);
        let b = to_screen(link.to.0, link.to.1);
        let delta = (b - a).truncate();
        let length = delta.length().max(1.0);
        let dim = optics.vignette(field_radius((a + b) / 2.0));
        sprite.color = if link.hard {
            Color::srgba(0.80 * dim, 0.78 * dim, 0.70 * dim, 0.85)
        } else {
            Color::srgba(0.55 * dim, 0.70 * dim, 0.85 * dim, 0.35)
        };
        sprite.custom_size = Some(Vec2::new(length, if link.hard { 2.0 } else { 1.0 }));
        // Drawn under the cells, so a junction reads as something the cells sit on rather
        // than something laid over them.
        transform.translation = ((a + b) / 2.0).with_z(0.5);
        transform.rotation = Quat::from_rotation_z(delta.y.atan2(delta.x));
    }

    // Dust on the objective: drawn in screen space, in front of everything, and not affected
    // by pan or zoom, because it is on the lens and not in the water.
    let mote_pool = motes.iter().count();
    for i in mote_pool..frame.motes.len() {
        commands.spawn((
            MoteSprite(i),
            Sprite {
                image: art_handles.image.clone(),
                // Dust on the objective, and dust is not square. Fixed at one silhouette
                // because a mote is a couple of pixels and nobody is going to count them.
                texture_atlas: Some(TextureAtlas {
                    layout: art_handles.layout.clone(),
                    index: 0,
                }),
                color: Color::NONE,
                custom_size: Some(Vec2::splat(1.0)),
                ..default()
            },
        ));
    }
    for (marker, mut sprite, mut transform) in &mut motes {
        let Some(m) = frame.motes.get(marker.0) else {
            sprite.color = Color::NONE;
            continue;
        };
        sprite.color = Color::srgba(0.85, 0.85, 0.82, m.alpha);
        sprite.custom_size = Some(Vec2::splat(m.radius * 2.0));
        transform.translation = Vec3::new((m.u - 0.5) * size.x, (m.v - 0.5) * size.y, 10.0);
    }
}

/// Everything egui draws, and the one place the layout is decided (M10.1).
///
/// Docked panels rather than floating windows. The visible reason is that a rail cannot be
/// dragged over the thing you are looking at. The load-bearing one is that whatever egui does
/// *not* claim is, by definition, the slide — so the slide finally has a rectangle of its own,
/// which is what [`ui::route`] needs and what the whole scroll-wheel complaint came down to.
///
/// No `CentralPanel` is added, deliberately. The slide is drawn by Bevy underneath egui, so
/// the middle has to stay unclaimed: a central panel would paint over it, and egui would then
/// report an area there for the pointer to be "over".
fn panels(
    mut contexts: EguiContexts,
    mut sim: ResMut<SlideRes>,
    mut view: ResMut<View>,
    mut exit: MessageWriter<AppExit>,
    diagnostics: Res<DiagnosticsStore>,
    mut dressed: Local<bool>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let ctx = ctx.clone();

    // The theme, once. Here rather than in a startup system because the context does not exist
    // until this schedule runs — which is the same reason the interface is in this schedule at
    // all — and a `Local` is cheaper than a resource for a flag nothing else reads.
    if !*dressed {
        skin::apply(&ctx);
        *dressed = true;
    }

    let frame = sim.latest.frame.clone();

    // egui 0.35 replaced `SidePanel` and `TopBottomPanel` with one `Panel`, and a panel is now
    // shown *inside a `Ui`* rather than against a `Context`. So the whole viewport becomes a
    // root `Ui` on the background layer and everything is laid out in that.
    //
    // This is a better model than the one it replaces — a panel was always a region of
    // something, and now it says so — but it does mean the layout has an explicit root where
    // it used to have an implicit one.
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    // Order is layout: egui hands space out from the outside in, so the menu takes the top, the
    // status bar the very bottom, the drawer the strip above it, and the rails what is left
    // between them.
    let mut quit = false;
    menu_bar(&mut root, &mut sim, &mut view, &mut quit);
    status_bar(&mut root, &sim, &view, &frame, &diagnostics);
    // The parameter draft belongs to the panel that edits it: when that tab is not the one on
    // show, the draft goes with it, so reopening reads the world afresh rather than presenting
    // edits from ten minutes ago as though they were still pending.
    if view.panels.drawer != Some(Panel::Parameters) {
        sim.draft = None;
    }
    drawer(&mut root, &mut sim, &mut view);

    if view.panels.cell {
        egui::Panel::left("rail_left")
            .resizable(true)
            .default_size(270.0)
            .size_range(210.0..=460.0)
            .frame(skin::panel_frame())
            .show(&mut root, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| cell_body(ui, &mut sim, &mut view));
            });
    }
    if view.panels.metrics || view.panels.legend {
        egui::Panel::right("rail_right")
            .resizable(true)
            .default_size(260.0)
            .size_range(210.0..=460.0)
            .frame(skin::panel_frame())
            .show(&mut root, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if view.panels.metrics {
                        metrics_body(ui, &sim);
                    }
                    if view.panels.metrics && view.panels.legend {
                        ui.separator();
                    }
                    if view.panels.legend {
                        legend_body(ui, &sim, &view, &frame);
                    }
                });
            });
    }

    // Whatever is left over is the slide. Recorded here for `handle_input` to route against on
    // the next frame — one frame stale, which is invisible for a rectangle that only moves when
    // a panel is opened or dragged, and much cheaper than laying the UI out twice.
    let rect = root.available_rect_before_wrap();
    view.viewport = Rect::new(rect.min.x, rect.min.y, rect.max.x, rect.max.y);

    if quit {
        exit.write(AppExit::Success);
    }
}

/// A menu item that is designed but not built yet.
///
/// Shown disabled rather than hidden. The shape of the application is worth seeing even where
/// it is hollow, and a greyed item that says which milestone owns it is honest in a way that
/// an item which silently does nothing is not.
fn soon(ui: &mut egui::Ui, label: &str, shortcut: &str, why: &str) {
    ui.add_enabled_ui(false, |ui| skin::menu_item(ui, label, shortcut))
        .inner
        .on_disabled_hover_text(why);
}

/// A drag value with how far along its range it stands painted behind the number.
///
/// A bare `DragValue` is a flat filled box, and a flat filled box reads as a bar that is either
/// empty or full — so the only thing that moves while you drag is the number, which is the thing
/// you are least likely to be watching when your hand is on the pointer. 180 and 900 look
/// identical at a glance on a control whose range is 16 to 1024.
///
/// The fill is computed *after* `add` has run, from the value the drag has already written this
/// frame, so it tracks the pointer rather than arriving once the pointer stops. That ordering is
/// the whole of it: the widget both reads and writes the value, and reading it beforehand paints
/// last frame's answer.
///
/// The track is ours rather than egui's because the drag value paints its own background, and
/// anything painted after the widget covers the number instead of sitting under it. Hence the
/// two reserved shapes before it and the transparent `weak_bg_fill` inside: what egui would have
/// drawn as the background is drawn here, in the same colour it would have used, with the fill
/// on top of it and the number on top of that.
fn ranged_drag(
    ui: &mut egui::Ui,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    speed: f32,
    prefix: &str,
    suffix: &str,
) -> egui::Response {
    let (lo, hi) = (*range.start(), *range.end());

    let track = ui.painter().add(egui::Shape::Noop);
    let fill = ui.painter().add(egui::Shape::Noop);

    let response = ui
        .scope(|ui| {
            let widgets = &mut ui.style_mut().visuals.widgets;
            widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
            widgets.hovered.weak_bg_fill = egui::Color32::TRANSPARENT;
            widgets.active.weak_bg_fill = egui::Color32::TRANSPARENT;
            ui.add(
                egui::DragValue::new(value)
                    .range(lo..=hi)
                    .speed(speed)
                    .prefix(prefix)
                    .suffix(suffix),
            )
        })
        .inner;

    // Clicking a drag value turns it into a text field. A bar behind a caret is a bar behind
    // half-typed digits that do not mean anything yet, so there isn't one.
    if response.has_focus() {
        return response;
    }

    let visuals = *ui.style().interact(&response);
    let rect = response.rect;
    let radius = visuals.corner_radius;
    ui.painter().set(
        track,
        egui::Shape::rect_filled(rect, radius, visuals.weak_bg_fill),
    );

    let span = hi.saturating_sub(lo);
    let along = (*value).clamp(lo, hi).saturating_sub(lo);
    let fraction = if span == 0 {
        0.0
    } else {
        along as f32 / span as f32
    };
    let mut bar = rect;
    bar.max.x = rect.min.x + rect.width() * fraction;
    // Square on the leading edge, so that a bar a fifth of the way along is a bar and not a
    // small pill sitting on the left — egui shrinks a corner radius to fit whatever it is
    // rounding, and a narrow rounded rectangle stops looking like a measurement.
    let ends = if fraction > 0.999 {
        radius
    } else {
        egui::CornerRadius {
            ne: 0,
            se: 0,
            ..radius
        }
    };
    if bar.width() >= 1.0 {
        ui.painter().set(
            fill,
            egui::Shape::rect_filled(bar, ends, ui.visuals().selection.bg_fill),
        );
    }

    response
}

/// The menu bar, and the transport controls at its right-hand end.
///
/// Every keyboard shortcut in `keyboard` appears against its menu item, and no shortcut exists
/// that is not in a menu. The previous build had fourteen single-key bindings and no way at all
/// to discover any of them.
fn menu_bar(root: &mut egui::Ui, sim: &mut SlideRes, view: &mut View, quit: &mut bool) {
    const LATER: &str = "M10.2 — configuration and slide files";

    egui::Panel::top("menu_bar").show(root, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                skin::menu_caption(ui, "slide files");
                soon(ui, "New slide…", "Ctrl+N", LATER);
                soon(ui, "Open slide…", "Ctrl+O", LATER);
                soon(ui, "Save slide", "Ctrl+S", LATER);
                soon(ui, "Save slide as…", "", LATER);
                skin::menu_rule(ui);
                soon(ui, "Export…", "", LATER);
                skin::menu_rule(ui);
                if skin::menu_item(ui, "Quit", "Ctrl+Q").clicked() {
                    *quit = true;
                    ui.close();
                }
            });

            ui.menu_button("Slide", |ui| {
                ui.menu_button("New slide…", |ui| {
                    ui.label("start again, at a size you choose");
                    ranged_drag(ui, &mut view.new_size, 16..=1024, 4.0, "slide  ", " squares")
                        .on_hover_text(
                            "the slide is square. Everything scales with the area: the matter \
                             seeded into it, the carrying capacity, and how long a population \
                             takes to fill it",
                        );
                    ranged_drag(ui, &mut view.new_founders, 1..=64, 0.25, "founders  ", "")
                        .on_hover_text(
                            "how many ancestors to start with, spread evenly over the slide. \
                             One is a clean experiment — everything that follows descends from \
                             it. Sixteen grow as sixteen colonies and keep far more diversity",
                        );
                    ui.separator();
                    if ui
                        .button("Create")
                        .on_hover_text("throws away what is on the slide now")
                        .clicked()
                    {
                        let size = view.new_size;
                        sim.new_slide(size, view.new_founders);
                        // Look at it. The camera stays where it was told to be otherwise, and
                        // making a sixteen-square slide while parked over the middle of a
                        // five-hundred-square one leaves you staring at open water with no clue
                        // that anything happened.
                        view.centre = Vec2::splat(size as f32 / 2.0);
                        view.zoom = (BASE_SCALE * 6.0 / size as f32).clamp(0.05, 40.0);
                        ui.close();
                    }
                });
                ui.menu_button("New scenario…", |ui| {
                    ui.label("an empty slide, stopped, to build one on");
                    ui.small(
                        "No light gradient, no chemistry, no walls, nobody home. Paint what \
                         you want on it with the tools, seed it, then Save below — and what \
                         comes back when you open it is what you built.",
                    );
                    ranged_drag(ui, &mut view.new_size, 16..=1024, 4.0, "size  ", " squares");
                    ui.separator();
                    if ui
                        .button("Create")
                        .on_hover_text("throws away what is on the slide now")
                        .clicked()
                    {
                        let size = view.new_size;
                        sim.new_scenario(size);
                        view.centre = Vec2::splat(size as f32 / 2.0);
                        view.zoom = (BASE_SCALE * 6.0 / size as f32).clamp(0.05, 40.0);
                        // Straight into the tool you need first: a blank slide is a slide with
                        // nothing to select.
                        view.tool = Tool::Paint;
                        ui.close();
                    }
                });
                ui.menu_button("Scenario library", |ui| {
                    let found = library::scenarios();
                    if found.is_empty() {
                        ui.weak("no scenarios/ directory here");
                    }
                    for entry in found {
                        if ui
                            .button(&entry.label)
                            .on_hover_text(entry.path.display().to_string())
                            .clicked()
                        {
                            open_scenario(sim, view, &entry.path);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Open scenario…", |ui| {
                    ui.label("a .ron, by path");
                    ui.text_edit_singleline(&mut view.file_path);
                    if ui.button("Open").clicked() {
                        let path = std::path::PathBuf::from(view.file_path.trim());
                        open_scenario(sim, view, &path);
                        ui.close();
                    }
                });
                if ui
                    .add(
                        egui::Button::new("Parameters…")
                            .shortcut_text(Panel::Parameters.key())
                            .selected(view.panels.is_open(Panel::Parameters)),
                    )
                    .on_hover_text("every cost, rate and mutation the living half runs on")
                    .clicked()
                {
                    view.panels.toggle(Panel::Parameters);
                    ui.close();
                }
                ui.menu_button("Save parameters as…", |ui| {
                    ui.label("the running world's scenario, back out as a .ron");
                    ui.small(
                        "every parameter as it stands now, including anything changed \
                         mid-run — which is how a setting you tuned by hand becomes a \
                         scenario you can start ten runs from.",
                    );
                    ui.text_edit_singleline(&mut view.file_path);
                    if ui.button("Save").clicked() {
                        let path = std::path::PathBuf::from(view.file_path.trim());
                        let mut scenario = {
                            let held = sim.engine.handle();
                            let slide = held.slide();
                            slide.world().scenario().clone()
                        };
                        // A slide built from `New scenario…` is still called "untitled", and a
                        // library full of untitleds is a library you cannot read. The file name
                        // is the one thing the author has definitely already chosen.
                        if scenario.name == "untitled" {
                            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                scenario.name = stem.replace('_', " ");
                            }
                        }
                        view.file_note = Some(match library::save(&path, &scenario) {
                            Ok(written) => Ok(format!("wrote {}", written.display())),
                            Err(e) => Err(e.to_string()),
                        });
                        ui.close();
                    }
                });
                ui.separator();
                if ui
                    .add(egui::Button::new("Reseed").shortcut_text("R"))
                    .on_hover_text("wipe the slide and start the ancestor over")
                    .clicked()
                {
                    sim.reseed();
                    ui.close();
                }
                if ui
                    .button("Packing bench")
                    .on_hover_text(
                        "a slide with the biology switched off: cells that cannot divide, \
                         die or grow, and no Brownian jitter, gathered by gravity towards \
                         the middle. For looking at how volumes behave without wondering \
                         whether what you are seeing is the simulation",
                    )
                    .clicked()
                {
                    sim.bench();
                    ui.close();
                }
            });

            ui.menu_button("Simulation", |ui| {
                let running = sim.engine.rate().is_running();
                if skin::menu_item(ui, if running { "Pause" } else { "Run" }, "Space").clicked() {
                    view.paused = !sim.engine.toggle_pause().is_running();
                    ui.close();
                }
                if skin::menu_item(ui, "Step one tick", ".").clicked() {
                    sim.engine.step();
                }

                // Speed is a live control and not a submenu (UI.md §8.7). `Simulation ▸ Speed ▸
                // 1×` is three levels to change one thing, and the menu shuts on the click — so
                // comparing ½× against 8× meant opening it twice. Here the menu stays up and the
                // row is the answer, for the same reason the toolbox is a panel: a thing you
                // adjust while watching has to stay where you can reach it.
                skin::menu_caption(ui, "speed");
                let now = sim.engine.rate();
                if let Some(rate) = skin::segmented(
                    ui,
                    &[
                        ("pause", "0", Rate::Paused),
                        ("½×", "`", Rate::half()),
                        ("1×", "-", Rate::times(1)),
                        ("8×", "=", Rate::times(8)),
                        ("max", "⌫", Rate::Unlimited),
                    ],
                    now,
                ) {
                    sim.engine.set_rate(rate);
                    view.paused = rate == Rate::Paused;
                }
                ui.label(skin::text(
                    Role::Small,
                    "½× is 30 ticks a second, for watching a division rather than catching one.",
                ));
                skin::menu_rule(ui);
                let count = sim.latest.interventions.len();
                let showing =
                    view.panels.is_open(Panel::Ecology) && view.ecology == Ecology::Interventions;
                if ui
                    .add(
                        egui::Button::new(if count == 0 {
                            "Interventions…".to_string()
                        } else {
                            format!("Interventions… ({count})")
                        })
                        .selected(showing),
                    )
                    .on_hover_text("what has been changed in this world, and when")
                    .clicked()
                {
                    // Opens the ecology pane on that view, as `f` does for the food web,
                    // rather than toggling: the pane is where the log lives now.
                    view.panels.set(Panel::Ecology, true);
                    view.ecology = Ecology::Interventions;
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                for panel in Panel::ALL {
                    let mut open = view.panels.is_open(panel);
                    if ui
                        .add(
                            egui::Button::new(panel.title())
                                .shortcut_text(panel.key())
                                .selected(open),
                        )
                        .clicked()
                    {
                        open = !open;
                        view.panels.set(panel, open);
                    }
                }
                ui.separator();
                ui.menu_button("Overlays", |ui| {
                    for (i, name) in sim.chem_names.clone().into_iter().enumerate() {
                        let on = sim.engine.overlay_enabled(i);
                        let key = if i < 9 {
                            (i + 1).to_string()
                        } else {
                            String::new()
                        };
                        if ui
                            .add(egui::Button::new(name).shortcut_text(key).selected(on))
                            .clicked()
                        {
                            sim.engine.toggle_overlay(i);
                        }
                    }
                });
                if ui
                    .add(
                        egui::Button::new("Rounded cells")
                            .shortcut_text("C")
                            .selected(view.rounded),
                    )
                    .on_hover_text(
                        "shaded, irregular cells rather than flat squares. Presentation only \
                         — it changes which tile of a baked atlas each sprite samples",
                    )
                    .clicked()
                {
                    view.rounded = !view.rounded;
                }
                if ui
                    .add(
                        egui::Button::new("Organelles")
                            .shortcut_text("N")
                            .selected(view.organelles),
                    )
                    .on_hover_text(
                        "the blobs inside each cell. Off is how you look at the cells \
                         themselves — a crowd of them is mostly organelles by area, and the \
                         shape of the crowd is underneath",
                    )
                    .clicked()
                {
                    view.organelles = !view.organelles;
                }
                if ui
                    .add(
                        egui::Button::new("Optics")
                            .shortcut_text("O")
                            .selected(sim.engine.optics_enabled()),
                    )
                    .on_hover_text("vignette, defocus and dust on the objective")
                    .clicked()
                {
                    sim.engine.set_optics(!sim.engine.optics_enabled());
                }
                if ui
                    .add(
                        egui::Button::new("Flow")
                            .shortcut_text("V")
                            .selected(sim.engine.flow_enabled()),
                    )
                    .on_hover_text(
                        "which way the water is going, and how fast. Arrows point \
                         downstream and are drawn only where the water is actually moving, \
                         so bare slide means still.",
                    )
                    .clicked()
                {
                    sim.engine.set_flow(!sim.engine.flow_enabled());
                }
                ui.separator();
                if ui
                    .add(
                        egui::Button::new("Follow selection")
                            .shortcut_text("T")
                            .selected(view.follow),
                    )
                    .clicked()
                {
                    view.follow = !view.follow;
                }
                if ui
                    .add(egui::Button::new("Reset camera").shortcut_text("Home"))
                    .clicked()
                {
                    view.centre = Vec2::new(
                        sim.latest.frame.width as f32 / 2.0,
                        sim.latest.frame.height as f32 / 2.0,
                    );
                    view.zoom = 1.0;
                    ui.close();
                }
            });

            ui.menu_button("Tools", |ui| {
                for (tool, key) in TOOLS {
                    if ui
                        .add(
                            egui::Button::new(tool.name())
                                .shortcut_text(key)
                                .selected(view.tool == tool),
                        )
                        .clicked()
                    {
                        view.tool = tool;
                        ui.close();
                    }
                }
                ui.separator();
                // The settings live in the toolbox panel, not here. A menu closes the moment
                // you click the slide, so a dose adjusted from a menu costs open-change-close
                // for every stroke — see `Panel::Toolbox`.
                if ui
                    .add(egui::Button::new("Toolbox…").shortcut_text("T"))
                    .on_hover_text("what the tools are loaded with, and the sources on the slide")
                    .clicked()
                {
                    view.panels.set(Panel::Toolbox, true);
                    ui.close();
                }
            });

            ui.menu_button("Help", |ui| {
                ui.label("keys");
                ui.separator();
                for (key, what) in [
                    ("space", "run / pause"),
                    (".", "step one tick"),
                    ("0 ` - = ⌫", "speed"),
                    ("1–9", "chemical overlays"),
                    ("[ ]", "step one overlay at a time"),
                    ("v", "flow field"),
                    ("F1–F10", "tools"),
                    ("drag", "pan the slide"),
                    ("wheel", "zoom about the pointer"),
                    ("click", "select a cell"),
                ] {
                    ui.small(format!("{key:<10} {what}"));
                }
            });

            // The transport, at the right-hand end, mirroring the keys rather than replacing
            // them. `right_to_left` so it stays pinned to the edge as the window resizes.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if skin::chip(ui, "⏭", None, false)
                    .on_hover_text("step one tick  (.)")
                    .clicked()
                {
                    sim.engine.step();
                }
                // Slowest last, because the layout is right-to-left: this reads ½× 1× 8× max
                // on screen, which is the order the keys are in and the order a speed control
                // is in everywhere else.
                for (label, rate) in [
                    ("max", Rate::Unlimited),
                    ("8×", Rate::times(8)),
                    ("1×", Rate::times(1)),
                    ("½×", Rate::half()),
                ] {
                    if skin::chip(ui, label, None, sim.engine.rate() == rate).clicked() {
                        sim.engine.set_rate(rate);
                        view.paused = false;
                    }
                }
                let running = sim.engine.rate().is_running();
                // Never accented: run and pause are one *action* whose label already says which
                // way it goes, and the accent means state. The speed chips beside it carry the
                // state, and two lit chips side by side say nothing at all.
                if skin::chip(ui, if running { "⏸" } else { "▶" }, None, false)
                    .on_hover_text("run / pause  (space)")
                    .clicked()
                {
                    view.paused = !sim.engine.toggle_pause().is_running();
                }
                ui.label(skin::text(Role::Section, "transport"));
            });
        });
    });
}

/// One line along the bottom: what is selected, what the world is doing, and the scale bar.
///
/// The scale bar is the microscope's, not a debug readout — it is the thing in the corner of
/// every frame of the footage this is modelled on, and it is the only honest way to say how
/// big anything on the slide is.
fn status_bar(
    root: &mut egui::Ui,
    sim: &SlideRes,
    view: &View,
    frame: &Frame,
    diagnostics: &DiagnosticsStore,
) {
    egui::Panel::bottom("status_bar")
        .frame(
            skin::panel_frame().inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 3,
                bottom: 3,
            }),
        )
        .show(root, |ui| {
            ui.horizontal(|ui| {
                if sim.reading().is_some() {
                    ui.label(
                        egui::RichText::new(&sim.latest.species)
                            .italics()
                            .size(Role::Body.size())
                            .color(skin::col(Role::Body.ink().unwrap_or(theme::DIM))),
                    );
                    tick(ui);
                }
                ui.label(skin::text(Role::Label, format!("tick {}", thousands(frame.tick as i64))));
                tick(ui);
                ui.label(skin::text(
                    Role::Label,
                    format!("{} cells", thousands(frame.population as i64)),
                ));
                if frame.largest_cluster > 1 {
                    tick(ui);
                    ui.label(skin::text(
                        Role::Label,
                        format!("largest organism {}", frame.largest_cluster),
                    ));
                }
                tick(ui);
                // What the last file operation said. In the status bar rather than in the menu
                // that triggered it, because that menu closed on the click — and an error nobody
                // sees is an error that looks like nothing happening.
                if let Some(note) = &view.file_note {
                    match note {
                        Ok(m) => ui.label(skin::text(Role::Label, m.clone())),
                        Err(m) => ui.label(skin::moody(Role::Label, Mood::Bad, m.clone())),
                    }
                    .on_hover_text("the last file this session opened or wrote");
                    tick(ui);
                }
                ui.label(skin::text(
                    Role::Label,
                    match view.tool {
                        // Only where it means something. A width beside "select" is noise.
                        Tool::DrawBarrier | Tool::EraseBarrier => {
                            format!("tool {} · brush {}", view.tool.name(), view.brush)
                        }
                        _ => format!("tool {}", view.tool.name()),
                    },
                ));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // The two halves of the working target, side by side and never added
                    // together (`docs/MILESTONES.md`). Until M10.1 there was only one number
                    // here and it was both of them at once, which is how a slow tick and a slow
                    // frame became indistinguishable.
                    let fps = diagnostics
                        .get(&FrameTimeDiagnosticsPlugin::FPS)
                        .and_then(Diagnostic::smoothed)
                        .unwrap_or(0.0);
                    ui.label(skin::text(
                        Role::Label,
                        format!("{:.0} fps · {} t/s", fps, sim.engine.ticks_per_second()),
                    ))
                    .on_hover_text(
                        "frames a second and ticks a second, measured separately. The working \
                         target is 50,000 cells at 30 of each.",
                    );
                    tick(ui);
                    ui.label(skin::text(
                        Role::Label,
                        match frame.lod {
                            Lod::Dots => "points",
                            Lod::Packed => "packed",
                            Lod::Organelles => "organelles",
                            Lod::Full => "full",
                        },
                    ));
                    tick(ui);
                    // Magnification is reported the way the objective would: relative to the
                    // base scale, so "1×" is one substrate square to eight pixels.
                    ui.label(skin::text(Role::Label, format!("{:.0}×", view.zoom * 100.0)));
                    ui.add_space(8.0);
                    scale_bar(ui, BASE_SCALE * view.zoom);
                });
            });
        });
}

/// A separator between two readings in the status bar: a short vertical tick, not a full-height
/// rule, so the bar reads as one line of readings rather than as a row of boxes.
fn tick(ui: &mut egui::Ui) {
    ui.add_space(5.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 11.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, skin::col(theme::RULE));
    ui.add_space(5.0);
}

/// The microscope's scale bar: how far across the slide a stretch of screen actually is.
///
/// The thing in the corner of every frame of the footage this is modelled on, and the only
/// honest answer to "how big is that". Measured in substrate squares rather than in the microns
/// `docs/UI.md` §2 sketches — see [`ui::scale_bar`] for why that is not a detail.
fn scale_bar(ui: &mut egui::Ui, pixels_per_square: f32) {
    const ROOM: f32 = 112.0;
    let (squares, length) = ui::scale_bar(pixels_per_square, ROOM);
    let length = length.min(ROOM);
    ui.label(skin::text(
        Role::Label,
        format!("{squares} {}", if squares == 1 { "square" } else { "squares" }),
    ));
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(length, 9.0), egui::Sense::hover());
    let ink = skin::col(theme::DIM);
    let y = rect.center().y;
    ui.painter()
        .hline(rect.x_range(), y, egui::Stroke::new(1.0, ink));
    for x in [rect.left(), rect.right() - 1.0] {
        ui.painter().vline(
            x,
            egui::Rangef::new(y - 3.5, y + 3.5),
            egui::Stroke::new(1.0, ink),
        );
    }
}

/// The bottom drawer: one tab at a time, for everything that wants width rather than height.
///
/// A listing, a source buffer, a tree and a food web are all wide and short. Putting them in a
/// side rail is how a genome listing ends up forty characters across with the operand column
/// wrapped off the end.
fn drawer(root: &mut egui::Ui, sim: &mut SlideRes, view: &mut View) {
    let Some(showing) = view.panels.drawer else {
        return;
    };
    egui::Panel::bottom("drawer")
        .resizable(true)
        .default_size(300.0)
        .size_range(120.0..=760.0)
        .frame(skin::panel_frame())
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for panel in Panel::ALL.into_iter().filter(|p| p.dock() == Dock::Drawer) {
                    if skin::chip(ui, panel.title(), Some(panel.key()), panel == showing).clicked()
                    {
                        view.panels.drawer = Some(panel);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if skin::chip(ui, "▼", None, false)
                        .on_hover_text("collapse the drawer")
                        .clicked()
                    {
                        view.panels.drawer = None;
                    }
                });
            });
            ui.add_space(2.0);
            skin::hairline(ui);
            ui.add_space(4.0);
            // egui 0.35's `Panel` sizes to its content unless the content claims the space, so
            // `default_size` alone gave a drawer as tall as whatever was in it — 120 pixels for
            // the genome listing, which is two lines of it. Claiming the height makes the
            // drawer the size it says it is and lets the scroll areas inside do their job.
            ui.set_min_height(ui.available_height());
            match showing {
                Panel::Genome => genome_body(ui, sim, view),
                Panel::Ecology => ecology_body(ui, sim, view),
                Panel::Toolbox => toolbox_body(ui, sim, view),
                Panel::Parameters => parameters_body(ui, sim, view),
                Panel::Editor => editor_body(ui, sim),
                Panel::Debugger => debugger_body(ui, sim),
                // The rails' panels are never the drawer's tab; `Panels::set` will not put
                // one there.
                Panel::Cell | Panel::Metrics | Panel::Legend => {}
            }
        });
}

/// A cell's flattened sides, as the mesh wants them.
///
/// The seam distances arrive as a fraction of the cell's own radius, which is what makes them
/// independent of how big it is drawn; the shader works in field units where the body radius is
/// `FIELD_FILL`, so that is the one conversion. Slots the cell does not use are left at the
/// default, which is a seam nothing can reach.
fn squash_of(dot: &mm_app::CellDot) -> [cellmesh::Squash; cellmesh::SQUASH_PER_CELL] {
    let mut out = [cellmesh::Squash::default(); cellmesh::SQUASH_PER_CELL];
    // Deepest first when there are more seams than the mesh can carry, and *never* the order
    // they arrived in. `NeighbourIndex::within` walks its buckets in ascending y, so taking the
    // first `SQUASH_PER_CELL` takes the neighbours above and drops the ones below — and world
    // +y is *down* on screen, so a cell would be drawn overlapping its lower neighbours and
    // never its upper ones. A directional artefact out of nothing but an iteration order.
    //
    // `face` is how far along its normal the seam sits as a fraction of the radius, so smaller
    // is a deeper cut and the ones that matter most sort first.
    let mut seams: Vec<&mm_app::slide::Squash> = dot.squash.iter().collect();
    if seams.len() > cellmesh::SQUASH_PER_CELL {
        seams.sort_by(|a, b| {
            a.face
                .partial_cmp(&b.face)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    for (slot, s) in out.iter_mut().zip(seams.into_iter()) {
        *slot = cellmesh::Squash {
            nx: s.nx,
            // Negated, because `to_screen` negates it. The slide's rows run downwards and the
            // screen's run upwards, so a neighbour below a cell in the world is above it in
            // the picture — and a seam that did not turn over with everything else flattened
            // the cell on the side where there was nobody.
            ny: -s.ny,
            face: s.face,
        };
    }
    out
}

/// One run of a genome listing, in the pane's monospace.
fn listing_ink(color: egui::Color32, background: egui::Color32) -> egui::text::TextFormat {
    egui::text::TextFormat {
        font_id: egui::FontId::monospace(11.0),
        color,
        background,
        ..Default::default()
    }
}

/// The colour of one classified run of a listing (M10.3b).
///
/// Numbers are the loudest thing on the line, and deliberately: the whole point of the reading
/// form is that `%001111` becomes `60`, and a 60 in the same grey as everything around it has
/// not really arrived. Everything else is tuned to stay out of its way — an opcode column you
/// read by shape rather than by colour, prose dimmer than the values it describes.
fn ink_colour(ink: mm_app::inspector::Ink, current: bool) -> egui::Color32 {
    use mm_app::inspector::Ink;

    match ink {
        // Lifted on the current line only: against the green backing the ordinary greys go
        // muddy, and this is the one line somebody is definitely reading.
        Ink::Opcode if current => egui::Color32::from_rgb(225, 235, 225),
        Ink::Opcode => egui::Color32::from_rgb(200, 208, 220),
        Ink::Number => egui::Color32::from_rgb(240, 195, 120),
        // Distinct from a number, because in the source form the whole point is that these
        // bits are *not* the value they encode.
        Ink::Pattern => egui::Color32::from_rgb(150, 195, 165),
        Ink::Gene => egui::Color32::from_rgb(150, 175, 215),
        Ink::Marker => egui::Color32::from_rgb(120, 190, 190),
        Ink::Miss => egui::Color32::from_rgb(220, 145, 110),
        Ink::Note => egui::Color32::from_gray(130),
    }
}

/// One run of editor source, in the editor's own monospace.
///
/// The font is resolved from the style rather than fixed, because a layouter replaces the
/// widget's own text layout entirely — pick a different size here and the caret stops landing
/// where the characters are.
fn source_ink(
    font: &egui::FontId,
    color: egui::Color32,
    background: Option<egui::Color32>,
) -> egui::text::TextFormat {
    egui::text::TextFormat {
        font_id: font.clone(),
        color,
        background: background.unwrap_or(egui::Color32::TRANSPARENT),
        ..Default::default()
    }
}

/// The colour of one token of `.mm` source.
///
/// The same palette the genome pane reads with, mapped through [`ink_colour`] rather than
/// copied — an opcode that is one colour in the listing and another in the editor is two
/// languages as far as anyone looking at both is concerned.
fn token_colour(kind: mm_asm::highlight::TokenKind) -> egui::Color32 {
    use mm_app::inspector::Ink;
    use mm_asm::highlight::TokenKind as T;

    match kind {
        T::Opcode => ink_colour(Ink::Opcode, false),
        T::Number => ink_colour(Ink::Number, false),
        T::Pattern => ink_colour(Ink::Pattern, false),
        // A promoter and a label are both names for somewhere to go, and the listing already
        // draws the genes it invents in this colour.
        T::Promoter | T::LabelDef | T::LabelRef => ink_colour(Ink::Gene, false),
        // What the assembler would reject, coloured as you type it rather than after you press
        // assemble. Same colour as a jump that never fires, which is the same kind of news.
        T::Unknown => ink_colour(Ink::Miss, false),
        T::Comment => egui::Color32::from_gray(105),
        T::Space => ink_colour(Ink::Note, false),
    }
}

/// The tools and what they are loaded with (`docs/UI.md` §4).
///
/// A panel rather than a menu. These began in the Tools menu and it does not work: a menu closes
/// the moment you click the slide, so adjusting a dose between strokes was open, change, close,
/// paint, open again. Anything you adjust *while* working has to stay on screen while you work,
/// and building a slide is nothing but adjusting and working.
fn toolbox_body(ui: &mut egui::Ui, sim: &mut SlideRes, view: &mut View) {
    // The drawer's shape (UI.md §8.6): a wide work area, and the prose in the column.
    //
    // This tab is why the rule exists. It was a vertical stack of a slider, a combo box, a drag
    // value and a text edit with four paragraphs of explanation *between* the controls — a
    // narrow column in the widest space in the window, which is the exact failure
    // `ui::Panel::dock` has a test against. The prose is worth reading; it is not worth reading
    // between two settings you are comparing.
    skin::drawer_split(
        ui,
        "toolbox_notes",
        |ui| toolbox_work(ui, sim, view),
        |ui| {
            skin::section(ui, "why this is a panel", false);
            ui.label(skin::text(
                Role::Body,
                "A menu shuts the moment you click the slide, so changing a dose between two \
                 strokes was open, change, close, paint, and open again. Anything you adjust \
                 while working has to stay on screen while you work.",
            ));
            skin::section(ui, "one chemical, four tools", true);
            ui.label(skin::text(
                Role::Body,
                "Paint, unpaint, source and drain all use the chemical above — they are four \
                 things you do to one chemical, and four separate settings would be four places \
                 to notice you had the wrong one.",
            ));
            skin::section(ui, "a brush is a disc", true);
            ui.label(skin::text(
                Role::Body,
                "The eraser is the same width as the pen, so it can always take back what the \
                 pen just drew. Three squares is the narrowest stroke that is solid on the \
                 diagonal; at one, a diagonal run touches only at its corners and a cell fits \
                 through the gap.",
            ));
            skin::section(ui, "dose and drain", true);
            ui.label(skin::text(
                Role::Body,
                "A dose is what one stamp puts in a square, and what a new source supplies per \
                 step; 1024 is one unit. A drain takes a share of a square rather than an \
                 amount, so it settles into balance with whatever reaches it instead of \
                 scouring the slide dry.",
            ));
        },
    );
}

/// The toolbox's work area: the tools, their settings in one row, and the flux table.
fn toolbox_work(ui: &mut egui::Ui, sim: &mut SlideRes, view: &mut View) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
        for (tool, key) in TOOLS {
            if skin::chip(ui, tool.name(), Some(key), view.tool == tool).clicked() {
                view.tool = tool;
            }
        }
    });
    ui.label(skin::text(
        Role::Small,
        "right-click the slide to use the selected tool; drag to pan.",
    ));

    // All three settings groups on one line, which is what the width is for.
    ui.add_space(theme::SECTION_GAP);
    ui.horizontal_wrapped(|ui| {
        ui.label(skin::text(Role::Label, "brush"));
        ui.add(egui::Slider::new(&mut view.brush, ui::BRUSH_MIN..=ui::BRUSH_MAX).show_value(false));
        ui.label(skin::text(Role::Value, view.brush.to_string()));
        ui.label(skin::text(Role::Small, "squares · a disc, not a box"));

        ui.add_space(10.0);
        ui.label(skin::text(Role::Label, "loaded"));
        if let Some(rgb) = sim.chem_colours.get(view.load).copied() {
            skin::swatch(ui, rgb, true);
        }
        egui::ComboBox::from_id_salt("tool chemical")
            .selected_text(skin::text(
                Role::Value,
                sim.chem_names
                    .get(view.load)
                    .cloned()
                    .unwrap_or_else(|| view.load.to_string()),
            ))
            .show_ui(ui, |ui| {
                for (i, name) in sim.chem_names.iter().enumerate() {
                    ui.selectable_value(&mut view.load, i, name);
                }
            });
        ui.label(skin::text(Role::Label, "dose"));
        ui.add(
            egui::DragValue::new(&mut view.dose)
                .speed(256.0)
                .range(0..=1_000_000),
        );
        ui.label(skin::text(Role::Label, "drain"));
        ui.add(
            egui::Slider::new(&mut view.drain_rate, 1..=mm_core::Q10_ONE)
                .logarithmic(true)
                .show_value(false),
        );
        ui.label(skin::text(
            Role::Value,
            format!("{}/1024", view.drain_rate),
        ));

        ui.add_space(10.0);
        ui.label(skin::text(Role::Label, "seed"));
        ui.add(
            egui::TextEdit::singleline(&mut view.place_genome)
                .desired_width(150.0)
                .font(skin::font(Role::Value))
                .hint_text("ancestor.mm"),
        )
        .on_hover_text("a file in genomes/. The seed tool drops founders of it.");
        ui.add(
            egui::DragValue::new(&mut view.place_count)
                .speed(0.2)
                .range(1..=64)
                .prefix("× "),
        );
    });

    // The flux already on the slide. Listing them is the only way to *find* one: a source is an
    // area of water that behaves differently, and until it has filled up there is nothing there
    // to see but its outline.
    //
    // A table rather than a sentence each. Four sources described in prose are four sentences to
    // read; in columns they are four rows to scan, and the chemical, the rectangle and the rate
    // line up down the page.
    let flux = {
        let held = sim.engine.handle();
        let slide = held.slide();
        slide.world().flux().to_vec()
    };
    ui.add_space(theme::SECTION_GAP);
    skin::hairline(ui);
    skin::section(ui, "sources and drains on the slide", true);
    if flux.is_empty() {
        ui.label(skin::text(
            Role::Small,
            "none. Pick source or drain and drag a rectangle on the slide.",
        ));
        return;
    }
    let mut remove = None;
    egui::ScrollArea::vertical()
        .id_salt("flux")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, f) in flux.iter().enumerate() {
                let (kind, chemical, rect, rate) = flux_columns(f, &sim.chem_names);
                let rgb = chem_rgb(f, &sim.chem_colours);
                ui.horizontal(|ui| {
                    skin::swatch(ui, rgb, kind == "source");
                    ui.label(
                        skin::text(Role::Label, kind).color(skin::col(if kind == "source" {
                            Mood::Good.rgb()
                        } else {
                            Mood::Bad.rgb()
                        })),
                    );
                    ui.add_sized(
                        egui::vec2(108.0, theme::row::HEIGHT),
                        egui::Label::new(skin::text(Role::Value, chemical)).truncate(),
                    );
                    ui.add_sized(
                        egui::vec2(130.0, theme::row::HEIGHT),
                        egui::Label::new(skin::text(Role::Label, rect)),
                    );
                    ui.label(skin::text(Role::Label, rate));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if skin::chip(ui, "×", None, false)
                            .on_hover_text("remove")
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                });
            }
        });
    if let Some(i) = remove {
        let held = sim.engine.handle();
        held.slide().world_mut().remove_flux(i);
    }
}

/// A source or drain split into the four columns the table draws, rather than the one sentence
/// it used to be.
fn flux_columns(f: &mm_core::Flux, names: &[String]) -> (&'static str, String, String, String) {
    let named = |c: usize| {
        names
            .get(c)
            .cloned()
            .unwrap_or_else(|| format!("chemical {c}"))
    };
    match f {
        mm_core::Flux::Source {
            chemical,
            x,
            y,
            width,
            height,
            per_tick,
        } => (
            "source",
            named(*chemical),
            format!("({x}, {y}) {width}×{height}"),
            format!("+{:.2} / tick", *per_tick as f32 / mm_core::Q10_ONE as f32),
        ),
        mm_core::Flux::Drain {
            chemical,
            x,
            y,
            width,
            height,
            rate,
        } => (
            "drain",
            named(*chemical),
            format!("({x}, {y}) {width}×{height}"),
            format!("−{rate}/1024 / tick"),
        ),
    }
}

/// The chemical colour a flux row is swatched in. Out of the scenario, like every chemical
/// colour, and not restyled.
fn chem_rgb(f: &mm_core::Flux, colours: &[[u8; 3]]) -> [u8; 3] {
    let c = match f {
        mm_core::Flux::Source { chemical, .. } | mm_core::Flux::Drain { chemical, .. } => *chemical,
    };
    colours.get(c).copied().unwrap_or([160, 160, 160])
}

/// The world's books: what energy comes in against what leaves, and where the matter is.
///
/// Almost all of this was already being measured and thrown away. `mm_core::metrics::Sample`
/// carries twenty-odd fields into the history buffer every few hundred ticks — the whole
/// per-chemical mass budget among them — and the metrics rail drew seven of them. This is not
/// new instrumentation; it is drawing what the instrument already records.
///
/// Two halves, because they answer different questions. **Energy** is a flow and the question is
/// whether it balances: income against what leaves, and where those lines meet is where the
/// economy has found its level. **Matter** is a stock and the question is where it has gone.
///
/// On a closed slide the matter total cannot move, and this pane said so. That is no longer
/// true and the claim had to go: a scenario with `flux` on it takes matter in at one edge and
/// lets it out at another, so the total moves by exactly what crossed the boundary and by
/// nothing else. Which is the sharper statement of I4 anyway — the invariant was never "the
/// number is constant", it was "nothing changes it that has not been accounted for", and a
/// closed slide is the special case where the accounting is empty.
fn budget_view(ui: &mut egui::Ui, sim: &SlideRes) {
    let history = &sim.latest.history;
    let Some(now) = history.latest().cloned() else {
        ui.weak("nothing sampled yet");
        return;
    };

    egui::ScrollArea::vertical()
        .id_salt("budget")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Both ledgers first, and in one block. They used to open each of their own
            // sections, which put the material one three scrolls below the fold in a drawer of
            // ordinary height — so the pane answered "is the energy economy balanced" on sight
            // and made you go looking for "is the slide filling up". The two nets are the whole
            // question this pane exists for and they belong side by side, above everything that
            // explains them.
            //
            // Rates, differenced at sample time rather than here — see `Sample::absorbed` for
            // why a panel must never difference the cumulative counters itself.
            let open = now.matter_in > 0 || now.matter_out > 0;
            // Heat is not the only way out any more. Energy also leaves latent in matter that
            // crosses the boundary, and a net ignoring that reads as an economy still filling
            // up on a slide that is merely being flushed through.
            let energy_net = now.absorbed - now.dissipation - now.exported;
            let matter_net = now.influx - now.efflux;
            // Near zero against what is coming in: everything arriving is going straight back
            // out and nothing is accumulating. Away from zero and something is still filling or
            // still draining.
            let settled = |net: i64, gross: i64| {
                if net.abs() * 8 < gross.max(1) {
                    egui::Color32::from_rgb(140, 220, 150)
                } else {
                    egui::Color32::from_rgb(240, 200, 120)
                }
            };
            egui::Grid::new("budget_books")
                .num_columns(4)
                .spacing([14.0, 4.0])
                .show(ui, |ui| {
                    ui.strong("energy");
                    ui.label(format!("in {}", now.absorbed));
                    ui.label(if now.exported > 0 {
                        format!("out {} + {} washed out", now.dissipation, now.exported)
                    } else {
                        format!("out {} as heat", now.dissipation)
                    });
                    ui.colored_label(
                        settled(energy_net, now.absorbed),
                        format!("net {energy_net:+}"),
                    )
                    .on_hover_text(
                        "per sample, not per tick. Near zero means the energy economy has \
                         equilibrated — as much is leaving, as heat and in matter washed off \
                         the slide, as is arriving.",
                    );
                    ui.end_row();

                    if open {
                        ui.strong("matter");
                        ui.label(format!("in {}", now.influx));
                        ui.label(format!("out {}", now.efflux));
                        ui.colored_label(
                            settled(matter_net, now.influx),
                            format!("net {matter_net:+}"),
                        )
                        .on_hover_text(
                            "per sample, not per tick. Near zero means the standing stock has \
                             found its level: what is on the slide is what the flow supports. \
                             A drain takes a *fraction* of what is there, so outflow rises \
                             with the stock until it meets the inflow — which is why this one \
                             converges where the energy economy has no such term and does not.",
                        );
                        ui.end_row();
                    }
                });

            ui.add_space(8.0);
            ui.separator();
            ui.heading("energy");
            ui.small(
                "Energy is not conserved — it degrades. It enters as light and leaves as heat \
                 on every conversion, and what is left over is what the world is holding.",
            );
            ui.add_space(4.0);
            let income = history.series(|s| s.absorbed);
            let heat = history.series(|s| s.dissipation);
            if now.imported > 0 {
                ui.small(format!(
                    "of that income, {} arrived latent in matter rather than as light",
                    now.imported
                ));
            }
            ui.small("income");
            skin::sparkline(ui, &income.normalised(), Mood::Good.rgb());
            ui.small("dissipated as heat");
            skin::sparkline(ui, &heat.normalised(), Mood::Bad.rgb());
            if now.exported > 0 || now.energy_exported > 0 {
                ui.small("carried off the slide in matter");
                skin::sparkline(
                    ui,
                    &history.series(|s| s.exported).normalised(),
                    Mood::Bad.rgb(),
                );
            }
            ui.small(format!("held by living things — now {}", now.energy_stored));
            skin::sparkline(
                ui,
                &history.series(|s| s.energy_stored).normalised(),
                skin::plot_neutral(),
            );

            ui.add_space(8.0);
            ui.separator();
            ui.heading("matter");
            if open {
                ui.small(format!(
                    "{} on the slide. Matter is conserved exactly (I4), so this moves only by \
                     what crosses the edge — and by nothing else.",
                    now.total_matter
                ));
            } else {
                ui.small(format!(
                    "{} in total, and on a closed slide that number never moves (I4). \
                     Everything below is the same matter in different places.",
                    now.total_matter
                ));
            }
            ui.add_space(4.0);

            // The boundary, and only when there is one. A row of zeroes on every closed slide
            // would teach the reader to stop looking at this block, which is the opposite of
            // what it is for. The numbers are up top with the other ledger; these are the
            // shapes, which is what says whether it is *settling* or merely balanced today.
            if open {
                ui.small("arriving from off-slide");
                skin::sparkline(
                    ui,
                    &history.series(|s| s.influx).normalised(),
                    Mood::Good.rgb(),
                );
                ui.small("leaving over the edge");
                skin::sparkline(
                    ui,
                    &history.series(|s| s.efflux).normalised(),
                    Mood::Bad.rgb(),
                );
                ui.small("total on the slide");
                skin::sparkline(
                    ui,
                    &history.series(|s| s.total_matter).normalised(),
                    skin::plot_neutral(),
                );
                ui.add_space(6.0);
            }

            // A bar per chemical in its own colour, against the largest. A stacked plot of
            // sixteen series is unreadable and a table of sixteen numbers says nothing about
            // proportion; what the question "where is my matter" wants is the shape.
            let peak = now.chemicals.iter().copied().max().unwrap_or(1).max(1);
            egui::Grid::new("budget_matter")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    for (c, amount) in now.chemicals.iter().enumerate() {
                        let name = sim
                            .chem_names
                            .get(c)
                            .cloned()
                            .unwrap_or_else(|| format!("{c}"));
                        ui.label(name);
                        ui.weak(format!("{amount}"));
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width().max(40.0), 10.0),
                            egui::Sense::hover(),
                        );
                        let share = (*amount as f64 / peak as f64).clamp(0.0, 1.0) as f32;
                        let rgb = sim.chem_colours.get(c).copied().unwrap_or([160, 160, 160]);
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                rect.min,
                                egui::vec2(rect.width() * share, rect.height()),
                            ),
                            1.0,
                            egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                        );
                        ui.end_row();
                    }
                });

            ui.add_space(6.0);
            ui.small(format!(
                "carrion in the fluid {} — a number that climbs and stays climbed means \
                 nothing is eating the dead",
                now.carrion
            ));
        });
}

/// The parameter editor (M10.2, `docs/UI.md` §4).
///
/// Every cost, rate and mutation the living half of the world runs on. Until M10.2 these were
/// reachable from code and from nothing else, so every scenario ran on the compiled-in defaults
/// and the numbers arrived at by measurement were constants.
///
/// Applying is a button, not a keystroke: each apply is an intervention that goes on the
/// world's record, and one per keystroke would be a useless record.
///
/// A drawer tab rather than the floating window this started as. A window over the slide is a
/// window you have to move to see what your change did, and egui's window frame draws no fill
/// in this build — over a lit microscope slide that came out as ghost text with cells swimming
/// through it. The drawer paints its own background and takes its space from the viewport, so
/// the numbers are legible and the slide above them is unobscured.
fn parameters_body(ui: &mut egui::Ui, sim: &mut SlideRes, view: &mut View) {
    // Read lazily, against the world as it stands. Taking the lock once on open rather than
    // every frame is the whole reason this is cheap enough to leave sitting there. `panels`
    // drops the draft when this tab is not the one on show.
    if sim.draft.is_none() {
        let held = sim.engine.handle();
        let slide = held.slide();
        let live = slide.world().biology().clone();
        // `set_light` and `set_current` write straight into the running scenario, so the
        // scenario *is* what is in force. There is no separate founding value to revert an
        // environment to, which is why it has no "back to the scenario" of its own.
        let env = Environment {
            light: slide.world().scenario().light.clone(),
            current: slide.world().scenario().current.clone(),
        };
        sim.draft = Some(Draft {
            editing: live.clone(),
            live,
            founding: slide.world().scenario().biology.clone(),
            env: env.clone(),
            env_live: env,
        });
    }
    let Some(mut draft) = sim.draft.take() else {
        return;
    };
    let mut apply = false;
    let mut apply_env = false;

    // How far the world has drifted from the file that describes it, and how much of that is
    // not in force yet. Two different questions, and the editor answered neither: one global
    // "not applied" label said that *something* had been touched, and with fifty-one fields
    // under six collapsed headers the only way to find out what was to open all six and read.
    let dirty = draft.editing != draft.live;
    let drifted: usize = params::FIELDS
        .iter()
        .filter(|f| {
            let now = mm_core::params::get(&draft.editing, f.path);
            now.is_some() && now != mm_core::params::get(&draft.founding, f.path)
        })
        .count();

    skin::drawer_split(
        ui,
        "parameters_notes",
        |ui| {
            let height = ui.available_height() - FOOTER_HEIGHT;
            ui.horizontal_top(|ui| {
                // The rail of pages.
                ui.allocate_ui_with_layout(
                    egui::vec2(theme::GROUP_COLUMN, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_size(egui::vec2(theme::GROUP_COLUMN, height));
                        let edge = ui.max_rect();
                        ui.painter().vline(
                            edge.right(),
                            edge.y_range(),
                            egui::Stroke::new(1.0, skin::col(theme::HAIR)),
                        );
                        for page in ParamPage::all() {
                            let count = match page {
                                ParamPage::Group(g) => params::group(g).len(),
                                _ => 0,
                            };
                            let on = view.params_page == page;
                            ui.horizontal(|ui| {
                                if skin::chip(ui, page.title(), None, on).clicked() {
                                    view.params_page = page;
                                }
                                if count > 0 {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| ui.label(skin::text(Role::Label, count.to_string())),
                                    );
                                }
                            });
                        }
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_size(egui::vec2(ui.available_width(), height));
                        match view.params_page {
                            ParamPage::Environment => {
                                environment_editor(ui, &mut draft, &mut apply_env);
                            }
                            ParamPage::Group(group) => {
                                parameter_table(ui, &mut draft, group, &sim.chem_names);
                            }
                            ParamPage::Pathways => {
                                ui.label(skin::text(
                                    Role::Small,
                                    "Which reactions this world offers. An organelle picks one \
                                     with its second control word, so a mitochondrion can only \
                                     burn what it is set to burn — and a lineage must either \
                                     make that substrate itself or eat something that does.",
                                ));
                                egui::ScrollArea::vertical()
                                    .id_salt("pathways_scroll")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        pathway_grid(ui, &mut draft, &sim.chem_names);
                                    });
                            }
                            ParamPage::Catalogue => {
                                egui::ScrollArea::vertical()
                                    .id_salt("catalogue_scroll")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| catalogue_grid(ui, &mut draft));
                            }
                        }
                    },
                );
            });

            // The footer, which is where applying happens and where it says what applying will
            // cost. The tick is named before you commit rather than described afterwards in a
            // tooltip: an intervention is a permanent entry on the world's record.
            skin::hairline(ui);
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                if drifted > 0 {
                    ui.label(skin::moody(
                        Role::Label,
                        Mood::Warn,
                        format!(
                            "{drifted} field{} changed from the scenario",
                            if drifted == 1 { "" } else { "s" }
                        ),
                    ));
                } else {
                    ui.label(skin::text(Role::Label, "as the scenario has it"));
                }
                ui.label(skin::text(
                    Role::Small,
                    if dirty {
                        format!(
                            "Applying records an intervention at tick {}.",
                            sim.latest.frame.tick
                        )
                    } else {
                        "In force.".to_string()
                    },
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(dirty, egui::Button::new(skin::text(Role::Label, "apply")))
                        .on_hover_text(
                            "change the running world. Recorded as an intervention, so the run \
                             still replays exactly and the timeline says when you did it.",
                        )
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui
                        .add_enabled(dirty, egui::Button::new(skin::text(Role::Label, "discard")))
                        .on_hover_text("back to what the world is running on")
                        .clicked()
                    {
                        draft.editing = draft.live.clone();
                    }
                    if ui
                        .add_enabled(
                            draft.editing != draft.founding,
                            egui::Button::new(skin::text(Role::Label, "revert all")),
                        )
                        .on_hover_text("every value as the scenario file has it")
                        .clicked()
                    {
                        draft.editing = draft.founding.clone();
                    }
                });
            });
        },
        |ui| {
            skin::section(ui, "what applying does", false);
            ui.label(skin::text(
                Role::Body,
                "A change is folded into the scenario at tick 0 and an intervention recorded \
                 after it, so the run still reproduces exactly and the timeline says when you \
                 did it. That is why applying is a button and not a keystroke: one intervention \
                 per keypress is a record nobody can read.",
            ));
            skin::section(ui, "value, and default", true);
            ui.label(skin::text(
                Role::Body,
                "The default column is what the scenario file says. A value that differs from \
                 it is marked, and the footer counts them — so a world that has drifted from \
                 the file describing it says so, rather than looking freshly loaded.",
            ));
            skin::section(ui, "the raw number is the truth", true);
            ui.label(skin::text(
                Role::Body,
                "The editable field is always the raw integer, because that is what the \
                 scenario holds and what somebody comparing two files will see. The unit \
                 column is the courtesy: 20480 is unreadable and 20.00 is obvious, but only \
                 one of them is what is written down.",
            ));
        },
    );

    if apply {
        let held = sim.engine.handle();
        held.slide().world_mut().set_biology(draft.editing.clone());
        draft.live = draft.editing.clone();
    }
    if apply_env {
        let held = sim.engine.handle();
        let mut slide = held.slide();
        let world = slide.world_mut();
        world.set_light(draft.env.light.clone());
        world.set_current(draft.env.current.clone());
        draft.env_live = draft.env.clone();
    }
    sim.draft = Some(draft);
}

/// The light falling on the slide and the water moving under it.
///
/// # Why this is not part of the intervention record, and why that is a problem
///
/// `set_biology` folds a change into the scenario at tick 0 and records an `Intervention`
/// after it, so `(scenario, seed, interventions)` still reproduces the run — `docs/UI.md` §4
/// chose that over refusing mid-run edits precisely so a world could be nudged and watched.
///
/// `set_light` and `set_current` write straight into the running scenario and record nothing,
/// so a light regime changed at tick 40,000 is indistinguishable on reload from one the
/// scenario always had. A `.mmslide` is unaffected — it carries the world's state and its
/// current scenario, so it resumes correctly — but replaying the original `.ron` no longer
/// reproduces the run. The barrier tools have had the same hole since M6.
///
/// Closing it means `Intervention` growing beyond `BiologyConfig`, which bumps the snapshot
/// format and touches the diff view that reconstructs "what changed". That is its own change;
/// this one says so in the interface rather than leaving it to be discovered.
fn environment_editor(ui: &mut egui::Ui, draft: &mut Draft, apply: &mut bool) {
    use mm_core::light::{CurrentField, LightRegime};

    ui.small(
        "The weather. Light is what enters the world and the current is what carries \
         everything through it — including cells, which drift with the water unless \
         something holds them.",
    );
    ui.add_space(4.0);

    let q10 = |v: &mut i32, ui: &mut egui::Ui, label: &str| {
        ui.add(
            egui::DragValue::new(v)
                .speed(8.0)
                .prefix(format!("{label} ")),
        );
    };

    ui.horizontal(|ui| {
        ui.label("light");
        let mut kind = light_kind(&draft.env.light);
        egui::ComboBox::from_id_salt("light regime")
            .selected_text(kind)
            .show_ui(ui, |ui| {
                for name in LIGHT_KINDS {
                    ui.selectable_value(&mut kind, name, name);
                }
            });
        if kind != light_kind(&draft.env.light) {
            draft.env.light = default_light(kind);
        }
    });
    ui.horizontal_wrapped(|ui| match &mut draft.env.light {
        LightRegime::Uniform { intensity } => q10(intensity, ui, "intensity"),
        LightRegime::DayNight {
            period_ticks,
            day,
            night,
        } => {
            ui.add(
                egui::DragValue::new(period_ticks)
                    .speed(16.0)
                    .prefix("period "),
            );
            q10(day, ui, "day");
            q10(night, ui, "night");
        }
        LightRegime::Directional { bright, dark, from } => {
            q10(bright, ui, "bright");
            q10(dark, ui, "dark");
            let mut which = edge_index(*from);
            egui::ComboBox::from_id_salt("bright edge")
                .selected_text(EDGES[which].0)
                .show_ui(ui, |ui| {
                    for (i, (name, _)) in EDGES.iter().enumerate() {
                        ui.selectable_value(&mut which, i, *name);
                    }
                });
            *from = EDGES[which.min(EDGES.len() - 1)].1;
        }
        LightRegime::PointSource {
            x,
            y,
            intensity,
            half_life_squares,
        } => {
            ui.add(egui::DragValue::new(x).prefix("x "));
            ui.add(egui::DragValue::new(y).prefix("y "));
            q10(intensity, ui, "intensity");
            ui.add(egui::DragValue::new(half_life_squares).prefix("half-life "));
        }
        LightRegime::SlowDecline {
            start,
            end,
            over_ticks,
        } => {
            q10(start, ui, "start");
            q10(end, ui, "end");
            ui.add(
                egui::DragValue::new(over_ticks)
                    .speed(1000.0)
                    .prefix("over "),
            );
        }
        LightRegime::Seasonal {
            day_ticks,
            year_ticks,
            summer_day,
            winter_day,
            night,
        } => {
            ui.add(egui::DragValue::new(day_ticks).speed(16.0).prefix("day "));
            ui.add(
                egui::DragValue::new(year_ticks)
                    .speed(1000.0)
                    .prefix("year "),
            );
            q10(summer_day, ui, "summer");
            q10(winter_day, ui, "winter");
            q10(night, ui, "night");
        }
    });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("current");
        let mut kind = current_kind(&draft.env.current);
        egui::ComboBox::from_id_salt("current field")
            .selected_text(kind)
            .show_ui(ui, |ui| {
                for name in CURRENT_KINDS {
                    ui.selectable_value(&mut kind, name, name);
                }
            });
        if kind != current_kind(&draft.env.current) {
            draft.env.current = default_current(kind);
        }
    });
    ui.horizontal_wrapped(|ui| match &mut draft.env.current {
        CurrentField::Still => {
            ui.weak("no flow");
        }
        CurrentField::Uniform { vx, vy } => {
            q10(vx, ui, "vx");
            q10(vy, ui, "vy");
        }
        CurrentField::Rotational { strength }
        | CurrentField::Shear { strength }
        | CurrentField::Convergent { strength } => q10(strength, ui, "strength"),
    });
    ui.small(format!(
        "velocity is Q10 squares per fluid step; the solver clamps at {} \
         (a quarter of a square).",
        mm_core::fixed::Q10_ONE / 4
    ));

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let dirty = draft.env != draft.env_live;
        if ui
            .add_enabled(dirty, egui::Button::new("apply environment"))
            .on_hover_text(
                "change the light and the flow on the running world. NOT recorded as an \
                 intervention — a slide file resumes correctly, but replaying the original \
                 scenario will not reproduce this run.",
            )
            .clicked()
        {
            *apply = true;
        }
        if ui
            .add_enabled(dirty, egui::Button::new("discard"))
            .clicked()
        {
            draft.env = draft.env_live.clone();
        }
        if dirty {
            ui.colored_label(egui::Color32::from_rgb(240, 200, 120), "not applied");
        } else {
            ui.weak("in force");
        }
    });
}

const LIGHT_KINDS: [&str; 6] = [
    "uniform",
    "day/night",
    "directional",
    "point source",
    "slow decline",
    "seasonal",
];

const CURRENT_KINDS: [&str; 5] = ["still", "uniform", "rotational", "shear", "convergent"];

const EDGES: [(&str, mm_core::light::Edge); 4] = [
    ("left", mm_core::light::Edge::Left),
    ("right", mm_core::light::Edge::Right),
    ("top", mm_core::light::Edge::Top),
    ("bottom", mm_core::light::Edge::Bottom),
];

fn edge_index(e: mm_core::light::Edge) -> usize {
    EDGES.iter().position(|(_, v)| *v == e).unwrap_or(0)
}

fn light_kind(r: &mm_core::light::LightRegime) -> &'static str {
    use mm_core::light::LightRegime as L;
    match r {
        L::Uniform { .. } => LIGHT_KINDS[0],
        L::DayNight { .. } => LIGHT_KINDS[1],
        L::Directional { .. } => LIGHT_KINDS[2],
        L::PointSource { .. } => LIGHT_KINDS[3],
        L::SlowDecline { .. } => LIGHT_KINDS[4],
        L::Seasonal { .. } => LIGHT_KINDS[5],
    }
}

fn current_kind(c: &mm_core::light::CurrentField) -> &'static str {
    use mm_core::light::CurrentField as C;
    match c {
        C::Still => CURRENT_KINDS[0],
        C::Uniform { .. } => CURRENT_KINDS[1],
        C::Rotational { .. } => CURRENT_KINDS[2],
        C::Shear { .. } => CURRENT_KINDS[3],
        C::Convergent { .. } => CURRENT_KINDS[4],
    }
}

/// A usable starting point for each variant, so choosing one from the list produces a world
/// that visibly does the thing rather than a set of zeroes that does nothing.
fn default_light(kind: &str) -> mm_core::light::LightRegime {
    use mm_core::light::{Edge, LightRegime as L};
    let full = mm_core::fixed::Q10_ONE;
    match kind {
        k if k == LIGHT_KINDS[1] => L::DayNight {
            period_ticks: 2_000,
            day: full,
            night: 0,
        },
        k if k == LIGHT_KINDS[2] => L::Directional {
            bright: full,
            dark: 0,
            from: Edge::Top,
        },
        k if k == LIGHT_KINDS[3] => L::PointSource {
            x: 32,
            y: 32,
            intensity: full,
            half_life_squares: 8,
        },
        k if k == LIGHT_KINDS[4] => L::SlowDecline {
            start: full,
            end: 0,
            over_ticks: 1_000_000,
        },
        k if k == LIGHT_KINDS[5] => L::Seasonal {
            day_ticks: 2_000,
            year_ticks: 100_000,
            summer_day: full,
            winter_day: full / 4,
            night: 0,
        },
        _ => L::Uniform { intensity: full },
    }
}

/// Half the solver's ceiling, which is brisk enough to see and slow enough that a cell with a
/// holdfast can still hold against it.
fn default_current(kind: &str) -> mm_core::light::CurrentField {
    use mm_core::light::CurrentField as C;
    let brisk = mm_core::fixed::Q10_ONE / 8;
    match kind {
        k if k == CURRENT_KINDS[1] => C::Uniform { vx: brisk, vy: 0 },
        k if k == CURRENT_KINDS[2] => C::Rotational { strength: brisk },
        k if k == CURRENT_KINDS[3] => C::Shear { strength: brisk },
        k if k == CURRENT_KINDS[4] => C::Convergent { strength: brisk },
        _ => C::Still,
    }
}

/// One labelled parameter: its value, its reading, and whether it has been moved.
/// How tall the parameter editor's footer is, reserved out of the table's height so that Apply
/// does not scroll away from the thing it applies.
const FOOTER_HEIGHT: f32 = 26.0;

/// The five columns a parameter is drawn in, and their widths.
///
/// `field`, `value`, `unit`, `default`, and the note — which takes whatever is left. The unit
/// and the default are the two the editor did not have: without a default there is no way to
/// see that a value has moved without remembering where it started, and without the note
/// visible the explanation is one hover at a time through fifty-one fields.
const PARAM_COLUMNS: [f32; 4] = [186.0, 84.0, 66.0, 70.0];

/// A cell of the parameter table, at an exact place.
///
/// Laid out from the row's left edge rather than by following the previous cell, and this is
/// the whole reason it exists. `allocate_ui_with_layout` hands back the rect its content
/// *used*, not the one it asked for, so a `DragValue` that sizes itself to `42` leaves the next
/// column starting thirty points to the left of where `8192` leaves it — and a table whose
/// columns move with the width of their contents is not a table. Absolute offsets cannot drift.
fn param_cell<R>(
    ui: &mut egui::Ui,
    row: egui::Rect,
    x: f32,
    width: f32,
    right: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let rect = egui::Rect::from_min_size(
        egui::pos2(row.left() + x, row.top()),
        egui::vec2(width, row.height()),
    );
    let layout = if right {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(rect).layout(layout),
        |ui| add(ui),
    )
    .inner
}

/// Where each column starts, from the row's left edge.
fn param_column_x(i: usize) -> f32 {
    PARAM_COLUMNS
        .iter()
        .take(i)
        .map(|w| w + theme::row::GUTTER)
        .sum()
}

/// One group's fields, as a table with a header that does not scroll away.
fn parameter_table(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    group: params::Group,
    chemicals: &[String],
) {
    let (row, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::row::HEIGHT),
        egui::Sense::hover(),
    );
    for (i, head) in ["field", "value", "unit", "default"].into_iter().enumerate() {
        param_cell(
            ui,
            row,
            param_column_x(i),
            PARAM_COLUMNS[i],
            i == 1 || i == 3,
            |ui| ui.label(skin::text(Role::Section, head)),
        );
    }
    param_cell(
        ui,
        row,
        param_column_x(4),
        row.width() - param_column_x(4),
        false,
        |ui| ui.label(skin::text(Role::Section, "what it does")),
    );
    skin::hairline(ui);

    egui::ScrollArea::vertical()
        .id_salt("parameters")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for field in params::group(group) {
                parameter_row(ui, draft, field, chemicals);
            }
        });
}

/// One parameter: what it is called, what it is set to, what that means, what the scenario said,
/// and what it does.
fn parameter_row(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    field: &params::Field,
    chemicals: &[String],
) {
    use mm_core::params::Value;

    let (row, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::row::HEIGHT),
        egui::Sense::hover(),
    );

    let Some(value) = mm_core::params::get(&draft.editing, field.path) else {
        param_cell(ui, row, 0.0, PARAM_COLUMNS[0], false, |ui| {
            ui.label(skin::text(Role::Label, field.label))
        });
        param_cell(ui, row, param_column_x(1), PARAM_COLUMNS[1], false, |ui| {
            ui.label(skin::moody(Role::Label, Mood::Bad, "unreadable"))
        });
        return;
    };

    // Marked when it differs from the file, so a world that has drifted from the scenario
    // describing it says so rather than looking freshly loaded. A warm ground and a warm left
    // edge as well as a warm number, because one coloured value in a column of fifty-one is a
    // thing you find by looking for it rather than a thing you notice.
    let founding = mm_core::params::get(&draft.founding, field.path);
    let moved = founding != Some(value);
    if moved {
        ui.painter().rect_filled(
            row,
            0.0,
            skin::col(Mood::Warn.rgb()).gamma_multiply(0.10),
        );
        ui.painter().rect_filled(
            egui::Rect::from_min_size(row.min, egui::vec2(2.0, row.height())),
            0.0,
            skin::col(Mood::Warn.rgb()),
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(row, 0.0, skin::col(theme::Ground::Sunk.rgb()));
    }

    param_cell(ui, row, 6.0, PARAM_COLUMNS[0] - 6.0, false, |ui| {
        ui.add(egui::Label::new(skin::text(Role::Label, field.label)).truncate())
    });

    let mut edited = None;
    param_cell(ui, row, param_column_x(1), PARAM_COLUMNS[1], true, |ui| {
        if let Value::Bool(b) = value {
            let mut b = b;
            if ui.checkbox(&mut b, "").changed() {
                edited = Some(Value::Bool(b));
            }
        } else {
            let mut v = value.as_int();
            // A tenth of the current magnitude per pixel, so a value of twenty thousand drags
            // in useful steps and a value of three does not leap past itself.
            let speed = (v.abs() as f64 / 100.0).max(1.0);
            if ui.add(egui::DragValue::new(&mut v).speed(speed)).changed() {
                edited = Some(Value::Int(v));
            }
        }
    });

    param_cell(ui, row, param_column_x(2), PARAM_COLUMNS[2], false, |ui| {
        ui.label(skin::text(
            Role::Small,
            field.reading(value, chemicals).unwrap_or_default(),
        ))
    });
    param_cell(ui, row, param_column_x(3), PARAM_COLUMNS[3], true, |ui| {
        match founding {
            // Only where it has moved. A default printed against every one of fifty-one
            // unchanged rows is a second column of the same numbers, and the eye stops reading
            // the column that is always the same as the one beside it.
            Some(was) if moved => ui.label(skin::text(
                Role::Label,
                field
                    .reading(was, chemicals)
                    .unwrap_or_else(|| was.as_int().to_string()),
            )),
            _ => ui.label(skin::text(Role::Label, "·")),
        }
    });
    let note_x = param_column_x(4);
    param_cell(ui, row, note_x, (row.width() - note_x).max(0.0), false, |ui| {
        ui.add(egui::Label::new(skin::text(Role::Small, field.note)).truncate())
    });
    response.on_hover_text(field.note);

    if let Some(value) = edited {
        // Refused rather than clamped when it does not fit — see `mm_core::params::set`. The
        // widget snaps back on the next frame, which is the honest thing for a value the
        // simulation could not have held.
        if let Some(next) = mm_core::params::set(&draft.editing, field.path, value) {
            draft.editing = next;
        }
    }
}

/// The metabolic pathways: one row per reaction, read left to right as the reaction itself.
fn pathway_grid(ui: &mut egui::Ui, draft: &mut Draft, chemicals: &[String]) {
    use mm_core::params::Value;

    egui::Grid::new("pathways")
        .num_columns(params::PATHWAY_COLUMNS.len() * 2 + 1)
        .striped(true)
        .show(ui, |ui| {
            ui.label("");
            for (_, heading) in params::PATHWAY_COLUMNS {
                ui.label(egui::RichText::new(heading).small().strong());
                ui.label("");
            }
            ui.end_row();

            for n in 0..mm_core::organelle::PATHWAY_COUNT {
                ui.label(format!("pathway {n}"));
                for (suffix, _) in params::PATHWAY_COLUMNS {
                    let path = format!("{}{n}.{suffix}", params::PATHWAY_PREFIX);
                    let Some(value) = mm_core::params::get(&draft.editing, &path) else {
                        ui.label("-");
                        ui.label("");
                        continue;
                    };
                    let mut v = value.as_int();
                    if ui.add(egui::DragValue::new(&mut v).speed(0.1)).changed() {
                        if let Some(next) =
                            mm_core::params::set(&draft.editing, &path, Value::Int(v))
                        {
                            draft.editing = next;
                        }
                    }
                    // The chemical's name beside its index. A table of bare numbers is a
                    // table nobody can read a reaction out of.
                    match usize::try_from(value.as_int())
                        .ok()
                        .and_then(|i| chemicals.get(i))
                    {
                        Some(name) => ui.weak(name),
                        None => ui.weak("?"),
                    };
                }
                ui.end_row();
            }
        });
}

/// The organelle catalogue: one row per slot type, one column per cost.
fn catalogue_grid(ui: &mut egui::Ui, draft: &mut Draft) {
    use mm_core::params::Value;

    egui::Grid::new("catalogue")
        .num_columns(params::CATALOGUE_COLUMNS.len() + 1)
        .striped(true)
        .show(ui, |ui| {
            ui.label("");
            for (_, heading) in params::CATALOGUE_COLUMNS {
                ui.label(egui::RichText::new(heading).small().strong());
            }
            ui.end_row();

            for slot in 0..mm_core::organelle::SLOT_COUNT {
                let kind = mm_core::OrganelleType::from_operand(slot as i16);
                ui.label(kind.name());
                for (suffix, _) in params::CATALOGUE_COLUMNS {
                    let path = format!("{}{slot}.{suffix}", params::CATALOGUE_PREFIX);
                    let Some(value) = mm_core::params::get(&draft.editing, &path) else {
                        ui.label("-");
                        continue;
                    };
                    let mut v = value.as_int();
                    let speed = (v.abs() as f64 / 100.0).max(1.0);
                    if ui.add(egui::DragValue::new(&mut v).speed(speed)).changed() {
                        if let Some(next) =
                            mm_core::params::set(&draft.editing, &path, Value::Int(v))
                        {
                            draft.editing = next;
                        }
                    }
                }
                ui.end_row();
            }
        });
}

/// What has been changed in this world, and when (M10.2).
///
/// The experiment log. A run is reproducible from `(scenario, seed)` — I1 — and a parameter
/// changed at tick forty thousand breaks that unless the change is part of the record. It is,
/// and this is the record: the hand reaching into the world is part of the world's history, in
/// the same way its extinctions are.
///
/// A view of the ecology pane rather than a window of its own, because it belongs beside the
/// timeline: the timeline marks *when* a parameter was changed, and this says *what* the change
/// was. Two windows to read one event is two windows.
fn interventions_view(ui: &mut egui::Ui, sim: &SlideRes) {
    let log = &sim.latest.interventions;
    if log.is_empty() {
        ui.weak("nothing has been changed in this world");
        ui.small(
            "Parameters you change while it is running are recorded here, so the run \
             still replays exactly from its scenario and seed.",
        );
        return;
    }
    ui.label(format!(
        "{} change{} since tick 0",
        log.len(),
        if log.len() == 1 { "" } else { "s" }
    ));
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("interventions")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Each entry holds the whole configuration, so what *changed* is the
            // difference from the one before it — or from the scenario, for the first.
            let mut previous = sim.latest.founding.clone();
            for step in log {
                ui.label(
                    egui::RichText::new(format!("tick {}", step.tick))
                        .strong()
                        .monospace(),
                );
                let before = mm_core::params::fields(&previous);
                let after = mm_core::params::fields(&step.biology);
                let mut said = false;
                for ((path, was), (_, now)) in before.iter().zip(after.iter()) {
                    if was == now {
                        continue;
                    }
                    said = true;
                    let label = params::describe(path).map_or(path.as_str(), |f| f.label);
                    // Monospace, because egui's bundled proportional font has no `→` and
                    // renders it as a missing-glyph box. The monospace one has it, and a
                    // before-and-after reads better in columns anyway.
                    ui.small(
                        egui::RichText::new(format!("    {label}:  {was} → {now}")).monospace(),
                    );
                }
                if !said {
                    ui.small("    (no parameter differed)");
                }
                previous = step.biology.clone();
            }
        });
}

/// The legend: what the colours on the slide mean, and what they are scaled against.
fn legend_body(ui: &mut egui::Ui, sim: &SlideRes, view: &View, frame: &Frame) {
    ui.horizontal(|ui| {
        ui.label(skin::text(Role::Section, "overlays"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // One button, both directions, labelled with the one it will do. "none" while
            // anything is showing, so it is always the way out of what you are looking at;
            // "all" only from a bare slide, where there is nothing to be surprised by.
            let mask = sim.engine.overlays();
            let (label, hint) = if mask == 0 {
                (
                    "all",
                    "every chemical at once — where there is anything at all",
                )
            } else {
                ("none", "bare slide, which is a reading too")
            };
            if ui.small_button(label).on_hover_text(hint).clicked() {
                sim.engine
                    .set_overlays(ui::toggle_all(mask, sim.chem_names.len()));
            }
            if ui
                .small_button("]")
                .on_hover_text("next, on its own")
                .clicked()
            {
                let n = sim.chem_names.len();
                sim.engine
                    .set_overlays(ui::step_solo(sim.engine.overlays(), n, 1));
            }
            if ui
                .small_button("[")
                .on_hover_text("previous, on its own")
                .clicked()
            {
                let n = sim.chem_names.len();
                sim.engine
                    .set_overlays(ui::step_solo(sim.engine.overlays(), n, -1));
            }
        });
    });

    // Every chemical, always, one click each.
    //
    // This was a read-only list of whatever happened to be on, and switching one meant
    // View ▸ Overlays ▸ item — three levels of menu, with the menu itself covering the part of
    // the plate you were trying to look at. The legend is already in the rail, already open, and
    // already knows the colours; making its rows the control removes the menu from the loop
    // rather than adding a second way to do the same thing.
    //
    // The peak is on the rows that are *on*, and it is not decoration: each layer is normalised
    // against its own maximum, so the colours are legible and meaningless without it.
    let peaks: std::collections::BTreeMap<usize, (i32, i64)> = frame
        .overlays
        .iter()
        .map(|l| (l.chemical, (l.peak, l.total)))
        .collect();
    for (c, name) in sim.chem_names.iter().enumerate() {
        let on = sim.engine.overlay_enabled(c);
        let rgb = sim.chem_colours.get(c).copied().unwrap_or([160, 160, 160]);
        ui.horizontal(|ui| {
            // Filled when it is on, outlined when it is not, so the state reads at a glance
            // down the column rather than needing the highlight to be noticed.
            skin::swatch(ui, rgb, on);
            let label = match peaks.get(&c) {
                Some((peak, _)) => {
                    format!("{name}   {:.1}", *peak as f32 / mm_core::Q10_ONE as f32)
                }
                None => name.clone(),
            };
            let hint = if c < 9 {
                format!(
                    "key {}. Click to toggle; [ and ] step one at a time.",
                    c + 1
                )
            } else {
                "Click to toggle; [ and ] step one at a time.".to_string()
            };
            if ui.selectable_label(on, label).on_hover_text(hint).clicked() {
                sim.engine.toggle_overlay(c);
            }
        });
    }
    if let Some(event) = &sim.last_tool {
        ui.separator();
        ui.small(format!("{} — {event:?}", view.tool.name()));
    }
}

/// The live metric plots.
fn metrics_body(ui: &mut egui::Ui, sim: &SlideRes) {
    skin::section(ui, "metrics", false);
    let history = &sim.latest.history;
    if history.is_empty() {
        ui.label(skin::text(Role::Small, "no samples yet"));
        return;
    }
    // One plotted series: its label, how to get its value out of a sample, and what colour it
    // is drawn in. The colour is a reading about the reading — income is good news, dissipation
    // is energy leaving — and it is what stops four lines in one rail from being four identical
    // green wobbles that have to be told apart by their captions.
    type Series<'a> = (&'a str, Box<dyn Fn(&Sample) -> i64>, theme::Rgb);
    let series: [Series; 4] = [
        (
            "population",
            Box::new(|s: &Sample| s.population as i64),
            skin::plot_neutral(),
        ),
        (
            "dissipation",
            Box::new(|s: &Sample| s.dissipation),
            Mood::Bad.rgb(),
        ),
        (
            "light income ‰",
            Box::new(|s: &Sample| s.trophic_light),
            Mood::Good.rgb(),
        ),
        (
            "distinct genomes",
            Box::new(|s: &Sample| s.distinct_genomes as i64),
            skin::plot_neutral(),
        ),
    ];
    for (name, pick, colour) in series {
        let s = history.series(pick);
        // The value on the same line as the name, right-aligned, so the column of current
        // readings can be scanned down without stopping at four plots on the way.
        skin::stat(ui, name, &s.values.last().copied().unwrap_or(0).to_string());
        skin::sparkline(ui, &s.normalised(), colour);
    }
    if let Some(latest) = history.latest() {
        ui.add_space(theme::SECTION_GAP);
        skin::hairline(ui);
        ui.add_space(4.0);
        skin::stat(
            ui,
            "fidelity",
            &format!(
                "{:.2}",
                latest.mean_fidelity as f32 / mm_core::Q10_ONE as f32
            ),
        );
        skin::stat(ui, "loadouts", &latest.distinct_loadouts.to_string());
        skin::stat(ui, "matter", &thousands(latest.total_matter));
    }
}

/// A big number with its thousands parted, because `80535715840` is not a number anybody reads
/// and `80 535 715 840` is.
///
/// A thin space rather than a comma: the readings either side of it are decimal, and a comma in
/// a column of decimals is a decimal point somewhere in the world.
fn thousands(v: i64) -> String {
    let digits = v.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if v < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{2009}');
        }
        out.push(c);
    }
    out
}

/// The genome of the selected cell, live (`g`).
///
/// Read-only by default and running: the highlighted line is where the cell's own instruction
/// pointer is right now, and it moves while you watch. "edit" hands the disassembly to the
/// editor, and applying it writes the new bytes back into the same living cell without
/// stopping the world — see [`tools::rewrite_genome`] for what happens to the machine.
fn genome_body(ui: &mut egui::Ui, sim: &mut SlideRes, view: &mut View) {
    let Some(c) = sim.reading().cloned() else {
        ui.weak("no cell selected — click one on the slide");
        return;
    };

    let here = {
        sim.listing.of(&c.genome, c.genome_hash, c.vm_config);
        sim.listing.line_at(c.ip)
    };
    let over_nucleus = c.nucleus_capacity > 0 && c.genome_len > c.nucleus_capacity;
    let mut apply: Option<Vec<u8>> = None;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(sim.latest.species.clone()).strong());
        ui.weak(format!("{:016x}", c.genome_hash));
        ui.separator();
        ui.label(format!(
            "{} bytes / nucleus {}",
            c.genome_len, c.nucleus_capacity
        ));
        if over_nucleus {
            ui.colored_label(
                egui::Color32::from_rgb(230, 130, 90),
                "⚠ truncated at division",
            )
            .on_hover_text(
                "SPEC §4.1: a genome longer than its nucleus is cut short in every \
                 daughter. The lineage stops without an error.",
            );
        }
    });

    ui.horizontal(|ui| {
        if ui
            .button("edit")
            .on_hover_text("load this genome into the editor")
            .clicked()
        {
            sim.editor
                .load_bytes(c.genome.bytes(), sim.latest.species.clone());
            sim.editing = Some(c.id);
            view.panels.set(Panel::Editor, true);
        }
        ui.separator();
        // Two documents, one pane. Reading is the default because it is what the pane is for:
        // source is the form you switch to when you are about to edit, and the editor is the
        // only thing that consumes it.
        for (label, reading, hint) in [
            (
                "reading",
                true,
                "templates resolved to what they mean — immediates in decimal, jumps to the \
                 offset they reach, promoters to their gene",
            ),
            (
                "source",
                false,
                "the reassemblable form, byte for byte. This is the text the editor hands back.",
            ),
        ] {
            if ui
                .selectable_label(view.genome_reading == reading, label)
                .on_hover_text(hint)
                .clicked()
            {
                view.genome_reading = reading;
            }
        }
        ui.separator();
        ui.checkbox(&mut view.genome_follow_ip, "follow ip")
            .on_hover_text("scroll to the pointer when it moves");
        if sim.editing == Some(c.id) {
            let built = sim.editor.build().bytes().map(|b| b.to_vec());
            let ready = built.is_some() && !sim.editor.is_dirty();
            if ui
                .add_enabled(ready, egui::Button::new("apply to this cell"))
                .on_hover_text(
                    "replace this cell's genome and let it carry on running. Its body, \
                     chemistry and position are untouched; a division in progress is \
                     abandoned.",
                )
                .clicked()
            {
                apply = built;
            }
            if !ready {
                ui.weak("assemble in the editor first");
            }
        }
    });

    // Scroll when the pointer has moved to a line we have not already followed it to — not on
    // every frame it is on screen.
    let chase = view.genome_follow_ip && view.genome_scrolled_to != Some(c.ip);

    ui.add_space(2.0);
    skin::hairline(ui);

    // The genes and the diagnostics go in the column (UI.md §8.6): what the listing is made of
    // and whether it is sound are both things about the listing that the listing cannot say
    // about itself.
    let genes = gene_spans(&c.genome);
    let genome_len = c.genome_len;
    skin::drawer_split(
        ui,
        "genome_genes",
        |ui| {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (n, line) in sim.listing.lines().iter().enumerate() {
                let current = here == Some(n);
                ui.horizontal(|ui| {
                    // The pointer, in the margin, so the eye has one column to follow rather
                    // than hunting for a highlight.
                    ui.label(
                        egui::RichText::new(if current { "▶" } else { " " })
                            .monospace()
                            .color(egui::Color32::from_rgb(140, 230, 140)),
                    );
                    let (text, spans) = if view.genome_reading {
                        (&line.reading, &line.reading_spans)
                    } else {
                        (&line.text, &line.text_spans)
                    };
                    // The line the pointer is on keeps its ink and takes a background, rather
                    // than being flattened to one bright colour. The `▶` in the margin already
                    // says which line it is; throwing the colours away to say it again would
                    // make the one line you are watching the only unreadable one.
                    let backing = if current {
                        egui::Color32::from_rgb(45, 70, 45)
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &format!("{:>4}  ", line.offset),
                        0.0,
                        listing_ink(egui::Color32::from_gray(110), backing),
                    );
                    for span in spans {
                        let Some(part) = text.get(span.start..span.end) else {
                            continue;
                        };
                        job.append(
                            part,
                            0.0,
                            listing_ink(ink_colour(span.ink, current), backing),
                        );
                    }
                    // The operand column ends where the label column begins, and a listing
                    // whose second column wanders is one nobody can scan down.
                    let pad = 30usize.saturating_sub(text.chars().count());
                    job.append(
                        &" ".repeat(pad),
                        0.0,
                        listing_ink(egui::Color32::PLACEHOLDER, backing),
                    );
                    ui.label(job);
                    // Only beside the source form. In the reading the binding *is* the
                    // operand, and printing it twice on one line is noise.
                    if let (false, Some(label)) = (view.genome_reading, &line.label) {
                        ui.label(
                            egui::RichText::new(label)
                                .monospace()
                                .size(11.0)
                                .color(egui::Color32::from_rgb(150, 175, 215)),
                        );
                    }
                });
                if current && chase {
                    ui.scroll_to_cursor(Some(egui::Align::Center));
                }
            }
        });
        },
        |ui| {
            skin::section(ui, "genes", false);
            if genes.is_empty() {
                ui.label(skin::text(
                    Role::Small,
                    "no GENE headers. Every byte of this genome is one straight run, which the \
                     VM executes from offset zero and wraps.",
                ));
            }
            let longest = genes.iter().map(|(_, n)| *n).max().unwrap_or(1).max(1);
            for (name, bytes) in &genes {
                skin::row(
                    ui,
                    name,
                    Some((*bytes as f32 / longest as f32, skin::plot_neutral())),
                    &format!("{bytes} B"),
                    Role::Value.ink().unwrap_or(theme::DIM),
                );
            }

            skin::section(ui, "diagnostics", true);
            skin::stat(ui, "bytes", &format!("{genome_len} B"));
            skin::stat(ui, "nucleus", &c.nucleus_capacity.to_string());
            skin::stat(ui, "genes", &genes.len().to_string());
            if over_nucleus {
                ui.label(skin::moody(
                    Role::Body,
                    Mood::Bad,
                    "Longer than its nucleus, so every daughter gets it cut short. The lineage \
                     will stop without an error (SPEC §4.1).",
                ));
            } else {
                ui.label(skin::text(
                    Role::Body,
                    "Fits its nucleus, so it copies whole into every daughter.",
                ));
            }
        },
    );
    if chase {
        view.genome_scrolled_to = Some(c.ip);
    }

    if let Some(bytes) = apply {
        let held = sim.engine.handle();
        let event = tools::rewrite_genome(held.slide().world_mut(), c.id, bytes);
        sim.last_tool = Some(event);
    }
}

/// Each gene's name and how many bytes it runs for.
///
/// A gene runs from its own `GENE` header to the next one, and the last runs to the end of the
/// genome — which is what the VM does with it, since execution simply carries on. Named through
/// `inspector::gene_label` so that this list and the listing beside it cannot disagree about
/// which gene is which.
fn gene_spans(genome: &mm_core::Genome) -> Vec<(String, u32)> {
    let promoters = genome.promoters();
    let end = genome.len() as u32;
    promoters
        .iter()
        .enumerate()
        .map(|(nth, p)| {
            let next = promoters
                .get(nth + 1)
                .map_or(end, |q| u32::from(q.offset));
            (
                mm_app::inspector::gene_label(nth),
                next.saturating_sub(u32::from(p.offset)),
            )
        })
        .collect()
}

/// Where the pointer is, in substrate squares. `None` if there is no window or no cursor.
///
/// Measured from the centre of the *viewport*, not of the window. With a rail open those are
/// not the same place, and using the window's centre would put every click a rail's-width
/// off — worse the wider the panel.
fn pointer_on_slide(
    window: &Query<&Window, With<PrimaryWindow>>,
    view: &View,
    scale: f32,
) -> Option<(f32, f32)> {
    let w = window.single().ok()?;
    let cursor = w.cursor_position()?;
    let (cx, cy) = view.viewport.centre();
    Some((
        view.centre.x + (cursor.x - cx) / scale,
        view.centre.y + (cursor.y - cy) / scale,
    ))
}

/// The cell inspector (M4, extended for tracking and the genome listing).
///
/// Everything drawn here comes from an [`Inspection`], which is a copy — the panel holds no
/// borrow of the world and there is no path from a click in it back to a tick.
fn cell_body(ui: &mut egui::Ui, sim: &mut SlideRes, view: &mut View) {
    let Some(c) = sim.reading().cloned() else {
        ui.weak("click a cell to inspect it");
        return;
    };

    let q10 = |v: i32| v as f32 / mm_core::Q10_ONE as f32;
    let chem_names = sim.chem_names.clone();

    // --- who and where ---
    skin::section(ui, "cell", false);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(sim.latest.species.clone())
                .italics()
                .size(15.0)
                .color(skin::col(Role::Value.ink().unwrap_or(theme::DIM))),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if skin::chip(ui, "track", None, view.follow)
                .on_hover_text("keep the camera centred on this cell (t)")
                .clicked()
            {
                view.follow = !view.follow;
            }
        });
    });
    ui.label(skin::text(
        Role::Label,
        format!(
            "age {} · born {} · ({:.1}, {:.1})",
            c.age,
            c.birth_tick,
            c.x as f32 / mm_core::fixed::POS_ONE as f32,
            c.y as f32 / mm_core::fixed::POS_ONE as f32
        ),
    ));

    // --- the readings that say whether it is doing well ---
    //
    // 400, 120 and 40 are reference points and not ceilings — energy and mass have no maximum —
    // so the bar clamps at them and the number beside it is what is actually true.
    ui.add_space(6.0);
    skin::row(
        ui,
        "energy",
        Some((q10(c.energy) / 400.0, Mood::Warn.rgb())),
        &format!("{:.1}", q10(c.energy)),
        Mood::Warn.rgb(),
    );
    let ink = Role::Value.ink().unwrap_or(theme::DIM);
    skin::row(
        ui,
        "mass",
        Some((q10(c.mass) / 120.0, ink)),
        &format!("{:.1}", q10(c.mass)),
        // No mood: mass being high is not good news or bad news, it is just mass. The design
        // gives this bar a blue that is the accent in all but name, and the accent means
        // selection. A reading with nothing to say about itself is drawn in ink.
        ink,
    );
    let hurt = if c.damage > 0 {
        Mood::Bad.rgb()
    } else {
        theme::DIM
    };
    skin::row(
        ui,
        "damage",
        Some((q10(c.damage) / 40.0, hurt)),
        &format!("{:.1}", q10(c.damage)),
        hurt,
    );

    ui.add_space(theme::SECTION_GAP);
    skin::hairline(ui);

    // --- the schematic ---
    let placed = mm_app::inspector::placements(&c.slots);
    ui.horizontal(|ui| {
        ui.label(skin::text(Role::Section, "loadout"));
        ui.label(skin::text(
            Role::Label,
            format!("{} of {} slots", placed.len(), c.slots.len()),
        ));
    });
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 150.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    let centre = rect.center();
    let r = (rect.height() / 2.0 - 8.0).max(8.0);
    // The membrane: the circle everything else lives inside.
    painter.circle_stroke(centre, r, egui::Stroke::new(1.5, skin::col(theme::RULE)));
    for p in &placed {
        let at = centre + egui::vec2(p.dx * r, p.dy * r);
        let rgb = mm_app::slide::organelle_rgb(p.kind);
        let colour = egui::Color32::from_rgba_unmultiplied(
            (rgb[0] * 255.0) as u8,
            (rgb[1] * 255.0) as u8,
            (rgb[2] * 255.0) as u8,
            (255.0 * p.built) as u8,
        );
        painter.circle_filled(at, p.radius * r, colour);
        if response
            .hover_pos()
            .is_some_and(|h| h.distance(at) <= p.radius * r)
        {
            let slot = c.slots[p.slot];
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                egui::Id::new("organelle"),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| {
                ui.label(format!("slot {}: {}", slot.index, slot.kind.name()));
                ui.label(format!("param {}", slot.param));
                ui.label(format!("control {:?}", slot.control));
                // Which reaction it runs (M10.3). Only the three organelles that run one:
                // on anything else `control[1]` means something else or nothing, and
                // labelling it a pathway would be a confident lie.
                if matches!(
                    slot.kind,
                    OrganelleType::Mitochondrion
                        | OrganelleType::Chloroplast
                        | OrganelleType::Lysosome
                ) {
                    let n = mm_core::organelle::MetabolicChemistry::pathway_index(slot.control[1]);
                    ui.label(format!("pathway {n}"))
                        .on_hover_text("which metabolic reaction this one runs");
                }
                if let Some(n) = slot.remaining_build {
                    ui.weak(format!("building, {n} to go"));
                }
            });
        }
    }
    if placed.is_empty() {
        painter.text(
            centre,
            egui::Align2::CENTER_CENTER,
            "bare membrane",
            skin::font(Role::Small),
            skin::col(theme::DIM),
        );
    }

    ui.add_space(theme::SECTION_GAP);
    skin::hairline(ui);
    egui::ScrollArea::vertical().show(ui, |ui| {
        // --- the genome, with the cell's own instruction pointer in it ---
        let over = c.nucleus_capacity > 0 && c.genome_len > c.nucleus_capacity;
        skin::section(ui, "machine", true);
        skin::stat(ui, "genome", &format!("{} B", c.genome_len));
        skin::stat(ui, "nucleus", &c.nucleus_capacity.to_string());
        skin::stat(ui, "ip", &c.ip.to_string());
        ui.horizontal(|ui| {
            if over {
                ui.label(skin::moody(Role::Label, Mood::Bad, "⚠ truncated at division"))
                .on_hover_text(
                    "SPEC §4.1: a genome longer than its nucleus is cut short in every \
                             daughter. The lineage will stop without an error.",
                );
            }
        });
        ui.horizontal(|ui| {
            match c.fidelity {
                Some(f) => ui.label(skin::text(Role::Label, format!("fidelity {:.2}", q10(f)))),
                // Not "fidelity 0.00". A cell with no nucleus has no fidelity, cannot
                // copy its genome and cannot divide — which is a different and much
                // louder fact than copying badly.
                None => ui.label(skin::moody(
                    Role::Label,
                    Mood::Bad,
                    "no nucleus — cannot divide",
                )),
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if c.halted {
                    ui.label(skin::moody(Role::Label, Mood::Warn, "halted"));
                } else {
                    ui.label(skin::moody(Role::Label, Mood::Good, "running"));
                }
            });
        });

        if skin::chip(ui, "open genome ▸", Some("G"), false)
            .on_hover_text("the disassembly, live, in the drawer (g)")
            .clicked()
        {
            view.panels.set(Panel::Genome, true);
        }

        // --- what it is holding ---
        //
        // Every chemical it has any of, in the row grammar, against the largest of them — so
        // "a lot of sugar and a trace of peroxide" is a shape rather than two numbers to
        // compare in your head.
        skin::section(ui, "interior chemistry", true);
        let peak = c.interior.iter().copied().max().unwrap_or(0).max(1);
        let mut any = false;
        for (i, v) in c.interior.iter().enumerate() {
            if *v == 0 {
                continue;
            }
            any = true;
            let rgb = sim.chem_colours.get(i).copied().unwrap_or([160, 160, 160]);
            skin::row(
                ui,
                chem_names.get(i).map_or("?", String::as_str),
                // The bar in the chemical's own colour, the number in ink. Some of those
                // colours are very dark — `carbon` is `#464650` — and a reading you cannot
                // read is not a reading.
                Some((*v as f32 / peak as f32, rgb)),
                &format!("{:.2}", q10(*v)),
                Role::Value.ink().unwrap_or(theme::DIM),
            );
        }
        if !any {
            ui.label(skin::text(Role::Small, "nothing in it"));
        }

        ui.collapsing(skin::text(Role::Label, "registers and stacks"), |ui| {
            ui.label(skin::text(Role::Label, format!("stack {:?}", c.stack)));
            ui.label(skin::text(Role::Label, format!("calls {:?}", c.call_stack)));
            if c.ln > 0 {
                ui.label(skin::text(
                    Role::Label,
                    format!("copying: {} bytes to go, from {} to {}", c.ln, c.pa, c.pb),
                ));
            }
            let live: Vec<String> = c
                .registers
                .iter()
                .enumerate()
                .filter(|(_, v)| **v != 0)
                .map(|(n, v)| format!("r{n}={v}"))
                .collect();
            ui.label(skin::text(
                Role::Label,
                if live.is_empty() {
                    "registers all zero".to_string()
                } else {
                    live.join("  ")
                },
            ));
            let ram: Vec<String> = c
                .ram
                .iter()
                .enumerate()
                .filter(|(_, v)| **v != 0)
                .map(|(n, v)| format!("[{n}]={v}"))
                .collect();
            ui.label(skin::text(
                Role::Label,
                if ram.is_empty() {
                    "ram all zero".to_string()
                } else {
                    ram.join("  ")
                },
            ));
        });
    });
}

/// Refresh the ecology pane's copy of the world's history, if it needs it.
///
/// The one place this pane reaches into the world. Everything the three views draw comes out of
/// here, so the lock is taken once every couple of seconds rather than three times a frame.
fn refresh_ecology(sim: &mut SlideRes, view: &View) {
    let now = sim.latest.frame.tick;
    let wanted = view.species;
    let fresh = sim.ecology.as_ref().is_some_and(|e| {
        e.showing == wanted && now.saturating_sub(e.at) < ECOLOGY_STALE_AFTER && now >= e.at
    });
    if fresh {
        return;
    }

    let meddles = intervention_summaries(sim);
    let held = sim.engine.handle();
    let slide = held.slide();
    let world = slide.world();
    let archive = world.archive();
    let showing = wanted.or_else(|| wiki::notable(archive, 1).first().copied());
    let gathered = EcologyView {
        at: now,
        showing: wanted,
        tree: wiki::layout(archive),
        timeline: wiki::timeline_with(
            archive,
            world.events().events(),
            &meddles,
            world.tick_count(),
        ),
        page: showing.and_then(|id| wiki::page(archive, id)),
        species: archive.len(),
        living: archive.living(),
    };
    drop(slide);
    sim.ecology = Some(gathered);
}

/// The ecology pane: the tree of life, the food web and the timeline (M10.4).
///
/// One pane rather than two panels, sharing one selection. "Where did this come from" and "what
/// is it eating" are one question, and answering them on opposite sides of the screen made it
/// two.
fn ecology_body(ui: &mut egui::Ui, sim: &mut SlideRes, view: &mut View) {
    refresh_ecology(sim, view);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for which in Ecology::ALL {
            if skin::chip(ui, which.title(), None, view.ecology == which).clicked() {
                view.ecology = which;
            }
        }
        if view.ecology == Ecology::Tree {
            ui.separator();
            ui.add(
                egui::Slider::new(&mut view.tree_floor, 0..=200)
                    .text("hide peaks under")
                    .logarithmic(true),
            )
            .on_hover_text(
                "a long run makes thousands of species, most of them one cell that divided \
                 twice. Drawing every one turns the tree into a solid block.",
            );
        }
    });
    ui.separator();

    // The view on the left, the selected species on the right. The page is what makes the
    // three views one pane rather than three.
    let page_width = (ui.available_width() * 0.34).clamp(220.0, 420.0);
    let view_width = (ui.available_width() - page_width - 12.0).max(160.0);
    let height = ui.available_height();
    ui.horizontal_top(|ui| {
        // `vertical` inside each column, and it is not decoration. `horizontal_top` puts its
        // children in a left-to-right layout and `allocate_ui` *inherits* that — so without it
        // every widget in a column lays out beside the last one rather than under it. It put
        // the species heading and its description on one line running off the window, and
        // strung the food web's summary, graph and edge list across the screen in a row.
        ui.allocate_ui(egui::vec2(view_width, height), |ui| {
            ui.vertical(|ui| match view.ecology {
                Ecology::Tree => tree_view(ui, sim, view),
                Ecology::Web => foodweb_body(ui, sim),
                Ecology::Timeline => timeline_view(ui, sim, view),
                Ecology::Interventions => interventions_view(ui, sim),
                Ecology::Budget => budget_view(ui, sim),
            });
        });
        ui.separator();
        ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
            ui.vertical(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("species_page")
                    .auto_shrink([false, false])
                    .show(ui, |ui| species_page(ui, sim, view));
            });
        });
    });
}

/// The phylogenetic tree, painted.
///
/// Horizontal is time, so the shape of the chart means something: a burst of divergence is a
/// fan, a long quiet lineage is a straight line, and a mass extinction is a wall of branches
/// stopping at the same tick. It was an indented text list until M10.4 — the data was right and
/// the presentation was a footnote.
fn tree_view(ui: &mut egui::Ui, sim: &SlideRes, view: &mut View) {
    let Some(eco) = sim.ecology.as_ref() else {
        return;
    };
    if eco.tree.nodes.is_empty() {
        ui.weak("nothing has lived here yet");
        return;
    }
    let (tree, now, total, living) = (&eco.tree, eco.at, eco.species, eco.living);

    let plot = wiki::plot(tree, now, view.tree_floor);
    ui.small(format!(
        "{total} species, {living} alive — {} drawn",
        plot.branches.len()
    ));

    let by_id: std::collections::BTreeMap<_, _> = tree.nodes.iter().map(|n| (n.id, n)).collect();

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Tall enough that every drawn branch gets a few pixels of its own, so a thousand
            // species scrolls rather than collapsing into one line.
            let height = (plot.branches.len() as f32 * 9.0).clamp(80.0, 40_000.0);
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().max(120.0), height),
                egui::Sense::click(),
            );
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(110));

            let inset = 8.0;
            let at = |x: f32, y: f32| -> egui::Pos2 {
                egui::pos2(
                    rect.left() + inset + (rect.width() - inset * 2.0) * x,
                    rect.top() + inset + (rect.height() - inset * 2.0) * y,
                )
            };

            // Forks first, under the branches: a divergence is a joint, not a thing.
            for fork in &plot.forks {
                let a = at(fork.x, fork.y_parent);
                let b = at(fork.x, fork.y_child);
                painter.line_segment([a, b], egui::Stroke::new(1.0, egui::Color32::from_gray(80)));
            }

            let mut hovered: Option<&wiki::TreeNode> = None;
            for branch in &plot.branches {
                let Some(node) = by_id.get(&branch.id) else {
                    continue;
                };
                let a = at(branch.x0, branch.y);
                let b = at(branch.x1, branch.y);
                let [r, g, bl] = node.guild.rgb();
                // Extinct lineages fade. They still carry the shape of the tree, so they are
                // drawn — but the eye should find what is alive first.
                let dim = if branch.alive { 1.0 } else { 0.45 };
                let selected = view.species == Some(branch.id);
                let width = 1.0 + branch.weight * 5.0;
                painter.line_segment(
                    [a, b],
                    egui::Stroke::new(
                        if selected { width + 2.0 } else { width },
                        egui::Color32::from_rgb(
                            (r * 255.0 * dim) as u8,
                            (g * 255.0 * dim) as u8,
                            (bl * 255.0 * dim) as u8,
                        ),
                    ),
                );
                // A tick where it ended, so an extinction is a full stop rather than a line
                // that happens to be short.
                if !branch.alive {
                    painter.line_segment(
                        [b + egui::vec2(0.0, -3.0), b + egui::vec2(0.0, 3.0)],
                        egui::Stroke::new(1.0, egui::Color32::from_gray(120)),
                    );
                }
                if let Some(p) = response.hover_pos() {
                    if (p.y - a.y).abs() <= 4.0 && p.x >= a.x - 4.0 && p.x <= b.x + 4.0 {
                        hovered = Some(node);
                    }
                }
            }

            if let Some(node) = hovered {
                if response.clicked() {
                    view.species = Some(node.id);
                }
                egui::Tooltip::always_open(
                    ui.ctx().clone(),
                    ui.layer_id(),
                    egui::Id::new("branch"),
                    egui::PopupAnchor::Pointer,
                )
                .show(|ui| {
                    ui.label(egui::RichText::new(&node.name).strong());
                    ui.small(node.guild.label());
                    ui.small(format!(
                        "founded {}  ·  peak {}",
                        node.founded_tick, node.peak_population
                    ));
                    match node.extinct_tick {
                        Some(t) => ui.small(format!("extinct at {t}")),
                        None => ui.small(format!("{} alive now", node.population)),
                    };
                });
            }
        });
}

/// The timeline, full width and scrubbable (M10.4).
///
/// Was twenty-six pixels in the corner of the wiki. What it is for is comparing *when* things
/// happened — including when somebody changed a parameter, which is why interventions are on
/// the same axis and in their own colour.
fn timeline_view(ui: &mut egui::Ui, sim: &SlideRes, view: &mut View) {
    let Some(timeline) = sim.ecology.as_ref().map(|e| &e.timeline) else {
        return;
    };

    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 54.0),
        egui::Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(110));
    let x_of = |at: u32| rect.left() + rect.width() * (at as f32 / 1000.0);

    for entry in &timeline.entries {
        let x = x_of(entry.at);
        painter.line_segment(
            [
                egui::pos2(x, rect.top() + 4.0),
                egui::pos2(x, rect.bottom() - 14.0),
            ],
            egui::Stroke::new(1.5, egui::Color32::from_rgb(220, 180, 110)),
        );
    }
    // Interventions in their own colour, below the events, because one is something that
    // happened and the other is something somebody did.
    for meddle in &timeline.meddles {
        let x = x_of(meddle.at);
        painter.line_segment(
            [
                egui::pos2(x, rect.bottom() - 16.0),
                egui::pos2(x, rect.bottom() - 2.0),
            ],
            egui::Stroke::new(2.0, egui::Color32::from_rgb(120, 190, 240)),
        );
    }

    // Scrubbing. The world cannot be rewound — nothing keeps past states — so this moves a
    // cursor and selects what was happening there, which is the honest version of the gesture.
    if let Some(p) = response.interact_pointer_pos() {
        let at = (((p.x - rect.left()) / rect.width().max(1.0)) * 1000.0).clamp(0.0, 1000.0);
        view.scrub = Some(at as u32);
        if let Some(entry) = timeline.nearest(at as u32) {
            view.species = Some(entry.species);
        }
    }
    if let Some(at) = view.scrub {
        let x = x_of(at);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(1.0, egui::Color32::from_gray(230)),
        );
    }

    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("timeline_entries")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if timeline.entries.is_empty() && timeline.meddles.is_empty() {
                ui.weak("nothing has happened yet");
                return;
            }
            // Nearest the cursor first when scrubbing, newest first otherwise.
            let mut rows: Vec<(u64, String, Option<mm_core::phylogeny::SpeciesId>)> = timeline
                .entries
                .iter()
                .map(|e| {
                    (
                        e.tick,
                        format!("{} ({})", e.headline, e.species_name),
                        Some(e.species),
                    )
                })
                .chain(
                    timeline
                        .meddles
                        .iter()
                        .map(|m| (m.tick, format!("you changed {}", m.summary), None)),
                )
                .collect();
            match view.scrub {
                Some(at) => {
                    let want = (at as u64 * timeline.span) / 1000;
                    rows.sort_by_key(|(tick, _, _)| tick.abs_diff(want));
                }
                None => rows.sort_by(|a, b| b.0.cmp(&a.0)),
            }
            for (tick, text, species) in rows.into_iter().take(60) {
                let label = format!("tick {tick:>9} — {text}");
                if species.is_some() {
                    if ui.selectable_label(false, label).clicked() {
                        view.species = species;
                    }
                } else {
                    ui.small(
                        egui::RichText::new(label).color(egui::Color32::from_rgb(120, 190, 240)),
                    );
                }
            }
        });
}

/// What each intervention changed, in a phrase, for the timeline.
fn intervention_summaries(sim: &SlideRes) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut previous = sim.latest.founding.clone();
    for step in &sim.latest.interventions {
        let before = mm_core::params::fields(&previous);
        let after = mm_core::params::fields(&step.biology);
        let changed: Vec<String> = before
            .iter()
            .zip(after.iter())
            .filter(|((_, was), (_, now))| was != now)
            .map(|((path, was), (_, now))| {
                let label = params::describe(path).map_or(path.as_str(), |f| f.label);
                format!("{label} {was} → {now}")
            })
            .collect();
        out.push((
            step.tick,
            match changed.len() {
                0 => "nothing".to_string(),
                1 => changed[0].clone(),
                n => format!("{} and {} more", changed[0], n - 1),
            },
        ));
        previous = step.biology.clone();
    }
    out
}

/// One species, as the wiki tells it (M5, SPEC §10.5).
///
/// Reads the archive through [`mm_app::wiki`], which copies everything out — so this holds no
/// borrow of the world and nothing in it can reach a tick.
fn species_page(ui: &mut egui::Ui, sim: &SlideRes, view: &mut View) {
    let Some(page) = sim.ecology.as_ref().and_then(|e| e.page.as_ref()) else {
        ui.weak("nothing has lived here yet");
        return;
    };

    // A species name is a binomial and is set in italics, which is the one typographic
    // convention this application inherits from outside itself.
    ui.label(
        egui::RichText::new(&page.name)
            .italics()
            .size(15.0)
            .color(skin::col(Role::Value.ink().unwrap_or(theme::DIM))),
    );
    ui.label(skin::text(Role::Body, &page.description));

    ui.add_space(theme::SECTION_GAP);
    skin::stat(ui, "founded", &thousands(page.founded_tick as i64));
    skin::stat(ui, "births", &thousands(page.births as i64));
    skin::stat(ui, "deaths", &thousands(page.deaths as i64));
    skin::stat(ui, "depth", &page.depth.to_string());

    if let Some((id, name)) = &page.parent {
        if ui.link(skin::text(Role::Label, format!("diverged from {name}"))).clicked() {
            view.species = Some(*id);
        }
    }
    if !page.children.is_empty() {
        skin::section(ui, format!("{} descendants", page.children.len()).as_str(), true);
        for (id, name) in page.children.iter().take(8) {
            if ui.link(skin::text(Role::Label, name)).clicked() {
                view.species = Some(*id);
            }
        }
    }

    skin::section(ui, "population", true);
    ui.label(skin::text(Role::Label, format!("peak {}", page.curve_peak)));
    let values: Vec<f32> = page.curve.iter().map(|(_, v)| *v).collect();
    skin::sparkline(ui, &values, Mood::Good.rgb());

    skin::section(
        ui,
        format!("founder genome · {} B", page.founder_genome.len()).as_str(),
        true,
    );
    ui.label(skin::text(
        Role::Label,
        format!("{:016x}", page.fingerprint),
    ));
    let hex: String = page
        .founder_genome
        .iter()
        .take(64)
        .map(|b| format!("{b:02x}"))
        .collect();
    ui.label(skin::text(Role::Small, hex).monospace());
}

/// The food web (M8).
///
/// Nodes are stacked by trophic level and edges are drawn as arrows whose width is the matter
/// that actually went along them over the last window. Edges the engine measured exactly are
/// solid; edges whose total is known but whose attribution is not are dashed, so the panel
/// never presents arithmetic as observation.
fn foodweb_body(ui: &mut egui::Ui, sim: &SlideRes) {
    use mm_app::foodweb::{Basis, Node};

    let web = sim.latest.web.clone();
    let peak = web.peak() as f32;

    ui.horizontal(|ui| {
        ui.label(skin::text(Role::Body, web.summary()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(skin::text(
                Role::Label,
                format!("averaged over {} ticks", web.window_ticks),
            ));
        });
    });
    ui.add_space(4.0);

    let (rect, _) = ui.allocate_exact_size(
        // Four trophic levels want room. Below this the bands are twenty points apart and
        // every edge comes out near-horizontal, which says nothing about who is above whom.
        egui::vec2(ui.available_width(), (ui.available_height() - 34.0).max(230.0)),
        egui::Sense::hover(),
    );
    let painter = ui.painter();

    // Levels run 0..=3 with carrion on top; put level 0 at the bottom of the rect so
    // the picture reads the way a food pyramid does.
    let place = |node: Node| -> egui::Pos2 {
        let across = match node {
            Node::Light | Node::Producers | Node::Scavengers => 0.25,
            Node::Dissolved | Node::Osmotrophs | Node::Predators => 0.75,
            Node::Carrion => 0.5,
        };
        let up = node.level() as f32 / 3.0;
        egui::pos2(
            rect.left() + rect.width() * across,
            rect.bottom() - 20.0 - (rect.height() - 44.0) * up,
        )
    };

    // Every chip's rectangle, before any edge is drawn, because an edge has to stop at the
    // boundary of the box it points at and cannot know where that is until the box exists.
    //
    // This is the fix for the picture that was there: edges ran centre to centre and the chips
    // were painted over them afterwards, so a line vanished under its own node and reappeared
    // in mid-air on the far side, which reads as an arrow that misses. Nothing was wrong with
    // the numbers; the drawing was.
    let label_of = |node: Node, count: u32| -> String {
        if node.is_source() {
            node.label().to_string()
        } else {
            format!("{} {}", node.label(), count)
        }
    };
    let mut boxes: std::collections::BTreeMap<Node, egui::Rect> = std::collections::BTreeMap::new();
    for occ in &web.nodes {
        let galley = painter.layout_no_wrap(
            label_of(occ.node, occ.count),
            skin::font(Role::Label),
            skin::col(Role::Value.ink().unwrap_or(theme::DIM)),
        );
        boxes.insert(
            occ.node,
            egui::Rect::from_center_size(place(occ.node), galley.size() + egui::vec2(14.0, 7.0)),
        );
    }

    for edge in &web.edges {
        if edge.weight <= 0 {
            continue;
        }
        let (Some(from), Some(to)) = (boxes.get(&edge.from), boxes.get(&edge.to)) else {
            continue;
        };
        let width = 1.0 + 5.0 * (edge.weight as f32 / peak).clamp(0.0, 1.0);
        let colour = if edge.is_recycling() {
            Mood::Good.rgb()
        } else if edge.is_death() {
            Mood::Bad.rgb()
        } else {
            skin::plot_neutral()
        };
        let (a, b) = (from.center(), to.center());
        let dir = (b - a).normalized();
        let start = edge_of(*from, dir, 3.0);
        let head = 7.0 + width * 0.5;
        let tip = edge_of(*to, -dir, 4.0);
        let stop = tip - dir * head;
        let stroke = egui::Stroke::new(width, skin::col(colour));

        if edge.basis == Basis::Measured {
            painter.line_segment([start, stop], stroke);
        } else {
            // Dashed, because the total is measured but who it belongs to is not.
            let steps = 9;
            for k in (0..steps).step_by(2) {
                let t0 = k as f32 / steps as f32;
                let t1 = (k + 1) as f32 / steps as f32;
                painter.line_segment([start.lerp(stop, t0), start.lerp(stop, t1)], stroke);
            }
        }
        // The head, so an edge says which way the matter went. A food web without arrows is a
        // diagram of who is *near* whom.
        let barb = egui::vec2(-dir.y, dir.x) * head * 0.46;
        painter.add(egui::Shape::convex_polygon(
            vec![tip, stop + barb, stop - barb],
            skin::col(colour),
            egui::Stroke::NONE,
        ));
        // And the weight on the edge rather than in a list underneath it — on a ground of its
        // own, because a number drawn straight onto a four-pixel line comes out as `61▲00`.
        // The design achieves this with a stroked outline; egui has no text stroke, so the
        // ground is a rectangle and the effect is the same.
        let galley = painter.layout_no_wrap(
            thousands(edge.weight / mm_core::Q10_ONE as i64),
            skin::font(Role::Label),
            skin::col(Role::Label.ink().unwrap_or(theme::DIM)),
        );
        // Beside the edge, not on it. A near-vertical edge between two levels is thirty points
        // long once it has been clipped to both chips, and a plate in the middle of that covers
        // the whole arrow — so the number is pushed off along the perpendicular, away from the
        // centre of the picture so it does not land on the edge running parallel beside it.
        let across = egui::vec2(-dir.y, dir.x);
        let outward = if (rect.center().x - start.x).signum() == across.x.signum() {
            -1.0
        } else {
            1.0
        };
        let mid = start.lerp(stop, 0.5) + across * outward * 11.0;
        let plate = egui::Rect::from_center_size(mid, galley.size() + egui::vec2(6.0, 2.0));
        painter.rect_filled(plate, 2.0, skin::col(theme::Ground::Panel.rgb()));
        painter.galley(plate.center() - galley.size() / 2.0, galley, egui::Color32::WHITE);
    }

    for occ in &web.nodes {
        let Some(box_rect) = boxes.get(&occ.node) else {
            continue;
        };
        let fill = if occ.node.is_source() {
            theme::Ground::Slide.rgb()
        } else {
            theme::Ground::Raised.rgb()
        };
        painter.rect_filled(*box_rect, 3.0, skin::col(fill));
        painter.rect_stroke(
            *box_rect,
            3.0,
            egui::Stroke::new(1.0, skin::col(theme::RULE)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            box_rect.center(),
            egui::Align2::CENTER_CENTER,
            label_of(occ.node, occ.count),
            skin::font(Role::Label),
            skin::col(Role::Value.ink().unwrap_or(theme::DIM)),
        );
    }

    // What the four kinds of line mean. The list of edges that used to sit here said the same
    // thing in words, one row per edge, directly under a picture that had just said it.
    skin::hairline(ui);
    ui.add_space(3.0);
    ui.horizontal_wrapped(|ui| {
        for (colour, dashed, what) in [
            (skin::plot_neutral(), false, "measured"),
            (skin::plot_neutral(), true, "shared out by population"),
            (Mood::Bad.rgb(), false, "death"),
            (Mood::Good.rgb(), false, "recycled"),
        ] {
            let (swatch, _) = ui.allocate_exact_size(egui::vec2(24.0, 8.0), egui::Sense::hover());
            let y = swatch.center().y;
            let stroke = egui::Stroke::new(if dashed { 2.0 } else { 4.0 }, skin::col(colour));
            if dashed {
                for k in [0.0f32, 0.5] {
                    ui.painter().line_segment(
                        [
                            egui::pos2(swatch.left() + swatch.width() * k, y),
                            egui::pos2(swatch.left() + swatch.width() * (k + 0.35), y),
                        ],
                        stroke,
                    );
                }
            } else {
                ui.painter()
                    .line_segment([egui::pos2(swatch.left(), y), egui::pos2(swatch.right(), y)], stroke);
            }
            ui.label(skin::text(Role::Label, what));
            ui.add_space(6.0);
        }
    });
}

/// Where a ray from a box's centre leaves the box, plus a little clearance.
///
/// The arithmetic that makes an arrow touch what it points at. Walking out along the direction
/// until whichever axis reaches its half-extent first is the box's boundary; without it every
/// edge is drawn from centre to centre and then buried under the chips painted on top.
fn edge_of(rect: egui::Rect, dir: egui::Vec2, pad: f32) -> egui::Pos2 {
    let half = rect.size() / 2.0;
    let tx = if dir.x.abs() < 1e-6 {
        f32::INFINITY
    } else {
        (half.x + pad) / dir.x.abs()
    };
    let ty = if dir.y.abs() < 1e-6 {
        f32::INFINITY
    } else {
        (half.y + pad) / dir.y.abs()
    };
    rect.center() + dir * tx.min(ty)
}

/// The genome editor (M6).
///
/// Syntax highlighting comes from `mm_asm::highlight`, which classifies against the real
/// opcode table, and diagnostics come from actually assembling — so neither can drift from the
/// language the way a second, approximate definition in the front-end would.
fn editor_body(ui: &mut egui::Ui, sim: &mut SlideRes) {
    ui.horizontal(|ui| {
        ui.label(skin::text(Role::Label, "name"));
        ui.add(
            egui::TextEdit::singleline(&mut sim.editor.name)
                .desired_width(180.0)
                .font(skin::font(Role::Value)),
        );
        if skin::chip(ui, "assemble", None, false).clicked() {
            sim.editor.assemble();
        }
        if skin::chip(ui, "export", None, false).clicked() {
            sim.last_export = sim.editor.export().map(|f| f.to_text());
        }
        if skin::chip(ui, "from selected cell", None, false).clicked() {
            if let Some(cell) = sim.selected {
                let held = sim.engine.handle();
                let file = tools::copy_genome(held.slide().world(), cell);
                if let Some(file) = file {
                    sim.editor.load_bytes(&file.bytes, file.name);
                }
            }
        }
    });
    ui.label(skin::text(Role::Label, sim.editor.status()));
    ui.add_space(2.0);
    skin::hairline(ui);
    ui.add_space(3.0);

    // Diagnostics first: they are why anyone opened this panel.
    let mut source = sim.editor.source().to_string();
    let errors: Vec<(u32, u32, String)> = sim
        .editor
        .build()
        .errors()
        .iter()
        .map(|e| (e.line, e.col, e.message.clone()))
        .collect();
    // Which source lines are wrong, one-based as the assembler numbers them, so the layouter
    // below can mark them where you are actually looking rather than only in a list.
    let bad_lines: std::collections::BTreeSet<u32> = errors.iter().map(|(l, _, _)| *l).collect();

    // Beside the buffer rather than above it (UI.md §8.6). Above, a long list of errors pushed
    // the very line you were fixing off the bottom of the pane — the panel is at its most
    // useful exactly when it has the most to say, which is when it took the most room away.
    let quoted: Vec<(u32, u32, String, String)> = {
        let lines: Vec<&str> = source.lines().collect();
        errors
            .iter()
            .map(|(line, col, message)| {
                (
                    *line,
                    *col,
                    message.clone(),
                    lines
                        .get(line.saturating_sub(1) as usize)
                        .map(|t| t.trim().to_string())
                        .unwrap_or_default(),
                )
            })
            .collect()
    };
    // "Assembles clean" is a claim, and an empty buffer that has never been assembled has not
    // earned it. Said only where there are bytes to say it about.
    let assembled = sim.editor.build().bytes().map(<[u8]>::len);
    let diagnostics = move |ui: &mut egui::Ui| {
        skin::section(ui, "diagnostics · live", false);
        match (quoted.is_empty(), assembled) {
            (true, Some(bytes)) => {
                ui.label(skin::moody(
                    Role::Label,
                    Mood::Good,
                    format!("assembles clean · {bytes} B"),
                ));
            }
            (true, None) => {
                ui.label(skin::text(Role::Label, "nothing assembled yet"));
            }
            (false, _) => {}
        }
        for (line, col, message, text) in &quoted {
            // A warm left edge and the position in mono, so a list of six errors is six things
            // rather than six sentences.
            ui.horizontal(|ui| {
                let (edge, _) = ui.allocate_exact_size(egui::vec2(2.0, 30.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(edge, 0.0, skin::col(Mood::Bad.rgb()));
                ui.vertical(|ui| {
                    ui.label(skin::moody(Role::Label, Mood::Bad, format!("{line}:{col} {message}")));
                    // The offending line, quoted. `3:12` means counting to line three
                    // otherwise, and counting to line three is the part nobody enjoys.
                    if !text.is_empty() {
                        ui.label(skin::text(Role::Label, text));
                    }
                });
            });
        }
        ui.add_space(theme::SECTION_GAP);
        ui.label(skin::text(
            Role::Body,
            "The buffer reassembles on every keystroke, so the line numbers in an error always \
             point at the text as it is now.",
        ));
    };

    // The source, highlighted as you type.
    //
    // egui 0.35 takes a `layouter` on `TextEdit`, which lays the buffer out through a closure
    // of our own — so the colours and the caret come from one widget rather than from a
    // styled copy sitting beside an editable one. That was the arrangement here before, and
    // the two halves scrolled independently.
    //
    // The lexer is `mm_asm::highlight`, the assembler's own, so the editor cannot disagree
    // with the language about what an opcode is. Called on the widget's live text rather than
    // through `Editor::highlight`, which reads the last text handed to `set_source` and is
    // therefore one keystroke behind.
    skin::drawer_split(
        ui,
        "editor_diagnostics",
        |ui| {
    egui::ScrollArea::vertical()
        .id_salt("source")
        .show(ui, |ui| {
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
                let mut job = egui::text::LayoutJob::default();
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                for (n, text) in buf.as_str().split('\n').enumerate() {
                    if n > 0 {
                        job.append(
                            "\n",
                            0.0,
                            source_ink(&font, egui::Color32::PLACEHOLDER, None),
                        );
                    }
                    // A line the assembler rejected, tinted behind its text. The message is
                    // in the list above; this is what says *there*.
                    let wrong = bad_lines.contains(&(n as u32 + 1));
                    let backing = wrong.then(|| egui::Color32::from_rgb(70, 34, 34));
                    for token in mm_asm::highlight::line(text) {
                        let Some(part) = text.get(token.start..token.end) else {
                            continue;
                        };
                        job.append(
                            part,
                            0.0,
                            source_ink(&font, token_colour(token.kind), backing),
                        );
                    }
                }
                job.wrap.max_width = wrap_width;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let response = ui.add(
                egui::TextEdit::multiline(&mut source)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(18)
                    .layouter(&mut layouter),
            );
            if response.changed() {
                sim.editor.set_source(source.clone());
                // Assembled on every change rather than only on the button. Diagnostics that
                // describe the text as it was several edits ago are worse than none: they
                // point at line numbers that have since moved. The button stays, because it
                // is also how you find out that nothing is wrong.
                sim.editor.assemble();
            }
        });

            if let Some(text) = &sim.last_export {
                skin::hairline(ui);
                ui.label(skin::text(Role::Label, "exported — copy this:"));
                let mut shown = text.clone();
                ui.add(
                    egui::TextEdit::multiline(&mut shown)
                        .code_editor()
                        .desired_rows(4),
                );
            }
        },
        diagnostics,
    );
}

/// The debugger (M6).
///
/// Breakpoints act on the viewer; instruction stepping acts on a sandbox. Neither can reach
/// the simulation — see `debugger.rs` for why that is structural rather than careful.
fn debugger_body(ui: &mut egui::Ui, sim: &mut SlideRes) {
    // --- breakpoints, over the live world ---
    ui.label("breakpoints");
    if let Some(tripped) = sim.breakpoints.tripped() {
        ui.colored_label(
            egui::Color32::from_rgb(240, 200, 120),
            format!("stopped: {}", tripped.describe()),
        );
        if ui.button("continue").clicked() {
            sim.breakpoints.rearm();
            // At the speed you were running at when it tripped, not at 1×: a breakpoint set
            // while watching something slowly is set *because* you were watching it slowly.
            sim.engine.unpause();
        }
    }
    let tick = sim.latest.frame.tick;
    ui.horizontal(|ui| {
        if ui.button("+1,000 ticks").clicked() {
            sim.breakpoints.add(Breakpoint::AtTick(tick + 1_000));
        }
        if ui.button("on death").clicked() {
            if let Some(cell) = sim.selected {
                sim.breakpoints.add(Breakpoint::CellDies(cell));
            }
        }
        if ui.button("clear").clicked() {
            sim.breakpoints.clear();
        }
    });
    let listed: Vec<String> = sim
        .breakpoints
        .iter()
        .map(|(p, on)| format!("{} {}", if on { "●" } else { "○" }, p.describe()))
        .collect();
    for text in listed {
        ui.small(text);
    }
    ui.separator();

    // --- the sandbox, for instruction stepping ---
    ui.label("sandbox");
    let world_tick = sim.latest.frame.tick;
    if ui.button("take from selected cell").clicked() {
        let held = sim.engine.handle();
        let taken = sim
            .selected
            .and_then(|cell| Sandbox::of(held.slide().world(), cell));
        sim.sandbox = taken;
    }
    let Some(sandbox) = sim.sandbox.as_mut() else {
        ui.weak("select a cell and take a copy");
        return;
    };
    let behind = world_tick.saturating_sub(sandbox.taken_at_tick);
    if behind > 0 {
        // Said plainly, so nobody reads a sandbox as the live cell.
        ui.colored_label(
            egui::Color32::from_rgb(200, 180, 120),
            format!("a copy, taken {behind} ticks ago — the live cell has moved on"),
        );
    }
    ui.horizontal(|ui| {
        if ui.button("step").clicked() {
            sandbox.step();
        }
        if ui.button("step tick").clicked() {
            sandbox.step_tick();
        }
        if ui.button("×16").clicked() {
            for _ in 0..16 {
                sandbox.step();
            }
        }
    });
    ui.label(format!(
        "ip {}  next {}  ran {} ({}/{} this tick){}",
        sandbox.vm.ip,
        sandbox
            .next_op()
            .map_or("-".to_string(), |op| op.name().to_string()),
        sandbox.executed,
        sandbox.in_tick,
        sandbox.budget(),
        if sandbox.vm.halted { "  HALTED" } else { "" }
    ));
    ui.separator();
    ui.label("stack (top last)");
    ui.small(format!("{:?}", unwound(&sandbox.vm)));
    ui.collapsing("registers", |ui| {
        ui.small(format!("{:?}", sandbox.vm.regs));
    });
    ui.collapsing("ram", |ui| {
        ui.small(format!("{:?}", sandbox.vm.ram));
    });
    ui.collapsing("disassembly", |ui| {
        let listing = mm_asm::disassemble(sandbox.genome.bytes());
        let here = sandbox.vm.ip as u32;
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .id_salt("disasm")
            .show(ui, |ui| {
                for line in &listing.lines {
                    let marker = if line.offset == here { "▶ " } else { "  " };
                    ui.small(
                        egui::RichText::new(format!(
                            "{marker}{:>5}  {}",
                            line.offset,
                            line.to_source()
                        ))
                        .monospace(),
                    );
                }
            });
    });
    ui.small("world-facing reads return zero in a sandbox: there is nothing to eat.");
}

/// The data stack in push order, top last — the same unwinding the inspector does.
fn unwound(vm: &mm_core::vm::Vm) -> Vec<i16> {
    let live = (vm.dlen as usize).min(mm_core::vm::DATA_STACK_LEN);
    (0..live)
        .rev()
        .filter_map(|back| {
            let at = (vm.dsp as usize).wrapping_sub(back) % mm_core::vm::DATA_STACK_LEN;
            vm.data.get(at).copied()
        })
        .collect()
}

