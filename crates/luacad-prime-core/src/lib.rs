//! # prime-core
//!
//! A headless, physically based path tracer. This crate is a pure rendering
//! library — it has **no** windowing, OpenGL, or image-codec dependencies — so
//! it builds and tests anywhere and can be driven by any front-end (the
//! `prime` CLI, a test harness, a future GUI, …).
//!
//! ## Pipeline
//!
//! A [`scene::Scene`] bundles a material table, a [`bvh::Bvh`] of
//! [`geometry::Primitive`]s, a [`camera::CameraConfig`], and a
//! [`scene::Background`]. It is handed to [`integrator::render`], which
//! path-traces it in parallel into a [`framebuffer::Framebuffer`] of linear HDR
//! color. The framebuffer resolves to 8-bit sRGB via [`color`].
//!
//! Build a scene programmatically (see [`demo`]) or describe it as data with
//! [`desc::SceneDesc`] (RON, replacing the legacy Java-serialization format).

/// The scalar type used throughout the renderer. Aliased so precision is a
/// single edit; `f32` keeps geometry compact and intersection fast.
pub type Float = f32;

/// Linear RGB color is represented with the same value type as a vector.
pub type Color = math::Vec3;

/// Index into a [`scene::Scene`]'s material table.
pub type MaterialId = usize;

pub mod aabb;
pub mod bvh;
pub mod camera;
pub mod color;
pub mod demo;
pub mod env;
pub mod framebuffer;
pub mod geometry;
pub mod hit;
pub mod integrator;
pub mod material;
pub mod math;
pub mod noise;
pub mod obj;
pub mod ray;
pub mod sampler;
pub mod scene;
pub mod spectrum;
pub mod texture;

#[cfg(feature = "serde")]
pub mod desc;

pub use math::Vec3;

/// Commonly used types, for `use prime_core::prelude::*;`.
pub mod prelude {
    pub use crate::camera::CameraConfig;
    pub use crate::color::Tonemap;
    pub use crate::env::EnvMap;
    pub use crate::geometry::{Primitive, Sphere, Triangle};
    pub use crate::integrator::{
        render, render_aovs, render_to_srgb, AovBuffers, ProgressiveRenderer, RenderSettings,
    };
    pub use crate::material::Material;
    pub use crate::math::Vec3;
    pub use crate::scene::{Background, Scene};
    pub use crate::{Color, Float, MaterialId};

    #[cfg(feature = "serde")]
    pub use crate::desc::{ObjectDesc, SceneDesc};
}
