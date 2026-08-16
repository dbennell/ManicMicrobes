//! Every limb on the slide, as one mesh.
//!
//! A limb is a thing a cell has grown that reaches **outside its own membrane** — a spike, a
//! cilium, a flagellum, a holdfast, an exoenzyme's cloud. Six of the catalogue's twenty drawable
//! organelles do, and every one of them was drawn as a coloured dot on the ring inside the cell,
//! so nothing a cell built ever changed its silhouette. See `docs/MORPHOLOGY.md`.
//!
//! # Why this is not the cell mesh
//!
//! Unioning a limb into the body's own signed-distance field would get it the membrane ring, the
//! shading and the haze for free, and it fails three ways:
//!
//! 1. **The quad has no room.** `cellmesh::FIELD_FILL` is 0.65 and the remaining 0.35 is spoken
//!    for — the wobble reaches a fifth and the antialiasing fade needs the rest. A flagellum is
//!    one to three radii long.
//! 2. **Every cell would pay.** The quad is per cell, and on the mixed benchmark slide 3.8% of
//!    cells carry a cilium. Sizing every quad for a flagellum is fill rate spent on nothing.
//! 3. **The seams would cut them off.** The body field is intersected with twelve half-planes; a
//!    flagellum crossing a shared wall would be sliced at the wall, which is right for a body and
//!    wrong for a limb.
//!
//! Widening `CellMaterial` instead is worse for a different reason. It changes the body's vertex
//! layout, so every probe that photographs a cell — `shader_probe`, `nine_cells`,
//! `overlap_detector`, `swell_probe`, `packing_probe` — is re-baselined at once, and a regression
//! in the picture stops being attributable. That is the failure `docs/OVERLAPS.md` is about, and
//! the whole value of those probes is that they are *unchanged*.
//!
//! So: a third mesh, a third material, its own shader, drawn under the cells. The body path is not
//! opened at all, and any change in how a cell looks is a bug in here.
//!
//! # Limbs may be drawn over other cells
//!
//! Deliberately, and it is the one place on the slide where that is allowed. A spike wounds the
//! cell it is touching, so a spike drawn over its victim is the honest picture — and the seam work
//! `OVERLAPS.md` records was about *bodies*, which tile and must never be drawn twice. A limb is
//! not a body: it has no seam, takes part in no packing solve, and `tests/overlap_detector.rs`
//! must not be pointed at this mesh.
//!
//! Nothing here knows Bevy exists. [`build`] is arithmetic over a [`crate::slide::Frame`] and is
//! tested without a graphics stack, as `cellmesh.rs` is.

use crate::slide::LimbDot;

/// Which signed-distance field the fragment shader evaluates. Must match `limb.wgsl`.
///
/// A number rather than a branch on the organelle type, because the shader has no catalogue and
/// several types could reasonably come to share a form.
pub mod form {
    /// A tuft of hairs, beating. Many small ones, which is what a cilium organelle is.
    pub const CILIUM: f32 = 1.0;
    /// One whip with a travelling wave, longer than the body it drives.
    pub const FLAGELLUM: f32 = 2.0;
    /// A barb. Length is how far out it is, thickness is what the cell built.
    pub const SPIKE: f32 = 3.0;
    /// A stalk with rootlets: taut when it is gripping, limp when it has let go.
    pub const HOLDFAST: f32 = 4.0;
    /// A cloud around the body rather than a limb off it. Hollow — see [`LimbDot::inner`].
    ///
    /// [`LimbDot::inner`]: crate::slide::LimbDot::inner
    pub const HALO: f32 = 5.0;
    /// A hard junction: a rivet across the wall two cells share, or a strut across the water
    /// between them when they have been pulled apart.
    pub const BAND: f32 = 6.0;
    /// A soft junction: a pore, faint, because one is a body and the other is a conversation.
    pub const CHANNEL: f32 = 7.0;
}

