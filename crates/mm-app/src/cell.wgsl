// What a cell looks like up close, evaluated per pixel (M10.5).
//
// The baked atlas this replaces put the shape in a texture: sixteen silhouettes, shared by
// every cell wearing one, fixed at bake time and soft when magnified past the tile. Here the
// shape is a signed-distance field evaluated per pixel per cell, so every cell has its own
// outline, it stays crisp at any zoom, and it can respond to what the cell is actually doing.
//
// See `docs/UI.md` §7 for the design and `art.rs` for the baked version, which is still what
// organelles and dust use and is still what "rounded cells: off" selects.

#import bevy_sprite::{
    mesh2d_functions::{get_world_from_local, mesh2d_position_local_to_clip},
    mesh2d_view_bindings::view,
}

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    // The quad corner, -1..1. What the whole field is evaluated against.
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
    // x: the cell's own seed, which fixes its silhouette for life.
    // y: edge softness, which is the depth of field.
    // z: membrane integrity, 1 whole and 0 failing.
    // w: > 0.5 when the SDF is wanted at all; below that the cell is drawn as a plain quad,
    //    which is what "rounded cells: off" gets and is exactly the pre-M10.5 look.
    @location(3) shape: vec4<f32>,
};

struct Output {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) shape: vec4<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> Output {
    var out: Output;
    // The mesh carries screen-space positions already — one mesh for the whole population, so
    // there is no per-cell transform to apply and nothing to look up.
    out.clip_position = mesh2d_position_local_to_clip(
        get_world_from_local(vertex.instance_index),
        vec4<f32>(vertex.position, 1.0),
    );
    out.uv = vertex.uv;
    out.colour = vertex.colour;
    out.shape = vertex.shape;
    return out;
}

// Deterministic noise in 0..1. The same shape of hash `art.rs` uses, in the space WGSL has.
fn hash11(p: f32) -> f32 {
    var x = fract(p * 0.1031);
    x = x * (x + 33.33);
    x = x * (x + x);
    return fract(x);
}

fn hash21(p: vec2<f32>) -> f32 {
    var v = fract(vec3<f32>(p.xyx) * 0.1031);
    v += dot(v, v.yzx + 33.33);
    return fract((v.x + v.y) * v.z);
}

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
    let p = in.uv;
    let r = length(p);
    let seed = in.shape.x;
    let softness = in.shape.y;
    let integrity = clamp(in.shape.z, 0.0, 1.0);
    let rounded = in.shape.w > 0.5;

    if (!rounded) {
        // The flat look, through the same draw call. A square is a square.
        return in.colour;
    }

    // --- the outline ---
    //
    // Three harmonics of angle, with amplitudes and phases from the cell's own seed. Small
    // enough that the blob stays a blob: past about 0.2 total it reads as a splat, and past
    // 0.35 the outline folds back on itself.
    let theta = atan2(p.y, p.x);
    let a1 = 0.055 + 0.045 * hash11(seed);
    let a2 = 0.030 + 0.030 * hash11(seed + 7.0);
    let a3 = 0.015 + 0.020 * hash11(seed + 13.0);
    let wobble = a1 * sin(3.0 * theta + hash11(seed + 3.0) * 6.2831853)
        + a2 * sin(5.0 * theta + hash11(seed + 11.0) * 6.2831853)
        + a3 * sin(7.0 * theta + hash11(seed + 17.0) * 6.2831853);
    // A damaged cell loses its shape: the outline roughens as the membrane goes, which is a
    // thing the baked version could not do at all because the shape was fixed before the cell
    // existed.
    let wear = (1.0 - integrity) * 0.09 * sin(11.0 * theta + hash11(seed + 23.0) * 6.2831853);
    // 0.65 is `cellmesh::FIELD_FILL` and the two must agree. The margin is not slack: the
    // wobble adds up to a fifth, and the fade needs room outside that or it runs off the
    // quad's corners and the cell is drawn as a square.
    let radius = 0.65 * (1.0 + wobble + wear);

    // --- the edge ---
    //
    // `fwidth` is one pixel in this field's units, so the fade is a pixel wide however far the
    // cell is zoomed — which is the whole reason this is worth doing per pixel. Widened by the
    // defocus, so depth of field is a genuinely softer outline rather than a fainter square.
    //
    // Capped, and the cap is not a nicety. A cell four pixels across has `fwidth(r)` of about
    // a half, so the fade spans the entire quad and the smoothstep never reaches zero before
    // the corner — the cell renders as a soft *square*. At whole-slide zoom that made the
    // population a mix of squares and blobs depending on how big each one happened to be,
    // which read as two renderers fighting.
    let edge = min(fwidth(r) * 1.5 + softness, radius * 0.35);
    let alpha = 1.0 - smoothstep(radius - edge, radius + edge, r);
    if (alpha <= 0.001) {
        discard;
    }

    // --- the body ---
    //
    // The hemisphere normal. `sqrt(1 - r^2)` against a fixed light is what turns a disc into a
    // ball, and it is three operations.
    let t = min(r / max(radius, 0.001), 1.0);
    let nz = sqrt(max(0.0, 1.0 - t * t));
    var n = vec3<f32>(0.0, 0.0, 1.0);
    if (r > 0.0001) {
        n = vec3<f32>(p / r * t, nz);
    }
    // From the upper left, as a microscope's condenser throws it. Clip space has y up, so "up"
    // is positive here where it was negative in the baked version.
    let light = normalize(vec3<f32>(-0.42, 0.52, 0.74));
    let lambert = clamp(dot(n, light), 0.0, 1.0);
    // The bright edge that makes something look wet rather than matte.
    let rim = pow(1.0 - nz, 4.0);
    // Faint granularity: a flat disc reads as a sprite, a grainy one reads as cytoplasm.
    let grain = hash21(p * 37.0 + seed) - 0.5;
    // A thin brighter ring just inside the outline: the membrane, fading as it fails.
    let membrane = smoothstep(radius - 0.16, radius - 0.02, r) * integrity;

    let lum = clamp(0.34 + 0.62 * lambert + 0.30 * rim + 0.18 * membrane + 0.045 * grain, 0.0, 1.0);
    return vec4<f32>(in.colour.rgb * lum, in.colour.a * alpha);
}
