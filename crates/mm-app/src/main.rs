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
//! | | |
//! |---|---|
//! | drag | pan |
//! | wheel | zoom, whole-slide to single cell |
//! | click | select a cell for the inspector |
//! | `space` | pause / resume |
//! | `.` | step one tick |
//! | `0` `-` `=` `backspace` | speed: paused, 1×, 8×, as fast as it will go |
//! | `1`–`9` | toggle that chemical's overlay |
//! | `l` | legend |
//! | `p` | plots |
//! | `i` | inspector |
//! | `w` | species wiki, tree and timeline |
//! | `e` | genome editor |
//! | `d` | debugger |
//! | `F1`–`F5` | tool: select, move, remove, wall, erase |
//! | `o` | optics on/off |
//! | `r` | wipe and reseed the slide |
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
//! **Unverified visually.** This was written on a headless machine: it compiles, and
//! everything it draws is derived from data that is tested (`optics.rs`, `inspector.rs`,
//! `slide.rs`), but nobody has yet seen it draw a pixel. Treat the layout constants here as
//! first guesses.
//!
//! The vignette, depth-of-field and chromatic aberration are applied per-sprite from the
//! parameters in [`mm_app::optics`] rather than as a full-screen post-process pass. That is a
//! deliberate simplification: it needs no custom render graph node, it is exactly right for
//! the vignette and the aberration, and it approximates defocus by size and alpha rather than
//! by convolution. A real separable blur belongs in the post-process pass this leaves room
//! for.

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

use mm_app::debugger::{Breakpoint, Breakpoints, Sandbox};
use mm_app::editor::Editor;
use mm_app::inspector::Inspection;
use mm_app::slide::{Frame, Lod, Slide};
use mm_app::tools::{self, ToolEvent};
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
                advance_simulation,
                redraw,
                panels,
            )
                .chain(),
        )
        .run();
}

/// The simulation, as a Bevy resource.
///
/// Bevy owns the *box*, not the contents: nothing in this file can reach `World` except
/// through `Slide`, which only lends out frames.
#[derive(Resource)]
struct SlideRes {
    slide: Slide,
    frame: Frame,
    /// The cell the inspector is pointed at, if any.
    selected: Option<CellId>,
    inspection: Option<Inspection>,
    /// The genome editor (M6).
    editor: Editor,
    /// Breakpoints over the live world, and the sandbox for instruction stepping.
    breakpoints: Breakpoints,
    sandbox: Option<Sandbox>,
    /// What the last tool did, for the status line.
    last_tool: Option<ToolEvent>,
    /// Where a genome exported from the editor was written.
    last_export: Option<String>,
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
        let slide = Slide::new(petri()).expect("default scenario");
        let mut res = SlideRes {
            slide,
            frame: Frame::default(),
            selected: None,
            inspection: None,
            editor: Editor::new(),
            breakpoints: Breakpoints::new(),
            sandbox: None,
            last_tool: None,
            last_export: None,
        };
        res.reseed();
        res.frame = res.slide.frame();
        res
    }

    /// Wipe the slide and start the ancestor over. Bound to `r`.
    fn reseed(&mut self) {
        *self.slide.world_mut() = mm_core::World::new(petri()).expect("default scenario");
        self.slide.world_mut().set_biology(BiologyConfig {
            mutation: MutationRates::default(),
            ..BiologyConfig::default()
        });
        let Some(bytes) = ancestor_genome() else {
            return;
        };
        let world = self.slide.world_mut();
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
                cells.interior_mut(i)[11] = q10(40);
                cells.interior_mut(i)[14] = q10(40);
            }
        }
        // Filling a cytoplasm by hand creates matter, which is what scenario setup is for.
        self.slide.world_mut().adopt_current_contents_as_baseline();
        self.selected = None;
        self.inspection = None;
        self.sandbox = None;
        self.breakpoints.rearm();
    }
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
    legend: bool,
    plots: bool,
    inspector: bool,
    /// The species wiki, the tree and the timeline (M5).
    wiki: bool,
    /// The genome editor and the debugger (M6).
    editor: bool,
    debugger: bool,
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
            legend: true,
            plots: true,
            inspector: false,
            wiki: false,
            editor: false,
            debugger: false,
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

