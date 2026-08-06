//! What the cell shader is drawn through: the vertex attributes, the material and the mesh.
//!
//! This was in `main.rs`, and it moved because there are now two things that draw cells — the
//! microscope and the shader bench (`src/bin/shaderbench.rs`). Two copies of a vertex layout is a
//! thing that goes wrong quietly: a shader location that disagrees is a validation failure at
//! draw time with nothing to say which end is wrong, and a bench that has drifted from the app is
//! worse than no bench, because it exonerates code the app is still running.
//!
//! So: one definition of the layout, one `Material2d`, one shader. See `cellmesh.rs` for the
//! arithmetic that fills the buffers and `cell.wgsl` for the field itself. This is the only part
//! of the crate's library that knows Bevy exists, and it is behind the `render` feature for the
//! reason `lib.rs` gives.

use bevy::asset::{load_internal_asset, uuid_handle, RenderAssetUsages};
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, PrimitiveTopology, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    VertexFormat,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};

use crate::cellmesh;

/// The per-cell data the shader reads, beyond position, corner and colour.
///
/// The id is arbitrary but must be stable and must not collide with Bevy's own attributes,
/// which is what the large number is for.
pub const ATTRIBUTE_SHAPE: MeshVertexAttribute =
    MeshVertexAttribute::new("CellShape", 0x6D_6D_5F_63_65_6C_6C, VertexFormat::Float32x4);

/// Which way each of a cell's four seams faces, packed two 16-bit snorms per component.
pub const ATTRIBUTE_SQUASH_DIR: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashDir",
    0x6D_6D_5F_63_65_6C_6D,
    VertexFormat::Float32x4,
);

/// How far along each of those the seam sits.
pub const ATTRIBUTE_SQUASH_FACE: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashFace",
    0x6D_6D_5F_63_65_6C_6E,
    VertexFormat::Float32x4,
);

/// Seam directions 4..7.
pub const ATTRIBUTE_SQUASH_DIR2: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashDir2",
    0x6D_6D_5F_63_65_6C_6F,
    VertexFormat::Float32x4,
);

/// How far along seams 4..7 they sit.
pub const ATTRIBUTE_SQUASH_FACE2: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashFace2",
    0x6D_6D_5F_63_65_6C_70,
    VertexFormat::Float32x4,
);

/// Seams 8..11, for the cells a packed sheet presses on from every side.
///
/// Eight was called headroom over the six a monolayer settles on, and it was not: once the
/// neighbour search covered a cell's real neighbourhood, cells routinely found nine or ten, and
/// a cell that runs out of slots stops cutting for a neighbour that is still cutting for it —
/// which draws as five clean shared walls and one side simply overlapping.
pub const ATTRIBUTE_SQUASH_DIR3: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashDir3",
    0x6D_6D_5F_63_65_6C_71,
    VertexFormat::Float32x4,
);

/// How far along seams 8..11 they sit.
pub const ATTRIBUTE_SQUASH_FACE3: MeshVertexAttribute = MeshVertexAttribute::new(
    "CellSquashFace3",
    0x6D_6D_5F_63_65_6C_72,
    VertexFormat::Float32x4,
);

/// How much the cell was grown to keep its area, so the shader can hand it back at the seams.
///
/// A bare `Float32`: four bytes a vertex rather than the forty-eight a fifth `vec4` would cost,
/// and `CellShape` has no spare component.
pub const ATTRIBUTE_SWELL: MeshVertexAttribute =
    MeshVertexAttribute::new("CellSwell", 0x6D_6D_5F_63_65_6C_73, VertexFormat::Float32);

/// Embedded at compile time rather than loaded from an `assets/` directory, so the binary runs
/// from anywhere. The same thing `bevy_sprite` does for its own shaders.
///
/// [`reload`] can replace what is behind it at runtime, which is how the bench edits the shader
/// without a rebuild.
pub const CELL_SHADER: Handle<Shader> = uuid_handle!("6d6d5f63-656c-6c5f-7368-616465720001");

