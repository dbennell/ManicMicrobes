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

/// Distance to a line segment. The one primitive the bent forms are made of.
fn sd_segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

/// Deterministic noise in 0..1, as `cell.wgsl` has it. The halo's curdling.
fn hash21(p: vec2<f32>) -> f32 {
    var v = fract(vec3<f32>(p.xyx) * 0.1031);
    v += dot(v, v.yzx + 33.33);
    return fract((v.x + v.y) * v.z);
}

// --- the proportions ---------------------------------------------------------------------
//
// **The shader owns how thick a limb is and the CPU owns how big the quad is**, and the split is
// deliberate. `LimbDot::width` is the widest the form ever reaches — the wave envelope, the tuft's
// arc — so every constant below is a fraction of one half-width and every one of them plus its
// swing comes to at most 1. A form that reached past that would be silently clipped to the
// rectangle and would come out with a straight edge where its outline should be, which reads as a
// shape somebody meant.

/// How thick one hair of a tuft is at its root, in half-widths.
const HAIR: f32 = 0.10;
/// How far from the tuft's axis the outermost hairs are rooted.
const TUFT: f32 = 0.62;
/// How far the tip of a hair swings at full beat. `TUFT + SWING + HAIR == 1`.
const SWING: f32 = 0.28;

/// How thick a flagellum's whip is at its root, in half-widths.
const WHIP: f32 = 0.22;
/// The wave's amplitude at the tip, at full beat. `WHIP + WAVE < 1`.
const WAVE: f32 = 0.72;
/// How many wavelengths fit along a flagellum. Between one and two: a single bend reads as a bent
/// stick and three reads as a spring.
const WAVES: f32 = 1.5;

/// How thick a holdfast's stalk is at its root, in half-widths, and how far along the limb the
/// stalk ends and the rootlets start.
const STALK: f32 = 0.40;
const STALK_END: f32 = 0.55;

/// A tuft of hairs, beating.
///
/// Many small ones, because that is what a cilium organelle *is* — `docs/FEEDING.md` §7 and the
/// catalogue both say a cilium is many where a flagellum is one, and the picture should say it
/// before the physics is consulted.
///
/// The swing goes as `t * t`, so the root stays put and the tip moves, which is what a cilium
/// does; a hair that slid sideways as a rigid rod would read as a twitching whisker. The hairs are
/// offset in phase from one another so a tuft beats as a wave across itself rather than as one
/// object, which is the metachronal rhythm real ciliates have.
fn sd_cilium(q: vec2<f32>, aspect: f32, extent: f32, phase: f32, count: f32) -> f32 {
    let t = clamp((q.x + aspect) / (2.0 * aspect), 0.0, 1.0);
    // The sign of the power is which way the wave travels, and a cilium beating backwards pushes
    // its cell backwards. It is a thing a genome can do that nothing in the picture could say.
    let sense = select(-1.0, 1.0, extent >= 0.0);
    let swing = SWING * abs(extent) * t * t;
    let n = max(count, 1.0);
    var best = 1e6;
    for (var i = 0.0; i < n; i += 1.0) {
        // Rooted evenly across the arc; one hair sits on the axis.
        let spread = select((2.0 * i / (n - 1.0)) - 1.0, 0.0, n < 1.5);
        let root = TUFT * spread;
        let beat = swing * sin(6.2831853 * (sense * phase + i * 0.17));
        let w = HAIR * (1.0 - t);
        best = min(best, abs(q.y - root - beat) - w);
    }
    return max(best, q.x - aspect);
}

/// One whip with a travelling wave.
///
/// The amplitude grows along the length rather than being uniform, which is what a flagellar wave
/// actually does and is the difference between a whip and a wiggly line: the base is held by the
/// body and the far end is free.
fn sd_flagellum(q: vec2<f32>, aspect: f32, extent: f32, phase: f32, taper: f32) -> f32 {
    let t = clamp((q.x + aspect) / (2.0 * aspect), 0.0, 1.0);
    let sense = select(-1.0, 1.0, extent >= 0.0);
    let centre = WAVE * abs(extent) * t * sin(6.2831853 * (WAVES * t - sense * phase));
    let w = WHIP * mix(1.0, taper, t);
    return max(abs(q.y - centre) - w, q.x - aspect);
}

/// A stalk with rootlets: taut and splayed when it is gripping, limp and closed when it has let go.
///
/// The tension is [`sensing::holdfast_effort`] and is the readable half of it — a cell that has
/// let go looks like it has let go, which is a decision a genome makes every tick and which
/// nothing on the slide could previously show. The curl is seeded, so two holdfasts on one cell
/// do not slump identically.
fn sd_holdfast(q: vec2<f32>, aspect: f32, extent: f32, taper: f32, seed: f32) -> f32 {
    let t = clamp((q.x + aspect) / (2.0 * aspect), 0.0, 1.0);
    let slack = 1.0 - clamp(extent, 0.0, 1.0);
    // Which way it slumps. Fixed for this holdfast, so it does not wander frame to frame.
    //
    // **Both components of the hash's input vary with the seed**, and that is not decoration.
    // `hash21` folds its input through `fract(p * 0.1031)` first, so a constant `y` and a `x` that
    // walks by small integers is very nearly a straight ramp into it — and the answers come out
    // biased to one side of a half. What that draws is every slack holdfast on the slide slumping
    // the same way, which reads as a symmetry the cell does not have.
    // `limb_probe::a_holdfast_that_has_let_go_hangs_off_its_own_axis` is the measurement.
    let curl = (hash21(vec2<f32>(seed * 7.13 + 2.7, seed * 3.71 + 9.1)) - 0.5) * 1.1 * slack;
    let centre = curl * t * t;

    // The stalk, tapering along the curl.
    let x_end = -aspect + 2.0 * aspect * STALK_END;
    let y_end = curl * STALK_END * STALK_END;
    let along = clamp(t / STALK_END, 0.0, 1.0);
    var d = abs(q.y - centre) - STALK * mix(1.0, taper, along);
    d = max(d, q.x - x_end);

    // And three rootlets from the stalk's end, splayed by how hard it is holding on. Gripping
    // cement is spread out against what it is gripping; cement that has let go hangs together.
    let splay = 0.30 + 0.55 * clamp(extent, 0.0, 1.0);
    let start = vec2<f32>(x_end, y_end);
    for (var k = -1.0; k < 1.5; k += 1.0) {
        let tip = vec2<f32>(aspect, y_end + k * splay);
        let root_d = sd_segment(q, start, tip) - STALK * taper * 0.8;
        d = min(d, root_d);
    }
    return d;
}

