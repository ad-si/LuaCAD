//! The result of a successful ray/primitive intersection.

use crate::math::Vec3;
use crate::ray::Ray;
use crate::{Float, MaterialId};

#[derive(Clone, Copy, Debug)]
pub struct HitRecord {
    /// Ray parameter at the hit.
    pub t: Float,
    /// World-space hit position.
    pub p: Vec3,
    /// Shading normal, always oriented against the incoming ray.
    pub normal: Vec3,
    /// Surface tangent (ideally aligned with the +U texture direction), used to
    /// build the frame for normal mapping. Defaults to an arbitrary tangent;
    /// primitives override it with a UV-aligned one when they can.
    pub tangent: Vec3,
    /// Whether the ray hit the outward-facing side of the surface.
    pub front_face: bool,
    /// Surface texture coordinates.
    pub u: Float,
    pub v: Float,
    /// Surface area of the primitive that was hit. Used to compute the
    /// area-measure light pdf for multiple importance sampling when a path ray
    /// lands on an emitter.
    pub area: Float,
    /// Index into the scene's material table.
    pub material: MaterialId,
    /// Index of the primitive that was hit, in BVH traversal order (see
    /// `Bvh::primitive`). Set by the BVH; `u32::MAX` when the record came from
    /// intersecting a primitive directly.
    pub prim: u32,
}

impl HitRecord {
    /// Orient `outward_normal` (which must be unit length) to face the ray and
    /// record which side was hit.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn with_face_normal(
        ray: &Ray,
        t: Float,
        p: Vec3,
        outward_normal: Vec3,
        u: Float,
        v: Float,
        area: Float,
        material: MaterialId,
    ) -> HitRecord {
        let front_face = ray.dir.dot(outward_normal) < 0.0;
        let normal = if front_face {
            outward_normal
        } else {
            -outward_normal
        };
        HitRecord {
            t,
            p,
            normal,
            tangent: fallback_tangent(normal),
            front_face,
            u,
            v,
            area,
            material,
            prim: u32::MAX,
        }
    }
}

/// An arbitrary unit tangent perpendicular to `n` (used when a primitive has no
/// UV-aligned tangent).
#[inline]
pub fn fallback_tangent(n: Vec3) -> Vec3 {
    let a = if n.x.abs() > 0.9 {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    n.cross(a).normalize()
}