/// Where the shader is in the source tree, for [`reload`].
pub const CELL_SHADER_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/cell.wgsl");

/// The whole population, drawn by one material with one shader (M10.5).
///
/// No bindings: everything that varies rides in the vertex attributes, because it varies *per
/// cell* and a material's uniforms are per draw call — and the entire point is that there is one
/// draw call. See `cellmesh.rs` for why a mesh rather than instancing, and `cell.wgsl` for the
/// field itself.
#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub struct CellMaterial {}

impl Material2d for CellMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(CELL_SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(CELL_SHADER)
    }

    /// **Blend, and this is the whole reason the field is evaluated per pixel.**
    ///
    /// `Material2d::alpha_mode` defaults to `Opaque`, which is a blend state of *replace*: the
    /// fragment's alpha is written to the target and never composited. So `cell.wgsl` computed an
    /// antialiased edge one pixel wide at any magnification, and it was thrown away. What decided
    /// the drawn shape instead was the `discard` at the bottom of the fade — `alpha <= 0.001` —
    /// which is a hard, aliased edge at the *outer* end of the ramp rather than its middle.
    ///
    /// Measured on the shader bench, a plain circle with the wobble switched off, against the
    /// radius the data asked for:
    ///
    /// | | median | p10 | p90 | the edge |
    /// |---|---|---|---|---|
    /// | `Opaque` | **+1.85 px** | +1.39 | +2.19 | 1.000 → 0.144 → 0.000, one pixel, hard |
    /// | `Blend`  | **−0.00 px** | −0.01 | +0.01 | 0.870, 0.739, 0.500, 0.271, 0.128 |
    ///
    /// The same +1.8 px at 40 px per square and at 110, because it is not a radius error: it is
    /// the width of the fade, added to every cell whatever size it is drawn.
    ///
    /// Two consequences, and they are the report:
    ///
    /// * **Every cell was drawn about two pixels bigger than its own outline.** A shared wall is
    ///   computed by two cells agreeing on one plane, and then both of them overran it by two
    ///   pixels — so the wall was a four-pixel band that both claimed and neither owned, resolved
    ///   by whichever happened to be drawn second. That is a cell drawn over its neighbour with no
    ///   boundary between them, on every contact on the slide, from a cause that has nothing to do
    ///   with seams.
    /// * **The edge could not be antialiased, so it crawled.** A hard edge on a body moving by a
    ///   fraction of a pixel flips whole pixels between one cell and the next; an antialiased one
    ///   slides smoothly. Which is exactly "it does not show up if nothing is moving, and any
    ///   movement at all brings it out".
    ///
    /// `tests/shader_probe.rs` measures the geometry and `tools/check_outline.py` measures the
    /// pixels; between them the claim above is checkable rather than remembered.
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // **Named, and this is not tidiness.** `entry_point: None` means "the only entry point
        // for this stage", and it was correct for exactly as long as `cell.wgsl` had one
        // `@vertex` and one `@fragment`. The moment [`DotMaterial`] added a second of each it
        // became ambiguous — and what that produced was not a validation error naming the
        // shader. It was a heap abort inside the driver before a single line of log, with the
        // dot pipeline compiling perfectly because it had named its own. An hour, bisected
        // against the shader file, to find a default that had quietly stopped having one answer.
        descriptor.vertex.entry_point = Some("vertex".into());
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.entry_point = Some("fragment".into());
        }
        // The locations here are the locations in `cell.wgsl`, and the two have to agree
        // exactly — a mismatch is a validation failure at draw time with nothing to say which
        // end is wrong.
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
            ATTRIBUTE_SHAPE.at_shader_location(3),
            ATTRIBUTE_SQUASH_DIR.at_shader_location(4),
            ATTRIBUTE_SQUASH_FACE.at_shader_location(5),
            ATTRIBUTE_SQUASH_DIR2.at_shader_location(6),
            ATTRIBUTE_SQUASH_FACE2.at_shader_location(7),
            ATTRIBUTE_SQUASH_DIR3.at_shader_location(8),
            ATTRIBUTE_SQUASH_FACE3.at_shader_location(9),
            ATTRIBUTE_SWELL.at_shader_location(10),
        ])?];
        Ok(())
    }
}

