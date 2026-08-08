//! The cell shader, with nothing behind it.
//!
//! ```text
//! cargo run -p mm-app --bin shaderbench --features render --release
//! ```
//!
//! # What this is for
//!
//! The flickering overlaps have been chased for three days through the physics, the contact set,
//! the neighbour index and the seam solve, and the two things named in the original report —
//! *the fragment shader, or the data handed to it* — could not be tested, because every
//! experiment had to run a world to get a picture.
//!
//! There is no world here. The cells come from [`mm_app::phantom`], which places them by
//! arithmetic on a frame number and computes their seams all-pairs, and they are drawn through
//! the same `cell.wgsl`, the same `Material2d` and the same vertex layout the microscope uses —
//! `mm_app::cellpipe`, which exists so that the two cannot drift apart.
//!
//! So the bench splits the question in half:
//!
//! * **the artefact appears here** → the data was correct by construction, so it is the shader,
//!   the attribute packing, or the pixel grid.
//! * **it does not** → the shader is exonerated on this scene, and the fault is upstream in
//!   `slide.rs`. Then turn on the injections — `cap`, `reach`, `churn`, `staircase` — until it
//!   does appear, and that is the fault, named.
//!
//! # The controls that matter
//!
//! `Motion` is the first thing to move. **Drift** and **Orbit** are rigid: every cell keeps every
//! distance to every neighbour exactly, so no seam, face or swell changes by a float — all that
//! changes is where the outlines land on the pixel grid. If the picture crawls under *those*, no
//! data can be the cause and the answer is in the shader or in sampling. **Jitter** is the only
//! motion that genuinely moves cells relative to each other, and **Breathe** changes sizes without
//! moving anything.
//!
//! `outline` overlays the outline this frame's data *says* the cells have, as dots, computed on
//! the CPU by `phantom::Drawn::outline`. Where the drawn edge and the dots part company, the
//! shader and its inputs disagree, and you can see which way round.
//!
//! `cell.wgsl` is **hot-reloaded**: save the file and the next frame is drawn with it, without a
//! rebuild. That is what makes bisecting the field itself practical — comment out the wobble, set
//! `slack` to zero, drop the `smax` shoulder, and look, one edit at a time.
//!
//! # Photographs
//!
//! ```text
//! MM_BENCH_SHOT=/tmp/bench.png MM_BENCH_AT=120 MM_BENCH_MOTION=jitter \
//!   cargo run -p mm-app --bin shaderbench --features render --release
//! ```
//!
//! Every knob has an environment variable, and a frame is a pure function of its number, so two
//! runs a week apart photograph the same thing and the difference between two `.png`s means
//! something. Describing screenshots is how this went wrong for a long time: two changes were
//! reported as visible improvements when a pixel diff later showed they had altered nothing.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::Mesh2d;
use bevy::prelude::*;
use bevy::sprite_render::MeshMaterial2d;
use bevy::window::PrimaryWindow;
use bevy::winit::WinitSettings;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};

use mm_app::cellmesh::{self, FIELD_FILL, SQUASH_PER_CELL};
use mm_app::cellpipe;
use mm_app::phantom::{self, Bench, Drawn, Layout, Motion};

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Manic Microbes — shader bench".to_string(),
            ..default()
        }),
        ..default()
    }));
    cellpipe::plugin(&mut app);
    app.add_plugins(EguiPlugin::default())
        // Continuously, focused or not. Bevy's default waits a second between redraws for an
        // unfocused window, and a bench driven from a script never has focus — which makes a run
        // to frame 90 take a minute and a half and a `MM_BENCH_SHOT` look like a hang. The
        // microscope has the same behaviour and the note in `main::screenshot` records it.
        .insert_resource(WinitSettings::game())
        // The slide's own near-black by default, and settable — because the way to measure what
        // the shader actually drew is to photograph one frame against two different backgrounds
        // and solve each pixel for its alpha. See `tools/check_outline.py`.
        .insert_resource(ClearColor(background()))
        .insert_resource(State::from_env())
        .add_systems(Startup, setup)
        .add_systems(Update, (keys, watch_shader, redraw, photograph).chain())
        .add_systems(EguiPrimaryContextPass, panel)
        .run();
}

