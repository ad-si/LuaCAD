//! Thin-lens pinhole camera.
//!
//! Configuration ([`CameraConfig`]) is decoupled from the realized [`Camera`]:
//! the config is resolution-independent and serializable, and the `Camera` is
//! built for a specific aspect ratio at render time. This replaces the legacy
//! `Camera`, which owned the thread pool, the render loop, and OpenGL drawing
//! all at once.

use crate::math::sampling::random_in_unit_disk;
use crate::math::Vec3;
use crate::ray::Ray;
use crate::sampler::Sampler;
use crate::Float;
use std::f32::consts::PI;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug)]
pub struct CameraConfig {
    pub look_from: Vec3,
    pub look_at: Vec3,
    pub vup: Vec3,
    /// Vertical field of view, in degrees.
    pub vfov: Float,
    /// Lens aperture; 0 gives a perfect pinhole (everything in focus).
    pub aperture: Float,
    /// Focus distance; `None` focuses on `look_at`.
    pub focus_dist: Option<Float>,
}

impl Default for CameraConfig {
    fn default() -> Self {
        CameraConfig {
            look_from: Vec3::new(0.0, 0.0, 0.0),
            look_at: Vec3::new(0.0, 0.0, -1.0),
            vup: Vec3::new(0.0, 1.0, 0.0),
            vfov: 40.0,
            aperture: 0.0,
            focus_dist: None,
        }
    }
}

pub struct Camera {
    origin: Vec3,
    lower_left: Vec3,
    horizontal: Vec3,
    vertical: Vec3,
    u: Vec3,
    v: Vec3,
    lens_radius: Float,
}

impl Camera {
    pub fn new(config: &CameraConfig, aspect: Float) -> Camera {
        let theta = config.vfov * PI / 180.0;
        let h = (theta / 2.0).tan();
        let viewport_height = 2.0 * h;
        let viewport_width = aspect * viewport_height;

        let w = (config.look_from - config.look_at).normalize();
        let u = config.vup.cross(w).normalize();
        let v = w.cross(u);

        let focus_dist = config
            .focus_dist
            .unwrap_or_else(|| (config.look_from - config.look_at).length());

        let origin = config.look_from;
        let horizontal = u * (viewport_width * focus_dist);
        let vertical = v * (viewport_height * focus_dist);
        let lower_left = origin - horizontal * 0.5 - vertical * 0.5 - w * focus_dist;

        Camera {
            origin,
            lower_left,
            horizontal,
            vertical,
            u,
            v,
            lens_radius: config.aperture / 2.0,
        }
    }

    /// Generate a (normalized-direction) ray through normalized screen
    /// coordinates `s, t` in `[0, 1]`, where `(0, 0)` is bottom-left.
    pub fn get_ray(&self, s: Float, t: Float, sampler: &mut Sampler) -> Ray {
        let (origin, offset) = if self.lens_radius > 0.0 {
            let rd = random_in_unit_disk(sampler) * self.lens_radius;
            let offset = self.u * rd.x + self.v * rd.y;
            (self.origin + offset, offset)
        } else {
            (self.origin, Vec3::ZERO)
        };

        let target = self.lower_left + self.horizontal * s + self.vertical * t;
        Ray::new(origin, (target - origin - offset).normalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::Sampler;

    #[test]
    fn center_ray_points_at_look_at() {
        let cfg = CameraConfig {
            look_from: Vec3::new(0.0, 0.0, 0.0),
            look_at: Vec3::new(0.0, 0.0, -1.0),
            ..Default::default()
        };
        let cam = Camera::new(&cfg, 1.0);
        let mut sampler = Sampler::random(0);
        let ray = cam.get_ray(0.5, 0.5, &mut sampler);
        assert!((ray.dir - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4);
    }

    #[test]
    fn rays_are_unit_length() {
        let cam = Camera::new(&CameraConfig::default(), 16.0 / 9.0);
        let mut sampler = Sampler::random(0);
        for i in 0..10 {
            let s = i as Float / 10.0;
            let ray = cam.get_ray(s, 1.0 - s, &mut sampler);
            assert!((ray.dir.length() - 1.0).abs() < 1e-4);
        }
    }
}