/// Which field this limb is, and how thick the form is where the pixel is.
///
/// Two numbers rather than one because the shading needs the second: `1 + field` rounds a limb
/// across its own width only if the width is one, and a cilium hair is a tenth of that. Without
/// it a tuft comes out flat and a spike comes out round, which is backwards.
///
/// One `if` chain rather than one shader per form: a limb is a handful of pixels and there is one
/// draw call for all of them, so branching here costs a warp divergence at the boundary between
/// two limbs of different kinds and saves four pipelines.
fn field_of(form: f32, q: vec2<f32>, a: vec4<f32>, b: vec4<f32>) -> vec2<f32> {
    if (abs(form - FORM_SPIKE) < 0.5) {
        return vec2<f32>(sd_spike(q, a.w, b.z), 1.0);
    }
    if (abs(form - FORM_CILIUM) < 0.5) {
        return vec2<f32>(sd_cilium(q, a.w, a.y, a.z, b.x), HAIR);
    }
    if (abs(form - FORM_FLAGELLUM) < 0.5) {
        return vec2<f32>(sd_flagellum(q, a.w, a.y, a.z, b.z), WHIP);
    }
    if (abs(form - FORM_HOLDFAST) < 0.5) {
        return vec2<f32>(sd_holdfast(q, a.w, a.y, b.z, b.w), STALK);
    }
    // A field nothing can reach draws nothing, which is the right behaviour for a form this build
    // does not know — no quad of stray colour and no discard storm, just the dot on the ring that
    // was there before. The halo does not come through here: it is a cloud rather than an outline
    // and has its own branch in `fragment`.
    return vec2<f32>(1e6, 1.0);
}

/// The exoenzyme's cloud.
///
/// Not a limb and not an outline. An exoenzyme digests a neighbour from the outside and puts the
/// result in the *square*, where anyone standing there can take it — so what there is to draw is
/// a volume of water that has been made dangerous, and a hard edge on it would be a claim about a
/// boundary that does not exist.
///
/// Densest against the body and thinning outwards, curdled rather than smooth, and in the
/// enzyme's own colour rather than the cell's: it is the thing the cell has put into the water,
/// not the cell.
fn halo(colour: vec4<f32>, q: vec2<f32>, throttle: f32, inner: f32, seed: f32) -> vec4<f32> {
    let r = length(q);
    if (r > 1.0 || r < inner) {
        discard;
    }
    let t = (r - inner) / max(1.0 - inner, 1e-3);
    // Quadratic, so it reaches zero at the outer edge on its own and there is no circle to see.
    let fall = (1.0 - t) * (1.0 - t);
    let curdle = 0.55 + 0.45 * hash21(q * 6.0 + seed);
    let a = clamp(throttle, 0.0, 1.0) * 0.34 * fall * curdle;
    // A sickly yellow-green: it is a leaky public good dissolving whatever is standing in it, and
    // it should look like something you would not want to be standing in. Mixed towards the cell
    // so it still reads as that cell's doing.
    let enzyme = vec3<f32>(0.74, 0.82, 0.34);
    return vec4<f32>(mix(enzyme, colour.rgb, 0.25), a);
}

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
    let aspect = in.limb_a.w;
    let q = vec2<f32>(in.uv.x * aspect, in.uv.y);

    // The cloud takes its own path: it has no outline to antialias and no thickness to shade, and
    // running it through the machinery below would draw a hard-edged ring where a diffuse volume
    // is meant.
    if (abs(in.limb_a.x - FORM_HALO) < 0.5) {
        return halo(in.colour, q, in.limb_a.y, in.limb_b.y, in.limb_b.w);
    }

    let probe = field_of(in.limb_a.x, q, in.limb_a, in.limb_b);
    let field = probe.x;
    let thickness = max(probe.y, 1e-3);

    // **`fwidth` of the field itself, not of a coordinate.** The forms have very different
    // gradients — a spike's outline runs almost along the quad and a hair's across it — so a fade
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
    //
    // Divided by the form's own thickness, which is what `field_of` returns alongside the field.
    // Without it the rounding is measured against one half-width for every form, so a cilium hair
    // at a tenth of that comes out flat and a spike comes out fully round — backwards, since the
    // hair is the round one.
    let t = clamp(1.0 + field / thickness, 0.0, 1.0);
    let nz = sqrt(max(0.0, 1.0 - t * t));
    let lambert = clamp(nz * 0.85 + 0.15, 0.0, 1.0);
    // Darker than the body it grew from. A limb is thin, so it transmits less light than a whole
    // cell does, and drawing it at the body's own value made a spike read as a bright spur of
    // cytoplasm rather than as something harder.
    let lum = clamp(0.55 + 0.30 * lambert, 0.0, 1.0);
    return vec4<f32>(in.colour.rgb * lum, in.colour.a * alpha);
}
