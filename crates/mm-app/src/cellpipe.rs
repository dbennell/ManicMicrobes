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
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, PrimitiveTopology, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    VertexFormat,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{Material2d, Material2dKey, Material2dPlugin};

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

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
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

/// Register the shader and the material.
///
/// Must be added *after* `DefaultPlugins`, not before: the macro writes straight into
/// `Assets<Shader>`, and `AssetPlugin` is what puts that resource in the world. Before it, this is
/// a panic on the first line of `main` with a message about a missing resource rather than about
/// a shader.
pub fn plugin(app: &mut App) {
    load_internal_asset!(app, CELL_SHADER, "cell.wgsl", Shader::from_wgsl);
    app.add_plugins(Material2dPlugin::<CellMaterial>::default());
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

/// Put one frame's worth of vertices into the mesh.
pub fn upload(mesh: &mut Mesh, buffers: &cellmesh::Buffers) {
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, buffers.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, buffers.uvs.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, buffers.colours.clone());
    mesh.insert_attribute(ATTRIBUTE_SHAPE, buffers.shapes.clone());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_DIR, buffers.squash_dirs.clone());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_FACE, buffers.squash_faces.clone());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_DIR2, buffers.squash_dirs2.clone());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_FACE2, buffers.squash_faces2.clone());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_DIR3, buffers.squash_dirs3.clone());
    mesh.insert_attribute(ATTRIBUTE_SQUASH_FACE3, buffers.squash_faces3.clone());
    mesh.insert_attribute(ATTRIBUTE_SWELL, buffers.swells.clone());
    mesh.insert_indices(Indices::U32(buffers.indices.clone()));
}
