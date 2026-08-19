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
//! the same one draw call through `Material2d`, which is a supported, stable surface.
//!
//! The cost is bandwidth, and the figure here read 7 MB a frame at fifty thousand cells for a
//! long time after it stopped being true — it was written when a cell carried four seams. Twelve
//! of them is 152 bytes a vertex, four vertices a cell, so **32 MB a frame at fifty thousand
//! cells**, and two thirds of that is seams that below `Lod::Packed` are all the same sentinel.
//! It used to be twice that again, because `cellpipe::upload` cloned every attribute on the way
//! in; it now swaps them across instead, which is why [`Buffers::begin`] must clear.
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
/// * `w` — 1 to evaluate the field, 0 to draw a plain quad. The microscope always asks for the
///   field; the plain quad is the bench's calibration, through the same draw call rather than a
///   second code path.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Shape {
    pub seed: f32,
    pub softness: f32,
    pub integrity: f32,
    pub rounded: f32,
}

/// How many flattened sides a cell can be drawn with. Matches `mm_core::CONTACTS_PER_CELL`.
pub const SQUASH_PER_CELL: usize = 12;

/// A seam far enough out that nothing is ever cut by it.
///
/// Unused slots carry this rather than a flag, so the shader applies four seams unconditionally
/// and never branches on how many a cell happens to have.
///
/// Far, but *not* enormous, and the difference is not academic. The quad's corner is at
/// `sqrt(2)` in field units, so anything past about two already never cuts. A huge sentinel
/// instead breaks the smooth intersection that combines the seams: it interpolates as
/// `b + h*(a - b)`, and when `b` is a billion, `a - b` rounds to exactly `-b` in `f32` and the
/// field value it was supposed to preserve is annihilated. Every pixel comes back zero and the
/// cell draws as a translucent square.
pub const NO_SQUASH: f32 = 8.0;

/// One flat side, as the shader wants it: a direction and how far along it the seam sits.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Squash {
    pub nx: f32,
    pub ny: f32,
    /// In field units, so it is directly comparable with the outline radius.
    pub face: f32,
}

impl Default for Squash {
    fn default() -> Self {
        Squash {
            nx: 1.0,
            ny: 0.0,
            face: NO_SQUASH,
        }
    }
}

/// Two unit-vector components into one `f32`, as a pair of 16-bit snorms.
///
/// The seams cost two `vec4`s a vertex this way instead of three, which at fifty thousand cells
/// is about 3 MB a frame that does not get uploaded. Sixteen bits is far more direction than a
/// cell outline can show.
#[must_use]
pub fn pack_normal(nx: f32, ny: f32) -> f32 {
    let q = |v: f32| ((v.clamp(-1.0, 1.0) * 32767.0).round() as i32 as u32) & 0xFFFF;
    f32::from_bits(q(nx) | (q(ny) << 16))
}

/// Which vertex layout a frame is being built for.
///
/// A property of the *frame*, like [`crate::slide::Lod`] itself, and for the same reason: below
/// `Lod::Packed` no cell has a seam to draw, so filling seven vectors with the sentinel is work
/// that ends in a shader which — see the note above `DotVertex` in `cell.wgsl` — provably ignores
/// it. This is what says not to bother.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Detail {
    /// Position, corner, colour and shape. What `cellpipe::DotMaterial` draws.
    Plain,
    /// Those, and the twelve seams and the swell. What `cellpipe::CellMaterial` draws.
    #[default]
    Seamed,
}

