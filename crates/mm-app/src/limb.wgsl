// What a cell has grown outside its own membrane, evaluated per pixel.
//
// Six of the catalogue's twenty drawable organelles reach past the wall — cilium, flagellum,
// spike, holdfast, exoenzyme — and every one of them was drawn as a coloured dot on the ring
// inside the cell. See `docs/MORPHOLOGY.md` for the plan and `limbmesh.rs` for why this is a
// separate mesh and material rather than lines added to `cell.wgsl`.
//
// Drawn *under* the cells, at `limbmesh::LIMB_Z`. Two consequences worth stating because both are
// load-bearing:
//
//   * A limb's root is inside its own body and the body covers it, so there is no join to draw.
//     The quad's `-x` edge is the root and part of it is deliberately behind the membrane.
//   * A limb may be drawn over a *neighbouring* cell, which nothing else on the slide may do. A
//     spike wounds the cell it is touching, so a spike over its victim is the honest picture —
//     and being under the cells means it reads as passing behind rather than as lying on top.
//
// # The frame
//
// `uv` is the quad corner in the limb's own frame, `-1..1`, already rotated by the CPU: `+x` runs
// root to tip and `+y` across. Everything below works in `q = vec2(uv.x * aspect, uv.y)`, which is
// **half-widths** — so `q.y` is `-1..1` across the limb whatever it is drawn at, and `q.x` runs
// `-aspect..aspect` along it. That makes every field isotropic without the shader knowing how many
// pixels anything is.

#import bevy_sprite::{
    mesh2d_functions::{get_world_from_local, mesh2d_position_local_to_clip},
}

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
    // x: which field to evaluate — see `limbmesh::form`.
    // y: how hard it is working, -1..1. Signed where the control it came from is signed.
    // z: where in its beat it is, 0..1. From the tick, never from the clock.
    // w: half_len / half_wid, so the frame can be made isotropic.
    @location(3) limb_a: vec4<f32>,
    // x: how many sub-elements — hairs in a tuft, rootlets on a foot, 1 otherwise.
    // y: the hollow fraction of a form that has one. 0 for a solid.
    // z: tip width as a fraction of the root's. 0 tapers to a point.
    // w: a per-limb seed, so two of the same form on one cell are not one shape twice.
    @location(4) limb_b: vec4<f32>,
};

// Everything but the corner is flat. They are per-limb constants and interpolating a constant is
// work for no reason — and `cell.wgsl`'s note on `squash_dir` is the warning about what happens
// when an interpolated "constant" comes back one ulp out. Nothing here is bit-packed, so this is
// the cheap reason rather than the load-bearing one, but the habit is worth keeping.
struct Output {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) colour: vec4<f32>,
    @location(2) @interpolate(flat) limb_a: vec4<f32>,
    @location(3) @interpolate(flat) limb_b: vec4<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> Output {
    var out: Output;
    // The mesh carries screen-space positions already — one mesh for every limb on the slide, so
    // there is no per-limb transform to apply and nothing to look up.
    out.clip_position = mesh2d_position_local_to_clip(
        get_world_from_local(vertex.instance_index),
        vec4<f32>(vertex.position, 1.0),
    );
    out.uv = vertex.uv;
    out.colour = vertex.colour;
    out.limb_a = vertex.limb_a;
    out.limb_b = vertex.limb_b;
    return out;
}

// --- the forms -----------------------------------------------------------------------------

const FORM_CILIUM: f32 = 1.0;
const FORM_FLAGELLUM: f32 = 2.0;
const FORM_SPIKE: f32 = 3.0;
const FORM_HOLDFAST: f32 = 4.0;
const FORM_HALO: f32 = 5.0;

