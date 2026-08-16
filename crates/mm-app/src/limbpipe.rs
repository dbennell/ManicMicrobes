//! What the limb shader is drawn through: the vertex attributes, the material and the mesh.
//!
//! The third pipeline, beside [`crate::cellpipe`]'s two. See `limbmesh.rs` for the arithmetic that
//! fills the buffers, `limb.wgsl` for the fields, and `docs/MORPHOLOGY.md` for why a limb is not
//! drawn through the cell material.
//!
//! Its own file rather than four more items in `cellpipe.rs`, and that is the point of it: the
//! body's layout, materials and shader are finished work, and a change in how a cell looks has to
//! stay attributable to something that touched them. `docs/OVERLAPS.md` is the record of what it
//! costs when it is not.

use bevy::asset::{load_internal_asset, uuid_handle, RenderAssetUsages};
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, PrimitiveTopology, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    VertexFormat,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};

use crate::limbmesh;

/// Form, extent, phase and aspect. See `limbmesh::Buffers::limb_a`.
pub const ATTRIBUTE_LIMB_A: MeshVertexAttribute =
    MeshVertexAttribute::new("LimbA", 0x6D_6D_5F_6C_69_6D_61, VertexFormat::Float32x4);

/// Count, inner, taper and seed. See `limbmesh::Buffers::limb_b`.
pub const ATTRIBUTE_LIMB_B: MeshVertexAttribute =
    MeshVertexAttribute::new("LimbB", 0x6D_6D_5F_6C_69_6D_62, VertexFormat::Float32x4);

/// Embedded at compile time rather than loaded from an `assets/` directory, so the binary runs
/// from anywhere. [`reload`] can replace what is behind it, which is how the bench edits a form
/// without a rebuild.
pub const LIMB_SHADER: Handle<Shader> = uuid_handle!("6d6d5f6c-696d-625f-7368-616465720001");

/// Where the shader is in the source tree, for [`reload`].
pub const LIMB_SHADER_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/limb.wgsl");

/// Every limb on the slide, drawn by one material with one shader.
///
/// No bindings, for the reason [`crate::cellpipe::CellMaterial`] has none: everything that varies
/// varies *per limb*, a material's uniforms are per draw call, and the entire point is that there
/// is one draw call.
#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub struct LimbMaterial {}

impl Material2d for LimbMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(LIMB_SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(LIMB_SHADER)
    }

    /// Blend, for every reason [`crate::cellpipe::CellMaterial::alpha_mode`] measures at length:
    /// `Opaque` is a blend state of *replace*, so the antialiased edge the fragment shader computes
    /// is thrown away and what decides the drawn shape is the `discard` at the bottom of the fade —
    /// a hard edge about two pixels outside the outline, which crawls whenever anything moves.
    ///
    /// It matters more here than it does for a body. A spike is a few pixels wide, so two pixels of
    /// overrun is most of its width; and a limb is thin and often nearly aligned with the pixel
    /// grid, which is the worst case for an unantialiased edge.
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Named, though `limb.wgsl` has one entry point of each stage today. `cellpipe`'s note
        // records what the default cost when a second one arrived: not a validation error naming
        // the shader, but a heap abort inside the driver before a line of log.
        descriptor.vertex.entry_point = Some("vertex".into());
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.entry_point = Some("fragment".into());
        }
        // The locations here are the locations in `limb.wgsl`, and the two have to agree exactly —
        // a mismatch is a validation failure at draw time with nothing to say which end is wrong.
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
            ATTRIBUTE_LIMB_A.at_shader_location(3),
            ATTRIBUTE_LIMB_B.at_shader_location(4),
        ])?];
        Ok(())
    }
}

/// Register the shader and the material. Must be added after `DefaultPlugins`, as `cellpipe`'s is.
pub fn plugin(app: &mut App) {
    load_internal_asset!(app, LIMB_SHADER, "limb.wgsl", Shader::from_wgsl);
    app.add_plugins(Material2dPlugin::<LimbMaterial>::default());
}

/// Replace the compiled-in shader with whatever is on disk now, so a form can be edited without a
/// rebuild. See [`crate::cellpipe::reload`].
///
/// # Errors
///
/// If the file cannot be read.
pub fn reload(shaders: &mut Assets<Shader>, path: &str) -> std::io::Result<()> {
    let source = std::fs::read_to_string(path)?;
    let _ = shaders.insert(&LIMB_SHADER, Shader::from_wgsl(source, path.to_string()));
    Ok(())
}

