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
//! - [`inspector`] — a read-only transcript of one cell.
//! - [`editor`] — a `.mm` source buffer, its diagnostics and its exports.
//! - [`debugger`] — breakpoints over the live world, instruction stepping in a sandbox.
//! - [`wiki`] — the species wiki, the phylogenetic tree and the world timeline.
//! - [`tools`] — tweezers and barriers: the one place that is *meant* to touch the world.
//! - [`params`] — what the parameter editor calls each knob (M10.2).
//! - [`ui`] — the shell: pointer routing, panel state, camera arithmetic.
//!
//! All of them are testable without a graphics stack, which is the point. `main.rs` is the
//! only file that knows Bevy exists, and it is behind the `render` feature.

pub mod art;
pub mod cellmesh;
pub mod debugger;
pub mod editor;
pub mod engine;
pub mod foodweb;
pub mod inspector;
pub mod optics;
pub mod params;
pub mod slide;
pub mod tools;
pub mod ui;
pub mod wiki;

pub use inspector::Inspection;
pub use optics::{Mote, Optics};
pub use slide::{CellDot, Frame, Lod, MetricHistory, OverlayLayer, Slide};
pub use ui::{Focus, Panel, Panels, Target};
pub use wiki::{Page, Timeline, Tree};