fn setup(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());
}

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: EventReader<MouseMotion>,
    mut wheel: EventReader<MouseWheel>,
    mut view: ResMut<View>,
    mut sim: ResMut<SlideRes>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::Space) {
        view.paused = !view.paused;
        let speed = if view.paused { 0 } else { 1 };
        sim.slide.set_speed(speed);
    }
    if keys.just_pressed(KeyCode::Period) {
        // Step one tick, whatever the speed. A paused world you cannot advance is a
        // screenshot.
        sim.slide.request_step();
    }
    // Speed control, including "run as fast as the machine will go" (SPEC §14): the render
    // detaches from the tick rate rather than the tick rate bending to the render.
    for (key, speed) in [
        (KeyCode::Digit0, 0),
        (KeyCode::Minus, 1),
        (KeyCode::Equal, 8),
        (KeyCode::Backspace, 256),
    ] {
        if keys.just_pressed(key) {
            sim.slide.set_speed(speed);
            view.paused = speed == 0;
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
            sim.slide.toggle_overlay(i);
        }
    }
    if keys.just_pressed(KeyCode::KeyL) {
        view.legend = !view.legend;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        view.plots = !view.plots;
    }
    if keys.just_pressed(KeyCode::KeyI) {
        view.inspector = !view.inspector;
    }
    if keys.just_pressed(KeyCode::KeyO) {
        sim.slide.optics.enabled = !sim.slide.optics.enabled;
    }
    if keys.just_pressed(KeyCode::KeyW) {
        view.wiki = !view.wiki;
    }
    if keys.just_pressed(KeyCode::KeyE) {
        view.editor = !view.editor;
    }
    if keys.just_pressed(KeyCode::KeyD) {
        view.debugger = !view.debugger;
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

    for ev in wheel.read() {
        view.zoom = (view.zoom * (1.0 + ev.y * 0.1)).clamp(0.15, 40.0);
    }
    let scale = BASE_SCALE * view.zoom;
    sim.slide.set_zoom(scale);

    if buttons.pressed(MouseButton::Left) {
        for ev in motion.read() {
            // Both axes drag the slide with the pointer: push the mouse down and the slide
            // comes down with it, as though you had a finger on the plate. The vertical used
            // to be inverted — the double negative between "screen y is up, slide y is down"
            // and "the camera moves opposite to the content" had been applied once too often,
            // which is exactly the sort of thing that reads as correct on paper and wrong in
            // the hand.
            view.centre -= ev.delta / scale;
        }
    } else {
        motion.clear();
    }

    // Right-click applies the current tool. `Select` is the default and is the only one that
    // cannot change the world; the rest write, which is why the tool is chosen explicitly.
    if buttons.just_pressed(MouseButton::Right) {
        if let Some(cursor) = window.get_single().ok().and_then(|w| w.cursor_position()) {
            let size = window.get_single().map(|w| w.size()).unwrap_or(Vec2::ONE);
            let from_centre = cursor - size / 2.0;
            let slide_x = view.centre.x + from_centre.x / scale;
            let slide_y = view.centre.y + from_centre.y / scale;
            let square = (slide_x.floor() as i32, slide_y.floor() as i32);
            match view.tool {
                Tool::Select => {
                    sim.selected = sim.slide.cell_at(slide_x, slide_y, 3.0);
                    view.inspector = sim.selected.is_some();
                    // A new selection invalidates the sandbox: it was a copy of a different
                    // cell and showing it under a new name would be a lie.
                    sim.sandbox = None;
                }
                Tool::Move => {
                    if let Some(cell) = sim.selected {
                        let event =
                            tools::relocate(sim.slide.world_mut(), cell, square.0, square.1);
                        sim.last_tool = Some(event);
                    }
                }
                Tool::Remove => {
                    if let Some(cell) = sim.slide.cell_at(slide_x, slide_y, 3.0) {
                        let event = tools::remove(sim.slide.world_mut(), cell);
                        sim.last_tool = Some(event);
                        if sim.selected == Some(cell) {
                            sim.selected = None;
                            sim.sandbox = None;
                        }
                    }
                }
                Tool::DrawBarrier | Tool::EraseBarrier => {
                    if square.0 >= 0 && square.1 >= 0 {
                        let event = tools::set_barrier(
                            sim.slide.world_mut(),
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

/// Advance by a whole number of ticks, decided by the speed setting and nothing else.
///
/// Deliberately ignores `Time`: a simulation stepped by elapsed wall-clock would produce a
/// different world on a fast machine than on a slow one, and every guarantee in the spec rests
/// on it not doing that.
fn advance_simulation(mut sim: ResMut<SlideRes>) {
    // Breakpoints act on the viewer, not on the world: when one holds, the slide stops
    // advancing. Pausing provably does not change a world (`slide.rs`), so a breakpoint
    // cannot either — and there is no stop-in-the-middle-of-a-tick, because a tick is the
    // simulation's atom.
    if sim.breakpoints.tripped().is_none() {
        // The breakpoint set is taken out for the duration so that checking it — which needs
        // `&mut` for the tripped marker — can hold `&World` at the same time. Both live in the
        // same resource; neither can reach the other.
        let mut points = std::mem::take(&mut sim.breakpoints);
        let hit = points.check(sim.slide.world());
        sim.breakpoints = points;
        if hit {
            sim.slide.set_speed(0);
        }
    }
    sim.slide.advance_one_frame();
    sim.frame = sim.slide.frame();
    sim.inspection = sim.selected.and_then(|id| sim.slide.inspect(id));
    if sim.inspection.is_none() {
        // The cell died. Forget it rather than leaving a stale panel that looks live.
        sim.selected = None;
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
        ),
    >,
) {
    let frame = &sim.frame;
    let optics = &sim.slide.optics;
    let scale = BASE_SCALE * view.zoom;
    let Ok(window) = window.get_single() else {
        return;
    };
    let size = window.size();
    // Half-diagonal, for the field radius the vignette and the aberration are measured in.
    let half_diagonal = (size.x * size.x + size.y * size.y).sqrt() / 2.0;

    let to_screen = |x: f32, y: f32| -> Vec3 {
        Vec3::new(
            (x - view.centre.x) * scale,
            // Screen y is up, slide y is down.
            -(y - view.centre.y) * scale,
            0.0,
        )
    };
    // How far off the centre of the field a point is, as a fraction of the half-diagonal.
    let field_radius = |p: Vec3| -> f32 { (p.truncate().length() / half_diagonal).clamp(0.0, 1.0) };

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
                for k in 0..3 {
                    rgb[k] += layer.rgb[k] * shade / layers;
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

/// The legend, the plots and the inspector.
///
/// Reads `sim` immutably except for the speed control, which is the one thing on screen that
/// is *supposed* to reach the simulation — and it reaches it by setting a tick count, not by
/// touching a world.
fn panels(mut contexts: EguiContexts, mut sim: ResMut<SlideRes>, mut view: ResMut<View>) {
    let ctx = contexts.ctx_mut();
    let frame = sim.frame.clone();

    if view.legend {
        egui::Window::new("legend")
            .anchor(egui::Align2::LEFT_TOP, [8.0, 8.0])
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("tick {}", frame.tick));
                ui.label(format!("{} cells", frame.population));
                ui.label(match frame.lod {
                    Lod::Dots => "detail: points",
                    Lod::Organelles => "detail: organelles",
                    Lod::Full => "detail: full",
                });
                ui.separator();
                if frame.overlays.is_empty() {
                    ui.weak("no overlays — press 1-9");
                }
                for layer in &frame.overlays {
                    ui.horizontal(|ui| {
                        let [r, g, b] = layer.rgb;
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter().rect_filled(
                            rect,
                            2.0,
                            egui::Color32::from_rgb(
                                (r * 255.0) as u8,
                                (g * 255.0) as u8,
                                (b * 255.0) as u8,
                            ),
                        );
                        // The peak matters: each layer is normalised against its own maximum,
                        // so without this the colours are legible but meaningless.
                        ui.label(format!(
                            "{}  peak {:.1}",
                            layer.name,
                            layer.peak as f32 / mm_core::Q10_ONE as f32
                        ));
                    });
                }
                ui.separator();
                ui.label(format!("tool: {}  (F1-F5)", view.tool.name()));
                if let Some(event) = &sim.last_tool {
                    ui.small(format!("{event:?}"));
                }
                ui.horizontal(|ui| {
                    for (label, speed) in [("||", 0u32), ("1x", 1), ("8x", 8), ("fast", 256)] {
                        if ui.button(label).clicked() {
                            sim.slide.set_speed(speed);
                            view.paused = speed == 0;
                        }
                    }
                    if ui.button("step").clicked() {
                        sim.slide.request_step();
                    }
                });
            });
    }

    if view.plots {
        egui::Window::new("metrics")
            .anchor(egui::Align2::RIGHT_TOP, [-8.0, 8.0])
            .default_width(260.0)
            .show(ctx, |ui| {
                let history = sim.slide.history();
                if history.is_empty() {
                    ui.weak("no samples yet");
                    return;
                }
                let series: [(&str, Box<dyn Fn(&Sample) -> i64>); 4] = [
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
            });
    }

    if view.wiki {
        wiki_panel(ctx, &sim, &mut view);
    }

    if view.editor {
        editor_panel(ctx, &mut sim);
    }
    if view.debugger {
        debugger_panel(ctx, &mut sim);
    }

    if view.inspector {
        let inspection = sim.inspection.clone();
        egui::Window::new("cell")
            .anchor(egui::Align2::LEFT_BOTTOM, [8.0, -8.0])
            .default_width(300.0)
            .show(ctx, |ui| {
                let Some(c) = inspection else {
                    ui.weak("right-click a cell");
                    return;
                };
                ui.label(format!("species {}  age {}", c.species, c.age));
                ui.label(format!(
                    "energy {:.1}  mass {:.1}  damage {:.1}",
                    c.energy as f32 / mm_core::Q10_ONE as f32,
                    c.mass as f32 / mm_core::Q10_ONE as f32,
                    c.damage as f32 / mm_core::Q10_ONE as f32
                ));
                ui.label(format!(
                    "genome {} bytes  fidelity {:.2}",
                    c.genome_len,
                    c.fidelity as f32 / mm_core::Q10_ONE as f32
                ));
                ui.separator();
                ui.label(format!(
                    "ip {}  {}",
                    c.ip,
                    if c.halted { "halted" } else { "running" }
                ));
                ui.label(format!("stack {:?}", c.stack));
                ui.collapsing("registers", |ui| {
                    ui.label(format!("{:?}", c.registers));
                });
                ui.collapsing("ram", |ui| {
                    ui.label(format!("{:?}", c.ram));
                });
                ui.collapsing("organelles", |ui| {
                    for s in c.slots.iter().filter(|s| s.active || s.param > 0) {
                        ui.label(format!(
                            "{}: {:?} param {} control {:?}{}",
                            s.index,
                            s.kind,
                            s.param,
                            s.control,
                            match s.remaining_build {
                                Some(n) => format!("  building, {n} left"),
                                None => String::new(),
                            }
                        ));
                    }
                });
                ui.collapsing("chemistry", |ui| {
                    for (i, v) in c.interior.iter().enumerate() {
                        if *v != 0 {
                            ui.label(format!("{i}: {:.2}", *v as f32 / mm_core::Q10_ONE as f32));
                        }
                    }
                });
            });
    }
}

/// The species wiki, the phylogenetic tree and the world timeline (M5, SPEC §10.5).
///
/// Reads the archive through [`mm_app::wiki`], which copies everything out — so this panel
/// holds no borrow of the world and nothing in it can reach a tick.
fn wiki_panel(ctx: &egui::Context, sim: &SlideRes, view: &mut View) {
    let world = sim.slide.world();
    let archive = world.archive();

    egui::Window::new("wiki")
        .anchor(egui::Align2::RIGHT_BOTTOM, [-8.0, -8.0])
        .default_width(420.0)
        .default_height(460.0)
        .show(ctx, |ui| {
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
            let (rect, _) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
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
        });
}

/// The genome editor (M6).
///
/// Syntax highlighting comes from `mm_asm::highlight`, which classifies against the real
/// opcode table, and diagnostics come from actually assembling — so neither can drift from the
/// language the way a second, approximate definition in the front-end would.
fn editor_panel(ctx: &egui::Context, sim: &mut SlideRes) {
    egui::Window::new("editor")
        .anchor(egui::Align2::LEFT_TOP, [8.0, 220.0])
        .default_width(520.0)
        .default_height(420.0)
        .show(ctx, |ui| {
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
                        if let Some(file) = tools::copy_genome(sim.slide.world(), cell) {
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
        });
}

/// The debugger (M6).
///
/// Breakpoints act on the viewer; instruction stepping acts on a sandbox. Neither can reach
/// the simulation — see `debugger.rs` for why that is structural rather than careful.
fn debugger_panel(ctx: &egui::Context, sim: &mut SlideRes) {
    egui::Window::new("debugger")
        .anchor(egui::Align2::RIGHT_TOP, [-8.0, 300.0])
        .default_width(360.0)
        .show(ctx, |ui| {
            // --- breakpoints, over the live world ---
            ui.label("breakpoints");
            if let Some(tripped) = sim.breakpoints.tripped() {
                ui.colored_label(
                    egui::Color32::from_rgb(240, 200, 120),
                    format!("stopped: {}", tripped.describe()),
                );
                if ui.button("continue").clicked() {
                    sim.breakpoints.rearm();
                    sim.slide.set_speed(1);
                }
            }
            let tick = sim.slide.world().tick_count();
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
            let world_tick = sim.slide.world().tick_count();
            if ui.button("take from selected cell").clicked() {
                sim.sandbox = sim
                    .selected
                    .and_then(|cell| Sandbox::of(sim.slide.world(), cell));
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
        });
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