/// An empty mesh carrying every attribute the material's layout asks for.
///
/// Every one of them, even before there is a limb to put in it: a mesh missing an attribute the
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
    mesh.insert_attribute(ATTRIBUTE_LIMB_A, Vec::<[f32; 4]>::new());
    mesh.insert_attribute(ATTRIBUTE_LIMB_B, Vec::<[f32; 4]>::new());
    mesh.insert_indices(Indices::U32(Vec::new()));
    mesh
}

fn swap_attribute<T>(
    mesh: &mut Mesh,
    attribute: MeshVertexAttribute,
    mine: &mut Vec<T>,
    theirs: fn(&mut VertexAttributeValues) -> Option<&mut Vec<T>>,
) where
    Vec<T>: Into<VertexAttributeValues>,
{
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
/// A swap and not a copy, for the reason `cellpipe::upload` gives at length: what comes back is
/// the allocation the *next* frame is built in, so a steady population stops asking the allocator
/// for anything. The contract that makes it safe is `limbmesh::Buffers::begin`, which clears.
pub fn upload(mesh: &mut Mesh, buffers: &mut limbmesh::Buffers) {
    swap_attribute!(
        mesh,
        Mesh::ATTRIBUTE_POSITION,
        Float32x3,
        &mut buffers.positions
    );
    swap_attribute!(mesh, Mesh::ATTRIBUTE_UV_0, Float32x2, &mut buffers.uvs);
    swap_attribute!(mesh, Mesh::ATTRIBUTE_COLOR, Float32x4, &mut buffers.colours);
    swap_attribute!(mesh, ATTRIBUTE_LIMB_A, Float32x4, &mut buffers.limb_a);
    swap_attribute!(mesh, ATTRIBUTE_LIMB_B, Float32x4, &mut buffers.limb_b);

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limbmesh::{form, Buffers, Placed};

    fn frame(buffers: &mut Buffers, n: usize, mark: f32) {
        buffers.begin(n);
        for i in 0..n {
            buffers.push(Placed {
                cx: mark + i as f32,
                cy: mark,
                ux: 1.0,
                uy: 0.0,
                half_len: 4.0,
                half_wid: 1.0,
                rgba: [mark; 4],
                form: form::SPIKE,
                extent: mark,
                phase: 0.0,
                count: 1.0,
                inner: 0.0,
                taper: 0.0,
                seed: mark,
            });
        }
    }

    #[test]
    fn every_attribute_arrives_and_they_all_agree_in_length() {
        let mut mesh = empty_mesh();
        let mut buffers = Buffers::default();
        frame(&mut buffers, 5, 1.0);
        upload(&mut mesh, &mut buffers);
        for id in [
            Mesh::ATTRIBUTE_POSITION,
            Mesh::ATTRIBUTE_UV_0,
            Mesh::ATTRIBUTE_COLOR,
            ATTRIBUTE_LIMB_A,
            ATTRIBUTE_LIMB_B,
        ] {
            let values = mesh
                .attribute(id)
                .unwrap_or_else(|| panic!("{} never reached the mesh", id.name));
            assert_eq!(values.len(), 20, "{} is the wrong length", id.name);
        }
        assert_eq!(mesh.indices().map(bevy::mesh::Indices::len), Some(30));
    }

    #[test]
    fn the_mesh_gets_this_frame_and_never_the_last_one() {
        // The failure the swap makes possible: the vectors that arrive back from the mesh are
        // full, so a frame built without clearing them is last frame's limbs, on screen, one
        // behind — and a limb one frame behind is one that has come off the cell it belongs to.
        let mut mesh = empty_mesh();
        let mut buffers = Buffers::default();
        frame(&mut buffers, 3, 1.0);
        upload(&mut mesh, &mut buffers);
        frame(&mut buffers, 3, 2.0);
        upload(&mut mesh, &mut buffers);
        let a = match mesh.attribute(ATTRIBUTE_LIMB_A) {
            Some(VertexAttributeValues::Float32x4(v)) => v.clone(),
            other => panic!("limb_a is not Float32x4: {other:?}"),
        };
        assert!(
            a.iter().all(|v| v[1] == 2.0),
            "the mesh is holding a frame it was not given: {a:?}"
        );
    }

    #[test]
    fn a_shrinking_frame_does_not_leave_the_tail_of_a_larger_one() {
        let mut mesh = empty_mesh();
        let mut buffers = Buffers::default();
        frame(&mut buffers, 9, 1.0);
        upload(&mut mesh, &mut buffers);
        frame(&mut buffers, 2, 2.0);
        upload(&mut mesh, &mut buffers);
        assert_eq!(
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                .map(VertexAttributeValues::len),
            Some(8)
        );
        assert_eq!(mesh.indices().map(bevy::mesh::Indices::len), Some(12));
    }

    #[test]
    fn a_steady_population_stops_allocating() {
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
    }
}
