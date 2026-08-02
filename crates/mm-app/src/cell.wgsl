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
    @location(8) squash_dir3: vec4<f32>,
    @location(9) squash_face3: vec4<f32>,
    // How much this cell was grown to keep its area. The quad is already that big; this is what
    // the outline gives back along the seams. See the taper in the fragment shader.
    @location(10) swell: f32,
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
    @location(7) squash_dir3: vec4<f32>,
    @location(8) squash_face3: vec4<f32>,
    @location(9) swell: f32,
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
    out.squash_dir3 = vertex.squash_dir3;
    out.squash_face3 = vertex.squash_face3;
    out.swell = vertex.swell;
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

/// How wide the swell tapers out around a facet, in cosine of angle.
///
/// Wide enough that the outline arrives at the seam smoothly rather than with a visible kink,
/// narrow enough to leave the free arc between two facets something to grow into. At 0.25 a
/// facet spanning ±30° tapers over about the next 20°.
const TAPER: f32 = 0.25;

/// How much of its swell a cell keeps in direction `dir`, given one seam.
///
/// One where the direction is well clear of the facet, falling to zero at the facet's edge and
/// staying there across it. Inside the facet the seam is doing the cutting and the radius makes
/// no difference; what has to be exact is the *edge*, because that is the point the two cells
/// have to agree on.
///
/// `face / bare` is the cosine of the half-angle the facet subtends on the unswollen circle. An
/// unused slot carries a face far outside the circle, so its cosine comes out greater than one,
/// no direction can reach it, and it tapers nothing — which is why this must not be clamped
/// into range.
fn seam_room(dir: vec2<f32>, n: vec2<f32>, face: f32, bare: f32) -> f32 {
    let edge_cos = face / max(bare, 0.0001);
    return 1.0 - smoothstep(edge_cos - TAPER, edge_cos, dot(dir, n));
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
    let faces3 = in.squash_face3 * 0.65;
    // Measured against the cell's own radius, as a fraction: 1 is a seam just grazing the
    // outline, below that is a real cut, and an unused slot carries 8 and never wins the `min`.
    let nearest = min(
        min(min(in.squash_face.x, in.squash_face.y), min(in.squash_face.z, in.squash_face.w)),
        min(
            min(
                min(in.squash_face2.x, in.squash_face2.y),
                min(in.squash_face2.z, in.squash_face2.w),
            ),
            min(
                min(in.squash_face3.x, in.squash_face3.y),
                min(in.squash_face3.z, in.squash_face3.w),
            ),
        ),
    );
    // Full pressure by the time a seam has taken a quarter of the radius, rather than only when
    // one reaches the centre. The old scale reached 1 only for a cell cut clean through, so an
    // ordinary packed contact — which cuts about a sixth of the way in — read as 0.17 and left
    // the wobble almost untouched. That is why a packed sheet had wavy walls: the seam is a
    // straight line, but either side of it the cell's own irregularity is the same size as the
    // cut, so what you see is the wobble and not the wall.
    let pressure = clamp((1.0 - nearest) / 0.25, 0.0, 1.0);

    // --- which way the neighbours are ---
    //
    // Unpacked up here rather than down where the field is cut, because the outline needs them
    // now: how far a cell may swell depends on where its neighbours are.
    let d0 = unpack2x16snorm(bitcast<u32>(in.squash_dir.x));
    let d1 = unpack2x16snorm(bitcast<u32>(in.squash_dir.y));
    let d2 = unpack2x16snorm(bitcast<u32>(in.squash_dir.z));
    let d3 = unpack2x16snorm(bitcast<u32>(in.squash_dir.w));
    let d4 = unpack2x16snorm(bitcast<u32>(in.squash_dir2.x));
    let d5 = unpack2x16snorm(bitcast<u32>(in.squash_dir2.y));
    let d6 = unpack2x16snorm(bitcast<u32>(in.squash_dir2.z));
    let d7 = unpack2x16snorm(bitcast<u32>(in.squash_dir2.w));
    let d8 = unpack2x16snorm(bitcast<u32>(in.squash_dir3.x));
    let d9 = unpack2x16snorm(bitcast<u32>(in.squash_dir3.y));
    let d10 = unpack2x16snorm(bitcast<u32>(in.squash_dir3.z));
    let d11 = unpack2x16snorm(bitcast<u32>(in.squash_dir3.w));

    // --- how much of the swell this direction gets ---
    //
    // `area_swell` grows a clipped cell until what survives the cutting is the area it has, and
    // the growth is meant to go into the free arcs — that is the whole point of it, because the
    // free arcs are where the gaps between cells are. It did not: the CPU scaled the entire
    // circle and left the seam planes where they were, so much of the growth went into making
    // the *shared walls* longer instead.
    //
    // Which is what made a big cell's flat run on past the point where its small neighbour
    // actually touches it. The seam is the plane through the two crossing outlines, and that
    // gives both cells the same facet by construction — but only while both are drawn at the
    // radius the plane was computed from. Scale one circle up and it meets the same plane along
    // a longer chord, and the bigger circle's chord grows faster: at a swell of 1.15 a 2:1 pair
    // came out a third apart, and with the two swelling by different amounts, more than half.
    //
    // So the swell is tapered away across each facet's own angular span. At a facet's edge the
    // cell is back to the unswollen radius the plane was cut from, so both cells end their flat
    // at the same point again; between the facets it swells fully, which is where the gaps are
    // and where the growth was supposed to go in the first place.
    //
    // The area is then no longer exactly preserved — the taper gives back growth the solve had
    // already counted — so a crowded cell comes out slightly under its true area. That is the
    // approximation. Holding both the area and the facets exactly means putting the seam on the
    // radical line of the *drawn* circles, which needs each cell to know its neighbour's swell.
    let swell = max(in.swell, 1.0);
    // The unswollen radius, in field units: `FIELD_FILL` is the swollen one by construction,
    // because the quad was sized to it.
    let bare = 0.65 / swell;
    let dir = select(vec2<f32>(1.0, 0.0), p / r, r > 0.0001);
    var room = seam_room(dir, d0, faces.x, bare);
    room = min(room, seam_room(dir, d1, faces.y, bare));
    room = min(room, seam_room(dir, d2, faces.z, bare));
    room = min(room, seam_room(dir, d3, faces.w, bare));
    room = min(room, seam_room(dir, d4, faces2.x, bare));
    room = min(room, seam_room(dir, d5, faces2.y, bare));
    room = min(room, seam_room(dir, d6, faces2.z, bare));
    room = min(room, seam_room(dir, d7, faces2.w, bare));
    room = min(room, seam_room(dir, d8, faces3.x, bare));
    room = min(room, seam_room(dir, d9, faces3.y, bare));
    room = min(room, seam_room(dir, d10, faces3.z, bare));
    room = min(room, seam_room(dir, d11, faces3.w, bare));

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
    let slack = 1.0 - 0.96 * pressure;
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
    //
    // Swollen only where there is room for it. Between `bare` at a facet's edge and the full
    // 0.65 out in a free arc, which is the taper above.
    let radius = mix(bare, 0.65, room) * (1.0 + wobble + wear);

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
    // The shoulder scales with the cell, so a small one is not rounded away entirely and a
    // large one does not get a corner that reads as sharp.
    //
    // Small, and it has to be. `smax` is a *smooth* max, so it returns slightly more than the
    // true maximum near the seam — and in a field where positive is outside, more means the
    // outline is eroded back from the plane it was supposed to stop at. Both cells of a pair are
    // eroded, so the shared wall they were meant to meet along opens into a gap, and the erosion
    // is worst where two seams meet, which is exactly where a packed sheet needs to close up.
    // At a seventh of the radius that gap is wide enough to read as background, and a crowd of
    // cells that should be one continuous mass reads as separate rounded pebbles with shadows
    // between them.
    let shoulder = radius * 0.035;
    field = smax(field, dot(p, d0) - faces.x, shoulder);
    field = smax(field, dot(p, d1) - faces.y, shoulder);
    field = smax(field, dot(p, d2) - faces.z, shoulder);
    field = smax(field, dot(p, d3) - faces.w, shoulder);
    // And four more. Six is what a packed monolayer settles on; eight is headroom, because a
    // cell that runs out of seams stops cutting for a neighbour that is still cutting for it,
    // and the two are then drawn one over the other with no shared wall.
    field = smax(field, dot(p, d4) - faces2.x, shoulder);
    field = smax(field, dot(p, d5) - faces2.y, shoulder);
    field = smax(field, dot(p, d6) - faces2.z, shoulder);
    field = smax(field, dot(p, d7) - faces2.w, shoulder);
    // And the last four. See `ATTRIBUTE_SQUASH_DIR3`.
    field = smax(field, dot(p, d8) - faces3.x, shoulder);
    field = smax(field, dot(p, d9) - faces3.y, shoulder);
    field = smax(field, dot(p, d10) - faces3.z, shoulder);
    field = smax(field, dot(p, d11) - faces3.w, shoulder);

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

    // A dark ring just inside the outline: the membrane, fading as it fails.
    //
    // Against the *field* rather than the radius, so it includes the seams and runs along a
    // flattened side as well as a curved one. Measured against the radius it stopped at the flat
    // and left the pressed edges bare.
    //
    // Dark and heavy rather than a thin bright highlight, which is the change that makes a
    // packed crowd legible. Once cells tile there is no background left between them, so a
    // boundary drawn as *lighter than the cell* has nothing to contrast against and the mass
    // reads as one lumpy object. Every cell in a real monolayer is drawn with a dark wall for
    // exactly this reason. Scaled by the radius so it is a proportion of the cell rather than a
    // fixed width that swallows the small ones.
    // Thin, because every wall in a packed sheet is drawn twice — once by the cell on each
    // side. At half this width the two rings read as one line between two cells, which is what
    // a membrane between two cells is; at the width that looked right on a *solitary* cell they
    // stack into a dark band and push the two apart visually.
    //
    // Floored at the antialiasing width so the ring can never be thinner than the fade that
    // draws it, which would make it shimmer — but at `fwidth` alone and *not* at `edge`, which
    // is the same quantity plus `softness`. Flooring on `edge` meant defocus widened the
    // membrane: at high magnification `radius * 0.025` is about two pixels while a badly
    // defocused cell's `softness` is three and a half, so the floor won and that cell's wall
    // came out roughly twice as wide as its neighbour's. Since a shared wall is two half-rings,
    // the pair's boundary took the thick half from whichever side was further out of focus, and
    // a cell appeared thick-walled on some sides and thin on others depending only on the
    // *neighbour's* depth. An out-of-focus edge should lose contrast, not gain weight — so
    // where the fade is now wider than the wall, the wall correctly dissolves into it.
    let wall = max(radius * 0.025, fwidth(r) * 1.5);
    let membrane = smoothstep(-wall, -wall * 0.35, field) * integrity;

    // Flat-shaded, near enough. The hemisphere normal above is kept for a *hint* of form and for
    // the seam-following it does, but at the weight it used to carry — two thirds lambert plus a
    // strong rim — every cell read as a lit sphere, so a crowd of them read as a heap of pebbles
    // rather than as a sheet of cells. Cells in a monolayer are flat.
    let lum = clamp(0.72 + 0.17 * lambert + 0.05 * rim + 0.045 * grain, 0.0, 1.0);
    // The wall is the cell's own colour taken well down, rather than black: a dark version of
    // each cell keeps the species colouring readable through the outline.
    let body = in.colour.rgb * lum;
    let rgb = mix(body, in.colour.rgb * 0.16, membrane);
    return vec4<f32>(rgb, in.colour.a * alpha);
}
