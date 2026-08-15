//! What a cell looks like up close (M10.5, `docs/UI.md` §7).
//!
//! Cells were flat squares — a sprite with a colour and no texture — which reads as a data
//! visualisation rather than as something alive. This bakes a small atlas of shaded, slightly
//! irregular blobs to put under that colour instead.
//!
//! # Why baked rather than a fragment shader
//!
//! `docs/UI.md` §7 designs a real instanced pipeline with a signed-distance shader, and that is
//! still the right end state — it is the only version that scales to fifty thousand cells in
//! one draw call, and it is what M10.5 proper is for. This is not that. It is the same look,
//! computed once at startup into a texture, so the existing sprite path keeps working
//! untouched and the whole change is revertible by turning it off.
//!
//! What that costs: every cell of a given variant has the same silhouette, because the shape is
//! in the texture rather than evaluated per pixel per cell. Sixteen variants make that hard to
//! notice in a crowd and impossible to notice in motion, which is the bar for "a bit of visual
//! flare", not the bar for the real thing.
//!
//! What it keeps: one image means one texture binding, so `bevy_sprite` batches exactly as it
//! did and this costs no draw calls at all.
//!
//! # The look
//!
//! A hemisphere normal lit from the upper left, which is what turns a disc into a ball; a rim
//! term, which is what makes it look wet; a few harmonics of angle for the irregular edge; and
//! faint interior grain, because a flat disc reads as a sprite and a grainy one reads as
//! cytoplasm.
//!
//! Baked as **white with a luminance ramp**, so the sprite's own colour still supplies the
//! species tint and everything the renderer already decides — depth of field, vignette,
//! selection — keeps working by multiplication.
use rayon::prelude::*;

/// Side of one variant, in pixels.
///
/// Cells are a few pixels across at whole-slide zoom and tens of pixels at full magnification.
/// Sixty-four is comfortably above the second and the atlas is 16 KB either way.
pub const TILE: usize = 64;

/// How many silhouettes to bake.
///
/// A power of two so choosing one from a cell id is a mask. Sixteen is enough that a crowd does
/// not visibly repeat; the eye picks up a pattern at four and stops counting somewhere past ten.
pub const VARIANTS: usize = 16;

/// The tile that is not a blob: a plain opaque square, exactly what a cell looked like before.
///
/// Turning the effect off is then a change of tile index rather than a second code path — no
/// swapping textures, no branch in the draw loop, and the flat look stays available for a
/// screenshot that wants to show data rather than biology.
pub const FLAT: usize = VARIANTS;

/// Tiles in the atlas: every silhouette, plus [`FLAT`].
pub const TILES: usize = VARIANTS + 1;

/// Roughly how much of a tile's width the blob occupies.
///
/// A sprite draws the whole tile, transparent margin included, so a cell drawn at the size the
/// renderer asks for would come out this much *smaller* than it used to — noticeably so at
/// whole-slide zoom, where a cell is two pixels across and losing a fifth of them means losing
/// the cell. Callers divide by this to keep the visible body the size the simulation says.
pub const FILL: f32 = 0.82;

/// The whole atlas, laid out as one row: `TILES * TILE` by `TILE`, RGBA8.
#[must_use]
pub fn atlas() -> Vec<u8> {
    let width = atlas_width();
    let mut pixels = vec![0u8; width * TILE * 4];
    for v in 0..VARIANTS {
        bake(v, &mut pixels, width);
    }
    bake_flat(&mut pixels, width);
    pixels
}

/// Width of the atlas in pixels.
#[must_use]
pub fn atlas_width() -> usize {
    TILE * TILES
}

