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
//!   File  Slide  Simulation  View  Tools  Help        ⏸ ▶ 1× 8× 256× ⏭
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
//! | `space` | pause / resume |
//! | `.` | step one tick |
//! | `0` `-` `=` `backspace` | speed: paused, 1×, 8×, as fast as it will go |
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

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::diagnostic::{Diagnostic, DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::image::ImageSampler;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
// 0.17 split the renderer into `bevy_mesh`, `bevy_shader`, `bevy_camera` and
// `bevy_sprite_render`. Nothing below changed but where it lives.
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, Mesh2d, MeshVertexAttribute, MeshVertexBufferLayoutRef};
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, PrimitiveTopology, RenderPipelineDescriptor,
    SpecializedMeshPipelineError, TextureDimension, TextureFormat, VertexFormat,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{Material2d, Material2dKey, Material2dPlugin, MeshMaterial2d};
use bevy::window::PrimaryWindow;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};

use mm_app::art;
use mm_app::cellmesh;
use mm_app::debugger::{Breakpoint, Breakpoints, Sandbox};
use mm_app::editor::Editor;
use mm_app::engine::{Engine, Published, Rate};
use mm_app::inspector::Inspection;
use mm_app::params;
use mm_app::slide::{self, Frame, Lod, Slide};
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
/// `MM_SHOT_AFTER` is in frames, so there is a world to photograph rather than sixteen
/// ancestors; `MM_SHOT_ZOOM` and `MM_SHOT_FLAT` set the camera and the cell style up front,
/// because a run that exits after one photograph has nobody to drive it.
///
/// **It photographs the slide and not the panels.** Bevy 0.15 moved where the screenshot is
/// taken relative to the egui pass, so what comes out is the render this crate is responsible
/// for and none of the interface drawn over it. That is the half worth photographing anyway,
/// and the panels prove themselves a different way: `ctx.available_rect()` shrinking to the
/// viewport is the layout having happened.
fn screenshot(
    mut commands: Commands,
    mut sim: ResMut<SlideRes>,
    mut view: ResMut<View>,
    mut exit: MessageWriter<AppExit>,
    mut frames: Local<u32>,
    mut done: Local<Option<u32>>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    *frames += 1;
    if *frames == 1 {
        // The bench replaces the world, so it has to happen before the run rather than one
        // frame before the photograph like the panels do — the point of it is what the cells
        // settle into, and at tick zero they have not settled into anything.
        if std::env::var("MM_SHOT_BENCH").is_ok() {
            sim.bench();
            // From the world, not from `latest`: the engine publishes frames on its own
            // schedule, so the newest one still describes the slide that was just replaced.
            let (w, h) = {
                let held = sim.engine.handle();
                let slide = held.slide();
                let s = slide.world().substrate();
                (s.width(), s.height())
            };
            view.centre = Vec2::new(w as f32 / 2.0, h as f32 / 2.0);
        }
        if let Ok(zoom) = std::env::var("MM_SHOT_ZOOM") {
            view.zoom = zoom.parse().unwrap_or(view.zoom);
        }
        if std::env::var("MM_SHOT_FLAT").is_ok() {
            view.rounded = false;
        }
    }
    // Arranged one frame before the photograph rather than at startup, so a panel that needs a
    // selection has a populated world to select from.
    let after: u32 = std::env::var("MM_SHOT_AFTER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    if *frames + 1 == after {
        if let Ok(spec) = std::env::var("MM_SHOT_VIEW") {
            arrange(&spec, &mut sim, &mut view);
        }
    }
    let Ok(path) = std::env::var("MM_SHOT") else {
        return;
    };
    // Taken, and now waiting to be written. The save is asynchronous — the observer fires when
    // the image has come back off the GPU — so quitting the same frame would race it and
    // produce no file. A few frames' grace, then out.
    if let Some(taken_at) = *done {
        if frames.saturating_sub(taken_at) > 10 {
            exit.write(AppExit::Success);
        }
        return;
    }
    if *frames < after {
        return;
    }
    // An entity with an observer since 0.15, rather than a manager resource. Failure is the
    // observer's problem and is reported by it: a missing directory should not take down a run
    // that is otherwise doing its job.
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
    // `MM_SHOT` is a batch tool: it exists so a change can be photographed from a script, and
    // a window that sits open afterwards waiting to be killed by a `timeout` is a window
    // somebody has to close. It quits itself once the file is on disk.
    *done = Some(*frames);
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
            "ecology" => {
                view.panels.set(Panel::Ecology, true);
                view.ecology = match sub {
                    "web" => Ecology::Web,
                    "timeline" => Ecology::Timeline,
                    _ => Ecology::Tree,
                };
            }
            "editor" => view.panels.set(Panel::Editor, true),
            "debugger" => view.panels.set(Panel::Debugger, true),
            "params" => view.panels.set(Panel::Parameters, true),
            "interventions" => {
                view.panels.set(Panel::Ecology, true);
                view.ecology = Ecology::Interventions;
            }
            "bare" => {
                view.panels.metrics = false;
                view.panels.legend = false;
            }
            "nocells" => view.organelles = false,
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

/// Pixels per substrate square at zoom 1.
const BASE_SCALE: f32 = 8.0;

fn main() {
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
    load_internal_asset!(app, CELL_SHADER, "cell.wgsl", Shader::from_wgsl);
    app.add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(Material2dPlugin::<CellMaterial>::default())
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

/// The parameter editor's state while it is open.
struct Draft {
    /// What will be applied.
    editing: BiologyConfig,
    /// What the world is running on now, so "changed" can be shown and reverted.
    live: BiologyConfig,
    /// What the scenario says, so a value that has drifted from the file can be marked.
    founding: BiologyConfig,
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
        let mut slide = Slide::new(petri()).expect("default scenario");
        seed_ancestors(&mut slide);
        let chem_names = slide.chemical_names();
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
        let held = self.engine.handle();
        seed_ancestors(&mut held.slide());
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
        width: 48,
        height: 48,
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
        gravity: 2,
        // No thermal motion. The bench's premise is that whatever moves is volumes resolving
        // against each other, and the default 24 breaks it: every cell wanders about a
        // sixteenth of a square per tick for no reason to do with packing, which in a sheet of
        // cells with sharp shared walls is every boundary in the picture redrawing every frame.
        // Biology was zeroed here when the bench was built; this was missed because it lives in
        // the physics rather than in a rate.
        jitter: 0,
        seeding: vec![],
        ..Scenario::default()
    };
    *slide.world_mut() = mm_core::World::new(scenario).expect("packing scenario");

    // Nothing lives, nothing dies, nothing grows. Every rate that could change a cell is zero,
    // so the population is a constant and the only thing left in motion is geometry.
    let mut biology = BiologyConfig {
        mutation: MutationRates::none(),
        ..BiologyConfig::default()
    };
    biology.metabolism.rates.background_damage = 0;
    biology.metabolism.rates.metabolic_floor = 0;
    biology.ecology.crowding_damage = 0;
    biology.ecology.spike_damage = 0;
    slide.world_mut().set_biology(biology);

    let world = slide.world_mut();
    // One `HALT`. Not the ancestor with its organelles left off — an actual genome that does
    // nothing, so there is no chance of a cell here reaching into the world and no need to
    // wonder whether it did.
    let Ok(inert) = world
        .genomes()
        .intern(vec![mm_core::Op::Halt.canonical_byte()])
    else {
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
        let across = 15;
        let x = pos(15) + (k % across) as i32 * (mm_core::fixed::POS_ONE * 5 / 4);
        let y = pos(15) + (k / across) as i32 * (mm_core::fixed::POS_ONE * 5 / 4);
        let size = 18 + (k * 7 % 26) as i32;
        let id = world.spawn_cell(CellSeed {
            x,
            y,
            mass: q10(size),
            // Enough that nothing starves in the time anybody watches.
            energy: q10(1_000_000),
            membrane: 24,
            key: 11,
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
fn seed_ancestors(slide: &mut Slide) {
    *slide.world_mut() = mm_core::World::new(petri()).expect("default scenario");
    slide.world_mut().set_biology(BiologyConfig {
        mutation: MutationRates::default(),
        ..BiologyConfig::default()
    });
    let Some(bytes) = ancestor_genome() else {
        return;
    };
    {
        let world = slide.world_mut();
        let structural = world.biology().structural_chemical;
        for k in 0..16u32 {
            let Ok(genome) = world.genomes().intern(bytes.clone()) else {
                continue;
            };
            // A four-by-four grid spread across whatever size the slide is, rather than at
            // fixed coordinates that huddled in one corner the moment the slide grew.
            //
            // Spread on purpose, and not only to look even. Sixteen founders far enough apart
            // to grow independently for a while are sixteen experiments; sixteen founders in a
            // heap are one, because they interbreed and compete from the first tick. Standing
            // diversity is the thing the crowding bound costs, and this is some of it back.
            let size = slide_size() as i32;
            let cell_of = |n: u32| pos(size * (2 * n as i32 + 1) / 8);
            let id = world.spawn_cell(CellSeed {
                x: cell_of(k % 4),
                y: cell_of(k / 4),
                mass: q10(30),
                energy: q10(400),
                membrane: 24,
                key: 11,
                species: 0,
                parent: CellId::NONE,
                birth_tick: 0,
                genome,
            });
            if let Some(i) = world.cells_mut().index(id) {
                let cells = world.cells_mut();
                // The organelles its build gene would otherwise take many ticks to afford, so
                // there is something metabolising to look at straight away.
                cells.slots_mut(i)[1] = Organelle::finished(OrganelleType::Nucleus, 40);
                cells.slots_mut(i)[2] = Organelle::finished(OrganelleType::Mitochondrion, 50);
                cells.slots_mut(i)[3] = Organelle::finished(OrganelleType::Chloroplast, 60);
                // Including build material. Without it a seeded cell can never build anything
                // — every `BUILD` is silently skipped for want of structural matter — so the
                // slide would run its four given organelles forever and differentiation would
                // look like a genome that does not work.
                cells.interior_mut(i)[structural] = q10(200);
                cells.interior_mut(i)[11] = q10(40);
                cells.interior_mut(i)[14] = q10(40);
            }
        }
    }
    // Filling a cytoplasm by hand creates matter, which is what scenario setup is for.
    slide.world_mut().adopt_current_contents_as_baseline();
}

/// How wide and tall the default slide is, in substrate squares.
///
/// Quadruple the area the microscope opened on for its first ten milestones, which was 96. A
/// population that fills its slide in a thousand ticks has nothing left to do but subdivide, and
/// what is interesting about this project needs somewhere for a lineage to go that is not
/// already occupied — a frontier to spread into, a corner to be isolated in, room for two
/// strategies to be tried at once without immediately meeting.
///
/// Note that this is four times the *matter* as well as four times the room, because seeding is
/// per square: the carrying capacity scales with it rather than the crowd merely thinning out.
///
/// `MM_SLIDE=<n>` overrides it, for trying a size without a rebuild. The slide has been a
/// scenario field all along — `width` and `height` in every `.ron` — and this is only what the
/// app opens on when nobody has said otherwise.
const DEFAULT_SLIDE: u32 = 192;

/// The slide size to open on, from `MM_SLIDE` or [`DEFAULT_SLIDE`].
///
/// Clamped rather than trusted. A zero-square slide has nowhere to put a cell and fails
/// scenario validation, and the upper bound is where the substrate stops being something a
/// machine can diffuse sixty times a second.
fn slide_size() -> u32 {
    std::env::var("MM_SLIDE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(DEFAULT_SLIDE)
        .clamp(16, 1024)
}

/// The default slide: light, food, no flow. The habitat the ancestor was written for.
fn petri() -> Scenario {
    Scenario {
        name: "petri".to_string(),
        seed: 1,
        width: slide_size(),
        height: slide_size(),
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
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm");
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
    /// Which species page is open.
    species: Option<mm_core::phylogeny::SpeciesId>,
    /// Which of the ecology pane's three views is showing (M10.4).
    ecology: Ecology,
    /// Hide species whose peak population never reached this. A long run makes thousands of
    /// them, most one cell that divided twice.
    tree_floor: u32,
    /// Where the timeline's cursor is, in permille, or `None` when nobody has scrubbed.
    scrub: Option<u32>,
}

impl Default for View {
    fn default() -> Self {
        View {
            centre: Vec2::new(48.0, 48.0),
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
            species: None,
            ecology: Ecology::Tree,
            tree_floor: 2,
            scrub: None,
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
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Move => "move",
            Tool::Remove => "remove",
            Tool::DrawBarrier => "wall",
            Tool::EraseBarrier => "erase",
        }
    }
}

#[derive(Component)]
struct MoteSprite(usize);

/// A junction, drawn as a thin sprite stretched between two cells (M7).
#[derive(Component)]
struct JunctionSprite(usize);

/// The per-cell data the shader reads, beyond position, corner and colour.
///
/// The id is arbitrary but must be stable and must not collide with Bevy's own attributes,
/// which is what the large number is for.
const ATTRIBUTE_SHAPE: MeshVertexAttribute =
    MeshVertexAttribute::new("CellShape", 0x6D_6D_5F_63_65_6C_6C, VertexFormat::Float32x4);

/// Which way each of a cell's four seams faces, packed two 16-bit snorms per component.
const ATTRIBUTE_SQUASH_DIR: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashDir",
    0x6D_6D_5F_63_65_6C_6D,
    VertexFormat::Float32x4,
);

/// How far along each of those the seam sits.
const ATTRIBUTE_SQUASH_FACE: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashFace",
    0x6D_6D_5F_63_65_6C_6E,
    VertexFormat::Float32x4,
);

/// Seam directions 4..7.
const ATTRIBUTE_SQUASH_DIR2: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashDir2",
    0x6D_6D_5F_63_65_6C_6F,
    VertexFormat::Float32x4,
);

/// How far along seams 4..7 they sit.
const ATTRIBUTE_SQUASH_FACE2: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashFace2",
    0x6D_6D_5F_63_65_6C_70,
    VertexFormat::Float32x4,
);

/// Seams 8..11, for the cells a packed sheet presses on from every side.
///
/// Eight was called headroom over the six a monolayer settles on, and it was not: once the
/// neighbour search covered a cell's real neighbourhood, cells routinely found nine or ten, and
/// a cell that runs out of slots stops cutting for a neighbour that is still cutting for it —
/// which draws as five clean shared walls and one side simply overlapping.
const ATTRIBUTE_SQUASH_DIR3: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashDir3",
    0x6D_6D_5F_63_65_6C_71,
    VertexFormat::Float32x4,
);

/// How far along seams 8..11 they sit.
const ATTRIBUTE_SQUASH_FACE3: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashFace3",
    0x6D_6D_5F_63_65_6C_72,
    VertexFormat::Float32x4,
);

/// How much the cell was grown to keep its area, so the shader can hand it back at the seams.
///
/// A bare `Float32`: four bytes a vertex rather than the forty-eight a fifth `vec4` would cost,
/// and `CellShape` has no spare component.
const ATTRIBUTE_SWELL: MeshVertexAttribute =
    MeshVertexAttribute::new("CellSwell", 0x6D_6D_5F_63_65_6C_73, VertexFormat::Float32);

/// Embedded at compile time rather than loaded from an `assets/` directory, so the binary runs
/// from anywhere. The same thing `bevy_sprite` does for its own shaders.
const CELL_SHADER: Handle<Shader> = uuid_handle!("6d6d5f63-656c-6c5f-7368-616465720001");

/// The whole population, drawn by one material with one shader (M10.5).
///
/// No bindings: everything that varies rides in the vertex attributes, because it varies *per
/// cell* and a material's uniforms are per draw call — and the entire point is that there is one
/// draw call. See `cellmesh.rs` for why a mesh rather than instancing, and `cell.wgsl` for the
/// field itself.
#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
struct CellMaterial {}

impl Material2d for CellMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(CELL_SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(CELL_SHADER)
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The locations here are the locations in `cell.wgsl`, and the two have to agree
        // exactly — a mismatch is a validation failure at draw time with nothing to say which
        // end is wrong.
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
            ATTRIBUTE_SHAPE.at_shader_location(3),
            ATTRIBUTE_SQUASH_DIR.at_shader_location(4),
            ATTRIBUTE_SQUASH_FACE.at_shader_location(5),
            ATTRIBUTE_SQUASH_DIR2.at_shader_location(6),
            ATTRIBUTE_SQUASH_FACE2.at_shader_location(7),
            ATTRIBUTE_SQUASH_DIR3.at_shader_location(8),
            ATTRIBUTE_SQUASH_FACE3.at_shader_location(9),
            ATTRIBUTE_SWELL.at_shader_location(10),
        ])?];
        Ok(())
    }
}

