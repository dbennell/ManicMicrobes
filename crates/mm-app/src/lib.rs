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

pub mod slide;

pub use slide::{CellDot, Frame, Slide};