/// The vertex buffers for one frame's worth of cells.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Buffers {
    /// What [`Buffers::begin`] was last told to fill. Private, because a caller that changed it
    /// mid-frame would produce a mesh with four full attributes and seven half-full ones.
    detail: Detail,
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colours: Vec<[f32; 4]>,
    pub shapes: Vec<[f32; 4]>,
    /// Seam directions 0..3, packed one per component. See [`pack_normal`].
    pub squash_dirs: Vec<[f32; 4]>,
    /// How far along seams 0..3 they sit.
    pub squash_faces: Vec<[f32; 4]>,
    /// Seam directions 4..7.
    pub squash_dirs2: Vec<[f32; 4]>,
    pub squash_dirs3: Vec<[f32; 4]>,
    /// How far along seams 4..7 they sit.
    pub squash_faces2: Vec<[f32; 4]>,
    pub squash_faces3: Vec<[f32; 4]>,
    /// How much this cell was grown to keep its area. See [`Placed::swell`].
    ///
    /// One bare `f32` rather than a fifth component on [`Shape`], which is full. Four bytes a
    /// vertex against the forty-eight another `vec4` would cost, and nothing else needs the room.
    pub swells: Vec<f32>,
    /// How much of each cell is behind a shell, `0..=7/8`. See [`Placed::armour`].
    pub armours: Vec<f32>,
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
    ///
    /// **The clear is load-bearing and not merely tidy.** `cellpipe::upload` hands these vectors
    /// to the mesh and takes the mesh's back rather than copying, so on arrival here they hold
    /// the frame before last — full, and with its allocation, which is the point. Pushing
    /// without this appends to a frame that is already on screen.
    ///
    /// `detail` decides whether the seams get filled at all, and is remembered so that
    /// `cellpipe::upload` knows which of the two meshes these buffers are for.
    pub fn begin(&mut self, expect: usize, detail: Detail) {
        self.clear();
        self.detail = detail;
        self.positions.reserve(expect * 4);
        self.indices.reserve(expect * 6);
    }

    /// Which layout these buffers were last filled for.
    #[must_use]
    pub fn detail(&self) -> Detail {
        self.detail
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
        // Anticlockwise from the top left, matching the corners in `uv` — the corner is what the
        // whole signed-distance field is evaluated against, so it has to be exact.
        const CORNERS: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        for (dx, dy) in CORNERS {
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

        // Below `Lod::Packed` these are twelve sentinels and a one, twelve times a cell, for a
        // shader that provably ignores them. One branch a quad to not write 100 of the 152 bytes
        // a vertex — see `cellpipe::DotMaterial`.
        if self.detail == Detail::Plain {
            return;
        }
        let dirs = [
            pack_normal(p.squash[0].nx, p.squash[0].ny),
            pack_normal(p.squash[1].nx, p.squash[1].ny),
            pack_normal(p.squash[2].nx, p.squash[2].ny),
            pack_normal(p.squash[3].nx, p.squash[3].ny),
        ];
        let faces = [
            p.squash[0].face,
            p.squash[1].face,
            p.squash[2].face,
            p.squash[3].face,
        ];
        let dirs2 = [
            pack_normal(p.squash[4].nx, p.squash[4].ny),
            pack_normal(p.squash[5].nx, p.squash[5].ny),
            pack_normal(p.squash[6].nx, p.squash[6].ny),
            pack_normal(p.squash[7].nx, p.squash[7].ny),
        ];
        let faces2 = [
            p.squash[4].face,
            p.squash[5].face,
            p.squash[6].face,
            p.squash[7].face,
        ];
        let dirs3 = [
            pack_normal(p.squash[8].nx, p.squash[8].ny),
            pack_normal(p.squash[9].nx, p.squash[9].ny),
            pack_normal(p.squash[10].nx, p.squash[10].ny),
            pack_normal(p.squash[11].nx, p.squash[11].ny),
        ];
        let faces3 = [
            p.squash[8].face,
            p.squash[9].face,
            p.squash[10].face,
            p.squash[11].face,
        ];
        for _ in CORNERS {
            self.squash_dirs.push(dirs);
            self.squash_faces.push(faces);
            self.squash_dirs2.push(dirs2);
            self.squash_faces2.push(faces2);
            self.squash_dirs3.push(dirs3);
            self.squash_faces3.push(faces3);
            self.swells.push(p.swell);
            self.armours.push(p.armour);
        }
    }

    /// Every vector, whatever `detail` says. A tier change has to leave nothing of the old one
    /// behind: `Seamed` then `Plain` would otherwise keep the last seamed frame's seams, and
    /// upload them against four attributes of a completely different length.
    fn clear(&mut self) {
        self.positions.clear();
        self.uvs.clear();
        self.colours.clear();
        self.shapes.clear();
        self.squash_dirs.clear();
        self.squash_faces.clear();
        self.squash_dirs2.clear();
        self.squash_faces2.clear();
        self.squash_dirs3.clear();
        self.squash_faces3.clear();
        self.swells.clear();
        self.armours.clear();
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
    /// The sides this blob is flattened along. Default is four seams too far out to bite, so
    /// anything that does not care about squashing — organelles, motes — draws round.
    pub squash: [Squash; SQUASH_PER_CELL],
    /// How much larger than its unsquashed self this blob is drawn, to keep its area.
    ///
    /// `half` already includes it; this says how much of it is there, so the shader can give it
    /// back along the shared walls. See `slide::area_swell` and the taper in `cell.wgsl`. One for
    /// anything that was not swollen, which is everything except a clipped cell.
    pub swell: f32,
    /// How much of this cell is behind a shell, `0..=7/8` — `mm_core::organelle::shell_cover`
    /// as a fraction.
    ///
    /// **A ring and not an arc, and that is a decision about honesty rather than about taste.**
    /// Coverage is a *scalar*: `shell_cover` returns one number and the catalogue is explicit that
    /// the same `control[0]` closes the shell and shades the cell beneath it because "it is one
    /// surface doing one thing". There is no direction anywhere in it. An arc spanning
    /// `cover × 2π` of the perimeter looks more like a mineral test and invents a facing the
    /// simulation does not have; a rim that thickens with cover draws the number that exists.
    ///
    /// Zero for everything that is not a cell — organelles, motes, the bench's calibration quads —
    /// and zero is exactly the identity in `cell.wgsl`, so a slide with no shell on it is drawn
    /// the picture it was drawn before.
    pub armour: f32,
}

/// Build the vertex buffers for a frame.
///
/// `place` decides where each cell goes and returns `None` for one that should not be drawn —
/// off-screen, or dead this frame. Reuses the buffers so a steady population allocates nothing
/// after the first frame.
pub fn build(
    into: &mut Buffers,
    cells: &[CellDot],
    detail: Detail,
    place: impl Fn(&CellDot) -> Option<Placed>,
) {
    into.begin(cells.len(), detail);
    for dot in cells {
        if let Some(p) = place(dot) {
            into.push(p);
        }
    }
}

/// How much of its quad the signed-distance field fills, at rest.
///
/// The quad has to hold the outline *and* the fade around it, and the outline is not a circle:
/// three harmonics of wobble can push it out by a fifth. At the 0.82 the baked atlas used, a
/// big-wobble cell reached 0.98 of the quad and its fade ran off the corners — which drew a
/// square halo round the cell and, at whole-slide zoom, made the whole population look square.
///
/// `cell.wgsl` hard-codes the same number. If one moves, the other must.
pub const FIELD_FILL: f32 = 0.65;

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
                age: 1_000,
                integrity: 1.0,
                organelles: Vec::new(),
                limbs: Vec::new(),
                squash: Vec::new(),
                area_swell: 1.0,
                armour: 0.0,
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
            squash: Default::default(),
            swell: dot.area_swell,
            armour: 0.0,
        })
    }

    #[test]
    fn every_cell_becomes_one_quad() {
        let mut buf = Buffers::default();
        build(&mut buf, &dots(5), Detail::Seamed, placed);
        assert_eq!(buf.cells(), 5);
        assert_eq!(buf.positions.len(), 20);
        assert_eq!(buf.uvs.len(), 20);
        assert_eq!(buf.colours.len(), 20);
        assert_eq!(buf.shapes.len(), 20);
        assert_eq!(buf.indices.len(), 30);
    }

    #[test]
    fn armour_reaches_every_vertex_of_the_cell_it_belongs_to() {
        // A per-cell scalar has to be on all four corners or the shader interpolates it, and an
        // interpolated shell is a rim that fades across the cell — which reads as a cell half
        // armoured, a state that does not exist.
        let mut buf = Buffers::default();
        build(&mut buf, &dots(3), Detail::Seamed, |dot| {
            Some(Placed {
                armour: 0.5,
                ..placed(dot).unwrap()
            })
        });
        assert_eq!(buf.armours.len(), buf.positions.len());
        assert!(buf.armours.iter().all(|a| *a == 0.5));
        // And it clears with everything else, or a frame with no shell on it wears the last
        // frame's.
        build(&mut buf, &dots(2), Detail::Seamed, placed);
        assert_eq!(buf.armours.len(), 8);
        assert!(buf.armours.iter().all(|a| *a == 0.0));
    }

    #[test]
    fn every_attribute_is_the_same_length() {
        // A mesh whose attributes disagree is a validation error at draw time, several layers
        // from whichever `push` was forgotten.
        let mut buf = Buffers::default();
        build(&mut buf, &dots(9), Detail::Seamed, placed);
        let n = buf.positions.len();
        assert_eq!(buf.uvs.len(), n);
        assert_eq!(buf.colours.len(), n);
        assert_eq!(buf.shapes.len(), n);
    }

    #[test]
    fn no_index_points_past_the_end() {
        // The other way to get a validation error, and the one that reads as a driver crash.
        let mut buf = Buffers::default();
        build(&mut buf, &dots(7), Detail::Seamed, placed);
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
        build(&mut buf, &dots(1), Detail::Seamed, placed);
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
        build(&mut buf, &one, Detail::Seamed, placed);
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
        build(&mut buf, &dots(4), Detail::Seamed, |dot| {
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
        build(&mut buf, &dots(3), Detail::Seamed, |dot| {
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
        build(&mut buf, &dots(6), Detail::Seamed, placed);
        build(&mut buf, &dots(2), Detail::Seamed, placed);
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
