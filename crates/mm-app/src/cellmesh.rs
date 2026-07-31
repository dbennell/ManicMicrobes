//! Every cell on the slide, as one mesh (M10.5).
//!
//! # Why a mesh rather than sprites
//!
//! Cells were a sprite entity each: fifty thousand `Transform`s and `Sprite`s, extracted and
//! prepared by the renderer every frame, to draw fifty thousand quads that differ only in where
//! they are and what colour they are. The same objection as the chemical field, which was a
//! sprite per grid square until it became a texture.
//!
//! One mesh carries the whole population — four vertices per cell, one draw call, no entities.
//! The per-cell data rides along as vertex attributes, so the fragment shader can evaluate a
//! signed-distance field per pixel *per cell*: every cell gets its own outline rather than one
//! of sixteen baked silhouettes, it stays crisp at any magnification, and the shape can respond
//! to what the cell is actually doing. A failing membrane roughens the outline, which the baked
//! atlas could not do at all because the shape was fixed before the cell existed.
//!
//! # Why not true instancing
//!
//! A custom instanced pipeline would upload 32 bytes per cell instead of four vertices' worth,
//! and it is what `docs/UI.md` §7 ultimately describes. It also means a `SpecializedMeshPipeline`,
//! a custom `RenderCommand`, and your own extract, prepare and queue systems — the part of Bevy
//! with the least documentation and the most churn between releases. This gets the same look and
//! the same one draw call through `Material2d`, which is a supported, stable surface. The cost
//! is bandwidth: about 7 MB a frame at fifty thousand cells, which is a fifth of what the
//! chemical field texture was already costing and nobody noticed.
//!
//! Nothing here knows Bevy exists except through plain arrays. The vertex buffers are built by
//! [`build`], which is arithmetic over a [`Frame`] and is tested without a graphics stack.

use crate::slide::CellDot;

/// The per-cell data the shader needs that is not position, colour or corner.
///
/// One `vec4` rather than four attributes, because every attribute is a separate vertex buffer
/// binding and this is already the widest part of the upload.
///
/// * `x` — the cell's seed, which fixes its silhouette for life.
/// * `y` — edge softness, which is the depth of field.
/// * `z` — membrane integrity, 1 whole and 0 failing.
/// * `w` — 1 to evaluate the field, 0 to draw a plain quad. "Rounded cells: off" is this,
///   through the same draw call rather than a second code path.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Shape {
    pub seed: f32,
    pub softness: f32,
    pub integrity: f32,
    pub rounded: f32,
}

