//! Triangle primitive using the Möller–Trumbore intersection test, with
//! optional per-vertex (smooth) normals.

use crate::aabb::Aabb;
use crate::hit::HitRecord;
use crate::math::Vec3;
use crate::ray::Ray;
use crate::sampler::Sampler;
use crate::{Float, MaterialId};

#[derive(Clone, Copy, Debug)]
pub struct Triangle {
    pub v0: Vec3,
    pub v1: Vec3,
    pub v2: Vec3,
    /// Optional smooth shading normals at each vertex; falls back to the flat
    /// geometric normal when absent.
    pub normals: Option<[Vec3; 3]>,
    /// Optional per-vertex texture coordinates `(u, v)`.
    pub uvs: Option<[[Float; 2]; 3]>,
    /// World-space tangent (aligned with +U), computed from UVs. `None` if the
    /// triangle has no texture coordinates.
    pub tangent: Option<Vec3>,
    pub material: MaterialId,
    /// Precomputed surface area (for light sampling).
    area: Float,
}

impl Triangle {
    pub fn new(v0: Vec3, v1: Vec3, v2: Vec3, material: MaterialId) -> Self {
        let area = 0.5 * (v1 - v0).cross(v2 - v0).length();
        Triangle {
            v0,
            v1,
            v2,
            normals: None,
            uvs: None,
            tangent: None,
            material,
            area,
        }
    }

    #[inline]
    pub fn area(&self) -> Float {
        self.area
    }

    /// Uniformly sample a point on the triangle, returning the point and its
    /// (geometric) normal.
    pub fn sample(&self, sampler: &mut Sampler) -> (Vec3, Vec3) {
        let (mut r1, mut r2) = sampler.next_2d();
        if r1 + r2 > 1.0 {
            r1 = 1.0 - r1;
            r2 = 1.0 - r2;
        }
        let p = self.v0 + (self.v1 - self.v0) * r1 + (self.v2 - self.v0) * r2;
        (p, self.geometric_normal())
    }

    pub fn with_normals(mut self, normals: [Vec3; 3]) -> Self {
        self.normals = Some(normals);
        self
    }

    pub fn with_uvs(mut self, uvs: [[Float; 2]; 3]) -> Self {
        self.uvs = Some(uvs);
        // Tangent aligned with the +U texture direction (Lengyel's method).
        let e1 = self.v1 - self.v0;
        let e2 = self.v2 - self.v0;
        let (du1, dv1) = (uvs[1][0] - uvs[0][0], uvs[1][1] - uvs[0][1]);
        let (du2, dv2) = (uvs[2][0] - uvs[0][0], uvs[2][1] - uvs[0][1]);
        let det = du1 * dv2 - du2 * dv1;
        self.tangent = if det.abs() > 1e-8 {
            let t = (e1 * dv2 - e2 * dv1) * (1.0 / det);
            Some(t.normalize_or(self.geometric_normal()))
        } else {
            None
        };
        self
    }

    #[inline]
    fn geometric_normal(&self) -> Vec3 {
        (self.v1 - self.v0).cross(self.v2 - self.v0).normalize()
    }

    pub fn hit(&self, ray: &Ray, t_min: Float, t_max: Float) -> Option<HitRecord> {
        const EPS: Float = 1e-8;
        let e1 = self.v1 - self.v0;
        let e2 = self.v2 - self.v0;
        let pvec = ray.dir.cross(e2);
        let det = e1.dot(pvec);
        if det.abs() < EPS {
            return None; // ray parallel to triangle plane
        }
        let inv_det = 1.0 / det;

        let tvec = ray.origin - self.v0;
        let u = tvec.dot(pvec) * inv_det;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }

        let qvec = tvec.cross(e1);
        let v = ray.dir.dot(qvec) * inv_det;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }

        let t = e2.dot(qvec) * inv_det;
        if t < t_min || t > t_max {
            return None;
        }

        let w = 1.0 - u - v;
        let outward = match self.normals {
            Some([n0, n1, n2]) => (n0 * w + n1 * u + n2 * v).normalize_or(self.geometric_normal()),
            None => self.geometric_normal(),
        };

        // Texture coordinates: interpolate per-vertex UVs if present, else fall
        // back to the barycentric coordinates.
        let (tu, tv) = match self.uvs {
            Some([a, b, c]) => (
                w * a[0] + u * b[0] + v * c[0],
                w * a[1] + u * b[1] + v * c[1],
            ),
            None => (u, v),
        };

        let p = ray.at(t);
        let mut hit =
            HitRecord::with_face_normal(ray, t, p, outward, tu, tv, self.area, self.material);
        if let Some(tangent) = self.tangent {
            hit.tangent = tangent;
        }
        Some(hit)
    }

    pub fn aabb(&self) -> Aabb {
        // Pad degenerate (axis-aligned) triangles so the box has volume.
        let mut b = Aabb::from_points(&[self.v0, self.v1, self.v2]);
        const PAD: Float = 1e-4;
        let d = b.max - b.min;
        if d.x < PAD {
            b.min.x -= PAD;
            b.max.x += PAD;
        }
        if d.y < PAD {
            b.min.y -= PAD;
            b.max.y += PAD;
        }
        if d.z < PAD {
            b.min.z -= PAD;
            b.max.z += PAD;
        }
        b
    }

    pub fn centroid(&self) -> Vec3 {
        (self.v0 + self.v1 + self.v2) * (1.0 / 3.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_triangle_center() {
        let t = Triangle::new(
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            0,
        );
        let r = Ray::new(Vec3::new(0.0, 0.0, -3.0), Vec3::new(0.0, 0.0, 1.0));
        let h = t.hit(&r, 0.001, Float::INFINITY).expect("should hit");
        assert!((h.t - 3.0).abs() < 1e-4);
        assert!((h.normal - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4);
    }

    #[test]
    fn ray_misses_outside_triangle() {
        let t = Triangle::new(
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            0,
        );
        let r = Ray::new(Vec3::new(5.0, 5.0, -3.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(t.hit(&r, 0.001, Float::INFINITY).is_none());
    }
}
