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
//!  │  genome │ wiki │ food web │ editor │ debugger                  │
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
//! | `g` `w` `f` `e` `d` | drawer: genome, wiki, food web, editor, debugger |
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
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

use mm_app::debugger::{Breakpoint, Breakpoints, Sandbox};
use mm_app::editor::Editor;
use mm_app::engine::{Engine, Published, Rate};
use mm_app::inspector::Inspection;
use mm_app::params;
use mm_app::slide::{Frame, Lod, Slide};
use mm_app::tools::{self, ToolEvent};
use mm_app::ui::{self, Dock, Focus, Panel, Panels, Rect, Target};
use mm_app::wiki;
use mm_core::biology::BiologyConfig;
use mm_core::cell::CellSeed;
use mm_core::fixed::{pos, q10};
use mm_core::light::CurrentField;
use mm_core::metrics::Sample;
use mm_core::{CellId, LightRegime, MutationRates, Organelle, OrganelleType, Scenario, Seeding};

/// Pixels per substrate square at zoom 1.
const BASE_SCALE: f32 = 8.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Manic Microbes".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin)
        .add_plugins(EguiPlugin)
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.03)))
        .insert_resource(SlideRes::new())
        .insert_resource(View::default())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                handle_input,
                // Ordered so that a frame always shows the tick that has just finished,
                // rather than one caught halfway through being computed.
                collect_simulation,
                redraw,
                panels,
            )
                .chain(),
        )
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
    /// The parameter editor's working copy, and the two things it is compared against.
    ///
    /// Edits are applied on a button rather than on a keystroke, because every apply is an
    /// intervention that goes on the record and one per keystroke would be a useless record.
    draft: Option<Draft>,
}

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
    fn reseed(&mut self) {
        let held = self.engine.handle();
        seed_ancestors(&mut held.slide());
        self.selected = None;
        self.engine.select(None);
        self.sandbox = None;
        self.breakpoints.rearm();
    }
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
            let id = world.spawn_cell(CellSeed {
                x: pos((8 + (k % 4) * 20) as i32),
                y: pos((8 + (k / 4) * 20) as i32),
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

/// The default slide: light, food, no flow. The habitat the ancestor was written for.
fn petri() -> Scenario {
    Scenario {
        name: "petri".to_string(),
        seed: 1,
        width: 96,
        height: 96,
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
    /// The parameter editor, and the log of what has been changed in this world (M10.2).
    ///
    /// Floating windows rather than docked panels: both are things you open to do a job and
    /// then close, and neither wants to hold a rail's worth of screen for the rest of the
    /// session.
    parameters: bool,
    interventions: bool,
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
}

impl Default for View {
    fn default() -> Self {
        View {
            centre: Vec2::new(48.0, 48.0),
            zoom: 1.0,
            paused: false,
            panels: Panels::default(),
            parameters: false,
            interventions: false,
            viewport: Rect::default(),
            focus: Focus::default(),
            follow: false,
            genome_follow_ip: true,
            genome_scrolled_to: None,
            drag_distance: 0.0,
            tool: Tool::Select,
            species: None,
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
struct OverlaySquare(usize);

#[derive(Component)]
struct CellSprite(usize);

#[derive(Component)]
struct OrganelleSprite {
    cell: usize,
    nth: usize,
}

#[derive(Component)]
struct MoteSprite(usize);

/// A junction, drawn as a thin sprite stretched between two cells (M7).
#[derive(Component)]
struct JunctionSprite(usize);

fn setup(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());
}

#[allow(clippy::too_many_arguments)]
fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: EventReader<MouseMotion>,
    mut wheel: EventReader<MouseWheel>,
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
    let (wants_pointer, wants_keyboard) = {
        let ctx = contexts.ctx_mut();
        (
            ctx.wants_pointer_input() || ctx.is_pointer_over_area(),
            ctx.wants_keyboard_input(),
        )
    };
    let pointer = window
        .get_single()
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
        Panel::Wiki => KeyCode::KeyW,
        Panel::FoodWeb => KeyCode::KeyF,
        Panel::Editor => KeyCode::KeyE,
        Panel::Debugger => KeyCode::KeyD,
    }
}

/// Everything bound to the mouse, once it is established which region owns it.
#[allow(clippy::too_many_arguments)]
fn handle_mouse(
    buttons: &ButtonInput<MouseButton>,
    motion: &mut EventReader<MouseMotion>,
    wheel: &mut EventReader<MouseWheel>,
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

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn redraw(
    mut commands: Commands,
    sim: Res<SlideRes>,
    view: Res<View>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut squares: Query<
        (&OverlaySquare, &mut Sprite, &mut Transform),
        (
            Without<CellSprite>,
            Without<OrganelleSprite>,
            Without<MoteSprite>,
        ),
    >,
    mut cells: Query<
        (&CellSprite, &mut Sprite, &mut Transform),
        (
            Without<OverlaySquare>,
            Without<OrganelleSprite>,
            Without<MoteSprite>,
        ),
    >,
    mut organelles: Query<
        (&OrganelleSprite, &mut Sprite, &mut Transform),
        (
            Without<OverlaySquare>,
            Without<CellSprite>,
            Without<MoteSprite>,
        ),
    >,
    mut motes: Query<
        (&MoteSprite, &mut Sprite, &mut Transform),
        (
            Without<OverlaySquare>,
            Without<CellSprite>,
            Without<OrganelleSprite>,
            Without<JunctionSprite>,
        ),
    >,
    mut junctions: Query<
        (&JunctionSprite, &mut Sprite, &mut Transform),
        (
            Without<OverlaySquare>,
            Without<CellSprite>,
            Without<OrganelleSprite>,
            Without<MoteSprite>,
        ),
    >,
) {
    let frame = &sim.latest.frame;
    let optics = &sim.latest.optics;
    let scale = BASE_SCALE * view.zoom;
    let Ok(window) = window.get_single() else {
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

    // The chemical fields and the light, as one square each. Spawned once and then updated,
    // because respawning a quarter of a million sprites a frame is not a rendering strategy.
    if squares.is_empty() && plane > 0 {
        for i in 0..plane {
            commands.spawn((
                OverlaySquare(i),
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::NONE,
                        custom_size: Some(Vec2::splat(1.0)),
                        ..default()
                    },
                    ..default()
                },
            ));
        }
    }
    for (sq, mut sprite, mut transform) in &mut squares {
        let i = sq.0;
        let Some(l) = frame.light.get(i) else {
            continue;
        };
        let x = (i % frame.width.max(1) as usize) as f32;
        let y = (i / frame.width.max(1) as usize) as f32;
        // Light as a warm luminance under the chemical layers (SPEC §14).
        let warm = 0.10 * l;
        let mut rgb = [warm, warm * 0.92, warm * 0.75];
        // Layers add, so overlapping chemicals mix rather than one winning. Two overlays on
        // at once should look like two overlays on at once.
        let layers = (frame.overlays.len() as f32).max(1.0);
        for layer in &frame.overlays {
            if let Some(c) = layer.field.get(i) {
                // Square root, not the raw fraction. A field is normalised against its own
                // peak, and in a diffused world almost every square sits far below that peak —
                // so linear mapping renders the whole slide black except wherever the maximum
                // happens to be. The curve is presentation only; `layer.field` stays linear
                // and the legend still reports the peak the eye is being lied to about.
                let shade = c.max(0.0).sqrt();
                for (channel, tint) in rgb.iter_mut().zip(layer.rgb) {
                    *channel += tint * shade / layers;
                }
            }
        }
        let at = to_screen(x + 0.5, y + 0.5);
        let dim = optics.vignette(field_radius(at));
        sprite.color = Color::srgb(rgb[0] * dim, rgb[1] * dim, rgb[2] * dim);
        sprite.custom_size = Some(Vec2::splat(scale));
        transform.translation = at;
    }

    // Cells. Kept at a fixed pool size and hidden when unused, for the same reason.
    let wanted = frame.cells.len();
    let have = cells.iter().count();
    for i in have..wanted {
        commands.spawn((
            CellSprite(i),
            SpriteBundle {
                sprite: Sprite {
                    custom_size: Some(Vec2::splat(1.0)),
                    ..default()
                },
                ..default()
            },
        ));
    }
    for (marker, mut sprite, mut transform) in &mut cells {
        let Some(dot) = frame.cells.get(marker.0) else {
            sprite.color = Color::NONE;
            continue;
        };
        let at = to_screen(dot.x, dot.y).with_z(1.0);
        let r = field_radius(at);
        let dim = optics.vignette(r);
        // Depth of field, approximated: a defocused cell is bigger and fainter rather than
        // convolved. See the module docs.
        let blur = optics.blur(dot.depth);
        let softness = 1.0 - (blur / optics.max_blur.max(f32::EPSILON)).clamp(0.0, 0.75);
        let selected = sim.selected == Some(dot.id);
        let [cr, cg, cb] = dot.rgb;
        let tint = if selected { 1.0 } else { dim * softness };
        sprite.color = Color::srgba(cr * tint, cg * tint, cb * tint, softness.max(0.25));
        sprite.custom_size = Some(Vec2::splat(
            (dot.radius * 2.0 * scale + blur).max(if selected { 4.0 } else { 1.5 }),
        ));
        transform.translation = at;
    }

    // Organelles, only at the tiers that resolve them. At `Lod::Dots` every organelle sprite
    // is hidden and the loop above is the whole of the drawing.
    const MAX_SLOTS: usize = 16;
    let detailed = frame.lod.resolves_organelles();
    let organelle_pool = organelles.iter().count();
    if detailed && organelle_pool < wanted * MAX_SLOTS {
        for i in organelle_pool..(wanted * MAX_SLOTS) {
            commands.spawn((
                OrganelleSprite {
                    cell: i / MAX_SLOTS,
                    nth: i % MAX_SLOTS,
                },
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::NONE,
                        custom_size: Some(Vec2::splat(1.0)),
                        ..default()
                    },
                    ..default()
                },
            ));
        }
    }
    for (marker, mut sprite, mut transform) in &mut organelles {
        let found = detailed
            .then(|| frame.cells.get(marker.cell))
            .flatten()
            .and_then(|dot| dot.organelles.get(marker.nth).map(|o| (dot, o)));
        let Some((dot, o)) = found else {
            sprite.color = Color::NONE;
            continue;
        };
        let at = to_screen(dot.x + o.dx, dot.y + o.dy).with_z(2.0);
        let dim = optics.vignette(field_radius(at)) * o.built;
        // Chromatic aberration: the red and blue channels are drawn a hair apart at the edge
        // of the field. Applied to the smallest things on screen, where it reads as an
        // optical artefact rather than as a bug.
        let sep = optics.separation(field_radius(at));
        let [r, g, b] = o.rgb;
        sprite.color = Color::srgb(r * dim, g * dim, b * dim);
        sprite.custom_size = Some(Vec2::splat((o.radius * 2.0 * scale).max(1.0) + sep));
        transform.translation = at;
    }

    // Junctions. A stretched, rotated sprite per link: hard ones solid because they are
    // structure, soft ones faint because they are a channel rather than a body.
    let junction_pool = junctions.iter().count();
    for i in junction_pool..frame.junctions.len() {
        commands.spawn((
            JunctionSprite(i),
            SpriteBundle {
                sprite: Sprite {
                    color: Color::NONE,
                    custom_size: Some(Vec2::splat(1.0)),
                    ..default()
                },
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
            SpriteBundle {
                sprite: Sprite {
                    color: Color::NONE,
                    custom_size: Some(Vec2::splat(1.0)),
                    ..default()
                },
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
    mut exit: EventWriter<AppExit>,
    diagnostics: Res<DiagnosticsStore>,
) {
    let ctx = contexts.ctx_mut();
    let frame = sim.latest.frame.clone();

    // Order is layout: egui hands space out from the outside in, so the menu takes the top, the
    // status bar the very bottom, the drawer the strip above it, and the rails what is left
    // between them.
    let mut quit = false;
    menu_bar(ctx, &mut sim, &mut view, &mut quit);
    status_bar(ctx, &sim, &view, &frame, &diagnostics);
    drawer(ctx, &mut sim, &mut view);

    if view.panels.cell {
        egui::SidePanel::left("rail_left")
            .default_width(270.0)
            .width_range(210.0..=460.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| cell_body(ui, &mut sim, &mut view));
            });
    }
    if view.panels.metrics || view.panels.legend {
        egui::SidePanel::right("rail_right")
            .default_width(260.0)
            .width_range(210.0..=460.0)
            .show(ctx, |ui| {
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
    if view.parameters {
        parameter_window(ctx, &mut sim, &mut view);
    }
    if view.interventions {
        intervention_window(ctx, &sim, &mut view);
    }

    let rect = ctx.available_rect();
    view.viewport = Rect::new(rect.min.x, rect.min.y, rect.max.x, rect.max.y);

    if quit {
        exit.send(AppExit::Success);
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
fn menu_bar(ctx: &egui::Context, sim: &mut SlideRes, view: &mut View, quit: &mut bool) {
    const LATER: &str = "M10.2 — configuration and slide files";

    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
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
                    ui.close_menu();
                }
            });

            ui.menu_button("Slide", |ui| {
                soon(ui, "Scenario library", "", LATER);
                soon(ui, "Open scenario…", "", LATER);
                if ui
                    .add(
                        egui::Button::new("Parameters…")
                            .shortcut_text("Ctrl+,")
                            .selected(view.parameters),
                    )
                    .on_hover_text("every cost, rate and mutation the living half runs on")
                    .clicked()
                {
                    view.parameters = !view.parameters;
                    ui.close_menu();
                }
                soon(ui, "Save parameters as…", "", LATER);
                ui.separator();
                if ui
                    .add(egui::Button::new("Reseed").shortcut_text("R"))
                    .on_hover_text("wipe the slide and start the ancestor over")
                    .clicked()
                {
                    sim.reseed();
                    ui.close_menu();
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
                    ui.close_menu();
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
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                let count = sim.latest.interventions.len();
                if ui
                    .add(
                        egui::Button::new(if count == 0 {
                            "Interventions…".to_string()
                        } else {
                            format!("Interventions… ({count})")
                        })
                        .selected(view.interventions),
                    )
                    .on_hover_text("what has been changed in this world, and when")
                    .clicked()
                {
                    view.interventions = !view.interventions;
                    ui.close_menu();
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
                    ui.close_menu();
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
                        ui.close_menu();
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
    ctx: &egui::Context,
    sim: &SlideRes,
    view: &View,
    frame: &Frame,
    diagnostics: &DiagnosticsStore,
) {
    egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
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
fn drawer(ctx: &egui::Context, sim: &mut SlideRes, view: &mut View) {
    let Some(showing) = view.panels.drawer else {
        return;
    };
    egui::TopBottomPanel::bottom("drawer")
        .resizable(true)
        .default_height(300.0)
        .height_range(120.0..=760.0)
        .show(ctx, |ui| {
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
            match showing {
                Panel::Genome => genome_body(ui, sim, view),
                Panel::Wiki => wiki_body(ui, sim, view),
                Panel::FoodWeb => foodweb_body(ui, sim),
                Panel::Editor => editor_body(ui, sim),
                Panel::Debugger => debugger_body(ui, sim),
                // The rails' panels are never the drawer's tab; `Panels::set` will not put
                // one there.
                Panel::Cell | Panel::Metrics | Panel::Legend => {}
            }
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
fn parameter_window(ctx: &egui::Context, sim: &mut SlideRes, view: &mut View) {
    // Opened lazily, against the world as it stands. Taking the lock once on open rather than
    // every frame is the whole reason this is cheap enough to leave sitting there.
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
    let Some(draft) = sim.draft.take() else {
        return;
    };
    let mut draft = draft;
    let mut apply = false;
    let mut open = view.parameters;

    egui::Window::new("parameters")
        .open(&mut open)
        .default_width(560.0)
        .default_height(520.0)
        .show(ctx, |ui| {
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
        });

    if apply {
        let held = sim.engine.handle();
        held.slide().world_mut().set_biology(draft.editing.clone());
        draft.live = draft.editing.clone();
    }
    view.parameters = open;
    sim.draft = Some(draft);
    if !view.parameters {
        // Closed. The draft goes with it, so reopening reads the world afresh rather than
        // showing edits from ten minutes ago as though they were pending.
        sim.draft = None;
    }
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
fn intervention_window(ctx: &egui::Context, sim: &SlideRes, view: &mut View) {
    let mut open = view.interventions;
    egui::Window::new("interventions")
        .open(&mut open)
        .default_width(480.0)
        .default_height(320.0)
        .show(ctx, |ui| {
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
                            ui.small(format!("    {label}:  {was} → {now}"));
                        }
                        if !said {
                            ui.small("    (no parameter differed)");
                        }
                        previous = step.biology.clone();
                    }
                });
        });
    view.interventions = open;
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
        sim.listing.of(&c.genome, c.genome_hash);
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
                    let body =
                        egui::RichText::new(format!("{:>4}  {:<22}", line.offset, line.text))
                            .monospace()
                            .size(11.0);
                    ui.label(if current {
                        body.background_color(egui::Color32::from_rgb(45, 70, 45))
                            .color(egui::Color32::from_rgb(210, 255, 210))
                    } else {
                        body.color(egui::Color32::from_gray(175))
                    });
                    if let Some(label) = &line.label {
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
    let w = window.get_single().ok()?;
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
            egui::show_tooltip_at_pointer(
                ui.ctx(),
                ui.layer_id(),
                egui::Id::new("organelle"),
                |ui| {
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
                        let n =
                            mm_core::organelle::MetabolicChemistry::pathway_index(slot.control[1]);
                        ui.label(format!("pathway {n}"))
                            .on_hover_text("which metabolic reaction this one runs");
                    }
                    if let Some(n) = slot.remaining_build {
                        ui.weak(format!("building, {n} to go"));
                    }
                },
            );
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

/// The species wiki, the phylogenetic tree and the world timeline (M5, SPEC §10.5).
///
/// Reads the archive through [`mm_app::wiki`], which copies everything out — so this panel
/// holds no borrow of the world and nothing in it can reach a tick.
fn wiki_body(ui: &mut egui::Ui, sim: &SlideRes, view: &mut View) {
    // One of the panels that does take the world's lock: the archive is far too large to
    // publish every frame and the wiki is opened deliberately, to read. See
    // `engine::Published` for which panels are exempt and why.
    let held = sim.engine.handle();
    let slide = held.slide();
    let world = slide.world();
    let archive = world.archive();

    if archive.is_empty() {
        ui.weak("nothing has lived here yet");
        return;
    }
    ui.label(format!(
        "{} species, {} alive, {} pruned",
        archive.len(),
        archive.living(),
        archive.pruned()
    ));
    ui.separator();

    // --- the timeline ---
    let timeline = wiki::timeline(archive, world.events().events(), world.tick_count());
    ui.label("timeline");
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(90));
    for entry in &timeline.entries {
        let x = rect.left() + rect.width() * (entry.at as f32 / 1000.0);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(1.5, egui::Color32::from_rgb(220, 180, 110)),
        );
    }
    for entry in timeline.entries.iter().rev().take(6) {
        ui.small(format!(
            "tick {} — {} ({})",
            entry.tick, entry.headline, entry.species_name
        ));
    }
    ui.separator();

    // --- the tree ---
    ui.label("tree");
    egui::ScrollArea::vertical()
        .max_height(140.0)
        .id_source("tree")
        .show(ui, |ui| {
            let tree = wiki::layout(archive);
            for node in &tree.nodes {
                // Indent by depth: the column is the number of speciation events
                // between this species and the seeded founder.
                let indent = "  ".repeat(node.depth.min(12) as usize);
                let label = format!(
                    "{indent}{} {} ({})",
                    if node.alive { "●" } else { "○" },
                    node.name,
                    if node.alive {
                        node.population.to_string()
                    } else {
                        format!("peak {}", node.peak_population)
                    }
                );
                if ui
                    .selectable_label(view.species == Some(node.id), label)
                    .clicked()
                {
                    view.species = Some(node.id);
                }
            }
        });
    ui.separator();

    // --- the page ---
    let showing = view
        .species
        .or_else(|| wiki::notable(archive, 1).first().copied());
    let Some(page) = showing.and_then(|id| wiki::page(archive, id)) else {
        ui.weak("select a species");
        return;
    };
    egui::ScrollArea::vertical()
        .id_source("page")
        .show(ui, |ui| {
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
            // Loading it into an editor is M6. Showing it is not.
            let hex: String = page
                .founder_genome
                .iter()
                .take(64)
                .map(|b| format!("{b:02x}"))
                .collect();
            ui.small(egui::RichText::new(hex).monospace());
        });
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
    let errors: Vec<String> = sim
        .editor
        .build()
        .errors()
        .iter()
        .map(|e| format!("{}:{}: {}", e.line, e.col, e.message))
        .collect();
    if !errors.is_empty() {
        egui::ScrollArea::vertical()
            .max_height(90.0)
            .id_source("diagnostics")
            .show(ui, |ui| {
                for e in &errors {
                    ui.colored_label(egui::Color32::from_rgb(230, 120, 110), e);
                }
            });
        ui.separator();
    }

    // The source, highlighted line by line. Drawn read-only alongside an editable
    // buffer rather than as a rich text editor, because egui has no styled-input
    // widget and a plain one that silently dropped the colours would be worse.
    let mut source = sim.editor.source().to_string();
    egui::ScrollArea::vertical()
        .id_source("source")
        .show(ui, |ui| {
            let response = ui.add(
                egui::TextEdit::multiline(&mut source)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(18),
            );
            if response.changed() {
                sim.editor.set_source(source.clone());
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
            .id_source("disasm")
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
