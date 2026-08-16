//! Manic Microbes — the front-end.
//!
//! This is a fishtank and it has to be beautiful (SPEC §14). It is also the crate that must
//! never be able to affect what it is looking at.
//!
//! [`slide::Slide`] is where that separation lives, and it is deliberately free of any
//! graphics dependency: the renderer receives a [`slide::Frame`], which is a plain snapshot
//! with no reference back to the world. That means M4's central guarantee — that a world run
//! at sixty frames a second is bit-identical to one run headless — is checkable on a machine
//! with no display, which is where it is checked.
//!
//! The Bevy application is behind the `render` feature for the same reason.

//! # What is where
//!
//! - [`slide`] — the world, the frame, level of detail, overlays and the metric history.
//! - [`engine`] — the simulation thread, and the only place wall-clock time is allowed to
//!   decide anything.
//! - [`optics`] — the microscope's look, as parameters rather than as a shader.
//! - [`art`] — the baked cell atlas, and the chemical field's pixels.
//! - [`cellmesh`] — the whole population as one mesh, for the per-pixel cell shader.
//! - [`limbmesh`] — what cells have grown outside their membranes, as a second mesh under them.
//! - [`phantom`] — cells no simulation made, for testing the renderer rather than the slide.
//! - [`inspector`] — a read-only transcript of one cell.
//! - [`editor`] — a `.mm` source buffer, its diagnostics and its exports.
//! - [`debugger`] — breakpoints over the live world, instruction stepping in a sandbox.
//! - [`wiki`] — the species wiki, the phylogenetic tree and the world timeline.
//! - [`tools`] — tweezers and barriers: the one place that is *meant* to touch the world.
//! - [`params`] — what the parameter editor calls each knob (M10.2).
//! - [`theme`] — the palette and the type scale, as numbers rather than as a stylesheet.
//! - [`ui`] — the shell: pointer routing, panel state, camera arithmetic.
//! - [`threads`] — which cores rayon runs the simulation on.
//!
//! All of them are testable without a graphics stack, which is the point. `main.rs` is the
//! only file that knows Bevy exists, and it is behind the `render` feature.

pub mod art;
pub mod cellmesh;
/// The Bevy side of drawing a cell: the vertex layout, the material and the shader handle.
///
/// The one module of the library that knows Bevy exists, and behind the `render` feature for the
/// same reason the application is. It holds no logic — only the layout the shader and the mesh
/// must agree about — and it exists so that the microscope and the shader bench cannot drift
/// apart in what they draw through.
#[cfg(feature = "render")]
pub mod cellpipe;
/// The surface of rock, for the same reason and behind the same feature as [`cellpipe`]: a
/// material and the shader it names, and no logic of its own.
#[cfg(feature = "render")]
pub mod rockpipe;
pub mod limbmesh;
/// The Bevy side of drawing a limb, on the same terms as [`cellpipe`] and separate from it on
/// purpose: the body's layout, materials and shader are finished work, and a change in how a cell
/// looks has to stay attributable to something that touched them.
#[cfg(feature = "render")]
pub mod limbpipe;
pub mod debugger;
pub mod editor;
pub mod engine;
pub mod foodweb;
/// The window icon, drawn from its geometry rather than decoded — a membrane, the lit arc, a
/// nucleus and three organelles, on the website's numbers.
///
/// No Bevy in it and no image codec either, which is the reason it is drawn rather than unpacked:
/// this crate takes Bevy with `default-features = false` and never enables `png`, so there is
/// nothing in the graph that could read one. It writes the two container formats the operating
/// systems want as files — a `.ico` for the Windows executable, PNG faces for a macOS `.icns` —
/// so that no part of the build keeps a second copy of the mark.
///
/// **The module's own documentation is here rather than in the file.** `build.rs` includes
/// `src/icon.rs` textually, and an inner doc comment cannot survive a macro expansion.
pub mod icon;
pub mod inspector;
pub mod library;
pub mod optics;
pub mod params;
pub mod phantom;
/// [`theme`] in egui's vocabulary: the style, the fonts, and the widgets the panels are built
/// from.
///
/// Behind the `render` feature because it is the only half of the theme that needs a toolkit.
/// The half worth testing is [`theme`], which has no egui in it at all.
#[cfg(feature = "render")]
pub mod skin;
pub mod slide;
pub mod theme;
/// Rayon's pool, pinned to the cores worth having. A scheduling hint and nothing more.
pub mod threads;
pub mod tools;
pub mod ui;
pub mod wiki;

pub use inspector::Inspection;
pub use optics::{Mote, Optics};
pub use slide::{CellDot, Frame, Lod, MetricHistory, OverlayLayer, Slide};
pub use ui::{Focus, Panel, Panels, Target};
pub use wiki::{Page, Timeline, Tree};