/// The one entity the whole population is drawn as.
#[derive(Component)]
struct CellMesh;

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
    /// What the field texture is currently sized for, so a scenario with a different grid
    /// reallocates rather than painting into the wrong shape.
    field_size: (u32, u32),
    /// The population's vertex buffers, reused every frame so a steady world allocates nothing.
    cells: cellmesh::Buffers,
}

/// The single quad the chemical field is drawn on.
#[derive(Component)]
struct FieldQuad;

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cell_materials: ResMut<Assets<CellMaterial>>,
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
    let mut cells = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    cells.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
    cells.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
    cells.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
    cells.insert_attribute(ATTRIBUTE_SHAPE, Vec::<[f32; 4]>::new());
    cells.insert_attribute(ATTRIBUTE_SQUASH_DIR, Vec::<[f32; 4]>::new());
    cells.insert_attribute(ATTRIBUTE_SQUASH_FACE, Vec::<[f32; 4]>::new());
    cells.insert_attribute(ATTRIBUTE_SQUASH_DIR2, Vec::<[f32; 4]>::new());
    cells.insert_attribute(ATTRIBUTE_SQUASH_FACE2, Vec::<[f32; 4]>::new());
    cells.insert_attribute(ATTRIBUTE_SQUASH_DIR3, Vec::<[f32; 4]>::new());
    cells.insert_attribute(ATTRIBUTE_SQUASH_FACE3, Vec::<[f32; 4]>::new());
    cells.insert_attribute(ATTRIBUTE_SWELL, Vec::<f32>::new());
    cells.insert_indices(Indices::U32(Vec::new()));
    commands.spawn((
        CellMesh,
        // The vertices are rewritten every frame in screen space, so the bounding box Bevy
        // computes once from the first frame's positions is wrong by the second. Left to itself
        // the whole population would be frustum-culled the moment the camera moved, which looks
        // exactly like every cell dying at once.
        NoFrustumCulling,
        Mesh2d(meshes.add(cells)),
        MeshMaterial2d(cell_materials.add(CellMaterial {})),
        // Above the chemical field, below the organelles.
        Transform::from_xyz(0.0, 0.0, 1.0),
    ));

    let field = images.add(field);
    commands.spawn((
        FieldQuad,
        Sprite {
            image: field.clone(),
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
        pointer,
    );
}

/// Everything bound to a key, once it is established that the keyboard is ours.
fn keyboard(keys: &ButtonInput<KeyCode>, view: &mut View, sim: &mut SlideRes) {
    if keys.just_pressed(KeyCode::Space) {
        view.paused = !view.paused;
        sim.engine.set_rate(if view.paused {
            Rate::Paused
        } else {
            Rate::times(1)
        });
    }
    if keys.just_pressed(KeyCode::Period) {
        // Step one tick, whatever the speed. A paused world you cannot advance is a
        // screenshot.
        sim.engine.step();
    }
    // Speed control, including "run as fast as the machine will go" (SPEC §14): the render
    // detaches from the tick rate rather than the tick rate bending to the render.
    for (key, rate) in [
        (KeyCode::Digit0, Rate::Paused),
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

    // Right-click applies the current tool. `Select` is the default and is the only one that
    // cannot change the world; the rest write, which is why the tool is chosen explicitly.
    if buttons.just_pressed(MouseButton::Right) && target == Target::Slide {
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
                Tool::DrawBarrier | Tool::EraseBarrier => {
                    if square.0 >= 0 && square.1 >= 0 {
                        let event = tools::set_barrier(
                            held.slide().world_mut(),
                            square.0 as u32,
                            square.1 as u32,
                            view.tool == Tool::DrawBarrier,
                        );
                        sim.last_tool = Some(event);
                    }
                }
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
            Without<MoteSprite>,
            Without<JunctionSprite>,
        ),
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cell_mesh: Query<(&Mesh2d, &mut Visibility), With<CellMesh>>,
    mut motes: Query<
        (&MoteSprite, &mut Sprite, &mut Transform),
        (Without<FieldQuad>, Without<JunctionSprite>),
    >,
    mut junctions: Query<
        (&JunctionSprite, &mut Sprite, &mut Transform),
        (Without<FieldQuad>, Without<MoteSprite>),
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
            if let Some(mut image) = images.get_mut(&art_handles.field) {
                image.resize(Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                });
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
    }
    for (mut sprite, mut transform) in &mut field_quad {
        // Stretched over the whole grid, so a texel covers exactly the square it describes.
        let a = to_screen(0.0, 0.0);
        let b = to_screen(frame.width as f32, frame.height as f32);
        sprite.color = if plane > 0 { Color::WHITE } else { Color::NONE };
        sprite.custom_size = Some(Vec2::new((b.x - a.x).abs(), (a.y - b.y).abs()));
        transform.translation = ((a + b) / 2.0).with_z(0.0);
    }

    // Cells: the whole population as one mesh, one material, one draw call (M10.5).
    //
    // This was a sprite entity per cell — fifty thousand `Transform`s and `Sprite`s at the
    // target scale, extracted and prepared every frame to draw quads that differ only in where
    // they are and what colour they are. What it buys beyond the entities is the shape: the
    // fragment shader evaluates a signed-distance field per pixel per cell, so every cell has
    // its own outline, it stays crisp at any magnification, and a failing membrane roughens it.
    // The baked atlas could do none of that; it is still what organelles and dust wear.
    cellmesh::build(&mut art_handles.cells, &frame.cells, |dot| {
        let at = to_screen(dot.x, dot.y);
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
        let body = (dot.radius * 2.0 * scale * slide::PACKING * dot.area_swell * swell).max(
            if selected {
                12.0
            } else {
                1.5
            },
        );
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

    for (mesh_handle, mut visibility) in &mut cell_mesh {
        let Some(mut mesh) = meshes.get_mut(&mesh_handle.0) else {
            continue;
        };
        let buffers = &art_handles.cells;
        *visibility = if buffers.cells() == 0 {
            // An empty mesh is a validation error in some backends and a wasted draw in the
            // rest. A slide with nothing alive on it simply does not draw the layer.
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, buffers.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, buffers.uvs.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, buffers.colours.clone());
        mesh.insert_attribute(ATTRIBUTE_SHAPE, buffers.shapes.clone());
        mesh.insert_attribute(ATTRIBUTE_SQUASH_DIR, buffers.squash_dirs.clone());
        mesh.insert_attribute(ATTRIBUTE_SQUASH_FACE, buffers.squash_faces.clone());
        mesh.insert_attribute(ATTRIBUTE_SQUASH_DIR2, buffers.squash_dirs2.clone());
        mesh.insert_attribute(ATTRIBUTE_SQUASH_FACE2, buffers.squash_faces2.clone());
        mesh.insert_attribute(ATTRIBUTE_SQUASH_DIR3, buffers.squash_dirs3.clone());
        mesh.insert_attribute(ATTRIBUTE_SQUASH_FACE3, buffers.squash_faces3.clone());
        mesh.insert_attribute(ATTRIBUTE_SWELL, buffers.swells.clone());
        mesh.insert_indices(Indices::U32(buffers.indices.clone()));
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
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let ctx = ctx.clone();
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
            .show(&mut root, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| cell_body(ui, &mut sim, &mut view));
            });
    }
    if view.panels.metrics || view.panels.legend {
        egui::Panel::right("rail_right")
            .resizable(true)
            .default_size(260.0)
            .size_range(210.0..=460.0)
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
    ui.add_enabled(false, egui::Button::new(label).shortcut_text(shortcut))
        .on_disabled_hover_text(why);
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
                soon(ui, "New slide…", "Ctrl+N", LATER);
                soon(ui, "Open slide…", "Ctrl+O", LATER);
                soon(ui, "Save slide", "Ctrl+S", LATER);
                soon(ui, "Save slide as…", "", LATER);
                ui.separator();
                soon(ui, "Export…", "", LATER);
                ui.separator();
                if ui.button("Quit").clicked() {
                    *quit = true;
                    ui.close();
                }
            });

            ui.menu_button("Slide", |ui| {
                soon(ui, "Scenario library", "", LATER);
                soon(ui, "Open scenario…", "", LATER);
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
                soon(ui, "Save parameters as…", "", LATER);
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
                if ui
                    .add(
                        egui::Button::new(if running { "Pause" } else { "Run" })
                            .shortcut_text("Space"),
                    )
                    .clicked()
                {
                    sim.engine.set_rate(if running {
                        Rate::Paused
                    } else {
                        Rate::times(1)
                    });
                    view.paused = running;
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Step one tick").shortcut_text("."))
                    .clicked()
                {
                    sim.engine.step();
                }
                ui.menu_button("Speed", |ui| {
                    for (label, key, rate) in [
                        ("paused", "0", Rate::Paused),
                        ("1× — 60 ticks a second", "-", Rate::times(1)),
                        ("8×", "=", Rate::times(8)),
                        ("as fast as it will go", "backspace", Rate::Unlimited),
                    ] {
                        let now = sim.engine.rate() == rate;
                        if ui
                            .add(egui::Button::new(label).shortcut_text(key).selected(now))
                            .clicked()
                        {
                            sim.engine.set_rate(rate);
                            view.paused = rate == Rate::Paused;
                            ui.close();
                        }
                    }
                });
                ui.separator();
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
                for (tool, key) in [
                    (Tool::Select, "F1"),
                    (Tool::Move, "F2"),
                    (Tool::Remove, "F3"),
                    (Tool::DrawBarrier, "F4"),
                    (Tool::EraseBarrier, "F5"),
                ] {
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
            });

            ui.menu_button("Help", |ui| {
                ui.label("keys");
                ui.separator();
                for (key, what) in [
                    ("space", "run / pause"),
                    (".", "step one tick"),
                    ("0 - = ⌫", "speed"),
                    ("1–9", "chemical overlays"),
                    ("F1–F5", "tools"),
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
                if ui.button("⏭").on_hover_text("step one tick  (.)").clicked() {
                    sim.engine.step();
                }
                for (label, rate) in [
                    ("max", Rate::Unlimited),
                    ("8×", Rate::times(8)),
                    ("1×", Rate::times(1)),
                ] {
                    if ui
                        .add(egui::Button::new(label).selected(sim.engine.rate() == rate))
                        .clicked()
                    {
                        sim.engine.set_rate(rate);
                        view.paused = false;
                    }
                }
                let running = sim.engine.rate().is_running();
                if ui
                    .button(if running { "⏸" } else { "▶" })
                    .on_hover_text("run / pause  (space)")
                    .clicked()
                {
                    sim.engine.set_rate(if running {
                        Rate::Paused
                    } else {
                        Rate::times(1)
                    });
                    view.paused = running;
                }
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
    egui::Panel::bottom("status_bar").show(root, |ui| {
        ui.horizontal(|ui| {
            if sim.reading().is_some() {
                ui.label(egui::RichText::new(&sim.latest.species).italics().strong());
                ui.separator();
            }
            ui.label(format!("tick {}", frame.tick));
            ui.separator();
            ui.label(format!("{} cells", frame.population));
            if frame.largest_cluster > 1 {
                ui.separator();
                ui.label(format!("largest organism {}", frame.largest_cluster));
            }
            ui.separator();
            ui.label(format!("tool: {}", view.tool.name()));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Magnification is reported the way the objective would: relative to the base
                // scale, so "1×" is one substrate square to eight pixels.
                ui.label(format!("{:.0}×", view.zoom * 100.0));
                ui.separator();
                ui.label(match frame.lod {
                    Lod::Dots => "points",
                    Lod::Packed => "packed",
                    Lod::Organelles => "organelles",
                    Lod::Full => "full",
                });
                ui.separator();
                // The two halves of the working target, side by side and never added together
                // (`docs/MILESTONES.md`). Until M10.1 there was only one number here and it was
                // both of them at once, which is how a slow tick and a slow frame became
                // indistinguishable.
                let fps = diagnostics
                    .get(&FrameTimeDiagnosticsPlugin::FPS)
                    .and_then(Diagnostic::smoothed)
                    .unwrap_or(0.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{:.0} fps · {} t/s",
                        fps,
                        sim.engine.ticks_per_second()
                    ))
                    .monospace(),
                )
                .on_hover_text(
                    "frames a second and ticks a second, measured separately. The working \
                     target is 50,000 cells at 30 of each.",
                );
            });
        });
    });
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
        .show(root, |ui| {
            ui.horizontal(|ui| {
                for panel in Panel::ALL.into_iter().filter(|p| p.dock() == Dock::Drawer) {
                    if ui
                        .add(
                            egui::Button::new(panel.title())
                                .shortcut_text(panel.key())
                                .selected(panel == showing),
                        )
                        .clicked()
                    {
                        view.panels.drawer = Some(panel);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("▼")
                        .on_hover_text("collapse the drawer")
                        .clicked()
                    {
                        view.panels.drawer = None;
                    }
                });
            });
            ui.separator();
            // egui 0.35's `Panel` sizes to its content unless the content claims the space, so
            // `default_size` alone gave a drawer as tall as whatever was in it — 120 pixels for
            // the genome listing, which is two lines of it. Claiming the height makes the
            // drawer the size it says it is and lets the scroll areas inside do their job.
            ui.set_min_height(ui.available_height());
            match showing {
                Panel::Genome => genome_body(ui, sim, view),
                Panel::Ecology => ecology_body(ui, sim, view),
                Panel::Parameters => parameters_body(ui, sim),
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
    for (slot, s) in out.iter_mut().zip(dot.squash.iter()) {
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
fn parameters_body(ui: &mut egui::Ui, sim: &mut SlideRes) {
    // Read lazily, against the world as it stands. Taking the lock once on open rather than
    // every frame is the whole reason this is cheap enough to leave sitting there. `panels`
    // drops the draft when this tab is not the one on show.
    if sim.draft.is_none() {
        let held = sim.engine.handle();
        let slide = held.slide();
        let live = slide.world().biology().clone();
        sim.draft = Some(Draft {
            editing: live.clone(),
            live,
            founding: slide.world().scenario().biology.clone(),
        });
    }
    let Some(mut draft) = sim.draft.take() else {
        return;
    };
    let mut apply = false;

    let dirty = draft.editing != draft.live;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(dirty, egui::Button::new("apply"))
            .on_hover_text(
                "change the running world. Recorded as an intervention, so the run \
                 still replays exactly and the timeline says when you did it.",
            )
            .clicked()
        {
            apply = true;
        }
        if ui
            .add_enabled(dirty, egui::Button::new("discard"))
            .on_hover_text("back to what the world is running on")
            .clicked()
        {
            draft.editing = draft.live.clone();
        }
        if ui
            .add_enabled(
                draft.editing != draft.founding,
                egui::Button::new("back to the scenario"),
            )
            .on_hover_text("every value as the scenario file has it")
            .clicked()
        {
            draft.editing = draft.founding.clone();
        }
        ui.separator();
        if dirty {
            ui.colored_label(egui::Color32::from_rgb(240, 200, 120), "not applied");
        } else {
            ui.weak("in force");
        }
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("parameters")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for group in params::Group::ALL {
                egui::CollapsingHeader::new(group.title())
                    .default_open(group == params::Group::Metabolism)
                    .show(ui, |ui| {
                        egui::Grid::new(group.title())
                            .num_columns(3)
                            .striped(true)
                            .show(ui, |ui| {
                                for field in params::group(group) {
                                    parameter_row(ui, &mut draft, field, &sim.chem_names);
                                    ui.end_row();
                                }
                            });
                    });
            }
            // Both of these are tables rather than forms: four reactions of four
            // chemicals, and sixteen catalogue entries of seven costs.
            egui::CollapsingHeader::new("metabolic pathways")
                .default_open(true)
                .show(ui, |ui| {
                    ui.small(
                        "Which reactions this world offers. An organelle picks one \
                         with its second control word, so a mitochondrion can only \
                         burn what it is set to burn — and a lineage must either make \
                         that substrate itself or eat something that does.",
                    );
                    pathway_grid(ui, &mut draft, &sim.chem_names);
                });
            egui::CollapsingHeader::new("organelle catalogue")
                .default_open(false)
                .show(ui, |ui| {
                    catalogue_grid(ui, &mut draft);
                });
        });

    if apply {
        let held = sim.engine.handle();
        held.slide().world_mut().set_biology(draft.editing.clone());
        draft.live = draft.editing.clone();
    }
    sim.draft = Some(draft);
}

/// One labelled parameter: its value, its reading, and whether it has been moved.
fn parameter_row(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    field: &params::Field,
    chemicals: &[String],
) {
    use mm_core::params::Value;

    let Some(value) = mm_core::params::get(&draft.editing, field.path) else {
        ui.weak(field.label);
        ui.weak("unreadable");
        return;
    };

    // Marked when it differs from the file, so a world that has drifted from the scenario
    // describing it says so rather than looking freshly loaded.
    let moved = mm_core::params::get(&draft.founding, field.path) != Some(value);
    let label = if moved {
        egui::RichText::new(format!("• {}", field.label))
            .color(egui::Color32::from_rgb(230, 200, 130))
    } else {
        egui::RichText::new(field.label)
    };
    ui.label(label).on_hover_text(field.note);

    let mut edited = None;
    if let Value::Bool(b) = value {
        let mut b = b;
        if ui.checkbox(&mut b, "").changed() {
            edited = Some(Value::Bool(b));
        }
    } else {
        let mut v = value.as_int();
        // A tenth of the current magnitude per pixel, so a value of twenty thousand drags in
        // useful steps and a value of three does not leap past itself.
        let speed = (v.abs() as f64 / 100.0).max(1.0);
        if ui
            .add(egui::DragValue::new(&mut v).speed(speed))
            .on_hover_text(field.note)
            .changed()
        {
            edited = Some(Value::Int(v));
        }
    }

    match field.reading(value, chemicals) {
        Some(reading) => {
            ui.weak(reading);
        }
        None => {
            ui.label("");
        }
    }

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
    ui.label(egui::RichText::new("legend").strong());
    if frame.overlays.is_empty() {
        ui.weak("no overlays — press 1–9");
    }
    for layer in &frame.overlays {
        ui.horizontal(|ui| {
            let [r, g, b] = layer.rgb;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            ui.painter().rect_filled(
                rect,
                2.0,
                egui::Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8),
            );
            // The peak matters: each layer is normalised against its own maximum, so without
            // this the colours are legible but meaningless.
            ui.label(format!(
                "{}  peak {:.1}",
                layer.name,
                layer.peak as f32 / mm_core::Q10_ONE as f32
            ));
        });
    }
    if let Some(event) = &sim.last_tool {
        ui.separator();
        ui.small(format!("{} — {event:?}", view.tool.name()));
    }
}

/// The live metric plots.
fn metrics_body(ui: &mut egui::Ui, sim: &SlideRes) {
    ui.label(egui::RichText::new("metrics").strong());
    let history = &sim.latest.history;
    if history.is_empty() {
        ui.weak("no samples yet");
        return;
    }
    // One plotted series: its label, and how to get its value out of a sample.
    type Series<'a> = (&'a str, Box<dyn Fn(&Sample) -> i64>);
    let series: [Series; 4] = [
        ("population", Box::new(|s: &Sample| s.population as i64)),
        ("dissipation", Box::new(|s: &Sample| s.dissipation)),
        ("light income ‰", Box::new(|s: &Sample| s.trophic_light)),
        (
            "distinct genomes",
            Box::new(|s: &Sample| s.distinct_genomes as i64),
        ),
    ];
    for (name, pick) in series {
        let s = history.series(pick);
        ui.label(format!("{name}  {}", s.values.last().copied().unwrap_or(0)));
        sparkline(ui, &s.normalised());
    }
    if let Some(latest) = history.latest() {
        ui.separator();
        ui.label(format!(
            "fidelity {:.2}",
            latest.mean_fidelity as f32 / mm_core::Q10_ONE as f32
        ));
        ui.label(format!("loadouts {}", latest.distinct_loadouts));
        ui.label(format!("matter {}", latest.total_matter));
    }
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

    ui.separator();
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
    if chase {
        view.genome_scrolled_to = Some(c.ip);
    }

    if let Some(bytes) = apply {
        let held = sim.engine.handle();
        let event = tools::rewrite_genome(held.slide().world_mut(), c.id, bytes);
        sim.last_tool = Some(event);
    }
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
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(sim.latest.species.clone())
                .strong()
                .size(14.0),
        );
        if ui
            .selectable_label(view.follow, "track")
            .on_hover_text("keep the camera centred on this cell (t)")
            .clicked()
        {
            view.follow = !view.follow;
        }
    });
    ui.weak(format!(
        "age {}  born tick {}  at ({:.1}, {:.1})",
        c.age,
        c.birth_tick,
        c.x as f32 / mm_core::fixed::POS_ONE as f32,
        c.y as f32 / mm_core::fixed::POS_ONE as f32
    ));

    // --- the bars that say whether it is doing well ---
    ui.add_space(4.0);
    bar(
        ui,
        "energy",
        q10(c.energy),
        400.0,
        egui::Color32::from_rgb(230, 200, 90),
    );
    bar(
        ui,
        "mass",
        q10(c.mass),
        120.0,
        egui::Color32::from_rgb(140, 190, 230),
    );
    if c.damage > 0 {
        bar(
            ui,
            "damage",
            q10(c.damage),
            40.0,
            egui::Color32::from_rgb(220, 110, 110),
        );
    }

    ui.separator();

    // --- the schematic ---
    let placed = mm_app::inspector::placements(&c.slots);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 150.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    let centre = rect.center();
    let r = (rect.height() / 2.0 - 8.0).max(8.0);
    // The membrane: the circle everything else lives inside.
    painter.circle_stroke(
        centre,
        r,
        egui::Stroke::new(2.0, egui::Color32::from_rgb(150, 160, 170)),
    );
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
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(130),
        );
    }

    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        // --- the genome, with the cell's own instruction pointer in it ---
        let over = c.nucleus_capacity > 0 && c.genome_len > c.nucleus_capacity;
        ui.horizontal(|ui| {
            ui.label(format!(
                "genome {} bytes / nucleus {}",
                c.genome_len, c.nucleus_capacity
            ));
            if over {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 130, 90),
                    "⚠ truncated at division",
                )
                .on_hover_text(
                    "SPEC §4.1: a genome longer than its nucleus is cut short in every \
                             daughter. The lineage will stop without an error.",
                );
            }
        });
        ui.horizontal(|ui| {
            match c.fidelity {
                Some(f) => ui.weak(format!("fidelity {:.2}", q10(f))),
                // Not "fidelity 0.00". A cell with no nucleus has no fidelity, cannot
                // copy its genome and cannot divide — which is a different and much
                // louder fact than copying badly.
                None => ui.colored_label(
                    egui::Color32::from_rgb(230, 130, 90),
                    "no nucleus — cannot divide",
                ),
            };
            ui.weak(format!(
                "{}   ip {}",
                if c.halted { "halted" } else { "running" },
                c.ip
            ));
        });

        if ui
            .button("open genome ▸")
            .on_hover_text("the disassembly, live, in the drawer (g)")
            .clicked()
        {
            view.panels.set(Panel::Genome, true);
        }

        // --- what it is holding ---
        ui.collapsing("chemistry", |ui| {
            for (i, v) in c.interior.iter().enumerate() {
                if *v != 0 {
                    ui.label(format!("{:<16} {:.2}", chem_names[i], q10(*v)));
                }
            }
        });

        ui.collapsing("machine", |ui| {
            ui.label(format!("stack {:?}", c.stack));
            ui.label(format!("calls {:?}", c.call_stack));
            if c.ln > 0 {
                ui.label(format!(
                    "copying: {} bytes to go, from {} to {}",
                    c.ln, c.pa, c.pb
                ));
            }
            let live: Vec<String> = c
                .registers
                .iter()
                .enumerate()
                .filter(|(_, v)| **v != 0)
                .map(|(n, v)| format!("r{n}={v}"))
                .collect();
            ui.label(if live.is_empty() {
                "registers all zero".to_string()
            } else {
                live.join("  ")
            });
            let ram: Vec<String> = c
                .ram
                .iter()
                .enumerate()
                .filter(|(_, v)| **v != 0)
                .map(|(n, v)| format!("[{n}]={v}"))
                .collect();
            ui.label(if ram.is_empty() {
                "ram all zero".to_string()
            } else {
                ram.join("  ")
            });
        });
    });
}

