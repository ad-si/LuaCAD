//! A small, immutable, `Copy` 3-component vector.
//!
//! This replaces the legacy mutable `Vec3f` (public fields, mutate-in-place,
//! shared mutable `ZERO`/`UNIT_*` singletons). Because `Vec3` is `Copy` and
//! every operation returns a fresh value, there is no aliasing hazard and the
//! hot rendering loops allocate nothing on the heap.

use crate::Float;
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: Float,
    pub y: Float,
    pub z: Float,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    pub const ONE: Vec3 = Vec3 {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };

    #[inline]
    pub const fn new(x: Float, y: Float, z: Float) -> Self {
        Vec3 { x, y, z }
    }

    #[inline]
    pub const fn splat(v: Float) -> Self {
        Vec3 { x: v, y: v, z: v }
    }

    #[inline]
    pub fn dot(self, o: Vec3) -> Float {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    #[inline]
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    #[inline]
    pub fn length_squared(self) -> Float {
        self.dot(self)
    }

    #[inline]
    pub fn length(self) -> Float {
        self.length_squared().sqrt()
    }

    /// Returns the unit vector. Caller is responsible for non-zero length;
    /// use [`Vec3::normalize_or`] when that is not guaranteed.
    #[inline]
    pub fn normalize(self) -> Vec3 {
        self * (1.0 / self.length())
    }

    #[inline]
    pub fn normalize_or(self, fallback: Vec3) -> Vec3 {
        let len = self.length();
        if len > 0.0 {
            self * (1.0 / len)
        } else {
            fallback
        }
    }

    /// True if every component is within epsilon of zero (degenerate scatter
    /// directions are clamped to the surface normal by callers).
    #[inline]
    pub fn is_near_zero(self) -> bool {
        const EPS: Float = 1e-8;
        self.x.abs() < EPS && self.y.abs() < EPS && self.z.abs() < EPS
    }

    /// Mirror `self` about a unit normal `n`.
    #[inline]
    pub fn reflect(self, n: Vec3) -> Vec3 {
        self - n * (2.0 * self.dot(n))
    }

    /// Refract an incident *unit* vector `self` across unit normal `n`,
    /// `eta_ratio` = n_in / n_out. Assumes total internal reflection has
    /// already been ruled out by the caller.
    #[inline]
    pub fn refract(self, n: Vec3, eta_ratio: Float) -> Vec3 {
        let cos_theta = (-self).dot(n).min(1.0);
        let r_out_perp = (self + n * cos_theta) * eta_ratio;
        let r_out_parallel = n * -(1.0 - r_out_perp.length_squared()).abs().sqrt();
        r_out_perp + r_out_parallel
    }

    #[inline]
    pub fn sqrt(self) -> Vec3 {
        Vec3::new(self.x.sqrt(), self.y.sqrt(), self.z.sqrt())
    }

    #[inline]
    pub fn min(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }

    #[inline]
    pub fn max(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }

    #[inline]
    pub fn clamp(self, lo: Float, hi: Float) -> Vec3 {
        Vec3::new(
            self.x.clamp(lo, hi),
            self.y.clamp(lo, hi),
            self.z.clamp(lo, hi),
        )
    }

    #[inline]
    pub fn max_component(self) -> Float {
        self.x.max(self.y).max(self.z)
    }

    /// Component on `axis` (0 = x, 1 = y, 2 = z).
    #[inline]
    pub fn axis(self, axis: usize) -> Float {
        match axis {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    #[inline]
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    #[inline]
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    #[inline]
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// Scale by a scalar.
impl Mul<Float> for Vec3 {
    type Output = Vec3;
    #[inline]
    fn mul(self, s: Float) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Mul<Vec3> for Float {
    type Output = Vec3;
    #[inline]
    fn mul(self, v: Vec3) -> Vec3 {
        v * self
    }
}

/// Component-wise (Hadamard) product — the standard meaning of "color × color".
impl Mul<Vec3> for Vec3 {
    type Output = Vec3;
    #[inline]
    fn mul(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }
}

impl Div<Float> for Vec3 {
    type Output = Vec3;
    #[inline]
    fn div(self, s: Float) -> Vec3 {
        self * (1.0 / s)
    }
}

impl AddAssign for Vec3 {
    #[inline]
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}

impl SubAssign for Vec3 {
    #[inline]
    fn sub_assign(&mut self, o: Vec3) {
        *self = *self - o;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Float, b: Float) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn dot_and_cross() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        assert!(approx(x.dot(y), 0.0));
        assert_eq!(x.cross(y), Vec3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn normalize_is_unit_length() {
        let v = Vec3::new(3.0, 4.0, 12.0);
        assert!(approx(v.length(), 13.0));
        assert!(approx(v.normalize().length(), 1.0));
    }

    #[test]
    fn reflect_off_flat_ground() {
        // Going down-right, reflecting off an up-facing normal flips the y.
        let d = Vec3::new(1.0, -1.0, 0.0);
        let n = Vec3::new(0.0, 1.0, 0.0);
        assert_eq!(d.reflect(n), Vec3::new(1.0, 1.0, 0.0));
    }

    #[test]
    fn refract_straight_through_is_identity() {
        // eta_ratio == 1 means no bending.
        let d = Vec3::new(0.0, -1.0, 0.0);
        let n = Vec3::new(0.0, 1.0, 0.0);
        let r = d.refract(n, 1.0);
        assert!(approx(r.x, 0.0) && approx(r.y, -1.0) && approx(r.z, 0.0));
    }

    #[test]
    fn hadamard_product() {
        let a = Vec3::new(2.0, 3.0, 4.0);
        let b = Vec3::new(0.5, 2.0, 0.25);
        assert_eq!(a * b, Vec3::new(1.0, 6.0, 1.0));
    }
}
