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

    fn pixel(pixels: &[u8], variant: usize, x: usize, y: usize) -> [u8; 4] {
        let at = ((y * atlas_width()) + variant * TILE + x) * 4;
        [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
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
