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
    // Four seams where this cell is squashed against a neighbour, one per component: the
    // direction packed as a pair of 16-bit snorms, and how far along it the seam sits.
    // Unused slots carry a distance nothing can reach, so there is no count to branch on.
    @location(4) squash_dir: vec4<f32>,
    @location(5) squash_face: vec4<f32>,
    @location(6) squash_dir2: vec4<f32>,
    @location(7) squash_face2: vec4<f32>,
};

struct Output {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) shape: vec4<f32>,
    @location(3) squash_dir: vec4<f32>,
    @location(4) squash_face: vec4<f32>,
    @location(5) squash_dir2: vec4<f32>,
    @location(6) squash_face2: vec4<f32>,
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
    out.squash_dir = vertex.squash_dir;
    out.squash_face = vertex.squash_face;
    out.squash_dir2 = vertex.squash_dir2;
    out.squash_face2 = vertex.squash_face2;
    return out;
}

// Intersection with a rounded corner, rather than a mitred one.
//
// A plain `max` is a clean geometric intersection and it looks like scissors: the cell arrives
// at the seam, turns through a hard angle, and leaves. Real cells under pressure meet their
// neighbours along a flat and then curve away from it, and it is that curve — not the flat —
// that makes a clump read as one mass rather than as a pile of clipped discs.
//
// `k` is how much shoulder to leave, in the field's own units. IQ's polynomial smooth-min,
// negated into a max.
fn smax(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 - 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) + k * h * (1.0 - h);
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

    // --- how hard this cell is being squeezed ---
    //
    // Read off the nearest seam: 0 when nothing is cutting into the cell, approaching 1 as a
    // seam closes on its centre. Free, because the seams are already here.
    let faces = in.squash_face * 0.65;
    let faces2 = in.squash_face2 * 0.65;
    let nearest_face = min(
        min(min(faces.x, faces.y), min(faces.z, faces.w)),
        min(min(faces2.x, faces2.y), min(faces2.z, faces2.w)),
    );
    let pressure = clamp(1.0 - nearest_face / 0.65, 0.0, 1.0);

    // --- the outline ---
    //
    // Three harmonics of angle, with amplitudes and phases from the cell's own seed. Small
    // enough that the blob stays a blob: past about 0.2 total it reads as a splat, and past
    // 0.35 the outline folds back on itself.
    //
    // Damped by how hard the cell is being pressed, and that is what makes a clump look like a
    // clump. A seam is a straight line agreed by two cells from their own centres, but the
    // wobble is each cell's private business — so a cell whose outline happens to wander
    // *inwards* towards its neighbour stops short of the seam it agreed to, and leaves a gap
    // exactly where the two are supposed to be pressed together. It reads as cells floating
    // near each other rather than packed.
    //
    // A cell being squashed flat by its neighbours has no business being knobbly anyway: the
    // pressure that makes the seam is the same pressure that would smooth the membrane out.
    // So the irregularity is a luxury of having room, and a cell in a crowd gives it up.
    let theta = atan2(p.y, p.x);
    let slack = 1.0 - 0.9 * pressure;
    let a1 = (0.055 + 0.045 * hash11(seed)) * slack;
    let a2 = (0.030 + 0.030 * hash11(seed + 7.0)) * slack;
    let a3 = (0.015 + 0.020 * hash11(seed + 13.0)) * slack;
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

    // --- where the neighbours press in ---
    //
    // The outline so far is a distance from the edge: negative inside, zero on it. Each seam
    // is a half-plane the cell may not cross, and `max` against it is the intersection — so a
    // cell pressed between two others comes out with two flat sides and round everywhere else,
    // which is what a crowd of them actually looks like down a microscope.
    //
    // A plane rather than the neighbour's own outline subtracted. Both cells compute the same
    // seam from their own side (see `slide::Squash`), so they meet along one line with no gap
    // and no doubled edge — neither of them has to know what the other decided.
    //
    // Four unconditionally: the unused ones carry a distance no pixel of this quad can reach,
    // which is cheaper than branching on a count in a fragment shader.
    var field = r - radius;
    let d0 = unpack2x16snorm(bitcast<u32>(in.squash_dir.x));
    let d1 = unpack2x16snorm(bitcast<u32>(in.squash_dir.y));
    let d2 = unpack2x16snorm(bitcast<u32>(in.squash_dir.z));
    let d3 = unpack2x16snorm(bitcast<u32>(in.squash_dir.w));
    // The shoulder scales with the cell, so a small one is not rounded away entirely and a
    // large one does not get a corner that reads as sharp.
    let shoulder = radius * 0.14;
    field = smax(field, dot(p, d0) - faces.x, shoulder);
    field = smax(field, dot(p, d1) - faces.y, shoulder);
    field = smax(field, dot(p, d2) - faces.z, shoulder);
    field = smax(field, dot(p, d3) - faces.w, shoulder);
    // And four more. Six is what a packed monolayer settles on; eight is headroom, because a
    // cell that runs out of seams stops cutting for a neighbour that is still cutting for it,
    // and the two are then drawn one over the other with no shared wall.
    let d4 = unpack2x16snorm(bitcast<u32>(in.squash_dir2.x));
    let d5 = unpack2x16snorm(bitcast<u32>(in.squash_dir2.y));
    let d6 = unpack2x16snorm(bitcast<u32>(in.squash_dir2.z));
    let d7 = unpack2x16snorm(bitcast<u32>(in.squash_dir2.w));
    field = smax(field, dot(p, d4) - faces2.x, shoulder);
    field = smax(field, dot(p, d5) - faces2.y, shoulder);
    field = smax(field, dot(p, d6) - faces2.z, shoulder);
    field = smax(field, dot(p, d7) - faces2.w, shoulder);

    let alpha = 1.0 - smoothstep(-edge, edge, field);
    if (alpha <= 0.001) {
        discard;
    }

    // --- the body ---
    //
    // The hemisphere normal. `sqrt(1 - r^2)` against a fixed light is what turns a disc into a
    // ball, and it is three operations.
    //
    // Driven by the field rather than by the raw radius, so the ball follows the seams: where
    // a neighbour has flattened this cell the shading reaches the *flat* at full curvature
    // instead of continuing to a circle that is not there. `field` is zero on the outline
    // wherever the outline has ended up, so `t` is one there and zero in the middle.
    let t = clamp(1.0 + field / max(radius, 0.001), 0.0, 1.0);
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
    //
    // Against the *field* rather than the radius, which is the one thing this keeps from the
    // dark heavy version that was here in between: the field includes the seams, so the ring
    // runs along a flattened side as well as a curved one. Measured against the radius it
    // stopped at the flat and left the pressed edges bare.
    let membrane = smoothstep(-0.16, -0.02, field) * integrity;

    let lum = clamp(
        0.34 + 0.62 * lambert + 0.30 * rim + 0.18 * membrane + 0.045 * grain,
        0.0,
        1.0,
    );
    return vec4<f32>(in.colour.rgb * lum, in.colour.a * alpha);
}
