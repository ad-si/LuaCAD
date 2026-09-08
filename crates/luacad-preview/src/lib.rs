//! The 3D preview both LuaCAD front ends draw: the studio's viewport and the
//! browser playground's canvas.
//!
//! A model is not booleaned before it is shown. It is flattened into CSG
//! products — a list of primitives each tagged as intersected or subtracted
//! ([`flatten`]) — and those products are drawn by [WebCSG], which fills the
//! depth buffer with the visible surface of the product and lets the shading
//! pass color exactly that surface ([`render`]).
//!
//! The two halves are separate features because the playground splits them
//! across two WebAssembly modules: the engine flattens in a worker (it needs
//! Manifold, so it is built with Emscripten) and the viewer renders on the
//! page (it needs wgpu, so it is built for `wasm32-unknown-unknown`). The
//! scene travels between them through [`wire`].
//!
//! [WebCSG]: https://github.com/ad-si/WebCSG

#[cfg(feature = "flatten")]
pub mod flatten;
#[cfg(feature = "render")]
pub mod render;
pub mod tree;
pub mod wire;

#[cfg(feature = "flatten")]
pub use flatten::{flatten_geometries, solid_meshes};
pub use tree::{
  CsgGroup, CsgLeaf, CsgScene, Operation, OverlayMesh, Shading, SolidMesh,
};