/// A labelled bar, for the two or three numbers worth seeing at a glance rather than reading.
///
/// `full` is what counts as a full bar. There is no maximum for energy or mass, so it is a
/// reference point rather than a limit and the bar clamps at it.
fn bar(ui: &mut egui::Ui, label: &str, value: f32, full: f32, colour: egui::Color32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 16.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(120));
    let fraction = (value / full).clamp(0.0, 1.0);
    let mut filled = rect;
    filled.set_width(rect.width() * fraction);
    painter.rect_filled(filled, 2.0, colour.gamma_multiply(0.55));
    painter.text(
        rect.left_center() + egui::vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        format!("{label}  {value:.1}"),
        egui::FontId::proportional(11.0),
        egui::Color32::from_gray(230),
    );
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
        for which in Ecology::ALL {
            if ui
                .selectable_label(view.ecology == which, which.title())
                .clicked()
            {
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

    ui.heading(&page.name);
    ui.label(&page.description);
    ui.separator();
    ui.label(format!(
        "founded {}  ·  {} births  ·  {} deaths  ·  {} generations deep",
        page.founded_tick, page.births, page.deaths, page.depth
    ));
    if let Some((id, name)) = &page.parent {
        if ui.link(format!("diverged from {name}")).clicked() {
            view.species = Some(*id);
        }
    }
    if !page.children.is_empty() {
        ui.label(format!("{} descendant species:", page.children.len()));
        for (id, name) in page.children.iter().take(8) {
            if ui.link(format!("  {name}")).clicked() {
                view.species = Some(*id);
            }
        }
    }
    ui.separator();
    ui.label(format!("population — peak {}", page.curve_peak));
    let values: Vec<f32> = page.curve.iter().map(|(_, v)| *v).collect();
    sparkline(ui, &values);
    ui.separator();
    ui.small(format!(
        "founder genome, {} bytes, fingerprint {:016x}",
        page.founder_genome.len(),
        page.fingerprint
    ));
    let hex: String = page
        .founder_genome
        .iter()
        .take(64)
        .map(|b| format!("{b:02x}"))
        .collect();
    ui.small(egui::RichText::new(hex).monospace());
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

    ui.label(web.summary());
    ui.weak(format!("averaged over {} ticks", web.window_ticks));
    ui.separator();

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 220.0),
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
            rect.bottom() - 12.0 - (rect.height() - 32.0) * up,
        )
    };

    for edge in &web.edges {
        if edge.weight <= 0 {
            continue;
        }
        let (a, b) = (place(edge.from), place(edge.to));
        let width = 1.0 + 5.0 * (edge.weight as f32 / peak).clamp(0.0, 1.0);
        let colour = if edge.is_recycling() {
            egui::Color32::from_rgb(150, 200, 120)
        } else if edge.is_death() {
            egui::Color32::from_rgb(140, 110, 110)
        } else {
            egui::Color32::from_rgb(110, 150, 190)
        };
        if edge.basis == Basis::Measured {
            painter.line_segment([a, b], egui::Stroke::new(width, colour));
        } else {
            // Dashed, because the total is measured but who it belongs to is not.
            let steps = 9;
            for k in 0..steps {
                if k % 2 == 1 {
                    continue;
                }
                let t0 = k as f32 / steps as f32;
                let t1 = (k + 1) as f32 / steps as f32;
                painter.line_segment(
                    [a.lerp(b, t0), a.lerp(b, t1)],
                    egui::Stroke::new(width, colour),
                );
            }
        }
    }

    for occ in &web.nodes {
        let at = place(occ.node);
        let text = if occ.node.is_source() {
            occ.node.label().to_string()
        } else {
            format!("{} {}", occ.node.label(), occ.count)
        };
        let fill = if occ.node.is_source() {
            egui::Color32::from_black_alpha(200)
        } else {
            egui::Color32::from_rgb(30, 45, 40)
        };
        let galley = painter.layout_no_wrap(
            text,
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(220),
        );
        let box_rect = egui::Rect::from_center_size(at, galley.size() + egui::vec2(10.0, 6.0));
        painter.rect_filled(box_rect, 3.0, fill);
        painter.galley(
            box_rect.center() - galley.size() / 2.0,
            galley,
            egui::Color32::WHITE,
        );
    }

    ui.separator();
    egui::ScrollArea::vertical()
        .max_height(120.0)
        .show(ui, |ui| {
            for edge in &web.edges {
                if edge.weight <= 0 {
                    continue;
                }
                ui.small(format!(
                    "{} → {}: {}{}",
                    edge.from.label(),
                    edge.to.label(),
                    edge.weight / mm_core::Q10_ONE as i64,
                    if edge.basis == Basis::Measured {
                        ""
                    } else {
                        " (shared out)"
                    }
                ))
                .on_hover_text(edge.note);
            }
        });
}