/// The bench, its knobs, and where it has got to.
#[derive(Resource)]
struct State {
    bench: Bench,
    /// The frame number the phantom is evaluated at. **Not** Bevy's frame count: it stops when
    /// paused and moves by one on a step, so a picture can be held still and compared.
    at: u64,
    running: bool,
    /// One frame, then stop again.
    step: bool,
    /// Pixels per substrate square. `slide::Lod` calls 28 packed and 48 full; the microscope's
    /// magnification readout is this times 12.5.
    zoom: f32,
    /// Draw what the data says the outline is, over what the shader drew.
    outline: bool,
    /// The vertex buffers, reused so a steady scene allocates nothing.
    buffers: cellmesh::Buffers,
    /// Last modified time of `cell.wgsl`, for the hot reload.
    shader_stamp: Option<std::time::SystemTime>,
    /// What the reload said, for the panel.
    shader_note: String,
    /// This frame's measurements, and the previous frame's cells to compare against.
    report: phantom::Report,
    flicker: phantom::Flicker,
    previous: Vec<Drawn>,
    /// Where to photograph to, and at which frame, from the environment.
    shot: Option<(String, u64)>,
    /// How many photographs, and how many frames apart.
    ///
    /// A series is how the bench stands in for motion: the cells are at a different fixed
    /// position in each one, and every frame is a pure function of its number, so the whole
    /// series is reproducible and can be re-measured or re-photographed a week later. A single
    /// frame can only ever show whether the picture is right *once*.
    series: u32,
    every: u64,
    taken: u32,
    shot_taken: bool,
    /// Where to write the same frame's geometry as numbers. See [`dump`].
    dump: Option<String>,
    show_panel: bool,
    /// The signed-distance field, or plain quads. Both go through the same draw call, and the
    /// quads are the bench's calibration, because a quad's edges are where the vertices are and
    /// nothing else. If those do not measure where they were put, the disagreement is in the
    /// picture's coordinates and not in the field. The microscope has no such switch — it always
    /// draws the field.
    rounded: bool,
    /// Draw through `cellpipe::DotMaterial` — the narrow layout the microscope uses below
    /// `slide::Lod::Packed` — instead of `CellMaterial`.
    ///
    /// **This exists to be photographed twice.** The dot shader is meant to draw the identical
    /// picture wherever no seam is cutting, and "meant to" is the kind of claim that stops being
    /// true the first time someone edits one of the two and not the other. With `MM_BENCH_CAP=0`
    /// every cell has an empty seam list and a swell of exactly one — which is precisely what
    /// `slide.rs` hands the renderer below the tier — so the two materials are being given the
    /// same data and must produce the same pixels:
    ///
    /// ```text
    /// MM_BENCH_CAP=0 MM_BENCH_PANEL=0 MM_BENCH_AT=30 MM_BENCH_SHOT=/tmp/seamed.png ./shaderbench
    /// MM_BENCH_CAP=0 MM_BENCH_PANEL=0 MM_BENCH_AT=30 MM_BENCH_DOTS=1 \
    ///   MM_BENCH_SHOT=/tmp/plain.png ./shaderbench
    /// tools/compare_shots.py /tmp/seamed_000.png /tmp/plain_000.png --max-delta 2 --max-pixels 100
    /// ```
    ///
    /// **Not `cmp`, and the reason is the whole point of the tool.** The two are the same picture
    /// but not the same bytes: the driver may reassociate `mix` and `smax`, which moves the
    /// outline by a unit in the last place, which moves the antialiasing ramp by a level of grey.
    /// The note above `DotVertex` in `cell.wgsl` works through why. What that measures today is 14
    /// pixels of 921,600, worst delta 2 of 255, 13 of them on an edge — so the tolerance above is
    /// roughly seven times the observed difference and still nowhere near what a real divergence
    /// would produce.
    dots: bool,
}