/// The same population below [`crate::slide::Lod::Packed`], over a quarter of the vertex data.
///
/// Four attributes rather than eleven: position, corner, colour and shape. The twelve seam
/// directions, the twelve seam distances and the swell are **100 of the 152 bytes a vertex** and
/// below that tier every one of them is the same constant — `slide.rs` does not solve for seams
/// at all there, so each cell ships twelve `NO_SQUASH` sentinels and a swell of one, sixty
/// thousand times over. At sixteen thousand cells that is 10 MB a frame of the number 8.
///
/// | | bytes a vertex | bytes a cell, with indices |
/// |---|---|---|
/// | [`CellMaterial`] | 152 | 632 |
/// | `DotMaterial` | 52 | 232 |
///
/// The fragment shader sheds the same work: no `unpack2x16snorm`, no `seam_room`, no `smax`,
/// twelve of each. The note above `DotVertex` in `cell.wgsl` is the proof that all of it is dead
/// arithmetic at this tier rather than detail being given up, and the two stages share every line
/// that is *not* dead — one file, one wobble, one membrane.
///
/// `shape` stays a whole `vec4` though `integrity` is always one here and `rounded` is the same
/// for every cell in a frame. Both could go, for eight more bytes a vertex; keeping one [`Shape`]
/// across both materials is worth more than that.
///
/// [`Shape`]: crate::cellmesh::Shape
#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub struct DotMaterial {}

impl Material2d for DotMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(CELL_SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(CELL_SHADER)
    }

    /// Blend, for every reason [`CellMaterial::alpha_mode`] gives. An opaque cell here would be
    /// drawn two pixels bigger than its own outline, exactly as it was there.
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The same four locations `cell.wgsl` gives `DotVertex`, and the same first four the
        // full layout uses — so a cell means the same thing to both pipelines.
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
            ATTRIBUTE_SHAPE.at_shader_location(3),
        ])?];
        // Two entry points in one shader rather than two shader files. `Material2d::specialize`
        // runs last of everything that touches the descriptor, so this is the place it can be
        // done at all — and it is what keeps the wobble, the shading and the membrane from
        // existing twice and drifting apart at the tier boundary.
        descriptor.vertex.entry_point = Some("dot_vertex".into());
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.entry_point = Some("dot_fragment".into());
        }
        Ok(())
    }
}

/// Register the shader and the material.
///
/// Must be added *after* `DefaultPlugins`, not before: the macro writes straight into
/// `Assets<Shader>`, and `AssetPlugin` is what puts that resource in the world. Before it, this is
/// a panic on the first line of `main` with a message about a missing resource rather than about
/// a shader.
pub fn plugin(app: &mut App) {
    load_internal_asset!(app, CELL_SHADER, "cell.wgsl", Shader::from_wgsl);
    app.add_plugins(Material2dPlugin::<CellMaterial>::default());
    app.add_plugins(Material2dPlugin::<DotMaterial>::default());
}

/// Replace the compiled-in shader with whatever is on disk now.
///
/// The bench calls this when `cell.wgsl` changes so that a change to the field can be looked at
/// without a rebuild — which, for a fault that only shows up in motion, is the difference between
/// an experiment a minute and an experiment every three.
///
/// Nothing validates here: a shader is parsed when a pipeline is compiled, so a WGSL error is a
/// logged pipeline failure and the last good pipeline goes on drawing. Returns what it read, or
/// the error, for the bench to put on screen.
///
/// # Errors
///
/// If the file cannot be read.
pub fn reload(shaders: &mut Assets<Shader>, path: &str) -> std::io::Result<()> {
    let source = std::fs::read_to_string(path)?;
    // The handle is a fixed uuid and never a generational one, so the insert cannot be stale.
    let _ = shaders.insert(&CELL_SHADER, Shader::from_wgsl(source, path.to_string()));
    Ok(())
}

