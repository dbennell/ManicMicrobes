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
//! | `o` | optics on/off |
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

use mm_app::inspector::Inspection;
use mm_app::slide::{Frame, Lod, Slide};
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
            // Screen y is up and slide y is down, so dragging follows the pointer.
            view.centre -= ev.delta / scale * Vec2::new(1.0, -1.0);
        }
    } else {
        motion.clear();
    }

    // Selection for the inspector. Picks the nearest cell within a couple of squares, so a
    // click near a cell at far zoom still finds it.
    if buttons.just_pressed(MouseButton::Right) {
        if let Some(cursor) = window.get_single().ok().and_then(|w| w.cursor_position()) {
            let size = window.get_single().map(|w| w.size()).unwrap_or(Vec2::ONE);
            let from_centre = cursor - size / 2.0;
            let slide_x = view.centre.x + from_centre.x / scale;
            let slide_y = view.centre.y + from_centre.y / scale;
            sim.selected = sim.slide.cell_at(slide_x, slide_y, 3.0);
            view.inspector = sim.selected.is_some();
        }
    }
}

/// Advance by a whole number of ticks, decided by the speed setting and nothing else.
///
/// Deliberately ignores `Time`: a simulation stepped by elapsed wall-clock would produce a
/// different world on a fast machine than on a slow one, and every guarantee in the spec rests
/// on it not doing that.
fn advance_simulation(mut sim: ResMut<SlideRes>) {
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