impl State {
    fn from_env() -> Self {
        let num = |key: &str, or: f32| -> f32 {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(or)
        };
        let bench = Bench {
            layout: match std::env::var("MM_BENCH_LAYOUT").as_deref() {
                Ok("pair") => Layout::Pair,
                Ok("fifteen") => Layout::Fifteen,
                Ok("hex") => Layout::Hex,
                Ok("scatter") => Layout::Scatter,
                Ok("raft") => Layout::Raft,
                _ => Layout::Nine,
            },
            motion: match std::env::var("MM_BENCH_MOTION").as_deref() {
                Ok("still") => Motion::Still,
                Ok("jitter") => Motion::Jitter,
                Ok("orbit") => Motion::Orbit,
                Ok("breathe") => Motion::Breathe,
                _ => Motion::Drift,
            },
            spacing: num("MM_BENCH_SPACING", 1.0),
            spread: num("MM_BENCH_SPREAD", 0.3),
            dither: num("MM_BENCH_DITHER", 0.1),
            amplitude: num("MM_BENCH_AMPLITUDE", 0.05),
            speed: num("MM_BENCH_SPEED", 0.02),
            firmness: num("MM_BENCH_FIRMNESS", 0.0).clamp(0.0, 1.0),
            cap: num("MM_BENCH_CAP", SQUASH_PER_CELL as f32) as usize,
            reach: num("MM_BENCH_REACH", 1.52),
            churn: num("MM_BENCH_CHURN", 0.0),
            staircase: num("MM_BENCH_STAIRCASE", 0.0) > 0.5,
        };
        let shot = std::env::var("MM_BENCH_SHOT")
            .ok()
            .map(|path| (path, num("MM_BENCH_AT", 120.0) as u64));
        let dump = std::env::var("MM_BENCH_DUMP").ok();
        State {
            bench,
            at: 0,
            running: true,
            step: false,
            zoom: num("MM_BENCH_ZOOM", 70.0),
            outline: num("MM_BENCH_OUTLINE", 0.0) > 0.5,
            buffers: cellmesh::Buffers::default(),
            shader_stamp: None,
            shader_note: String::new(),
            report: phantom::Report::default(),
            flicker: phantom::Flicker::default(),
            previous: Vec::new(),
            shot,
            series: num("MM_BENCH_SERIES", 1.0).max(1.0) as u32,
            every: num("MM_BENCH_EVERY", 1.0).max(1.0) as u64,
            taken: 0,
            shot_taken: false,
            dump,
            show_panel: num("MM_BENCH_PANEL", 1.0) > 0.5,
            rounded: num("MM_BENCH_ROUNDED", 1.0) > 0.5,
            dots: num("MM_BENCH_DOTS", 0.0) > 0.5,
        }
    }
}

/// What the slide is drawn against. `MM_BENCH_BG=1,1,1` for white.
fn background() -> Color {
    let parts: Option<Vec<f32>> = std::env::var("MM_BENCH_BG")
        .ok()
        .map(|v| v.split(',').filter_map(|p| p.trim().parse().ok()).collect());
    match parts.as_deref() {
        Some([r, g, b]) => Color::srgb(*r, *g, *b),
        _ => Color::srgb(0.02, 0.02, 0.03),
    }
}

/// The one entity every cell of the phantom is drawn as.
#[derive(Component)]
struct CellMesh;

fn setup(
    mut commands: Commands,
    state: Res<State>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<cellpipe::CellMaterial>>,
    mut dot_materials: ResMut<Assets<cellpipe::DotMaterial>>,
) {
    // Order -1 so egui composites over the slide, as the microscope does it.
    commands.spawn((
        Camera2d,
        Camera {
            order: -1,
            ..default()
        },
    ));
    // The vertices are rewritten every frame in screen space, so the bounding box Bevy
    // computes from the first frame is wrong by the second.
    let common = (CellMesh, NoFrustumCulling, Transform::from_xyz(0.0, 0.0, 1.0));
    // One or the other, chosen once. The bench never crosses a tier mid-run — the whole point of
    // it is that a frame is a pure function of its number — so there is nothing to switch.
    if state.dots {
        commands.spawn((
            common,
            Mesh2d(meshes.add(cellpipe::dot_mesh())),
            MeshMaterial2d(dot_materials.add(cellpipe::DotMaterial {})),
        ));
    } else {
        commands.spawn((
            common,
            Mesh2d(meshes.add(cellpipe::empty_mesh())),
            MeshMaterial2d(materials.add(cellpipe::CellMaterial {})),
        ));
    }
}

