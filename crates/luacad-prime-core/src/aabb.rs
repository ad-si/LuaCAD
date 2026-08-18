//! Axis-aligned bounding box and the ray/box slab test that drives BVH
//! traversal.

use crate::math::Vec3;
use crate::ray::Ray;
use crate::Float;

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// An inverted, "empty" box that absorbs points/boxes via [`Aabb::union`]
    /// and [`Aabb::union_point`].
    pub const EMPTY: Aabb = Aabb {
        min: Vec3::splat(Float::INFINITY),
        max: Vec3::splat(Float::NEG_INFINITY),
    };

    #[inline]
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Aabb { min, max }
    }

    pub fn from_points(points: &[Vec3]) -> Self {
        let mut b = Aabb::EMPTY;
        for &p in points {
            b = b.union_point(p);
        }
        b
    }

    #[inline]
    pub fn union(self, o: Aabb) -> Aabb {
        Aabb::new(self.min.min(o.min), self.max.max(o.max))
    }

    #[inline]
    pub fn union_point(self, p: Vec3) -> Aabb {
        Aabb::new(self.min.min(p), self.max.max(p))
    }

    #[inline]
    pub fn centroid(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Index of the axis with the largest extent (0 = x, 1 = y, 2 = z).
    pub fn longest_axis(&self) -> usize {
        let d = self.max - self.min;
        if d.x >= d.y && d.x >= d.z {
            0
        } else if d.y >= d.z {
            1
        } else {
            2
        }
    }

    /// Surface area (used by the SAH split cost). Returns 0 for an empty box.
    pub fn surface_area(&self) -> Float {
        let d = self.max - self.min;
        if d.x < 0.0 || d.y < 0.0 || d.z < 0.0 {
            return 0.0;
        }
        2.0 * (d.x * d.y + d.y * d.z + d.z * d.x)
    }

    /// Slab test. Returns true if the ray intersects the box within
    /// `[t_min, t_max]`. Robust to axis-aligned rays via infinite slopes.
    #[inline]
    pub fn hit(&self, ray: &Ray, mut t_min: Float, mut t_max: Float) -> bool {
        for a in 0..3 {
            let inv_d = 1.0 / ray.dir.axis(a);
            let mut t0 = (self.min.axis(a) - ray.origin.axis(a)) * inv_d;
            let mut t1 = (self.max.axis(a) - ray.origin.axis(a)) * inv_d;
            if inv_d < 0.0 {
                std::mem::swap(&mut t0, &mut t1);
            }
            t_min = if t0 > t_min { t0 } else { t_min };
            t_max = if t1 < t_max { t1 } else { t_max };
            if t_max <= t_min {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_box_in_front() {
        let b = Aabb::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        let r = Ray::new(Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(b.hit(&r, 0.001, Float::INFINITY));
    }

    #[test]
    fn ray_misses_box_to_the_side() {
        let b = Aabb::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        let r = Ray::new(Vec3::new(5.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(!b.hit(&r, 0.001, Float::INFINITY));
    }

    #[test]
    fn box_behind_origin_is_not_hit() {
        let b = Aabb::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        let r = Ray::new(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(!b.hit(&r, 0.001, Float::INFINITY));
    }

    #[test]
    fn union_grows_to_contain() {
        let b = Aabb::EMPTY
            .union_point(Vec3::new(1.0, 2.0, 3.0))
            .union_point(Vec3::new(-1.0, 9.0, 4.0));
        assert_eq!(b.min, Vec3::new(-1.0, 2.0, 3.0));
        assert_eq!(b.max, Vec3::new(1.0, 9.0, 4.0));
        // extents: x=2, y=7, z=1 -> y is longest.
        assert_eq!(b.longest_axis(), 1);
    }
}
