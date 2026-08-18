//! A half-line: an origin and a direction.
//!
//! Unlike the legacy `Ray` (mutable length, an embedded color accumulator, and
//! defensive-cloning getters in the hot path), this `Ray` is a tiny immutable
//! `Copy` value. Intersection routines take explicit `t_min`/`t_max` bounds
//! instead of mutating the ray, and the integrator carries radiance/throughput
//! itself.

use crate::math::Vec3;
use crate::Float;

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Ray {
    #[inline]
    pub fn new(origin: Vec3, dir: Vec3) -> Self {
        Ray { origin, dir }
    }

    /// Point at parameter `t` along the ray.
    #[inline]
    pub fn at(&self, t: Float) -> Vec3 {
        self.origin + self.dir * t
    }
}