fn keys(input: Res<ButtonInput<KeyCode>>, mut state: ResMut<State>) {
    if input.just_pressed(KeyCode::Space) {
        state.running = !state.running;
    }
    if input.just_pressed(KeyCode::Period) {
        state.step = true;
    }
    if input.just_pressed(KeyCode::Comma) {
        // Backwards, which a closed form in the frame number makes free.
        state.at = state.at.saturating_sub(1);
        state.running = false;
    }
    if input.just_pressed(KeyCode::KeyP) {
        state.show_panel = !state.show_panel;
    }
    if input.just_pressed(KeyCode::KeyO) {
        state.outline = !state.outline;
    }
    if input.just_pressed(KeyCode::KeyM) {
        let next = Motion::ALL
            .iter()
            .position(|m| *m == state.bench.motion)
            .map_or(0, |i| (i + 1) % Motion::ALL.len());
        state.bench.motion = Motion::ALL[next];
    }
    if input.just_pressed(KeyCode::KeyL) {
        state.bench.layout = state.bench.layout.next();
    }
    if input.just_pressed(KeyCode::KeyR) {
        state.at = 0;
    }
    if input.just_pressed(KeyCode::Equal) {
        state.zoom = (state.zoom * 1.25).min(2000.0);
    }
    if input.just_pressed(KeyCode::Minus) {
        state.zoom = (state.zoom / 1.25).max(2.0);
    }
}

/// Redraw `cell.wgsl` from disk when it changes on disk.
///
/// A poll rather than a watcher: one `stat` a frame against a build of Bevy that would otherwise
/// need the `file_watcher` feature, which the application does not have and should not gain for
/// the sake of a bench.
fn watch_shader(mut state: ResMut<State>, mut shaders: ResMut<Assets<Shader>>) {
    let Ok(meta) = std::fs::metadata(cellpipe::CELL_SHADER_PATH) else {
        return;
    };
    let Ok(stamp) = meta.modified() else {
        return;
    };
    if state.shader_stamp == Some(stamp) {
        return;
    }
    let first = state.shader_stamp.is_none();
    state.shader_stamp = Some(stamp);
    // The first frame already has the compiled-in copy, and they are the same file.
    if first {
        state.shader_note = "compiled in".into();
        return;
    }
    state.shader_note = match cellpipe::reload(&mut shaders, cellpipe::CELL_SHADER_PATH) {
        Ok(()) => format!("reloaded at frame {}", state.at),
        Err(e) => format!("reload failed: {e}"),
    };
    eprintln!("cell.wgsl: {}", state.shader_note);
}