/// The genome editor (M6).
///
/// Syntax highlighting comes from `mm_asm::highlight`, which classifies against the real
/// opcode table, and diagnostics come from actually assembling — so neither can drift from the
/// language the way a second, approximate definition in the front-end would.
fn editor_body(ui: &mut egui::Ui, sim: &mut SlideRes) {
    ui.horizontal(|ui| {
        ui.label("name:");
        ui.text_edit_singleline(&mut sim.editor.name);
        if ui.button("assemble").clicked() {
            sim.editor.assemble();
        }
        if ui.button("export").clicked() {
            sim.last_export = sim.editor.export().map(|f| f.to_text());
        }
        if ui.button("from selected cell").clicked() {
            if let Some(cell) = sim.selected {
                let held = sim.engine.handle();
                let file = tools::copy_genome(held.slide().world(), cell);
                if let Some(file) = file {
                    sim.editor.load_bytes(&file.bytes, file.name);
                }
            }
        }
    });
    ui.label(sim.editor.status());
    ui.separator();

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

    if !errors.is_empty() {
        let lines: Vec<&str> = source.lines().collect();
        egui::ScrollArea::vertical()
            .max_height(120.0)
            .id_salt("diagnostics")
            .show(ui, |ui| {
                for (line, col, message) in &errors {
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 120, 110),
                            format!("{line}:{col}"),
                        );
                        ui.label(message);
                        // The offending line, quoted. `3:12` means counting to line three
                        // otherwise, and counting to line three is the part nobody enjoys.
                        if let Some(text) = lines.get(line.saturating_sub(1) as usize) {
                            ui.label(
                                egui::RichText::new(text.trim())
                                    .monospace()
                                    .color(egui::Color32::from_gray(140)),
                            );
                        }
                    });
                }
            });
        ui.separator();
    }

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
        ui.separator();
        ui.label("exported — copy this:");
        let mut shown = text.clone();
        ui.add(
            egui::TextEdit::multiline(&mut shown)
                .code_editor()
                .desired_rows(4),
        );
    }
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
            sim.engine.set_rate(Rate::times(1));
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

/// A minimal line plot. Values are already `0..=1`.
fn sparkline(ui: &mut egui::Ui, values: &[f32]) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(90));
    if values.len() < 2 {
        return;
    }
    let step = rect.width() / (values.len() - 1) as f32;
    let points: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            egui::pos2(
                rect.left() + i as f32 * step,
                // Higher values draw higher up, which is the only way round anybody reads a
                // plot, and the opposite of how screen y runs.
                rect.bottom() - v * rect.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(1.2, egui::Color32::from_rgb(120, 200, 160)),
    ));
}