/// Which form an organelle is drawn as, or `None` for the fifteen types that grow nothing outside
/// the membrane.
///
/// The single answer to "does this organelle have an outside", so that `slide.rs` deciding to
/// build a [`LimbDot`] and this deciding to draw one cannot disagree.
#[must_use]
pub fn form_of(kind: mm_core::OrganelleType) -> Option<f32> {
    use mm_core::OrganelleType as T;
    Some(match kind {
        T::Cilium => form::CILIUM,
        T::Flagellum => form::FLAGELLUM,
        T::Spike => form::SPIKE,
        T::Holdfast => form::HOLDFAST,
        T::Exoenzyme => form::HALO,
        _ => return None,
    })
}

/// The vertex buffers for one frame's worth of limbs.
///
/// Five attributes against the cell mesh's eleven. A limb has no seams to carry: it is not cut by
/// its neighbours and takes no part in the packing solve.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Buffers {
    pub positions: Vec<[f32; 3]>,
    /// The quad corner, `-1..1`, already in the limb's own frame: `+x` outward from the root,
    /// `+y` across. The CPU emits *rotated* corners, so the shader needs no direction and no
    /// trigonometry, and a long thin flagellum gets a long thin quad rather than a square one
    /// sized for the worst rotation.
    pub uvs: Vec<[f32; 2]>,
    /// The owning cell's colour, hazed and vignetted by the caller — so a limb fades with the
    /// cell it grew from rather than floating in front of a defocused one.
    pub colours: Vec<[f32; 4]>,
    /// `x` form, `y` extent, `z` phase, `w` aspect. See [`Placed`].
    pub limb_a: Vec<[f32; 4]>,
    /// `x` count, `y` inner, `z` taper, `w` seed.
    pub limb_b: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl Buffers {
    #[must_use]
    pub fn limbs(&self) -> usize {
        self.positions.len() / 4
    }

    /// Start a frame: empty, and reserve for what is expected.
    ///
    /// The clear is load-bearing for the reason `cellmesh::Buffers::begin`'s is: `limbpipe::upload`
    /// swaps these vectors with the mesh's rather than copying, so on arrival they hold the frame
    /// before last — full, and with its allocation, which is the point.
    pub fn begin(&mut self, expect: usize) {
        self.positions.clear();
        self.uvs.clear();
        self.colours.clear();
        self.limb_a.clear();
        self.limb_b.clear();
        self.indices.clear();
        self.positions.reserve(expect * 4);
        self.indices.reserve(expect * 6);
    }

    /// Add one limb's quad, rotated into place.
    pub fn push(&mut self, p: Placed) {
        if p.half_len <= 0.0 || p.half_wid <= 0.0 {
            return;
        }
        let base = self.positions.len() as u32;
        // The across-axis, from the along-axis. Left-handed on purpose is not a thing that
        // matters here — the forms are symmetric across `y` or use `seed` to break it.
        let (vx, vy) = (-p.uy, p.ux);
        const CORNERS: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        for (a, b) in CORNERS {
            let along = a * p.half_len;
            let across = b * p.half_wid;
            self.positions.push([
                p.cx + p.ux * along + vx * across,
                p.cy + p.uy * along + vy * across,
                0.0,
            ]);
            self.uvs.push([a, b]);
            self.colours.push(p.rgba);
            self.limb_a.push([p.form, p.extent, p.phase, p.aspect()]);
            self.limb_b.push([p.count, p.inner, p.taper, p.seed]);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Where one limb is drawn and what it is, worked out by the caller.
///
/// Screen space, like `cellmesh::Placed`: the camera, the optics and the selection are three
/// things this module has no business knowing about.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placed {
    /// The quad's centre — **not** the limb's root. The root is at the quad's `-x` edge, so the
    /// part of a limb that is inside its own body is the near end of the rectangle and the body,
    /// drawn over this mesh, covers it. Without that there is a hairline of background between a
    /// limb and the membrane it grows from.
    pub cx: f32,
    pub cy: f32,
    /// Unit vector along the limb, root to tip.
    pub ux: f32,
    pub uy: f32,
    /// Half the quad's extent along `u`, in pixels.
    pub half_len: f32,
    /// Half the quad's extent across it.
    pub half_wid: f32,
    pub rgba: [f32; 4],
    pub form: f32,
    /// How hard the limb is working, `-1..=1`. Signed where the control it came from is signed.
    pub extent: f32,
    pub phase: f32,
    pub count: f32,
    pub inner: f32,
    pub taper: f32,
    pub seed: f32,
}

impl Placed {
    /// The quad's shape, so the shader can work in an isotropic frame.
    ///
    /// `uv` is `-1..1` however long and thin the quad is, so a circle in `uv` is an ellipse on
    /// screen. The shader multiplies `uv.x` by this and everything downstream is in half-widths.
    ///
    /// Floored above zero rather than merely non-negative: an aspect of zero collapses the
    /// along-axis and every field in `limb.wgsl` divides by it somewhere.
    #[must_use]
    pub fn aspect(&self) -> f32 {
        (self.half_len / self.half_wid.max(1e-4)).max(1e-3)
    }
}

/// How far under the cells the limb mesh sits.
///
/// Below the cell mesh at 1.0, so a limb's root disappears under the body it grows from and there
/// is no join to draw.
pub const LIMB_Z: f32 = 0.9;

/// And the layer over them, for the things that are *at* a boundary rather than off a body.
///
/// The junctions, and only the junctions. A junction drawn under the cells is invisible in the one
/// case that matters — a packed pair, which is every pair a hard junction holds — because the
/// whole of it is inside the two bodies. It has to be over them or it is not a picture of
/// anything. See `slide::JunctionLine`.
pub const OVER_Z: f32 = 1.05;

/// Which of the two layers a form belongs in.
///
/// One answer, here, rather than a condition at the push site: a form in the wrong layer is
/// invisible or is drawn over a body it should be behind, and neither says which line was wrong.
#[must_use]
pub fn over_cells(form: f32) -> bool {
    form == form::BAND || form == form::CHANNEL
}

/// Build the vertex buffers for a frame.
///
/// `place` decides where each limb goes and returns `None` for one that should not be drawn — off
/// screen, or a form this build does not carry yet. Reuses the buffers, so a steady population
/// allocates nothing after the first frame.
pub fn build<'a>(
    into: &mut Buffers,
    limbs: impl Iterator<Item = &'a LimbDot> + Clone,
    place: impl Fn(&LimbDot) -> Option<Placed>,
) {
    into.begin(limbs.clone().count());
    for limb in limbs {
        if let Some(p) = place(limb) {
            into.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_core::OrganelleType as T;

    fn placed() -> Placed {
        Placed {
            cx: 0.0,
            cy: 0.0,
            ux: 1.0,
            uy: 0.0,
            half_len: 4.0,
            half_wid: 1.0,
            rgba: [1.0; 4],
            form: form::SPIKE,
            extent: 1.0,
            phase: 0.0,
            count: 1.0,
            inner: 0.0,
            taper: 0.0,
            seed: 0.0,
        }
    }

    #[test]
    fn every_limb_becomes_one_quad_and_every_attribute_agrees() {
        // A mesh whose attributes disagree in length is a validation error at draw time, several
        // layers from whichever push was forgotten.
        let mut buf = Buffers::default();
        buf.begin(3);
        for _ in 0..3 {
            buf.push(placed());
        }
        assert_eq!(buf.limbs(), 3);
        let n = 12;
        assert_eq!(buf.positions.len(), n);
        assert_eq!(buf.uvs.len(), n);
        assert_eq!(buf.colours.len(), n);
        assert_eq!(buf.limb_a.len(), n);
        assert_eq!(buf.limb_b.len(), n);
        assert_eq!(buf.indices.len(), 18);
        assert!(buf.indices.iter().all(|i| (*i as usize) < n));
    }

    #[test]
    fn the_quad_is_rotated_and_the_corners_stay_the_limbs_own_frame() {
        // The whole reason the CPU rotates: `uv` is the limb's frame whatever the mount angle, so
        // the shader needs no direction and a long thin limb gets a long thin quad.
        let mut buf = Buffers::default();
        buf.begin(1);
        // Straight up, so along-axis is +y and the across-axis is -x.
        buf.push(Placed {
            ux: 0.0,
            uy: 1.0,
            ..placed()
        });
        let mut corners = buf.uvs.clone();
        corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(
            corners,
            vec![[-1.0, -1.0], [-1.0, 1.0], [1.0, -1.0], [1.0, 1.0]],
            "the corners are not the field's coordinates"
        );
        let ys: Vec<f32> = buf.positions.iter().map(|p| p[1]).collect();
        let xs: Vec<f32> = buf.positions.iter().map(|p| p[0]).collect();
        assert_eq!(ys.iter().cloned().fold(f32::MIN, f32::max), 4.0);
        assert_eq!(ys.iter().cloned().fold(f32::MAX, f32::min), -4.0);
        assert_eq!(xs.iter().cloned().fold(f32::MIN, f32::max), 1.0);
        assert_eq!(xs.iter().cloned().fold(f32::MAX, f32::min), -1.0);
    }

    #[test]
    fn a_limb_with_no_size_is_dropped_rather_than_drawn_as_nothing() {
        let mut buf = Buffers::default();
        buf.begin(2);
        buf.push(Placed {
            half_len: 0.0,
            ..placed()
        });
        buf.push(Placed {
            half_wid: 0.0,
            ..placed()
        });
        assert_eq!(buf.limbs(), 0);
        assert!(buf.indices.is_empty());
    }

    #[test]
    fn building_again_replaces_rather_than_appends() {
        // The buffers are reused every frame and `upload` hands back the frame before last, full.
        // One missed clear and the mesh grows without bound until the machine stops.
        let mut buf = Buffers::default();
        buf.begin(6);
        for _ in 0..6 {
            buf.push(placed());
        }
        buf.begin(2);
        for _ in 0..2 {
            buf.push(placed());
        }
        assert_eq!(buf.limbs(), 2);
        assert_eq!(buf.indices.len(), 12);
    }

    #[test]
    fn an_aspect_is_never_zero_however_flat_the_quad() {
        // Every field in `limb.wgsl` divides by it somewhere, and a zero produces a quad of NaN
        // that renders as whatever the driver feels like.
        let flat = Placed {
            half_wid: 0.0,
            ..placed()
        };
        assert!(flat.aspect().is_finite() && flat.aspect() > 0.0);
        let stubby = Placed {
            half_len: 0.0,
            ..placed()
        };
        assert!(stubby.aspect().is_finite() && stubby.aspect() > 0.0);
        assert_eq!(placed().aspect(), 4.0);
    }

    #[test]
    fn exactly_the_organelles_that_reach_outside_have_a_form() {
        // The catalogue is append-only and the reservations are being filled one milestone at a
        // time. This is the list, written down once, so that `slide.rs` deciding to build a limb
        // and this deciding to draw one cannot come apart.
        let outside: Vec<&str> = mm_core::OrganelleType::all()
            .iter()
            .filter(|k| form_of(**k).is_some())
            .map(|k| k.name())
            .collect();
        assert_eq!(
            outside,
            vec!["cilium", "spike", "holdfast", "flagellum", "exoenzyme vesicle"]
        );
        // Nothing that lives inside the membrane grows one.
        for kind in [T::Nucleus, T::Chloroplast, T::Vacuole, T::Lysosome, T::Shell] {
            assert!(form_of(kind).is_none(), "{} grew a limb", kind.name());
        }
        // And no two forms share a code, or one would be drawn as the other.
        let mut codes: Vec<i32> = mm_core::OrganelleType::all()
            .iter()
            .filter_map(|k| form_of(*k))
            .map(|f| f as i32)
            .collect();
        codes.sort_unstable();
        let len = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), len, "two organelles share a form code");
    }
}