/// Distinct hues, because two cells drawn the same colour have no wall to look at.
///
/// Walked by the golden angle so that neighbours in a lattice are never near each other in hue,
/// and kept off full saturation so that the shader's own shading still reads.
fn colour(id: u64) -> [f32; 3] {
    let h = (id as f32 * 0.618_034).fract() * 6.0;
    let s = 0.45;
    let v = 0.85;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i.rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// The seams, as the mesh wants them: twelve slots, deepest first, and `y` turned over.
fn slots(cell: &Drawn) -> [cellmesh::Squash; SQUASH_PER_CELL] {
    let mut out = [cellmesh::Squash::default(); SQUASH_PER_CELL];
    let mut seams: Vec<&mm_app::slide::Squash> = cell.seams.iter().collect();
    if seams.len() > SQUASH_PER_CELL {
        seams.sort_by(|a, b| {
            a.face
                .partial_cmp(&b.face)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    for (slot, s) in out.iter_mut().zip(seams) {
        *slot = cellmesh::Squash {
            nx: s.nx,
            // Negated, because the screen's rows run upwards where the slide's run down. The
            // microscope does exactly this in `main::squash_of`, and a seam that did not turn
            // over with everything else flattened the cell on the side where there was nobody.
            ny: -s.ny,
            face: s.face,
        };
    }
    out
}

fn redraw(
    mut state: ResMut<State>,
    mut meshes: ResMut<Assets<Mesh>>,
    mesh: Query<&Mesh2d, With<CellMesh>>,
) {
    if state.running || state.step {
        state.at = state.at.wrapping_add(1);
        state.step = false;
    }
    let cells = state.bench.frame(state.at);
    state.report = phantom::inspect(&cells);
    if state.previous.len() == cells.len() {
        let previous = std::mem::take(&mut state.previous);
        state.flicker = phantom::flicker(&previous, &cells);
        state.previous = previous;
    }
    state.previous = cells.clone();

    let zoom = state.zoom;
    // The phantom is centred on the origin and so is the camera, so the transform is the zoom and
    // the turn-over. `y` is negated exactly as `main::to_screen` negates it.
    let to_screen = |x: f32, y: f32| Vec2::new(x * zoom, -y * zoom);

    let outline = state.outline;
    let rounded = if state.rounded { 1.0 } else { 0.0 };
    // Whichever layout `setup` spawned a mesh for. The zoom does not choose it here as it does in
    // the microscope: the bench's subject is the seams, so which material draws them is a knob.
    let detail = if state.dots {
        cellmesh::Detail::Plain
    } else {
        cellmesh::Detail::Seamed
    };
    let buffers = &mut state.buffers;
    buffers.begin(cells.len() * if outline { 4 } else { 1 }, detail);
    for cell in &cells {
        let at = to_screen(cell.blob.x, cell.blob.y);
        // Exactly what the microscope computes, and for the same reasons — see `main::redraw`.
        let body = cell.bare * 2.0 * zoom * cell.swell;
        let [r, g, b] = colour(cell.blob.id);
        buffers.push(cellmesh::Placed {
            x: at.x,
            y: at.y,
            half: body / (2.0 * FIELD_FILL),
            rgba: [r, g, b, 1.0],
            shape: cellmesh::Shape {
                seed: cellmesh::seed_of(cell.blob.id),
                softness: 0.0,
                integrity: 1.0,
                rounded,
            },
            squash: slots(cell),
            swell: cell.swell,
        });
    }
    // The outline the *data* says each cell has, laid over what the shader drew. Plain quads —
    // `rounded: 0` is the same draw call and no field at all — so a dot is a dot at any zoom.
    if outline {
        for cell in &cells {
            // Enough rays that the dots meet: one every pixel and a bit around the outline as it
            // is actually drawn. A fixed count leaves gaps at high magnification, and a ray that
            // falls between two dots is a disagreement that is not there — which is a bad thing
            // for an instrument whose whole job is to show a disagreement that is.
            let around = std::f32::consts::TAU * cell.bare * cell.swell * zoom;
            let rays = ((around / 1.2) as usize).clamp(64, 4096);
            for k in 0..rays {
                let theta = std::f32::consts::TAU * k as f32 / rays as f32;
                let d = cell.outline(theta, true);
                let (sy, sx) = theta.sin_cos();
                let at = to_screen(cell.blob.x + sx * d, cell.blob.y + sy * d);
                buffers.push(cellmesh::Placed {
                    x: at.x,
                    y: at.y,
                    half: 0.6,
                    rgba: [1.0, 1.0, 1.0, 1.0],
                    shape: cellmesh::Shape {
                        seed: 0.0,
                        softness: 0.0,
                        integrity: 1.0,
                        rounded: 0.0,
                    },
                    squash: Default::default(),
                    swell: 1.0,
                });
            }
        }
    }

    // Exactly one, as in the microscope and for the same reason: `upload` swaps the vertices
    // across rather than copying them, so a second mesh here would get the frame before last.
    if let Ok(handle) = mesh.single() {
        if let Some(mut m) = meshes.get_mut(&handle.0) {
            cellpipe::upload(&mut m, &mut state.buffers);
        }
    }
}

/// `MM_BENCH_SHOT`: photograph one frame and leave.
///
/// The only way a change to a shader can be checked without a person at the window, and the way
/// two versions of it are compared: same frame number, same knobs, two files, one pixel diff.
/// Write the frame's geometry in the photograph's own pixel coordinates.
///
/// The point of the pair is that a claim about the picture becomes checkable: the `.png` is what
/// the shader drew and this is what it was told to draw, in the same units, so the difference
/// between them is a number rather than an impression. `tools/check_outline.py` takes both.
///
/// Angles run anticlockwise from +x in *world* terms, which is clockwise in the image, because a
/// world offset maps to a pixel offset with no turn-over at all: the camera negates y and the
/// image's rows run down, and the two cancel.
fn dump(state: &State, window: &Window, path: &str) -> std::io::Result<()> {
    use std::fmt::Write as _;
    let (w, h) = (window.width(), window.height());
    let cells = state.bench.frame(state.at);
    let rays = 720usize;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# shader bench frame {} — {} cells, {:.1} px per square, window {w:.0}x{h:.0}",
        state.at,
        cells.len(),
        state.zoom,
    );
    let _ = writeln!(
        out,
        "# cell <id> <centre x px> <centre y px> <drawn radius px> <swell> <r> <g> <b>"
    );
    let _ = writeln!(out, "# rays <id> <n> then n outline radii in px, from +x, one per 360/n");
    let _ = writeln!(
        out,
        "# seam <id> <slot> <nx> <ny> <face px> — exactly the twelve slots the mesh carries, \
         after packing and unpacking, in the shader's own frame"
    );
    for c in &cells {
        let px = w * 0.5 + c.blob.x * state.zoom;
        let py = h * 0.5 + c.blob.y * state.zoom;
        let [cr, cg, cb] = colour(c.blob.id);
        let _ = writeln!(
            out,
            "cell {} {px:.3} {py:.3} {:.3} {:.5} {cr:.4} {cg:.4} {cb:.4}",
            c.blob.id,
            c.bare * c.swell * state.zoom,
            c.swell,
        );
        // The seams as the *mesh* has them, not as the phantom computed them: through `slots`,
        // and through the 16-bit pack and unpack the shader will do. If a seam is going to be lost
        // or mangled on the way to the GPU, it is lost here and this is where it shows.
        for (slot, sq) in slots(c).iter().enumerate() {
            let packed = cellmesh::pack_normal(sq.nx, sq.ny);
            let bits = packed.to_bits();
            let un = |half: u32| -> f32 {
                let v = (half & 0xFFFF) as u16 as i16;
                f32::from(v) / 32767.0
            };
            let (nx, ny) = (un(bits), un(bits >> 16));
            let _ = writeln!(
                out,
                "seam {} {slot} {nx:.6} {ny:.6} {:.4}",
                c.blob.id,
                sq.face * c.bare * c.swell * state.zoom,
            );
        }
        let _ = write!(out, "rays {} {rays}", c.blob.id);
        for k in 0..rays {
            let theta = std::f32::consts::TAU * k as f32 / rays as f32;
            let _ = write!(out, " {:.4}", c.outline(theta, true) * state.zoom);
        }
        out.push('\n');
    }
    std::fs::write(path, out)
}

/// `slide.png` and shot 3 becomes `slide_003.png`, as `main::numbered_path` does it, so that a
/// series and a lone frame are named the same way and one script reads both.
fn numbered(path: &str, n: u32) -> String {
    match path.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}_{n:03}.{ext}"),
        None => format!("{path}_{n:03}"),
    }
}

fn photograph(
    mut commands: Commands,
    mut state: ResMut<State>,
    mut exit: MessageWriter<AppExit>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut settled: Local<u32>,
) {
    let Some((path, at)) = state.shot.clone() else {
        return;
    };
    if state.shot_taken {
        // A frame or two for the encode to reach the disk, then out.
        *settled += 1;
        if *settled > 3 {
            exit.write(AppExit::Success);
        }
        return;
    }
    // Not before the mesh has actually reached the GPU. The first frames render an empty buffer
    // while the asset is uploaded, and a photograph taken then is a picture of the clear colour —
    // which the checker reports as "no ray left the clump", several steps from the cause.
    *settled += 1;
    let due = at + u64::from(state.taken) * state.every;
    if state.at < due || *settled < 8 {
        return;
    }
    *settled = 0;
    let n = state.taken;
    let file = numbered(&path, n);
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(file.clone()));
    if let (Some(where_to), Ok(window)) = (state.dump.clone(), windows.single()) {
        let where_to = numbered(&where_to, n);
        match dump(&state, window, &where_to) {
            Ok(()) => {}
            Err(e) => eprintln!("shader bench: could not write {where_to}: {e}"),
        }
    }
    eprintln!("shader bench: frame {} photographed to {file}", state.at);
    state.taken += 1;
    if state.taken >= state.series {
        state.shot_taken = true;
        state.running = false;
    }
}