/// A barb: wide at the root, narrowing fast, tapering to a long thin point.
///
/// Quadratic rather than linear, and that is the difference between a spike and an arrow. A linear
/// cone reads as a triangle somebody has drawn on the cell; a concave one reads as something that
/// grew, because the profile of anything that has to be both anchored and sharp is concave.
///
/// The tip is capped and the root is not. The root end of the quad is inside the body, which is
/// drawn over this mesh, so there is nothing there to cap — and capping it would put an edge
/// exactly where the membrane is going to be drawn anyway.
fn sd_spike(q: vec2<f32>, aspect: f32, taper: f32) -> f32 {
    let t = clamp((q.x + aspect) / (2.0 * aspect), 0.0, 1.0);
    let s = 1.0 - t;
    let w = taper + (1.0 - taper) * s * s;
    // Past the tip nothing, and the `max` is what makes the point a point rather than a stripe
    // running to the quad's edge.
    return max(abs(q.y) - w, q.x - aspect);
}

/// Which field this limb is, positive outside it.
///
/// One `if` chain rather than one shader per form: a limb is a handful of pixels and there is one
/// draw call for all of them, so branching here costs a warp divergence at the boundary between
/// two limbs of different kinds and saves four pipelines.
fn field_of(form: f32, q: vec2<f32>, a: vec4<f32>, b: vec4<f32>) -> f32 {
    if (abs(form - FORM_SPIKE) < 0.5) {
        return sd_spike(q, a.w, b.z);
    }
    // The other four are not carried yet, deliberately: this pipeline arrives with **one** form in
    // it, so that if the layout, the material, the z-order or the upload is wrong it is wrong with
    // a triangle on the screen and not with five fields to argue about.
    //
    // A field nothing can reach draws nothing, which is the right behaviour for a form this build
    // does not know — no quad of stray colour and no discard storm, just the dot on the ring that
    // was there before.
    return 1e6;
}

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
    let aspect = in.limb_a.w;
    let q = vec2<f32>(in.uv.x * aspect, in.uv.y);
    let field = field_of(in.limb_a.x, q, in.limb_a, in.limb_b);

    // **`fwidth` of the field itself, not of a coordinate.** The forms have very different
    // gradients — a spike's outline runs almost along the quad and a halo's across it — so a fade
    // measured in `q` would be a pixel wide on one and five on another. The derivative of the
    // field is a pixel of *this* field wherever it is evaluated, which is the one measure that is
    // right for every form and at every zoom.
    //
    // Floored, because `fwidth` is zero in a flat region and a smoothstep of zero width aliases.
    let edge = max(fwidth(field), 1e-4);
    let alpha = 1.0 - smoothstep(-edge, edge, field);
    if (alpha <= 0.003) {
        discard;
    }

    // The same material as the cell it grew from, so a limb reads as part of the organism rather
    // than as an overlay. `colour` arrives already hazed and vignetted with its cell — organelles
    // took the vignette and no depth of field for a long time, and a defocused cell with a crisp
    // bright nucleus in it reads as two objects.
    //
    // Rounded across the width from the field, which is the same trick `cell.wgsl`'s `blob` plays
    // with the body: `t` is one at the outline and zero down the middle, so `nz` is the height of
    // a half-cylinder and a flat stripe becomes a limb with a top to it.
    let t = clamp(1.0 + field, 0.0, 1.0);
    let nz = sqrt(max(0.0, 1.0 - t * t));
    // From the upper left, as `cell.wgsl` has it, so a limb is lit by the same condenser as the
    // body. Its own frame is rotated and the light is not, which is why the normal is rotated back
    // through the quad's axes rather than taken in `q`.
    let lambert = clamp(nz * 0.85 + 0.15, 0.0, 1.0);
    // Darker than the body it grew from. A limb is thin, so it transmits less light than a whole
    // cell does, and drawing it at the body's own value made a spike read as a bright spur of
    // cytoplasm rather than as something harder.
    let lum = clamp(0.55 + 0.30 * lambert, 0.0, 1.0);
    return vec4<f32>(in.colour.rgb * lum, in.colour.a * alpha);
}