/// An empty mesh carrying every attribute the material's layout asks for.
///
/// Every one of them, even before there is a cell to put in it: a mesh missing an attribute the
/// pipeline was specialised for does not draw, and says so several layers away from here.
#[must_use]
pub fn empty_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
    mesh.insert_attribute(ATTRIBUTE_SHAPE, Vec::<[f32; 4]>::new());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_DIR, Vec::<[f32; 4]>::new());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_FACE, Vec::<[f32; 4]>::new());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_DIR2, Vec::<[f32; 4]>::new());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_FACE2, Vec::<[f32; 4]>::new());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_DIR3, Vec::<[f32; 4]>::new());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_FACE3, Vec::<[f32; 4]>::new());
    mesh.insert_attribute(ATTRIBUTE_SWELL, Vec::<f32>::new());
    mesh.insert_indices(Indices::U32(Vec::new()));
    mesh
}

/// An empty mesh carrying the four attributes [`DotMaterial`]'s layout asks for, and no more.
///
/// Deliberately *not* [`empty_mesh`] with seven attributes left empty. An attribute the pipeline
/// was not specialised for is still packed and still uploaded — the whole saving is in the seven
/// that are not here at all.
#[must_use]
pub fn dot_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
    mesh.insert_attribute(ATTRIBUTE_SHAPE, Vec::<[f32; 4]>::new());
    mesh.insert_indices(Indices::U32(Vec::new()));
    mesh
}

/// Hand one attribute's vector to the mesh and take the mesh's back, or insert it if the mesh
/// has not got that attribute at all.
///
/// The insert cannot be reached through [`empty_mesh`], which puts every attribute in at the
/// format the layout asks for. It is here because the alternative to guessing wrong is an
/// attribute left silently empty, and a mesh whose attributes disagree in length does not draw —
/// several layers away from whichever line was wrong. One copy is a better failure than that.
fn swap_attribute<T>(
    mesh: &mut Mesh,
    attribute: MeshVertexAttribute,
    mine: &mut Vec<T>,
    theirs: fn(&mut VertexAttributeValues) -> Option<&mut Vec<T>>,
) where
    Vec<T>: Into<VertexAttributeValues>,
{
    // The borrow of `mesh` has to end before the fallback can take it again, so the match
    // yields a `bool` rather than wrapping the insert in its own arm.
    let swapped = match mesh.attribute_mut(attribute).and_then(theirs) {
        Some(slot) => {
            std::mem::swap(slot, mine);
            true
        }
        None => false,
    };
    if !swapped {
        mesh.insert_attribute(attribute, std::mem::take(mine));
    }
}

/// [`swap_attribute`] with the variant to unwrap, since every one of them is the same four lines.
macro_rules! swap_attribute {
    ($mesh:expr, $attribute:expr, $variant:ident, $mine:expr) => {
        swap_attribute($mesh, $attribute, $mine, |values| match values {
            VertexAttributeValues::$variant(vec) => Some(vec),
            _ => None,
        })
    };
}

