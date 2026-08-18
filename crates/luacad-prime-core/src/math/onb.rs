//! Orthonormal basis built around a surface normal, used to transform
//! locally-sampled directions (e.g. cosine-weighted hemisphere samples) into
//! world space.

use super::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct Onb {
    pub u: Vec3,
    pub v: Vec3,
    pub w: Vec3,
}

impl Onb {
    /// Build a basis whose `w` axis is the (unit) normal `n`.
    pub fn from_w(n: Vec3) -> Onb {
        let w = n.normalize();
        // Pick any axis not (nearly) parallel to w to seed the cross products.
        let a = if w.x.abs() > 0.9 {
            Vec3::new(0.0, 1.0, 0.0)
        } else {
            Vec3::new(1.0, 0.0, 0.0)
        };
        let v = w.cross(a).normalize();
        let u = w.cross(v);
        Onb { u, v, w }
    }

    /// Transform a vector expressed in this basis into world space.
    #[inline]
    pub fn local(&self, a: Vec3) -> Vec3 {
        self.u * a.x + self.v * a.y + self.w * a.z
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Float;

    #[test]
    fn basis_is_orthonormal() {
        let onb = Onb::from_w(Vec3::new(0.3, -0.7, 0.2));
        let approx = |a: Float, b: Float| (a - b).abs() < 1e-5;
        assert!(approx(onb.u.length(), 1.0));
        assert!(approx(onb.v.length(), 1.0));
        assert!(approx(onb.w.length(), 1.0));
        assert!(approx(onb.u.dot(onb.v), 0.0));
        assert!(approx(onb.u.dot(onb.w), 0.0));
        assert!(approx(onb.v.dot(onb.w), 0.0));
    }
}