/// The vertex buffers for one frame's worth of cells.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Buffers {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colours: Vec<[f32; 4]>,
    pub shapes: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl Buffers {
    #[must_use]
    pub fn cells(&self) -> usize {
        self.positions.len() / 4
    }

    /// Start a frame: empty, and reserve for what is expected.
    ///
    /// Four vertices and six indices each, so a burst of divisions does not reallocate
    /// mid-frame.
    pub fn begin(&mut self, expect: usize) {
        self.clear();
        self.positions.reserve(expect * 4);
        self.indices.reserve(expect * 6);
    }

    /// Add one quad.
    ///
    /// Public because organelles go into the same buffers as the cells that hold them — one
    /// mesh, one draw call, and every blob on the slide evaluated by the same field. Drawn
    /// after the cells because they are inside them.
    pub fn push(&mut self, p: Placed) {
        // A quad cannot be degenerate: a zero-width cell is four coincident vertices, which is
        // two triangles of no area and a wasted six indices per frame.
        if p.half <= 0.0 {
            return;
        }
        let base = self.positions.len() as u32;
        // Anticlockwise from the top left, matching the corners in `uv` — the corner is what
        // the whole signed-distance field is evaluated against, so it has to be exact.
        for (dx, dy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            self.positions
                .push([p.x + dx * p.half, p.y + dy * p.half, 0.0]);
            self.uvs.push([dx, dy]);
            self.colours.push(p.rgba);
            self.shapes.push([
                p.shape.seed,
                p.shape.softness,
                p.shape.integrity,
                p.shape.rounded,
            ]);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn clear(&mut self) {
        self.positions.clear();
        self.uvs.clear();
        self.colours.clear();
        self.shapes.clear();
        self.indices.clear();
    }
}

/// Where and how big a cell is drawn, and what colour, worked out by the caller.
///
/// Passed in rather than computed here because it needs the camera, the optics and the
/// selection — three things this module has no business knowing about. What it does know is how
/// to turn a rectangle into four vertices.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placed {
    /// Centre, in the same space the renderer draws in.
    pub x: f32,
    pub y: f32,
    /// Half the width of the quad. The blob fills it; there is no baked-in margin to correct
    /// for, because there is no texture.
    pub half: f32,
    pub rgba: [f32; 4],
    pub shape: Shape,
}

/// Build the vertex buffers for a frame.
///
/// `place` decides where each cell goes and returns `None` for one that should not be drawn —
/// off-screen, or dead this frame. Reuses the buffers so a steady population allocates nothing
/// after the first frame.
pub fn build(into: &mut Buffers, cells: &[CellDot], place: impl Fn(&CellDot) -> Option<Placed>) {
    into.begin(cells.len());
    for dot in cells {
        if let Some(p) = place(dot) {
            into.push(p);
        }
    }
}

/// A cell's seed, as the shader wants it.
///
/// From the cell's identity, so its outline is fixed for life — an outline that changed frame to
/// frame would shimmer, and following one cell around is a thing the microscope is for. Kept
/// small because the shader hashes it as an `f32` and a large integer loses its low bits to the
/// mantissa, which would give whole runs of cells the same shape.
#[must_use]
pub fn seed_of(id: u64) -> f32 {
    (id.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / 64.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dots(n: usize) -> Vec<CellDot> {
        (0..n)
            .map(|i| CellDot {
                id: mm_core::CellId::NONE,
                x: i as f32,
                y: 0.0,
                radius: 0.5,
                rgb: [1.0, 1.0, 1.0],
                depth: 0.0,
                cluster_size: 1,
                organelles: Vec::new(),
            })
            .collect()
    }

    fn placed(dot: &CellDot) -> Option<Placed> {
        Some(Placed {
            x: dot.x,
            y: dot.y,
            half: dot.radius,
            rgba: [1.0, 1.0, 1.0, 1.0],
            shape: Shape {
                rounded: 1.0,
                integrity: 1.0,
                ..Shape::default()
            },
        })
    }

    #[test]
    fn every_cell_becomes_one_quad() {
        let mut buf = Buffers::default();
        build(&mut buf, &dots(5), placed);
        assert_eq!(buf.cells(), 5);
        assert_eq!(buf.positions.len(), 20);
        assert_eq!(buf.uvs.len(), 20);
        assert_eq!(buf.colours.len(), 20);
        assert_eq!(buf.shapes.len(), 20);
        assert_eq!(buf.indices.len(), 30);
    }

    #[test]
    fn every_attribute_is_the_same_length() {
        // A mesh whose attributes disagree is a validation error at draw time, several layers
        // from whichever `push` was forgotten.
        let mut buf = Buffers::default();
        build(&mut buf, &dots(9), placed);
        let n = buf.positions.len();
        assert_eq!(buf.uvs.len(), n);
        assert_eq!(buf.colours.len(), n);
        assert_eq!(buf.shapes.len(), n);
    }

    #[test]
    fn no_index_points_past_the_end() {
        // The other way to get a validation error, and the one that reads as a driver crash.
        let mut buf = Buffers::default();
        build(&mut buf, &dots(7), placed);
        let n = buf.positions.len() as u32;
        assert!(
            buf.indices.iter().all(|i| *i < n),
            "an index is out of range"
        );
    }

    #[test]
    fn the_corners_cover_the_whole_field() {
        // The corner *is* the coordinate the signed-distance field is evaluated at, so the four
        // of them have to be exactly the corners of -1..1. Anything else and the blob is clipped
        // or floating in the middle of its own quad.
        let mut buf = Buffers::default();
        build(&mut buf, &dots(1), placed);
        let mut corners = buf.uvs.clone();
        corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(
            corners,
            vec![[-1.0, -1.0], [-1.0, 1.0], [1.0, -1.0], [1.0, 1.0]]
        );
    }

    #[test]
    fn a_quad_is_placed_where_it_was_put_and_sized_how_it_was_asked() {
        let mut buf = Buffers::default();
        let one = vec![CellDot {
            x: 10.0,
            y: -4.0,
            radius: 3.0,
            ..dots(1).remove(0)
        }];
        build(&mut buf, &one, placed);
        let xs: Vec<f32> = buf.positions.iter().map(|p| p[0]).collect();
        let ys: Vec<f32> = buf.positions.iter().map(|p| p[1]).collect();
        assert_eq!(xs.iter().cloned().fold(f32::MAX, f32::min), 7.0);
        assert_eq!(xs.iter().cloned().fold(f32::MIN, f32::max), 13.0);
        assert_eq!(ys.iter().cloned().fold(f32::MAX, f32::min), -7.0);
        assert_eq!(ys.iter().cloned().fold(f32::MIN, f32::max), -1.0);
    }

    #[test]
    fn a_cell_the_caller_refuses_costs_nothing() {
        let mut buf = Buffers::default();
        build(&mut buf, &dots(4), |dot| {
            if dot.x < 2.0 {
                None
            } else {
                placed(dot)
            }
        });
        assert_eq!(buf.cells(), 2);
        let n = buf.positions.len() as u32;
        assert!(buf.indices.iter().all(|i| *i < n));
    }

    #[test]
    fn a_cell_with_no_size_is_dropped_rather_than_drawn_as_nothing() {
        let mut buf = Buffers::default();
        build(&mut buf, &dots(3), |dot| {
            Some(Placed {
                half: 0.0,
                ..placed(dot).unwrap()
            })
        });
        assert_eq!(buf.cells(), 0);
        assert!(buf.indices.is_empty());
    }

    #[test]
    fn building_again_replaces_rather_than_appends() {
        // The buffers are reused every frame. One missed `clear` and the mesh grows without
        // bound until the machine stops.
        let mut buf = Buffers::default();
        build(&mut buf, &dots(6), placed);
        build(&mut buf, &dots(2), placed);
        assert_eq!(buf.cells(), 2);
        assert_eq!(buf.indices.len(), 12);
    }

    #[test]
    fn a_seed_is_stable_spread_and_small_enough_to_survive_a_float() {
        // Stable, or a cell's outline shimmers. Spread, because a burst of divisions produces
        // consecutive ids and a run of identical neighbours gives the game away. Small, because
        // the shader hashes it as an `f32`: a large integer loses its low bits to the mantissa
        // and whole runs of cells come out the same shape.
        assert_eq!(seed_of(12_345), seed_of(12_345));
        let seeds: Vec<f32> = (0..64).map(seed_of).collect();
        for (i, a) in seeds.iter().enumerate() {
            assert!(a.is_finite() && a.abs() < 1e7, "seed {i} is unusable: {a}");
            for b in &seeds[i + 1..] {
                assert!((a - b).abs() > 1e-4, "two consecutive ids share a seed");
            }
        }
    }
}
