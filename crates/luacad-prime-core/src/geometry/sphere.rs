//! Analytic sphere primitive. Spheres make compact, exact test scenes
//! (e.g. Cornell-box demos) without needing external mesh assets.

use crate::aabb::Aabb;
use crate::hit::HitRecord;
use crate::math::sampling::random_unit_vector;
use crate::math::{Onb, Vec3};
use crate::ray::Ray;
use crate::sampler::Sampler;
use crate::{Float, MaterialId};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug)]
pub struct Sphere {
    pub center: Vec3,
    pub radius: Float,
    pub material: MaterialId,
}

impl Sphere {
    pub fn new(center: Vec3, radius: Float, material: MaterialId) -> Self {
        Sphere {
            center,
            radius,
            material,
        }
    }

    pub fn hit(&self, ray: &Ray, t_min: Float, t_max: Float) -> Option<HitRecord> {
        let oc = ray.origin - self.center;
        let a = ray.dir.length_squared();
        let half_b = oc.dot(ray.dir);
        let c = oc.length_squared() - self.radius * self.radius;
        let disc = half_b * half_b - a * c;
        if disc < 0.0 {
            return None;
        }
        let sqrt_d = disc.sqrt();

        // Nearest root within the valid interval.
        let mut root = (-half_b - sqrt_d) / a;
        if root < t_min || root > t_max {
            root = (-half_b + sqrt_d) / a;
            if root < t_min || root > t_max {
                return None;
            }
        }

        let p = ray.at(root);
        let outward = (p - self.center) * (1.0 / self.radius);
        let (u, v) = sphere_uv(outward);
        let mut hit =
            HitRecord::with_face_normal(ray, root, p, outward, u, v, self.area(), self.material);
        // Tangent along increasing longitude (dP/dφ), for normal mapping.
        let tang = Vec3::new(-outward.z, 0.0, outward.x);
        if tang.length_squared() > 1e-12 {
            hit.tangent = tang.normalize();
        }
        Some(hit)
    }

    pub fn aabb(&self) -> Aabb {
        let r = Vec3::splat(self.radius);
        Aabb::new(self.center - r, self.center + r)
    }

    pub fn centroid(&self) -> Vec3 {
        self.center
    }

    #[inline]
    pub fn area(&self) -> Float {
        4.0 * PI * self.radius * self.radius
    }

    /// Uniformly sample a point on the sphere surface, returning the point and
    /// its outward normal.
    pub fn sample(&self, sampler: &mut Sampler) -> (Vec3, Vec3) {
        let dir = random_unit_vector(sampler);
        (self.center + dir * self.radius, dir)
    }

    /// Sample the cone of directions subtended by the sphere as seen from
    /// `from` — solid-angle sampling for lighting, which (unlike uniform area
    /// sampling) never wastes a sample on the invisible back hemisphere.
    /// Returns `(wi, distance to the near surface, solid-angle pdf)`, or
    /// `None` when `from` is inside the sphere (callers fall back to uniform
    /// area sampling).
    pub fn sample_cone(&self, from: Vec3, sampler: &mut Sampler) -> Option<(Vec3, Float, Float)> {
        let one_minus_cos_max = self.cone_one_minus_cos_max(from)?;
        let d = self.center - from;
        let dist2 = d.length_squared();
        let dist_c = dist2.sqrt();
        let w = d / dist_c;

        let (u1, u2) = sampler.next_2d();
        let cos_t = 1.0 - u1 * one_minus_cos_max;
        let sin2_t = ((1.0 - cos_t) * (1.0 + cos_t)).max(0.0);
        let sin_t = sin2_t.sqrt();
        let phi = 2.0 * PI * u2;

        let onb = Onb::from_w(w);
        let wi = onb.local(Vec3::new(sin_t * phi.cos(), sin_t * phi.sin(), cos_t));
        // Distance to the near intersection along wi (the sampled cone always
        // intersects; clamp the discriminant against rounding).
        let disc = (self.radius * self.radius - dist2 * sin2_t).max(0.0);
        let dist = (dist_c * cos_t - disc.sqrt()).max(0.0);
        let pdf = 1.0 / (2.0 * PI * one_minus_cos_max);
        Some((wi, dist, pdf))
    }