/// A plain white square, inset by a pixel.
///
/// The inset is not decoration. Tiles sit edge to edge and the sampler is linear, so a tile
/// that was opaque right to its border would pull its neighbour's border in when magnified.
/// One transparent pixel costs 1.5% of the sprite and makes that impossible.
fn bake_flat(pixels: &mut [u8], width: usize) {
    for py in 1..TILE - 1 {
        for px in 1..TILE - 1 {
            let at = ((py * width) + FLAT * TILE + px) * 4;
            if let Some(p) = pixels.get_mut(at..at + 4) {
                p.copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
}

/// Which variant a cell gets.
///
/// From the cell's identity, so it is stable for that cell's whole life — a blob whose
/// silhouette changed frame to frame would shimmer, and following one cell around is a thing
/// the microscope is for. Multiplied by an odd constant first so that consecutive ids, which is
/// what a burst of divisions produces, do not come out as a visible run of neighbours.
#[must_use]
pub fn variant_of(id: u64) -> usize {
    (id.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 59) as usize % VARIANTS
}

/// What a barrier square is painted, before the vignette.
///
/// A wall is drawn rather than left as a hole. It is opaque and it ignores both the light and
/// the chemical layers — which is not a stylistic choice but what the substrate already says:
/// `Substrate::set_blocked` evicts the square's contents and refuses to accept any, and the
/// light regime shadows it to zero, so a blocked square genuinely has nothing in it to paint.
///
/// Cool and desaturated, against a slide whose every other colour is warm light or a chemical
/// tint, so a wall does not read as a dense patch of something. Light enough to be plainly
/// visible on an unlit slide, dark enough not to compete with the cells, which are the subject.
pub const BARRIER_RGB: [f32; 3] = [0.17, 0.18, 0.22];

/// Paint the barrier mask into its own RGBA buffer, transparent everywhere else.
///
/// # Why this is not a layer of [`paint_field`]
///
/// It was, for exactly one commit, and the walls came out blurred — badly so at high
/// magnification, where a wall was a soft band several pixels wider than the square it stood on.
///
/// The cause is that the field texture is sampled **linearly**, and that is right for what was
/// in it and wrong for this. A diffusion field is a continuous quantity sampled on a grid, so
/// interpolating between two measured squares is a more faithful picture of it than hard blocks
/// are. A barrier is not a sampled continuum. It is a binary property of a square — blocked or
/// not, with nothing in between — so interpolating it invents half a wall, which is a value the
/// simulation never held and a thing the world does not contain. `UI.md` §1 asks that nothing be
/// drawn which the simulation did not produce, and a smeared wall is exactly that.
///
/// One sampler cannot be right for both, so they are two textures: this one nearest-sampled and
/// alpha-composited over the field, which keeps the chemistry smooth and gives a wall the hard
/// edge it actually has.
pub fn paint_barriers(
    into: &mut [u8],
    width: usize,
    height: usize,
    barriers: &[bool],
    dim: impl Fn(f32, f32) -> f32 + Sync,
) {
    // `par_chunks_mut` panics on a zero chunk size, where the plain loop this replaced simply
    // did nothing. A slide with no width is not a thing to draw.
    if width == 0 || height == 0 {
        return;
    }
    // A row per task, for the reasons written out over `paint_field`.
    into.par_chunks_mut(width * 4)
        .take(height)
        .enumerate()
        .for_each(|(y, row)| {
        for x in 0..width {
            let i = y * width + x;
            let Some(px) = row.get_mut(x * 4..x * 4 + 4) else {
                continue;
            };
            if barriers.get(i).copied().unwrap_or(false) {
                // The vignette applies here as it does to everything else on the plate: it is
                // the objective, and a wall is under the objective too.
                let d = dim(x as f32 + 0.5, y as f32 + 0.5);
                for k in 0..3 {
                    px[k] = ((BARRIER_RGB[k] * d).clamp(0.0, 1.0) * 255.0) as u8;
                }
                px[3] = 255;
            } else {
                // Fully transparent, so the field shows through unchanged. Zeroed rather than
                // left alone because this buffer is reused between frames and a wall that was
                // erased has to actually go.
                px.copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        });
}

/// Paint the chemical overlays and the light into an RGBA buffer (M10.5).
///
/// Barriers are **not** in here; they are their own texture, for the sampler reason written
/// out in [`paint_barriers`].
///
/// # Why this exists
///
/// The substrate was drawn as **one sprite entity per grid square** — 262,144 of them at
/// 512×512, every one carrying a `Transform` and a `Sprite`, extracted and prepared by the
/// renderer every frame, to show a single texel each. It was by a wide margin the largest cost
/// in the renderer and it bought nothing: a grid of coloured squares is a texture.
///
/// The arithmetic per square is unchanged — the same layers, the same square-root curve, the
/// same vignette. What goes is the quarter of a million entities.
///
/// `dim` is asked for the vignette at a square, in square coordinates, because where a square
/// lands on screen depends on the camera and this function has no business knowing about one.
/// Tests pass a closure that returns 1.
///
/// Returns the slide's mean colour *before* the vignette, which is what a defocused cell fades
/// into. Derived rather than named as a constant because the slide is not a fixed colour: it is
/// the warm light plus whichever overlays are switched on, so a haze fixed at the unlit brown
/// would fade cells towards a tone that is nowhere on screen the moment an overlay is enabled.
/// Accumulated here because this loop is already visiting every texel; asking separately would
/// be a second pass over the whole plane every frame.
pub fn paint_field(
    into: &mut [u8],
    width: usize,
    height: usize,
    light: &[f32],
    layers: &[(&[f32], [f32; 3])],
    dim: impl Fn(f32, f32) -> f32 + Sync,
) -> [f32; 3] {
    // Layers add rather than one winning, so two overlays on at once look like two overlays on
    // at once — divided by the count so the sum stays inside the channel.
    let share = (layers.len() as f32).max(1.0);
    // Same guard as `paint_barriers`: a zero chunk size is a panic, and an empty slide has no
    // mean colour to report. The caller already treats this as "nothing to paint".
    if width == 0 || height == 0 {
        return [0.0; 3];
    }
    // A row per task.
    //
    // This is the largest single cost in a rendered frame and it was being paid on one thread:
    // measured at 5.4ms against 0.26ms for building every cell's mesh, and — because it is one
    // pass over the whole grid whatever is living on it — near enough the same 5ms on a slide
    // with a thousand cells as on one with twenty thousand. It is a fixed toll the microscope
    // pays before a single cell is drawn.
    //
    // `dim` is a generic `impl Fn` rather than the `&dyn Fn` it was, which matters more than the
    // threading does on a per-texel basis: it was an indirect call that could not be inlined,
    // made a quarter of a million times a frame, around arithmetic small enough that the call
    // was a good share of the cost.
    //
    // The rows are collected in order and only then summed, rather than reduced as they finish.
    // Floating-point addition is not associative, so a reduction in completion order would make
    // the haze — and therefore every out-of-focus cell — depend on how rayon happened to split
    // the frame, and two runs of the same scenario would not produce the same screenshot. This
    // grouping is fixed, so they do.
    let rows: Vec<[f64; 3]> = into
        .par_chunks_mut(width * 4)
        .take(height)
        .enumerate()
        .map(|(y, row)| {
        // `f64`, because this runs to a quarter of a million texels at 512×512 and an `f32`
        // accumulator that has reached the tens of thousands cannot see a 0.1 added to it.
        let mut sum = [0.0f64; 3];
        {
            for x in 0..width {
            let i = y * width + x;
            // Light as a warm luminance under the chemical layers (SPEC §14).
            let warm = 0.10 * light.get(i).copied().unwrap_or(0.0);
            let mut rgb = [warm, warm * 0.92, warm * 0.75];
            for (field, tint) in layers {
                let Some(c) = field.get(i) else {
                    continue;
                };
                // Square root, not the raw fraction. A field is normalised against its own
                // peak, and in a diffused world almost every square sits far below that peak —
                // so linear mapping renders the whole slide black except wherever the maximum
                // happens to be. The curve is presentation only; the field stays linear and the
                // legend still reports the peak the eye is being lied to about.
                let shade = c.max(0.0).sqrt();
                for (channel, t) in rgb.iter_mut().zip(tint) {
                    *channel += t * shade / share;
                }
            }
            // Summed before the vignette. The cell it hazes is already multiplied by the
            // vignette at its own position, and a haze that carried the field's average dimming
            // as well would charge a cell at the centre for darkness at the corners.
            for k in 0..3 {
                sum[k] += f64::from(rgb[k]);
            }
            let d = dim(x as f32 + 0.5, y as f32 + 0.5);
            let Some(px) = row.get_mut(x * 4..x * 4 + 4) else {
                continue;
            };
            for k in 0..3 {
                px[k] = ((rgb[k] * d).clamp(0.0, 1.0) * 255.0) as u8;
            }
            px[3] = 255;
            }
        }
        sum
        })
        .collect();
    let mut sum = [0.0f64; 3];
    for row in rows {
        for k in 0..3 {
            sum[k] += row[k];
        }
    }
    let texels = (width * height) as f64;
    if texels == 0.0 {
        return [0.0; 3];
    }
    [
        (sum[0] / texels) as f32,
        (sum[1] / texels) as f32,
        (sum[2] / texels) as f32,
    ]
}

/// How long a speck of suspended particulate is followed before it starts again, in ticks.
///
/// Long enough that the motion reads as a current rather than a shimmer; short enough that a
/// speck never travels far from the block whose velocity it was given, which is the whole
/// approximation here — a speck is carried by the water where it *started*, not integrated
/// through the field it crosses.
pub const SPECK_LIFE: u64 = 84;

/// One speck of suspended particulate, offset from its lattice point in squares.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Speck {
    pub dx: f32,
    pub dy: f32,
    /// `0..1`, and never 1 at the ends of a life.
    pub alpha: f32,
}

/// Where one speck is at `tick`, or `None` if the water there is too clear to show it.
///
/// **A speck is not a thing.** There is no particle in the simulation it corresponds to, and
/// there must never be one: detritus is a chemical field, and the only honest reading of this
/// picture is *density* — how many specks are in a region is what the concentration says, and
/// which speck is which means nothing. So they cannot be clicked, selected, counted or
/// followed, and nothing in `Frame` gives them identity beyond the index that shuffles them.
///
/// A pure function of `(index, tick)`, like `optics::motes`, and for the same reason: the
/// alternative is a pile of positions the renderer has to integrate and keep, which drifts out
/// of step with the simulation the moment a frame is dropped, has to be rebuilt whenever the
/// camera or the slide changes, and would be the one piece of the view with a memory.
///
/// The speck starts at its lattice point, is carried along the local velocity for
/// [`SPECK_LIFE`] ticks, then begins again — fading in and out across that life so the restart
/// is not a jump. `conc` is the block's share of the busiest block, `0..1`, and gates the speck
/// against a threshold fixed per index: as the concentration rises past a speck's threshold it
/// fades up rather than appearing, so a gradient reads as a gradient and not as a set of rings.
#[must_use]
pub fn speck(index: u64, tick: u64, vel: [f32; 2], stride: f32, conc: f32) -> Option<Speck> {
    let h = mix(index);
    // Fixed per speck: which of them are visible at a given concentration never shuffles, so
    // rising concentration adds specks to the ones already there instead of redrawing the lot.
    let threshold = (h >> 40) as f32 / 16_777_216.0;
    let over = (conc - threshold) * SPECK_EDGE;
    if over <= 0.0 {
        return None;
    }
    let life = SPECK_LIFE.max(1);
    // Staggered, or every speck on the slide would restart on the same tick.
    let t = (tick.wrapping_add(h % life) % life) as f32;
    let u = t / life as f32;
    // A parabola rather than a triangle: zero at both ends with no kink in the middle, so a
    // speck neither appears nor snaps.
    let fade = 1.0 - (u * 2.0 - 1.0) * (u * 2.0 - 1.0);
    // Somewhere in its block rather than exactly on the lattice point, or the specks are a grid.
    let jx = ((h >> 8) & 0xFFFF) as f32 / 65_536.0 - 0.5;
    let jy = ((h >> 24) & 0xFFFF) as f32 / 65_536.0 - 0.5;
    Some(Speck {
        dx: jx * stride + vel[0] * t,
        dy: jy * stride + vel[1] * t,
        alpha: fade * over.min(1.0),
    })
}

/// How sharply a speck fades in once the concentration passes its threshold: `1/SPECK_EDGE` of
/// the range is spent fading rather than popping.
const SPECK_EDGE: f32 = 6.0;

fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^ (x >> 33)
}

/// Cheap deterministic noise in `0..1`, for the grain and the per-variant phases.
fn hash01(mut x: u64) -> f32 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^= x >> 33;
    // Top 24 bits, so the result is well distributed and exactly representable.
    (x >> 40) as f32 / 16_777_216.0
}

fn bake(variant: usize, pixels: &mut [u8], width: usize) {
    let seed = variant as u64 + 1;
    // Three harmonics of angle. Amplitudes small enough that the blob stays a blob: past about
    // 0.2 total it starts to look like a splat rather than a cell, and past 0.35 the outline
    // folds back on itself.
    let harmonics: [(f32, f32, f32); 3] = [
        (
            3.0,
            0.055 + 0.045 * hash01(seed * 7),
            hash01(seed * 11) * std::f32::consts::TAU,
        ),
        (
            5.0,
            0.030 + 0.030 * hash01(seed * 13),
            hash01(seed * 17) * std::f32::consts::TAU,
        ),
        (
            7.0,
            0.015 + 0.020 * hash01(seed * 19),
            hash01(seed * 23) * std::f32::consts::TAU,
        ),
    ];

    // Light from the upper left, as a microscope's condenser throws it. Screen y runs down, so
    // "up" is negative.
    let light = {
        let v = [-0.42f32, -0.52, 0.74];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / len, v[1] / len, v[2] / len]
    };

    for py in 0..TILE {
        for px in 0..TILE {
            // Sample at pixel centres, in -1..1.
            let u = (px as f32 + 0.5) / TILE as f32 * 2.0 - 1.0;
            let v = (py as f32 + 0.5) / TILE as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            let theta = v.atan2(u);

            // The irregular outline. `margin` leaves room for the wobble and for the edge
            // fade, so nothing is clipped by the tile.
            let margin = 1.0 - FILL;
            let wobble: f32 = harmonics
                .iter()
                .map(|(k, a, phase)| a * (k * theta + phase).sin())
                .sum();
            let radius = (1.0 - margin) * (1.0 + wobble);

            // One pixel of softness, plus a little, so the edge is not a staircase when the
            // sprite is drawn larger than the tile.
            let edge = 2.5 / TILE as f32;
            let alpha = 1.0 - smoothstep(radius - edge, radius + edge, r);
            if alpha <= 0.0 {
                continue;
            }

            // The hemisphere. This is the whole trick: `sqrt(1 - r^2)` against a fixed light
            // is what turns a disc into a ball, and it is three operations.
            let t = (r / radius.max(1e-3)).min(1.0);
            let nz = (1.0 - t * t).max(0.0).sqrt();
            let (nx, ny) = if r > 1e-4 {
                (u / r * t, v / r * t)
            } else {
                (0.0, 0.0)
            };
            let lambert = (nx * light[0] + ny * light[1] + nz * light[2]).clamp(0.0, 1.0);

            // The bright edge that makes something look wet rather than matte.
            let rim = (1.0 - nz).powi(4);
            // Faint granularity. A flat disc reads as a sprite; a grainy one reads as
            // cytoplasm.
            let grain = hash01(seed * 1_000_003 + (py * TILE + px) as u64) - 0.5;
            // A thin brighter ring just inside the outline: the membrane.
            let membrane = smoothstep(radius - 0.16, radius - 0.02, r);

            // Baked as a luminance ramp on white, so the sprite's colour still supplies the
            // tint. The floor keeps the unlit side from going black, which at these sizes
            // reads as a hole rather than as shadow.
            let lum = 0.34 + 0.62 * lambert + 0.30 * rim + 0.18 * membrane + 0.045 * grain;
            let lum = lum.clamp(0.0, 1.0);

            let shade = (lum * 255.0).round().clamp(0.0, 255.0) as u8;
            let a = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
            let at = ((py * width) + variant * TILE + px) * 4;
            if let Some(px4) = pixels.get_mut(at..at + 4) {
                px4[0] = shade;
                px4[1] = shade;
                px4[2] = shade;
                px4[3] = a;
            }
        }
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    if (b - a).abs() < f32::EPSILON {
        return if x < a { 0.0 } else { 1.0 };
    }
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property that makes this a picture of the field rather than a picture of particles.
    #[test]
    fn clear_water_shows_nothing_and_thick_water_shows_most_of_it() {
        let vel = [0.0, 0.0];
        let count = |conc| {
            (0..2000u64)
                .filter(|&i| speck(i, 0, vel, 1.0, conc).is_some())
                .count()
        };
        assert_eq!(count(0.0), 0, "clear water drew specks");
        let (thin, thick) = (count(0.2), count(0.8));
        assert!(
            thin < thick,
            "density did not follow concentration: {thin} against {thick}"
        );
        // Proportional, because that is the reading the picture invites: a region twice as
        // speckled should be about twice as concentrated. Thresholds are uniform over `0..1`,
        // so the share visible at `c` is `c`, and this checks the hash is flat enough to
        // deliver that rather than merely monotone.
        for conc in [0.2, 0.5, 0.8] {
            let share = count(conc) as f32 / 2000.0;
            assert!(
                (share - conc).abs() < 0.04,
                "at concentration {conc} the share drawn was {share}"
            );
        }
    }

    /// Which specks are visible must not reshuffle as the concentration changes, or a smooth
    /// gradient crawls.
    #[test]
    fn a_speck_that_is_visible_stays_visible_as_the_water_thickens() {
        for i in 0..500u64 {
            if speck(i, 0, [0.0, 0.0], 1.0, 0.4).is_some() {
                assert!(
                    speck(i, 0, [0.0, 0.0], 1.0, 0.9).is_some(),
                    "speck {i} vanished when there was more of it"
                );
            }
        }
    }

    #[test]
    fn still_water_leaves_a_speck_where_it_is() {
        let a = speck(7, 0, [0.0, 0.0], 4.0, 1.0).expect("visible");
        let b = speck(7, 40, [0.0, 0.0], 4.0, 1.0).expect("visible");
        assert_eq!((a.dx, a.dy), (b.dx, b.dy), "it moved in still water");
        assert!(
            a.dx.abs() <= 2.0 && a.dy.abs() <= 2.0,
            "jitter left its block"
        );
    }

    #[test]
    fn moving_water_carries_it_downstream_at_the_speed_of_the_water() {
        let vel = [0.05, -0.02];
        let mut seen = Vec::new();
        for tick in 0..SPECK_LIFE {
            let s = speck(3, tick, vel, 0.0, 1.0).expect("visible");
            seen.push((s.dx, s.dy));
        }
        // One speck's whole life, in order, starting from wherever its stagger puts it.
        let (x0, y0) = seen[0];
        let t0 = (x0 / vel[0]).round();
        for (n, (x, y)) in seen.iter().enumerate() {
            let t = t0 + n as f32;
            let t = if t >= SPECK_LIFE as f32 {
                t - SPECK_LIFE as f32
            } else {
                t
            };
            assert!((x - vel[0] * t).abs() < 1e-3, "x wrong at {n}: {x}");
            assert!((y - vel[1] * t).abs() < 1e-3, "y wrong at {n}: {y}");
        }
    }

    /// The restart has to be invisible, which means every speck passes through nothing at both
    /// ends of its life however bright it gets in the middle.
    #[test]
    fn every_speck_fades_out_at_both_ends_of_its_life() {
        for i in 0..200u64 {
            let mut dimmest = 1.0f32;
            for tick in 0..SPECK_LIFE {
                if let Some(s) = speck(i, tick, [0.01, 0.0], 1.0, 1.0) {
                    dimmest = dimmest.min(s.alpha);
                }
            }
            assert!(dimmest < 0.01, "speck {i} never faded out: {dimmest}");
        }
    }

    /// A speck only just over its threshold is *meant* to be faint — that is the fade-in, and
    /// it means the rarest specks in a thick patch are the dim ones. What must not happen is
    /// that everything is dim: a speck well clear of its threshold reaches full brightness.
    #[test]
    fn a_speck_well_clear_of_its_threshold_reaches_full_brightness() {
        let brightest = (0..200u64)
            .flat_map(|i| (0..SPECK_LIFE).filter_map(move |t| speck(i, t, [0.01, 0.0], 1.0, 1.0)))
            .fold(0.0f32, |m, s| m.max(s.alpha));
        assert!(
            brightest > 0.99,
            "nothing reached full brightness: {brightest}"
        );
    }

    use super::*;

    fn pixel(pixels: &[u8], variant: usize, x: usize, y: usize) -> [u8; 4] {
        let at = ((y * atlas_width()) + variant * TILE + x) * 4;
        [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
    }

    #[test]
    fn painting_a_field_writes_every_texel_opaque() {
        // A texel left untouched is a black square in the middle of the slide, and a texel left
        // transparent shows the window's clear colour through the water.
        let (w, h) = (7usize, 5usize);
        let mut buf = vec![7u8; w * h * 4];
        let light = vec![0.0f32; w * h];
        paint_field(&mut buf, w, h, &light, &[], &|_, _| 1.0);
        for i in 0..w * h {
            assert_eq!(buf[i * 4 + 3], 255, "texel {i} is not opaque");
        }
    }

    #[test]
    fn a_layer_shows_up_where_it_is_and_not_where_it_is_not() {
        let (w, h) = (4usize, 1usize);
        let mut buf = vec![0u8; w * h * 4];
        let light = vec![0.0f32; w * h];
        let field = vec![0.0f32, 1.0, 0.0, 0.0];
        paint_field(
            &mut buf,
            w,
            h,
            &light,
            &[(&field, [1.0, 0.0, 0.0])],
            &|_, _| 1.0,
        );
        assert!(buf[4] > 200, "the square holding the chemical is not red");
        assert_eq!(buf[0], 0, "a square holding nothing was painted anyway");
        assert_eq!(buf[3 * 4], 0);
    }

    #[test]
    fn two_layers_mix_rather_than_one_winning() {
        let (w, h) = (1usize, 1usize);
        let mut buf = vec![0u8; 4];
        let light = vec![0.0f32];
        let red = vec![1.0f32];
        let blue = vec![1.0f32];
        paint_field(
            &mut buf,
            w,
            h,
            &light,
            &[(&red, [1.0, 0.0, 0.0]), (&blue, [0.0, 0.0, 1.0])],
            &|_, _| 1.0,
        );
        assert!(buf[0] > 0 && buf[2] > 0, "one layer swallowed the other");
    }

    #[test]
    fn the_vignette_reaches_the_field() {
        // The field is drawn as one quad now, so the vignette has to be painted into it — the
        // per-sprite dimming that used to do it went with the sprites.
        let (w, h) = (2usize, 1usize);
        let mut buf = vec![0u8; w * 4];
        let light = vec![1.0f32; w];
        paint_field(&mut buf, w, h, &light, &[], &|x, _| {
            if x < 1.0 {
                1.0
            } else {
                0.0
            }
        });
        assert!(buf[0] > 0, "the lit square came out black");
        assert_eq!(buf[4], 0, "the vignetted square was not dimmed");
    }

    #[test]
    fn the_haze_is_the_colour_of_the_slide_it_came_from() {
        // What a defocused cell fades into. It has to follow the overlays: a haze fixed at the
        // unlit brown would fade cells towards a colour that is nowhere on screen the moment an
        // overlay is switched on, and they would read as tinted rather than as distant.
        let (w, h) = (2usize, 1usize);
        let mut buf = vec![0u8; w * 4];
        let light = vec![0.0f32; w];
        let field = vec![1.0f32, 0.0];
        let haze = paint_field(
            &mut buf,
            w,
            h,
            &light,
            &[(&field, [1.0, 0.0, 0.0])],
            &|_, _| 1.0,
        );
        // One of the two squares is fully red, so the mean is half red.
        assert!(
            (haze[0] - 0.5).abs() < 1e-5,
            "the haze did not follow the overlay: {haze:?}"
        );
        assert_eq!([haze[1], haze[2]], [0.0, 0.0]);

        // And before the vignette, not after. The cell being hazed is already dimmed by the
        // vignette where *it* stands, so a haze carrying the field's average dimming as well
        // would charge a cell at the centre of the field for darkness out at the corners.
        let dimmed = paint_field(
            &mut buf,
            w,
            h,
            &light,
            &[(&field, [1.0, 0.0, 0.0])],
            &|_, _| 0.0,
        );
        assert_eq!(dimmed, haze, "the vignette leaked into the haze");
    }

    #[test]
    fn an_empty_slide_has_a_haze_and_does_not_divide_by_zero() {
        let haze = paint_field(&mut [], 0, 0, &[], &[], &|_, _| 1.0);
        assert_eq!(haze, [0.0; 3]);
    }

    #[test]
    fn a_barrier_is_painted_as_a_wall_and_not_as_a_hole() {
        // The whole point. Before this, a wall was whatever the light and the chemistry were
        // *not* — which on a dark slide is nothing at all, and on a lit one is a shadow.
        let (w, h) = (3usize, 1usize);
        let mut buf = vec![0u8; w * 4];
        // Dark slide, no light anywhere, so nothing else can be painting this.
        let light = vec![0.0f32; w];
        let barriers = vec![false, true, false];
        paint_field(&mut buf, w, h, &light, &[], &|_, _| 1.0);
        let mut wall_layer = vec![0u8; w * 4];
        paint_barriers(&mut wall_layer, w, h, &barriers, &|_, _| 1.0);
        assert_eq!(
            [buf[0], buf[1], buf[2]],
            [0, 0, 0],
            "open water on an unlit slide is not black"
        );
        assert_eq!(
            wall_layer[3], 0,
            "open water is opaque in the wall layer, so it would hide the field"
        );
        let buf = wall_layer;
        assert!(
            buf[4] > 20 && buf[5] > 20 && buf[6] > 20,
            "the barrier square was not painted: {:?}",
            &buf[4..8]
        );
        assert!(
            buf[6] > buf[4],
            "the wall should be cool, not warm like the light"
        );
    }

    #[test]
    fn a_wall_is_opaque_to_the_overlays_that_cross_it() {
        // A blocked square holds nothing — `Substrate::set_blocked` evicts its contents and
        // refuses more — so an overlay painting through one would be drawing chemistry that
        // is not there.
        let (w, h) = (2usize, 1usize);
        let mut buf = vec![0u8; w * 4];
        let light = vec![0.0f32; w];
        let field = vec![1.0f32, 1.0f32];
        let barriers = vec![false, true];
        paint_field(
            &mut buf,
            w,
            h,
            &light,
            &[(&field, [1.0, 0.0, 0.0])],
            &|_, _| 1.0,
        );
        let mut wall_layer = vec![0u8; w * 4];
        paint_barriers(&mut wall_layer, w, h, &barriers, &|_, _| 1.0);
        assert!(buf[0] > 200, "the open square lost its overlay");
        // The field still paints chemistry under the wall — it is the wall layer on top,
        // opaque, that hides it. Cheaper than branching per texel, and invisible because a
        // blocked square holds nothing to paint in the first place.
        assert_eq!(
            wall_layer[7], 255,
            "the wall is not opaque over the overlay"
        );
        assert_eq!(wall_layer[3], 0, "open water is not transparent");
    }

    #[test]
    fn a_short_buffer_or_a_short_field_is_survivable() {
        // The grid changes size when a scenario is loaded, and for a frame the buffer and the
        // frame can disagree. Painting past the end of either is not an option.
        let mut buf = vec![0u8; 4];
        let light = vec![1.0f32; 100];
        let field = vec![0.5f32; 2];
        // The barrier mask is short too, for the same reason and with the same answer.
        paint_field(
            &mut buf,
            10,
            10,
            &light,
            &[(&field, [1.0, 1.0, 1.0])],
            &|_, _| 1.0,
        );
        // The barrier mask is short too, for the same reason and with the same answer.
        paint_barriers(&mut buf, 10, 10, &vec![true; 3], &|_, _| 1.0);
    }

    #[test]
    fn the_atlas_is_the_size_it_says_it_is() {
        let pixels = atlas();
        assert_eq!(pixels.len(), atlas_width() * TILE * 4);
        assert_eq!(atlas_width(), TILE * TILES);
    }

    #[test]
    fn the_flat_tile_is_a_plain_square_that_does_not_touch_its_neighbours() {
        // What "turn it off" selects. Opaque and unshaded everywhere inside, so a cell drawn
        // with it looks exactly like a cell did before — and clear at the border, so linear
        // sampling cannot drag the neighbouring blob in with it.
        let pixels = atlas();
        for (x, y) in [(TILE / 2, TILE / 2), (2, 2), (TILE - 3, TILE - 3)] {
            assert_eq!(pixel(&pixels, FLAT, x, y), [255, 255, 255, 255]);
        }
        for (x, y) in [(0, TILE / 2), (TILE - 1, TILE / 2), (TILE / 2, 0)] {
            assert_eq!(
                pixel(&pixels, FLAT, x, y)[3],
                0,
                "the flat tile reaches its own border at ({x}, {y})"
            );
        }
        assert!(
            !(0..1024u64).any(|id| variant_of(id) == FLAT),
            "a cell drew the flat tile by chance"
        );
    }

    #[test]
    fn every_variant_is_opaque_in_the_middle_and_clear_at_the_corners() {
        // The shape at all: a blob, not a square and not an empty tile. A variant that came out
        // wholly transparent would draw nothing and look exactly like a cell that had died.
        let pixels = atlas();
        for v in 0..VARIANTS {
            let middle = pixel(&pixels, v, TILE / 2, TILE / 2);
            assert_eq!(middle[3], 255, "variant {v} is not solid in the middle");
            for (x, y) in [(0, 0), (TILE - 1, 0), (0, TILE - 1), (TILE - 1, TILE - 1)] {
                assert_eq!(
                    pixel(&pixels, v, x, y)[3],
                    0,
                    "variant {v} reaches the corner at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn it_is_grey_so_the_sprite_colour_still_tints_it() {
        // Baked as luminance on white. A variant with colour of its own would fight the species
        // tint, and every cell on the slide would come out the same shade of whatever it was.
        let pixels = atlas();
        for v in 0..VARIANTS {
            for y in (2..TILE - 2).step_by(7) {
                for x in (2..TILE - 2).step_by(7) {
                    let p = pixel(&pixels, v, x, y);
                    assert_eq!(p[0], p[1], "variant {v} is not grey at ({x}, {y})");
                    assert_eq!(p[1], p[2], "variant {v} is not grey at ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn it_is_lit_from_one_side() {
        // The hemisphere is the whole reason this looks like a ball rather than a disc. If the
        // upper left is not brighter than the lower right, the normal is wrong or the light is
        // pointing at nothing.
        let pixels = atlas();
        let q = TILE / 4;
        for v in 0..VARIANTS {
            let lit = pixel(&pixels, v, q, q)[0];
            let shadowed = pixel(&pixels, v, TILE - q, TILE - q)[0];
            assert!(
                lit > shadowed,
                "variant {v} is not lit from the upper left: {lit} against {shadowed}"
            );
        }
    }

    #[test]
    fn the_edge_is_soft() {
        // Walking out along a row, alpha has to pass through something other than 0 and 255 or
        // the outline is a staircase and the whole exercise is pointless.
        let pixels = atlas();
        let y = TILE / 2;
        let mut partial = 0;
        for v in 0..VARIANTS {
            for x in 0..TILE {
                let a = pixel(&pixels, v, x, y)[3];
                if a > 0 && a < 255 {
                    partial += 1;
                }
            }
        }
        assert!(
            partial >= VARIANTS * 2,
            "only {partial} soft pixels in the lot"
        );
    }

    #[test]
    fn the_variants_are_actually_different() {
        // Sixteen copies of one blob is one blob. Compared by silhouette rather than by pixels,
        // because two variants could differ only in grain and still look identical in a crowd.
        let pixels = atlas();
        let silhouette = |v: usize| -> Vec<bool> {
            (0..TILE)
                .flat_map(|y| (0..TILE).map(move |x| (x, y)))
                .map(|(x, y)| pixel(&pixels, v, x, y)[3] > 127)
                .collect()
        };
        let shapes: Vec<Vec<bool>> = (0..VARIANTS).map(silhouette).collect();
        for a in 0..VARIANTS {
            for b in (a + 1)..VARIANTS {
                let differing = shapes[a]
                    .iter()
                    .zip(&shapes[b])
                    .filter(|(p, q)| p != q)
                    .count();
                assert!(
                    differing > TILE,
                    "variants {a} and {b} differ in only {differing} pixels"
                );
            }
        }
    }

    #[test]
    fn a_cell_keeps_its_silhouette_and_neighbours_do_not_share_one() {
        // Stable, because a blob that changed shape frame to frame would shimmer and following
        // one cell is a thing the microscope is for. Spread, because a burst of divisions
        // produces consecutive ids and a visible run of identical neighbours gives the game
        // away instantly.
        assert_eq!(variant_of(12_345), variant_of(12_345));
        let run: std::collections::BTreeSet<usize> = (0..VARIANTS as u64).map(variant_of).collect();
        assert!(
            run.len() >= VARIANTS / 2,
            "sixteen consecutive ids used only {} silhouettes",
            run.len()
        );
        assert!((0..1024u64).all(|id| variant_of(id) < VARIANTS));
    }

    #[test]
    fn baking_is_deterministic() {
        // Two runs of the microscope must not look different, and a variant is chosen by cell
        // id — so if the atlas were not stable, the same cell would look different between
        // sessions.
        assert_eq!(atlas(), atlas());
    }
}
