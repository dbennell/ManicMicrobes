// The surface of rock, evaluated per pixel.
//
// The barrier layer is one texel per square, nearest-sampled, and that is deliberate: a wall is
// blocked or not, and a linear sampler across its edge draws half a wall, which is a value the
// simulation never held (`art.rs`, `docs/UI.md` §1). So a square of rock is a flat block of one
// colour, and at high magnification a reef reads as a rectangle of paint.
//
// This roughens the *inside* of that block and nothing else. **The silhouette is untouched** —
// coverage still comes from the texel's alpha, so which squares are solid is exactly what the
// substrate says, and no fragment outside a blocked square is ever lit. What varies is the value
// of the colour within a square, which is a claim about nothing.
//
// Two things vary it: grain, which is fine and everywhere, and pitting, which is coarse and
// sparse. Both are evaluated in *slide* coordinates rather than screen ones, so the surface
// stays put on the rock when the view is panned or zoomed, and both run continuously across the
// boundary between two rock squares — a bed of rock is one surface, not a row of tiles.
//
// # Which walls
//
// Bedrock is left flat. The alpha channel of the barrier layer carries which kind of wall a
// square is (`art::WALL_BEDROCK` against `art::WALL_MINERAL`), so the shader can tell them apart
// without a second texture: rock made of minerals is weathered, and a wall that is a hole in the
// world is not made of anything to weather. It pairs with the grit along the exposed faces —
// smooth and clean means permanent, rough and gritty means it dissolves.
//
// # Level of detail
//
// The amplitude fades to nothing as a square shrinks below a few pixels. Grain finer than the
// pixels drawing it does not read as texture, it reads as noise that crawls when the view moves,
// and at whole-slide zoom a barrier is one or two pixels across. `fwidth` gives the size of a
// square on screen without a uniform to keep in step with the camera.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var barriers: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var barriers_sampler: sampler;

// A wall at all. Below this the square is open water and nothing is drawn.
const WALL: f32 = 0.25;
// Above this the wall is made of minerals, and weathers.
const MINERAL: f32 = 0.75;

// Hash of a lattice point: the same value for the same square of the slide, every frame, at
// every zoom, on any machine. No time in it — rock that shimmers is not rock.
fn hash2(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}

// The noise lattice, turned off the axes.
//
// **Value noise has a grid in it, and the walls have the same grid.** Left aligned, the features
// came out as dark rectangles that lined up with the squares — which reads as the drawing having
// a resolution rather than the rock having a surface. A rotation by an angle with no simple
// relation to the grid means a feature crosses a square boundary at an angle, every time.
const TILT: mat2x2<f32> = mat2x2<f32>(0.8434, -0.5373, 0.5373, 0.8434);

// Value noise: hash the four corners of the cell `p` falls in and interpolate smoothly. Returns
// 0..1. Smoothstep rather than a linear blend, so the derivative is continuous and the surface
// has no visible lattice in it.
fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(barriers, barriers_sampler, mesh.uv);
    // Slide coordinates, in squares. `textureDimensions` is the grid, so this needs no uniform
    // and cannot fall out of step with a scenario of a different size.
    let grid = vec2<f32>(textureDimensions(barriers));
    let sq = mesh.uv * grid;

    // How many pixels a square covers, from the rate the coordinate changes across the screen.
    // Taken before the discard below: a derivative is computed across a quad of fragments, and
    // asking for one where some of that quad has been thrown away is asking for a value that
    // does not exist.
    let per_px = max(fwidth(sq.x), 1e-6);
    if texel.a < WALL {
        discard;
    }
    let px_per_square = 1.0 / per_px;
    // Off below three pixels a square, full above sixteen. Between them it comes up gradually,
    // so a zoom does not switch the surface on.
    let lod = smoothstep(3.0, 16.0, px_per_square);
    let weathered = step(MINERAL, texel.a) * lod;
    // **Opaque, whatever the alpha said.** That channel is carrying which kind of wall this is,
    // not how much of one there is — a wall is solid, and one drawn at the alpha of its own
    // encoding would be a bedrock square you could see the water through.
    if weathered <= 0.0 {
        return vec4<f32>(texel.rgb, 1.0);
    }

    // Grain: three squares' worth of cells across one square, plus a finer octave at half the
    // weight. Centred on zero so it lightens as often as it darkens and the average value of the
    // rock is the colour `paint_barriers` chose.
    let g = TILT * sq;
    let grain = (value_noise(g * 3.0) - 0.5) + 0.5 * (value_noise(g * 7.0 + 19.7) - 0.5);
    // The fine octave alone is what aliases first, so it is held back until a square is wide.
    let fine = smoothstep(8.0, 24.0, px_per_square);

    // Pitting: the top of a coarser field, so it appears in patches rather than evenly. A pit is
    // a hollow, so it only ever darkens.
    let pit = smoothstep(0.60, 0.88, value_noise(TILT * sq * 2.6 + 4.3));

    var rgb = texel.rgb;
    rgb *= 1.0 + 0.26 * grain * weathered * mix(0.55, 1.0, fine);
    rgb *= 1.0 - 0.22 * pit * weathered;
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