/// Give the mesh one frame's worth of vertices, and take the last frame's back.
///
/// **A swap and not a copy, and that is the whole point of it.** `insert_attribute` takes its
/// values by value, so this was eleven `clone()`s — which allocated and copied the entire
/// population every frame and then dropped the lot. At 152 bytes a vertex over four vertices a
/// cell that is 10 MB a frame at sixteen thousand cells and 32 MB at fifty thousand, none of
/// which the renderer ever needed a second copy of.
///
/// Bevy then re-packs those attributes into one interleaved buffer and uploads them, and that
/// cost is real and is not avoidable from here. The clone was the half that bought nothing.
///
/// What comes back in `buffers` is the *previous* frame's vertices, still allocated. That is not
/// a leftover to be tidied away: it is the allocation the next frame gets built in, so a steady
/// population allocates nothing at all after the first couple of frames — the two sets of
/// vectors ping-pong between the mesh and the caller, each holding the high-water mark.
///
/// The contract that makes it safe is [`cellmesh::Buffers::begin`], which clears before it
/// fills. Every caller goes through it, directly or through [`cellmesh::build`]. A caller that
/// pushed without it would be appending to the frame before last.
/// Which of the two it is for is decided by [`cellmesh::Buffers::detail`], because the buffers
/// know what was filled into them and the mesh does not.
pub fn upload(mesh: &mut Mesh, buffers: &mut cellmesh::Buffers) {
    // The four both layouts share.
    swap_attribute!(
        mesh,
        Mesh::ATTRIBUTE_POSITION,
        Float32x3,
        &mut buffers.positions
    );
    swap_attribute!(mesh, Mesh::ATTRIBUTE_UV_0, Float32x2, &mut buffers.uvs);
    swap_attribute!(mesh, Mesh::ATTRIBUTE_COLOR, Float32x4, &mut buffers.colours);
    swap_attribute!(mesh, ATTRIBUTE_SHAPE, Float32x4, &mut buffers.shapes);

    // Indices are not an attribute and have their own accessor, but the same trade.
    let swapped = match mesh.indices_mut() {
        Some(Indices::U32(theirs)) => {
            std::mem::swap(theirs, &mut buffers.indices);
            true
        }
        _ => false,
    };
    if !swapped {
        mesh.insert_indices(Indices::U32(std::mem::take(&mut buffers.indices)));
    }

    // And the seven that only the seamed layout has. Skipped rather than swapped-as-empty: a
    // `Plain` frame never filled them, and handing seven empty vectors to a mesh whose other
    // four have sixty thousand entries is a mesh that does not draw.
    if buffers.detail() == cellmesh::Detail::Plain {
        return;
    }
    swap_attribute!(
        mesh,
        ATTRIBUTE_SQUASH_DIR,
        Float32x4,
        &mut buffers.squash_dirs
    );
    swap_attribute!(
        mesh,
        ATTRIBUTE_SQUASH_FACE,
        Float32x4,
        &mut buffers.squash_faces
    );
    swap_attribute!(
        mesh,
        ATTRIBUTE_SQUASH_DIR2,
        Float32x4,
        &mut buffers.squash_dirs2
    );
    swap_attribute!(
        mesh,
        ATTRIBUTE_SQUASH_FACE2,
        Float32x4,
        &mut buffers.squash_faces2
    );
    swap_attribute!(
        mesh,
        ATTRIBUTE_SQUASH_DIR3,
        Float32x4,
        &mut buffers.squash_dirs3
    );
    swap_attribute!(
        mesh,
        ATTRIBUTE_SQUASH_FACE3,
        Float32x4,
        &mut buffers.squash_faces3
    );
    swap_attribute!(mesh, ATTRIBUTE_SWELL, Float32, &mut buffers.swells);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cellmesh::{Buffers, Detail, Placed, Shape};

    /// `n` quads, each carrying `mark` everywhere a number will fit, so that a frame can be told
    /// apart from the one before it by looking at any attribute.
    fn frame(buffers: &mut Buffers, n: usize, mark: f32) {
        frame_at(buffers, n, mark, Detail::Seamed);
    }

    fn frame_at(buffers: &mut Buffers, n: usize, mark: f32, detail: Detail) {
        buffers.begin(n, detail);
        for i in 0..n {
            buffers.push(Placed {
                x: mark + i as f32,
                y: mark,
                half: 1.0,
                rgba: [mark; 4],
                shape: Shape {
                    seed: mark,
                    softness: 0.0,
                    integrity: 1.0,
                    rounded: 1.0,
                },
                squash: Default::default(),
                swell: mark,
            });
        }
    }

    fn positions(mesh: &Mesh) -> Vec<[f32; 3]> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
            other => panic!("positions are not Float32x3: {other:?}"),
        }
    }

    #[test]
    fn the_mesh_gets_the_frame_it_was_given() {
        let mut mesh = empty_mesh();
        let mut buffers = Buffers::default();
        frame(&mut buffers, 3, 1.0);
        let want = buffers.positions.clone();
        upload(&mut mesh, &mut buffers);
        assert_eq!(positions(&mesh), want);
    }

    #[test]
    fn the_mesh_gets_this_frame_and_never_the_last_one() {
        // The failure the swap makes possible, and the reason it is worth a test: the vectors
        // that arrive back from the mesh are full, so a frame built without clearing them —
        // or handed to a second mesh — is last frame's cells, on screen, one behind.
        let mut mesh = empty_mesh();
        let mut buffers = Buffers::default();
        frame(&mut buffers, 3, 1.0);
        upload(&mut mesh, &mut buffers);

        frame(&mut buffers, 3, 2.0);
        let want = buffers.positions.clone();
        upload(&mut mesh, &mut buffers);

        assert_eq!(positions(&mesh), want);
        // And again on an attribute the quad's corners do not move: a position is the centre
        // plus or minus `half`, so it can never read back as the mark itself.
        let swells = match mesh.attribute(ATTRIBUTE_SWELL) {
            Some(VertexAttributeValues::Float32(v)) => v.clone(),
            other => panic!("swells are not Float32: {other:?}"),
        };
        assert!(
            swells.iter().all(|s| *s == 2.0),
            "the mesh is holding a frame it was not given: {swells:?}"
        );
    }

    #[test]
    fn every_attribute_arrives_and_they_all_agree_in_length() {
        // A mesh whose attributes disagree is a validation error at draw time, several layers
        // from whichever swap was forgotten. Eleven of them and one set of indices.
        let mut mesh = empty_mesh();
        let mut buffers = Buffers::default();
        frame(&mut buffers, 5, 1.0);
        upload(&mut mesh, &mut buffers);

        let n = 5 * 4;
        for id in [
            Mesh::ATTRIBUTE_POSITION,
            Mesh::ATTRIBUTE_UV_0,
            Mesh::ATTRIBUTE_COLOR,
            ATTRIBUTE_SHAPE,
            ATTRIBUTE_SQUASH_DIR,
            ATTRIBUTE_SQUASH_FACE,
            ATTRIBUTE_SQUASH_DIR2,
            ATTRIBUTE_SQUASH_FACE2,
            ATTRIBUTE_SQUASH_DIR3,
            ATTRIBUTE_SQUASH_FACE3,
            ATTRIBUTE_SWELL,
        ] {
            let values = mesh
                .attribute(id)
                .unwrap_or_else(|| panic!("{} never reached the mesh", id.name));
            assert_eq!(values.len(), n, "{} is the wrong length", id.name);
        }
        assert_eq!(mesh.indices().map(bevy::mesh::Indices::len), Some(5 * 6));
    }

    #[test]
    fn a_shrinking_frame_does_not_leave_the_tail_of_a_larger_one() {
        let mut mesh = empty_mesh();
        let mut buffers = Buffers::default();
        frame(&mut buffers, 9, 1.0);
        upload(&mut mesh, &mut buffers);
        frame(&mut buffers, 2, 2.0);
        upload(&mut mesh, &mut buffers);
        assert_eq!(positions(&mesh).len(), 2 * 4);
        assert_eq!(mesh.indices().map(bevy::mesh::Indices::len), Some(2 * 6));
    }

    #[test]
    fn the_plain_tier_never_builds_a_seam() {
        // The saving, stated as a number rather than as an intention: 100 of the 152 bytes a
        // vertex are the seams and the swell, and at this tier they are not written at all.
        let mut buffers = Buffers::default();
        frame_at(&mut buffers, 7, 1.0, Detail::Plain);
        assert_eq!(buffers.cells(), 7);
        assert_eq!(buffers.positions.len(), 7 * 4);
        assert_eq!(buffers.shapes.len(), 7 * 4);
        assert_eq!(buffers.indices.len(), 7 * 6);
        assert!(buffers.squash_dirs.is_empty());
        assert!(buffers.squash_faces.is_empty());
        assert!(buffers.squash_dirs2.is_empty());
        assert!(buffers.squash_faces2.is_empty());
        assert!(buffers.squash_dirs3.is_empty());
        assert!(buffers.squash_faces3.is_empty());
        assert!(buffers.swells.is_empty());
    }

    #[test]
    fn the_dot_mesh_carries_four_attributes_and_the_cell_mesh_eleven() {
        let dots = dot_mesh();
        for id in [
            Mesh::ATTRIBUTE_POSITION,
            Mesh::ATTRIBUTE_UV_0,
            Mesh::ATTRIBUTE_COLOR,
            ATTRIBUTE_SHAPE,
        ] {
            assert!(dots.attribute(id).is_some(), "{} is missing", id.name);
        }
        for id in [ATTRIBUTE_SQUASH_DIR, ATTRIBUTE_SQUASH_FACE, ATTRIBUTE_SWELL] {
            assert!(
                dots.attribute(id).is_none(),
                "{} is on the dot mesh, and paying for itself every frame",
                id.name
            );
        }
        assert!(empty_mesh().attribute(ATTRIBUTE_SWELL).is_some());
    }

    #[test]
    fn a_plain_frame_fills_the_dot_mesh_and_leaves_no_ragged_attribute() {
        // The failure this guards: uploading a `Plain` frame through the seamed path would swap
        // seven *empty* vectors against four with sixty thousand entries in them, and a mesh
        // whose attributes disagree in length does not draw at all.
        let mut mesh = dot_mesh();
        let mut buffers = Buffers::default();
        frame_at(&mut buffers, 5, 1.0, Detail::Plain);
        upload(&mut mesh, &mut buffers);
        for id in [
            Mesh::ATTRIBUTE_POSITION,
            Mesh::ATTRIBUTE_UV_0,
            Mesh::ATTRIBUTE_COLOR,
            ATTRIBUTE_SHAPE,
        ] {
            assert_eq!(mesh.attribute(id).map(VertexAttributeValues::len), Some(20));
        }
        assert_eq!(mesh.indices().map(bevy::mesh::Indices::len), Some(30));
    }

    #[test]
    fn crossing_the_tier_leaves_nothing_of_the_other_one_behind() {
        // A `Seamed` frame then a `Plain` one: `begin` clears every vector, not only the ones
        // this tier is about to fill, so the seams of the frame before do not survive into a
        // frame that has none.
        let mut buffers = Buffers::default();
        frame_at(&mut buffers, 6, 1.0, Detail::Seamed);
        assert_eq!(buffers.swells.len(), 24);
        frame_at(&mut buffers, 6, 2.0, Detail::Plain);
        assert!(buffers.swells.is_empty());
        assert!(buffers.squash_dirs.is_empty());
        // And back again, into the same allocation.
        frame_at(&mut buffers, 6, 3.0, Detail::Seamed);
        assert_eq!(buffers.swells.len(), 24);
        assert!(buffers.swells.iter().all(|s| *s == 3.0));
    }

    #[test]
    fn a_steady_population_stops_allocating() {
        // The reason for the swap rather than a `take`: both sets of vectors keep their
        // allocation, so after they have both been grown once nothing is asked of the allocator
        // again. A `take` would leave the caller empty and reallocate the lot every frame.
        let mut mesh = empty_mesh();
        let mut buffers = Buffers::default();
        for mark in 0..4 {
            frame(&mut buffers, 64, mark as f32);
            upload(&mut mesh, &mut buffers);
        }
        let settled: Vec<usize> = (0..4)
            .map(|mark| {
                frame(&mut buffers, 64, mark as f32);
                let capacity = buffers.positions.capacity();
                upload(&mut mesh, &mut buffers);
                capacity
            })
            .collect();
        assert!(
            settled.windows(2).all(|w| w[0] == w[1]),
            "the buffers are still growing after four frames of the same size: {settled:?}"
        );
        assert!(settled[0] >= 64 * 4);
    }
}
