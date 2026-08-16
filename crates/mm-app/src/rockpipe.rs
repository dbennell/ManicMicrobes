//! The material the barrier layer is drawn through, so rock can have a surface.
//!
//! The layer itself is unchanged: one texel per square, nearest-sampled, painted by
//! [`crate::art::paint_barriers`]. What this adds is a fragment shader between that texture and
//! the screen, which roughens the inside of a mineral square and leaves everything else exactly
//! as it was — see `rock.wgsl` for what it does and why the silhouette is not part of it.
//!
//! It is a whole material rather than a bigger texture because the alternative is arithmetic per
//! *texel* rather than per *pixel*: painting the grain on the CPU means a barrier texture several
//! times the grid in each direction, repainted whenever the vignette moves, for detail that is
//! only visible at high magnification and is thrown away at every other zoom. A shader computes
//! exactly the pixels that are on the screen, at the size they are on the screen.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

/// Embedded at compile time, for the reason [`crate::cellpipe::CELL_SHADER`] gives: the binary
/// runs from anywhere rather than from beside an `assets/` directory.
pub const ROCK_SHADER: Handle<Shader> = uuid_handle!("6d6d5f72-6f63-6b5f-7368-616465720001");

/// The barrier layer, and the surface put on it.
///
/// One texture and its sampler, and nothing else — everything the shader needs beyond the texel
/// it can work out for itself. The grid comes from `textureDimensions`, so a scenario of a
/// different size needs no update here; the zoom comes from `fwidth`, so panning and zooming
/// need no uniform written per frame and there is no camera value to fall out of step with.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct RockMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub barriers: Handle<Image>,
}

impl Material2d for RockMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(ROCK_SHADER)
    }

    /// The vertex stage is Bevy's own `mesh2d` one. The quad is a rectangle with a position and a
    /// uv and nothing else to say, and a second copy of a standard vertex shader is a second
    /// thing to keep in step with the engine for no gain.
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Default
    }

    /// Blend, because the layer is transparent everywhere that is not a wall and the field is
    /// underneath it. The shader writes an alpha of one inside a wall and discards outside, so
    /// what blends is the boundary of the quad rather than the boundary of the rock.
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

pub fn plugin(app: &mut App) {
    load_internal_asset!(app, ROCK_SHADER, "rock.wgsl", Shader::from_wgsl);
    app.add_plugins(Material2dPlugin::<RockMaterial>::default());
}