    /// The solid-angle pdf [`Sphere::sample_cone`] assigns to *any* direction
    /// from `from` that reaches the sphere (the cone is sampled uniformly).
    /// `None` when `from` is inside the sphere.
    pub fn cone_pdf(&self, from: Vec3) -> Option<Float> {
        let one_minus = self.cone_one_minus_cos_max(from)?;
        Some(1.0 / (2.0 * PI * one_minus))
    }

    /// `1 - cos(θ_max)` for the cone the sphere subtends from `from`, in a
    /// cancellation-free form (`sin²θ / (1 + cosθ)`) so tiny, distant lights
    /// keep a finite, accurate pdf. `None` if `from` is inside the sphere.
    fn cone_one_minus_cos_max(&self, from: Vec3) -> Option<Float> {
        let d2 = (self.center - from).length_squared();
        let r2 = self.radius * self.radius;
        if d2 <= r2 * 1.0001 {
            return None;
        }
        let sin2_max = (r2 / d2).min(1.0);
        let cos_max = (1.0 - sin2_max).max(0.0).sqrt();
        let one_minus = sin2_max / (1.0 + cos_max);
        (one_minus > 0.0).then_some(one_minus)
    }
}

/// Map a point on the unit sphere to `(u, v)` in `[0, 1]`.
fn sphere_uv(p: Vec3) -> (Float, Float) {
    let theta = (-p.y).acos();
    let phi = (-p.z).atan2(p.x) + PI;
    (phi / (2.0 * PI), theta / PI)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_unit_sphere_at_front() {
        let s = Sphere::new(Vec3::ZERO, 1.0, 0);
        let r = Ray::new(Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 1.0));
        let h = s.hit(&r, 0.001, Float::INFINITY).expect("should hit");
        assert!((h.t - 4.0).abs() < 1e-4);
        assert!(h.front_face);
        assert!((h.normal - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4);
    }

    #[test]
    fn cone_samples_hit_the_sphere_at_the_predicted_distance() {
        use crate::sampler::Sampler;
        let s = Sphere::new(Vec3::new(0.0, 0.0, 10.0), 2.0, 0);
        let from = Vec3::ZERO;
        let mut sampler = Sampler::random(5);
        let expected_pdf = s.cone_pdf(from).expect("outside the sphere");
        for _ in 0..500 {
            let (wi, dist, pdf) = s.sample_cone(from, &mut sampler).unwrap();
            assert!(((pdf - expected_pdf) / expected_pdf).abs() < 1e-5);
            let h = s
                .hit(&Ray::new(from, wi), 1e-4, Float::INFINITY)
                .expect("every cone sample must hit the sphere");
            assert!(
                (h.t - dist).abs() < 1e-2,
                "distance mismatch: {} vs {dist}",
                h.t
            );
        }
        // From inside there is no cone.
        assert!(s.cone_pdf(Vec3::new(0.0, 0.0, 10.5)).is_none());
        assert!(s
            .sample_cone(Vec3::new(0.0, 0.0, 10.5), &mut sampler)
            .is_none());
    }

    #[test]
    fn ray_from_inside_hits_far_wall_with_flipped_normal() {
        let s = Sphere::new(Vec3::ZERO, 1.0, 0);
        let r = Ray::new(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0));
        let h = s
            .hit(&r, 0.001, Float::INFINITY)
            .expect("should hit from inside");
        assert!((h.t - 1.0).abs() < 1e-4);
        assert!(!h.front_face);
        // Normal points back toward the ray origin (inward).
        assert!(h.normal.dot(r.dir) < 0.0);
    }
}
