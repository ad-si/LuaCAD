//! Monte-Carlo sampling helpers, driven by a [`Sampler`].
//!
//! These replace the legacy `MathUtils.random*` grab-bag. Each routine consumes
//! a *fixed* number of sample dimensions (no rejection loops), which is what
//! lets the quasi-Monte-Carlo sampler assign stable dimensions to each decision
//! along a path.

use super::{onb::Onb, Vec3};
use crate::sampler::Sampler;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// Uniformly sample a point inside the unit disk (z = 0) via the concentric
/// (Shirley) map — a continuous, low-distortion, fixed-cost mapping. Used for
/// camera defocus blur.
pub fn random_in_unit_disk(sampler: &mut Sampler) -> Vec3 {
    let (u1, u2) = sampler.next_2d();
    let a = 2.0 * u1 - 1.0;
    let b = 2.0 * u2 - 1.0;
    if a == 0.0 && b == 0.0 {
        return Vec3::ZERO;
    }
    let (r, theta) = if a * a > b * b {
        (a, FRAC_PI_4 * (b / a))
    } else {
        (b, FRAC_PI_2 - FRAC_PI_4 * (a / b))
    };
    Vec3::new(r * theta.cos(), r * theta.sin(), 0.0)
}

/// A uniformly distributed unit vector on the sphere (fixed cost: 2D).
pub fn random_unit_vector(sampler: &mut Sampler) -> Vec3 {
    let (u1, u2) = sampler.next_2d();
    let z = 1.0 - 2.0 * u1;
    let r = (1.0 - z * z).max(0.0).sqrt();
    let phi = 2.0 * PI * u2;
    Vec3::new(r * phi.cos(), r * phi.sin(), z)
}

/// Cosine-weighted hemisphere sample, expressed in the local frame (z = up).
/// The pdf is `cos(theta) / pi`, which cancels the Lambertian cosine term.
pub fn random_cosine_direction(sampler: &mut Sampler) -> Vec3 {
    let (u1, u2) = sampler.next_2d();
    let phi = 2.0 * PI * u1;
    let r = u2.sqrt();
    Vec3::new(phi.cos() * r, phi.sin() * r, (1.0 - u2).sqrt())
}

/// Cosine-weighted hemisphere sample oriented around world-space normal `n`.
pub fn cosine_weighted_hemisphere(sampler: &mut Sampler, n: Vec3) -> Vec3 {
    Onb::from_w(n).local(random_cosine_direction(sampler))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_samples_stay_in_hemisphere() {
        let mut s = Sampler::random(42);
        let n = Vec3::new(0.0, 1.0, 0.0);
        for _ in 0..10_000 {
            let d = cosine_weighted_hemisphere(&mut s, n);
            assert!(d.dot(n) >= -1e-4, "sample fell below the hemisphere");
        }
    }

    #[test]
    fn unit_vectors_are_unit_length() {
        let mut s = Sampler::random(7);
        for _ in 0..1000 {
            assert!((random_unit_vector(&mut s).length() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn disk_samples_stay_inside() {
        let mut s = Sampler::random(9);
        for _ in 0..1000 {
            assert!(random_in_unit_disk(&mut s).length() <= 1.0 + 1e-4);
        }
    }
}
