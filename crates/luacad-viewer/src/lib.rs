//! The playground's 3D viewport: the same WebCSG preview the studio draws,
//! on the page's canvas.
//!
//! The engine module — Lua and Manifold, compiled with Emscripten and run in
//! a worker — flattens a script into CSG products and hands them over as the
//! byte buffer [`luacad_preview::wire`] describes. This module decodes that
//! buffer and renders it through wgpu on WebGPU — and only WebGPU: see
//! `create_viewer` for why WebGL2 cannot stand in for it.
//!
//! The page keeps the pointer handling (it owns the events) and this module
//! keeps the camera (it owns the matrices), so a drag is a call to
//! `Viewer::orbit` followed by `Viewer::render`.
//!
//! Only the camera builds off the web: a canvas surface exists nowhere else,
//! so the renderer is compiled for `wasm32-unknown-unknown` alone and
//! `cargo check` on a workstation sees the camera and its tests.

pub mod camera;
#[cfg(target_arch = "wasm32")]
mod viewer;

#[cfg(target_arch = "wasm32")]
pub use viewer::{Viewer, create_viewer};