fn panel(mut contexts: EguiContexts, mut state: ResMut<State>) {
    // `MM_BENCH_PANEL=0` draws the slide and nothing else, for photographs that are going to be
    // measured: the panel is opaque, it covers the leftmost cells at any real magnification, and
    // every ray that runs into it is a ray that cannot be checked.
    if !state.show_panel {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let ctx = ctx.clone();
    egui::Window::new("shader bench")
        .default_width(340.0)
        .show(&ctx, |ui| {
            let s = &mut *state;
            ui.horizontal(|ui| {
                if ui
                    .button(if s.running { "⏸ pause" } else { "▶ run" })
                    .clicked()
                {
                    s.running = !s.running;
                }
                if ui.button("step ›").clicked() {
                    s.step = true;
                    s.running = false;
                }
                if ui.button("‹ back").clicked() {
                    s.at = s.at.saturating_sub(1);
                    s.running = false;
                }
                ui.label(format!("frame {}", s.at));
            });

            ui.separator();
            ui.label(egui::RichText::new("the specimen").strong());
            egui::ComboBox::from_label("layout")
                .selected_text(s.bench.layout.label())
                .show_ui(ui, |ui| {
                    for l in Layout::ALL {
                        ui.selectable_value(&mut s.bench.layout, l, l.label());
                    }
                });
            ui.add(
                egui::Slider::new(&mut s.bench.spacing, 0.6..=1.4)
                    .text("spacing")
                    .custom_formatter(|v, _| format!("{v:.3} of touching")),
            );
            ui.add(egui::Slider::new(&mut s.bench.spread, 0.0..=0.6).text("size spread"));
            ui.add(
                egui::Slider::new(&mut s.bench.dither, 0.0..=0.35)
                    .text("dither off the lattice")
                    .custom_formatter(|v, _| format!("{v:.2} of a pitch")),
            );
            if s.bench.dither < 0.01 {
                ui.label("a clean lattice: every distance and every normal is one of a handful, \
                          which is where an off-by-one hides");
            }

            ui.separator();
            ui.label(egui::RichText::new("what is moving").strong());
            egui::ComboBox::from_label("motion")
                .selected_text(s.bench.motion.label())
                .show_ui(ui, |ui| {
                    for m in Motion::ALL {
                        ui.selectable_value(&mut s.bench.motion, m, m.label());
                    }
                });
            ui.label(match s.bench.motion {
                Motion::Still => "the control: the one condition the artefact is not reported in",
                Motion::Drift | Motion::Orbit => {
                    "rigid — no distance between any two cells changes, so no seam, face or \
                     swell can. Anything that crawls under this is the shader or the pixel grid."
                }
                Motion::Jitter => "the only motion that moves cells relative to each other",
                Motion::Breathe => "nobody moves; the radii change, as the mm-core staircase does",
            });
            ui.add(
                egui::Slider::new(&mut s.bench.amplitude, 0.0..=0.4)
                    .text("amplitude")
                    .custom_formatter({
                        let zoom = f64::from(s.zoom);
                        move |v, _| format!("{v:.3} sq = {:.2} px", v * zoom)
                    }),
            );
            ui.add(egui::Slider::new(&mut s.bench.speed, 0.0..=0.3).text("speed"));

            ui.separator();
            // Above the faults, and separated from them, because it is not one. Everything below
            // this asks "what if the data were wrong"; this asks "what should the picture be".
            ui.label(egui::RichText::new("what a cell is made of").strong());
            ui.label("0 is a bag of fluid that tiles into polygons — 1 is a walled body that stays round");
            ui.add(
                egui::Slider::new(&mut s.bench.firmness, 0.0..=1.0)
                    .text("firmness")
                    .custom_formatter(|v, _| {
                        format!(
                            "{v:.2}  {}",
                            match v {
                                x if x < 0.05 => "foam",
                                x if x < 0.35 => "soft",
                                x if x < 0.65 => "half",
                                x if x < 0.95 => "firm",
                                _ => "marbles",
                            }
                        )
                    }),
            );

            ui.separator();
            ui.label(egui::RichText::new("faults to inject").strong());
            ui.label("all off is data correct by construction — all-pairs, uncapped, in reach");
            ui.checkbox(&mut s.bench.staircase, "mm-core's radius staircase");
            ui.add(
                egui::Slider::new(&mut s.bench.cap, 0..=SQUASH_PER_CELL)
                    .text("seam cap (12 is the mesh's)"),
            );
            ui.add(
                egui::Slider::new(&mut s.bench.reach, 0.8..=2.0)
                    .text("neighbour reach")
                    .custom_formatter(|v, _| format!("{v:.2} of the drawn radii")),
            );
            ui.add(
                egui::Slider::new(&mut s.bench.churn, 0.0..=1.0)
                    .text("seam churn")
                    .custom_formatter(|v, _| format!("{:.0}% of cells lose one a frame", v * 100.0)),
            );

            ui.separator();
            ui.label(egui::RichText::new("the instrument").strong());
            ui.add(
                egui::Slider::new(&mut s.zoom, 4.0..=600.0)
                    .logarithmic(true)
                    .text("zoom")
                    .custom_formatter(|v, _| format!("{v:.0} px/sq = {:.0}×", v * 12.5)),
            );
            ui.checkbox(&mut s.outline, "outline the data says (white dots)");
            ui.label(format!("cell.wgsl: {}", s.shader_note));

            ui.separator();
            ui.label(egui::RichText::new("this frame").strong());
            let r = s.report;
            ui.label(format!(
                "{} cells, {} pairs in contact, max {} seams",
                r.cells, r.touching, r.max_seams
            ));
            let no_wall = format!("{} pairs overlapping with no wall", r.no_wall);
            ui.label(if r.no_wall == 0 {
                egui::RichText::new(no_wall).color(egui::Color32::from_rgb(120, 200, 140))
            } else {
                egui::RichText::new(format!("{no_wall}, worst {:.1}%", 100.0 * r.worst))
                    .color(egui::Color32::from_rgb(240, 140, 120))
            });
            let cross = format!("worst crossing {:.4} squares", r.wall_cross);
            ui.label(if r.wall_cross < 1e-3 {
                egui::RichText::new(cross).color(egui::Color32::from_rgb(120, 200, 140))
            } else {
                egui::RichText::new(format!("{cross} — a cell is drawn over its neighbour"))
                    .color(egui::Color32::from_rgb(240, 140, 120))
            });
            ui.label(format!(
                "worst gap between pressed cells {:.4} squares",
                r.wall_gap
            ));
            ui.label(format!("swell {:.3}..{:.3}", r.swell_lo, r.swell_hi));
            let f = s.flicker;
            ui.label(format!(
                "since last frame: {} cells resized (worst {:.1}%), {} changed seam count",
                f.resizing,
                100.0 * f.worst_swell,
                f.churned
            ));
            ui.label(format!(
                "worst outline movement {:.1}% of a radius",
                100.0 * f.worst_outline
            ));

            ui.separator();
            ui.label(
                egui::RichText::new(
                    "space pause · . step · , back · m motion · l layout · o outline · \
                     p panel · +/- zoom",
                )
                .small(),
            );
        });
}
