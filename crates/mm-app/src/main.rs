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
//! that resolve into organelles as you zoom in. Pan with the mouse, zoom with the wheel,
//! space to pause, `.` to step, `1`–`9` to change which chemical the overlay shows.
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
//! **Unverified visually.** This was written on a headless machine: it compiles, and the
//! simulation-side guarantees are tested, but nobody has yet seen it draw a pixel. The LOD
//! tiers, the depth-of-field falloff, the chromatic aberration and the dust motes of SPEC §14
//! are not here — this is the "deliberately unstyled" viewer M2 asks for, which exists so that
//! no later milestone is developed blind, and it is the scaffolding M4's presentation layer
//! goes on top of.

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use mm_app::slide::{Frame, Slide};
use mm_core::Scenario;

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
}

impl SlideRes {
    fn new() -> SlideRes {
        // The default slide until scenario loading arrives (M6's slide save/load).
        let scenario = Scenario {
            width: 96,
            height: 96,
            ..Scenario::stress(96, 96)
        };
        let slide = Slide::new(scenario).expect("default scenario");
        let frame = slide.frame();
        SlideRes { slide, frame }
    }
}

/// Where the camera is looking. Purely presentational — none of it reaches the world.
#[derive(Resource)]
struct View {
    centre: Vec2,
    zoom: f32,
    paused: bool,
}

impl Default for View {
    fn default() -> Self {
        View {
            centre: Vec2::new(48.0, 48.0),
            zoom: 1.0,
            paused: false,
        }
    }
}

#[derive(Component)]
struct OverlaySquare(usize);

#[derive(Component)]
struct CellSprite(usize);

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
    // Which chemical the overlay shows.
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
            sim.slide.set_overlay(i);
        }
    }

    for ev in wheel.read() {
        view.zoom = (view.zoom * (1.0 + ev.y * 0.1)).clamp(0.15, 40.0);
    }
    if buttons.pressed(MouseButton::Left) {
        let scale = BASE_SCALE * view.zoom;
        for ev in motion.read() {
            // Screen y is up and slide y is down, so dragging follows the pointer.
            view.centre -= ev.delta / scale * Vec2::new(1.0, -1.0);
        }
    } else {
        motion.clear();
    }
}

/// Advance by a whole number of ticks, decided by the speed setting and nothing else.
///
/// Deliberately ignores `Time`: a simulation stepped by elapsed wall-clock would produce a
/// different world on a fast machine than on a slow one, and every guarantee in the spec rests
/// on it not doing that.
fn advance_simulation(mut sim: ResMut<SlideRes>) {
    sim.slide.advance_one_frame();
    let frame = sim.slide.frame();
    sim.frame = frame;
}

#[allow(clippy::type_complexity)]
fn redraw(
    mut commands: Commands,
    sim: Res<SlideRes>,
    view: Res<View>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut squares: Query<(&OverlaySquare, &mut Sprite, &mut Transform), Without<CellSprite>>,
    mut cells: Query<(Entity, &CellSprite, &mut Sprite, &mut Transform), Without<OverlaySquare>>,
) {
    let frame = &sim.frame;
    let scale = BASE_SCALE * view.zoom;
    let Ok(_window) = window.get_single() else {
        return;
    };
    let to_screen = |x: f32, y: f32| -> Vec3 {
        Vec3::new(
            (x - view.centre.x) * scale,
            // Screen y is up, slide y is down.
            -(y - view.centre.y) * scale,
            0.0,
        )
    };

    // The chemical field and the light, as one square each. Spawned once and then updated,
    // because respawning a quarter of a million sprites a frame is not a rendering strategy.
    if squares.is_empty() {
        for i in 0..frame.overlay.len() {
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
        let (Some(v), Some(l)) = (frame.overlay.get(i), frame.light.get(i)) else {
            continue;
        };
        let x = (i % frame.width.max(1) as usize) as f32;
        let y = (i / frame.width.max(1) as usize) as f32;
        let [r, g, b] = frame.overlay_rgb;
        // Light as a warm luminance under the chemical's own colour (SPEC §14).
        let warm = 0.10 * l;
        sprite.color = Color::srgb(r * v + warm, g * v + warm * 0.92, b * v + warm * 0.75);
        sprite.custom_size = Some(Vec2::splat(scale));
        transform.translation = to_screen(x + 0.5, y + 0.5);
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
    for (_entity, marker, mut sprite, mut transform) in &mut cells {
        match frame.cells.get(marker.0) {
            Some(dot) => {
                sprite.color = Color::srgb(dot.rgb[0], dot.rgb[1], dot.rgb[2]);
                sprite.custom_size = Some(Vec2::splat((dot.radius * 2.0 * scale).max(1.5)));
                transform.translation = to_screen(dot.x, dot.y).with_z(1.0);
            }
            None => sprite.color = Color::NONE,
        }
    }
}
